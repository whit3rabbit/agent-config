//! Source parsing, ported from `src/source-parser.ts` of the upstream CLI.

use std::path::{Path, PathBuf};

use skills_store::error::{diag, SkillsError};
use skills_store::paths;

/// A parsed skill source.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSource {
    /// Source kind.
    pub kind: SourceKind,
    /// Clone/download URL, or the resolved local path.
    pub url: String,
    /// Subpath within the source.
    pub subpath: Option<String>,
    /// Resolved local path (local sources only).
    pub local_path: Option<PathBuf>,
    /// Branch/tag ref.
    pub ref_: Option<String>,
    /// Skill name filter from `@skill` syntax.
    pub skill_filter: Option<String>,
}

/// Source kinds (`ParsedSource['type']` upstream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// GitHub repository.
    GitHub,
    /// GitLab repository.
    GitLab,
    /// Generic git URL.
    Git,
    /// Local path.
    Local,
    /// RFC 8615 well-known discovery.
    WellKnown,
    /// Direct download (SKILL.md / archive).
    Download,
}

impl SourceKind {
    /// Upstream string form (used in lock files).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::GitHub => "github",
            SourceKind::GitLab => "gitlab",
            SourceKind::Git => "git",
            SourceKind::Local => "local",
            SourceKind::WellKnown => "well-known",
            SourceKind::Download => "download",
        }
    }
}

/// Split `scheme://host[:port]/path` into `(host, path)` (query and fragment
/// dropped); used by the git and download layers.
#[must_use]
pub fn host_and_path(url: &str) -> Option<(String, String)> {
    let rest = url.split_once("://")?.1;
    let (authority, remainder) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let host = authority.split('@').next_back().unwrap_or(authority);
    let (path, _) = take_path(remainder);
    Some((host.to_string(), path))
}

fn is_local_path(input: &str) -> bool {
    Path::new(input).is_absolute()
        || input.starts_with("./")
        || input.starts_with("../")
        || input == "."
        || input == ".."
        || regex_lite_windows_drive(input)
}

fn regex_lite_windows_drive(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

const SOURCE_ALIASES: &[(&str, &str)] = &[
    ("coinbase/agentWallet", "coinbase/agentic-wallet-skills"),
    ("vercel-labs/vercel-skills", "vercel-labs/agent-skills"),
];

/// Whether a ref is a full 40-char commit SHA (ported from `git.ts`).
#[must_use]
pub fn is_commit_sha(ref_: &str) -> bool {
    ref_.len() == 40 && ref_.chars().all(|c| c.is_ascii_hexdigit())
}

/// `owner/repo@name` selects a skill unless the value is a version pin or SHA.
fn is_version_pin(value: &str) -> bool {
    let version_pin = |v: &str| -> bool {
        let bytes = v.as_bytes();
        let mut i = 0;
        if bytes.first() == Some(&b'v') {
            i = 1;
        }
        let rest = &bytes[i..];
        // One or more digit groups separated by dots, with optional
        // prerelease/build suffixes.
        let mut saw_digit = false;
        let mut saw_dot_group = false;
        let mut j = 0;
        while j < rest.len() && rest[j].is_ascii_digit() {
            saw_digit = true;
            j += 1;
        }
        if !saw_digit {
            return false;
        }
        while j < rest.len() && rest[j] == b'.' {
            j += 1;
            let start = j;
            while j < rest.len() && rest[j].is_ascii_digit() {
                j += 1;
            }
            if j == start {
                return false;
            }
            saw_dot_group = true;
        }
        if !saw_dot_group {
            return false;
        }
        // Optional prerelease/build metadata.
        if j < rest.len() && (rest[j] == b'-' || rest[j] == b'+') {
            let marker = rest[j];
            j += 1;
            while j < rest.len()
                && (rest[j].is_ascii_alphanumeric() || rest[j] == b'-' || rest[j] == b'.')
            {
                j += 1;
            }
            let _ = marker;
        }
        j == rest.len()
    };
    version_pin(value) || is_commit_sha(value)
}

struct FragmentRef {
    input_without_fragment: String,
    ref_: Option<String>,
    skill_filter: Option<String>,
}

fn looks_like_git_source(input: &str) -> bool {
    if input.starts_with("github:") || input.starts_with("gitlab:") || input.starts_with("git@") {
        return true;
    }
    if starts_with_ssh_git(input) {
        return true;
    }
    if input.starts_with("http://") || input.starts_with("https://") {
        let (host, path) = split_url(input);
        if paths::is_github_host(&host) {
            return is_github_repo_or_tree_path(&path);
        }
        if host == "gitlab.com" {
            return is_gitlab_repo_or_tree_path(&path);
        }
        // Azure Repos: /_git/{repo} on any host.
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if let Some(index) = segments.iter().position(|s| *s == "_git") {
            if index + 1 < segments.len() {
                return true;
            }
        }
    }
    if starts_with_http_git_suffix(input) {
        return true;
    }
    !input.contains(':')
        && !input.starts_with('.')
        && !input.starts_with('/')
        && shorthand_shape(input)
}

fn shorthand_shape(input: &str) -> bool {
    // ^([^/]+)/([^/]+)(?:/(.+)|@(.+))?$
    let mut parts = input.splitn(3, '/');
    let Some(owner) = parts.next() else {
        return false;
    };
    if owner.is_empty() || owner.contains('/') {
        return false;
    }
    let Some(rest) = parts.next() else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let third = parts.next();
    match third {
        None => true,
        Some(t) => !t.is_empty(),
    }
}

fn starts_with_ssh_git(input: &str) -> bool {
    let lower = input.to_lowercase();
    lower.starts_with("ssh://") && (lower.contains(".git/") || lower.ends_with(".git"))
}

fn starts_with_http_git_suffix(input: &str) -> bool {
    let lower = input.to_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    let (path, _) = take_path(input);
    path.contains(".git/") || path.ends_with(".git") || path.ends_with(".git/")
}

/// Split `scheme://host/path?query` into `(host, path)` (query dropped).
fn split_url(input: &str) -> (String, String) {
    let without_scheme = input.split_once("://").map(|x| x.1).unwrap_or(input);
    let (authority, rest) = match without_scheme.find('/') {
        Some(index) => (&without_scheme[..index], &without_scheme[index..]),
        None => (without_scheme, ""),
    };
    let host = authority.split('@').next_back().unwrap_or(authority);
    let (path, _) = take_path(rest);
    (host.to_string(), path)
}

fn take_path(input: &str) -> (String, String) {
    let end = input.find(['?', '#']).unwrap_or(input.len());
    (input[..end].to_string(), input[end..].to_string())
}

fn is_github_repo_or_tree_path(path: &str) -> bool {
    // ^\/[^/]+\/[^/]+(?:\.git)?(?:\/tree\/[^/]+(?:\/.*)?)?\/?$
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return false;
    }
    if segments.len() == 2 {
        return true;
    }
    if segments[2] == "tree" && segments.len() >= 4 {
        return true;
    }
    // `.git` suffix folded into segment 1.
    segments.len() == 3 && segments[2].ends_with(".git")
}

fn is_gitlab_repo_or_tree_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // /owner/repo or /group/.../repo/-/tree/branch...
    if let Some(index) = segments.iter().position(|s| *s == "-") {
        return segments.get(index + 1).is_some_and(|s| *s == "tree")
            && segments.len() >= index + 3;
    }
    segments.len() >= 2
}

fn parse_fragment_ref(input: &str) -> FragmentRef {
    let Some(hash_index) = input.find('#') else {
        return FragmentRef {
            input_without_fragment: input.to_string(),
            ref_: None,
            skill_filter: None,
        };
    };
    let input_without_fragment = input[..hash_index].to_string();
    let fragment = &input[hash_index + 1..];
    if fragment.is_empty() || !looks_like_git_source(&input_without_fragment) {
        return FragmentRef {
            input_without_fragment: input.to_string(),
            ref_: None,
            skill_filter: None,
        };
    }
    match fragment.find('@') {
        None => FragmentRef {
            input_without_fragment,
            ref_: Some(url_decode(fragment)),
            skill_filter: None,
        },
        Some(at_index) => {
            let ref_part = &fragment[..at_index];
            let skill_part = &fragment[at_index + 1..];
            FragmentRef {
                input_without_fragment,
                ref_: if ref_part.is_empty() {
                    None
                } else {
                    Some(url_decode(ref_part))
                },
                skill_filter: if skill_part.is_empty() {
                    None
                } else {
                    Some(url_decode(skill_part))
                },
            }
        }
    }
}

fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn append_fragment_ref(input: &str, ref_: Option<&str>, skill_filter: Option<&str>) -> String {
    if ref_.is_none() && skill_filter.is_none() {
        return input.to_string();
    }
    let mut out = format!("{input}#");
    if let Some(ref_) = ref_ {
        out.push_str(&url_encode(ref_));
    }
    if let Some(skill) = skill_filter {
        out.push_str(&format!("@{}", url_encode(skill)));
    }
    out
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn is_hosted_artifact_url(input: &str) -> bool {
    if !(input.starts_with("http://") || input.starts_with("https://")) {
        return false;
    }
    let (host, path) = split_url(input);
    match host.as_str() {
        "raw.githubusercontent.com" | "codeload.github.com" | "objects.githubusercontent.com" => {
            true
        }
        "github.com" => {
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if segments.len() < 3 {
                return false;
            }
            matches!(segments[2], "archive" | "raw" | "releases")
                && (segments[2] != "releases"
                    || segments
                        .get(3)
                        .is_some_and(|s| *s == "download" || *s == "latest"))
        }
        "gitlab.com" => path.contains("/-/archive/") || path.contains("/-/raw/"),
        _ => false,
    }
}

/// Sanitize a subpath, rejecting `..` traversal (upstream `sanitizeSubpath`).
pub fn sanitize_subpath(subpath: &str) -> Result<String, SkillsError> {
    if subpath
        .replace('\\', "/")
        .split('/')
        .any(|segment| segment == "..")
    {
        return Err(diag::unsafe_subpath(subpath));
    }
    Ok(subpath.to_string())
}

/// Extract `owner/repo` (or GitLab subgroup paths) from a parsed source.
#[must_use]
pub fn get_owner_repo(parsed: &ParsedSource) -> Option<String> {
    if matches!(parsed.kind, SourceKind::Local | SourceKind::Download) {
        return None;
    }
    let url = &parsed.url;
    // git@host:owner/repo
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some(colon) = rest.find(':') {
            let path = rest[colon + 1..].trim_end_matches(".git");
            if path.contains('/') {
                return Some(path.to_string());
            }
            return None;
        }
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let path = rest.split_once('/').map(|x| x.1).unwrap_or("");
        let path = path.split(['?', '#']).next().unwrap_or(path);
        let path = path.trim_end_matches(".git");
        if path.contains('/') {
            return Some(path.to_string());
        }
        return None;
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let (host, path) = split_url(url);
    let _ = host;
    let path = path.trim_start_matches('/');
    let path = path.trim_end_matches(".git");
    if path.contains('/') {
        Some(path.to_string())
    } else {
        None
    }
}

/// Parse `owner/repo` into its parts.
#[allow(dead_code)] // parity with the upstream helper set
#[must_use]
pub fn parse_owner_repo(owner_repo: &str) -> Option<(String, String)> {
    let (owner, repo) = owner_repo.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

/// Parse a source string into a [`ParsedSource`] (upstream `parseSource`).
#[must_use]
pub fn parse_source(input: &str) -> ParsedSource {
    parse_source_with_host(input, &paths::github_host())
}

/// Parse with a pinned GitHub host (used for public lock entries).
#[must_use]
pub fn parse_source_with_host(input: &str, github_host: &str) -> ParsedSource {
    if is_local_path(input) {
        let resolved = std::env::current_dir().unwrap_or_default().join(input);
        let resolved = lexical_clean(&resolved);
        return ParsedSource {
            kind: SourceKind::Local,
            url: resolved.to_string_lossy().to_string(),
            subpath: None,
            local_path: Some(resolved),
            ref_: None,
            skill_filter: None,
        };
    }

    let fragment = parse_fragment_ref(input);
    let mut input = fragment.input_without_fragment;

    for (alias, target) in SOURCE_ALIASES {
        if input == *alias {
            input = (*target).to_string();
            break;
        }
    }

    if let Some(rest) = input.strip_prefix("github:") {
        return parse_source_with_host(
            &append_fragment_ref(
                rest,
                fragment.ref_.as_deref(),
                fragment.skill_filter.as_deref(),
            ),
            github_host,
        );
    }
    if let Some(rest) = input.strip_prefix("gitlab:") {
        let joined = format!("https://gitlab.com/{rest}");
        return parse_source_with_host(
            &append_fragment_ref(
                &joined,
                fragment.ref_.as_deref(),
                fragment.skill_filter.as_deref(),
            ),
            github_host,
        );
    }

    if is_hosted_artifact_url(&input) {
        return ParsedSource {
            kind: SourceKind::Download,
            url: input,
            subpath: None,
            local_path: None,
            ref_: None,
            skill_filter: None,
        };
    }

    // GitHub SSH tree URL: git@host:owner/repo/tree/branch[/path]
    if let Some(parsed) = parse_github_ssh_tree(&input) {
        return parsed;
    }

    // https://github.com/owner/repo/tree/branch/path
    if let Some(parsed) = parse_github_url(&input, fragment.ref_.clone()) {
        return parsed;
    }

    // GitLab tree URLs on any instance.
    if let Some(parsed) = parse_gitlab_url(&input, fragment.ref_.clone()) {
        return parsed;
    }

    // gitlab.com shorthand.
    if let Some(rest) = input.strip_prefix("https://gitlab.com/") {
        let repo_path = rest.trim_end_matches(".git").trim_end_matches('/');
        if repo_path.contains('/') && !repo_path.is_empty() {
            return ParsedSource {
                kind: SourceKind::GitLab,
                url: format!("https://gitlab.com/{repo_path}.git"),
                subpath: None,
                local_path: None,
                ref_: fragment.ref_,
                skill_filter: None,
            };
        }
    }

    // Shorthand: owner/repo, owner/repo/subpath, owner/repo@skill.
    let shorthand_source_type = if github_host == "github.com" {
        SourceKind::GitHub
    } else {
        SourceKind::Git
    };
    let shorthand_ok = !input.contains(':') && !input.starts_with('.') && !input.starts_with('/');
    if shorthand_ok {
        // owner/repo@skill
        if let Some(at_index) = input.find('@') {
            let (left, skill_filter) = input.split_at(at_index);
            let skill_filter = &skill_filter[1..];
            let mut left_parts = left.split('/');
            let owner = left_parts.next();
            let repo = left_parts.next();
            let no_more = left_parts.next().is_none();
            if no_more && left.contains('/') {
                // left must be exactly owner/repo
            }
            if let (Some(owner), Some(repo), true) = (owner, repo, no_more) {
                if !owner.is_empty() && !repo.is_empty() && !skill_filter.is_empty() {
                    if fragment.ref_.is_none()
                        && fragment.skill_filter.is_none()
                        && is_version_pin(skill_filter)
                    {
                        return ParsedSource {
                            kind: shorthand_source_type,
                            url: format!("https://{github_host}/{owner}/{repo}.git"),
                            subpath: None,
                            local_path: None,
                            ref_: Some(skill_filter.to_string()),
                            skill_filter: None,
                        };
                    }
                    return ParsedSource {
                        kind: shorthand_source_type,
                        url: format!("https://{github_host}/{owner}/{repo}.git"),
                        subpath: None,
                        local_path: None,
                        ref_: fragment.ref_,
                        skill_filter: fragment
                            .skill_filter
                            .or_else(|| Some(skill_filter.to_string())),
                    };
                }
            }
        }
        // owner/repo[/subpath]
        if shorthand_shape(&input) {
            let mut parts = input.splitn(3, '/');
            let owner = parts.next().unwrap_or("");
            let repo = parts.next().unwrap_or("");
            let subpath = parts
                .next()
                .map(|s| s.trim_end_matches('/'))
                .filter(|s| !s.is_empty());
            let subpath = subpath.map(|s| sanitize_subpath(s).ok()).unwrap_or(None);
            return ParsedSource {
                kind: shorthand_source_type,
                url: format!("https://{github_host}/{owner}/{repo}.git"),
                subpath,
                local_path: None,
                ref_: fragment.ref_,
                skill_filter: fragment.skill_filter,
            };
        }
    }

    if is_well_known_url(&input) {
        return ParsedSource {
            kind: SourceKind::WellKnown,
            url: input,
            subpath: None,
            local_path: None,
            ref_: None,
            skill_filter: None,
        };
    }

    ParsedSource {
        kind: SourceKind::Git,
        url: input,
        subpath: None,
        local_path: None,
        ref_: fragment.ref_,
        skill_filter: None,
    }
}

fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn parse_github_ssh_tree(input: &str) -> Option<ParsedSource> {
    // ^git@([^:]+):([^/]+)/([^/]+?)(?:\.git)?/tree/([^/]+)(?:/(.+))?$
    let rest = input.strip_prefix("git@")?;
    let (host, remainder) = rest.split_once(':')?;
    if !paths::is_github_host(host) {
        return None;
    }
    let (repo_part, tree_and_more) = remainder.split_once("/tree/")?;
    let repo = repo_part.trim_end_matches(".git");
    let (owner, repo) = repo_part_owners(repo)?;
    let mut segments = tree_and_more.splitn(2, '/');
    let branch = segments.next()?.to_string();
    let subpath = segments
        .next()
        .map(|s| sanitize_subpath(s).ok())
        .unwrap_or(None);
    Some(ParsedSource {
        kind: SourceKind::Git,
        url: format!("git@{host}:{owner}/{repo}.git"),
        subpath,
        local_path: None,
        ref_: Some(branch),
        skill_filter: None,
    })
}

fn repo_part_owners(repo: &str) -> Option<(&str, &str)> {
    let (owner, name) = repo.split_once('/')?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((owner, name))
}

fn parse_github_url(input: &str, fragment_ref: Option<String>) -> Option<ParsedSource> {
    let (host, path) = split_url(input);
    if host != "github.com" {
        return None;
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    let owner = segments[0];
    let raw_repo = segments[1].trim_end_matches(".git");
    if segments.len() >= 5
        && segments[2] == "tree"
        && !segments[3].is_empty()
        && !segments[4].is_empty()
    {
        let branch = segments[3].to_string();
        let subpath = segments[4..].join("/");
        let subpath = sanitize_subpath(&subpath).ok();
        return Some(ParsedSource {
            kind: SourceKind::GitHub,
            url: format!("https://github.com/{owner}/{raw_repo}.git"),
            subpath,
            local_path: None,
            ref_: Some(branch).or(fragment_ref),
            skill_filter: None,
        });
    }
    if segments.len() == 4 && segments[2] == "tree" {
        return Some(ParsedSource {
            kind: SourceKind::GitHub,
            url: format!("https://github.com/{owner}/{raw_repo}.git"),
            subpath: None,
            local_path: None,
            ref_: Some(segments[3].to_string()).or(fragment_ref),
            skill_filter: None,
        });
    }
    Some(ParsedSource {
        kind: SourceKind::GitHub,
        url: format!("https://github.com/{owner}/{raw_repo}.git"),
        subpath: None,
        local_path: None,
        ref_: fragment_ref,
        skill_filter: None,
    })
}

fn parse_gitlab_url(input: &str, fragment_ref: Option<String>) -> Option<ParsedSource> {
    if !(input.starts_with("http://") || input.starts_with("https://")) {
        return None;
    }
    let protocol_end = input.find("://")?;
    let protocol = &input[..protocol_end];
    let (host, path) = split_url(input);
    if host == "github.com" {
        return None;
    }
    // /repoPath/-/tree/branch[/subpath]
    let marker = "/-/tree/";
    let marker_index = path.find(marker)?;
    let repo_path = path[1..marker_index].trim_end_matches(".git");
    if repo_path.is_empty() {
        return None;
    }
    let rest = &path[marker_index + marker.len()..];
    let mut segments = rest.splitn(2, '/');
    let branch = segments.next()?.to_string();
    if branch.is_empty() {
        return None;
    }
    let subpath = segments
        .next()
        .map(|s| sanitize_subpath(s).ok())
        .unwrap_or(None);
    Some(ParsedSource {
        kind: SourceKind::GitLab,
        url: format!("{protocol}://{host}/{repo_path}.git"),
        subpath,
        local_path: None,
        ref_: Some(branch).or(fragment_ref),
        skill_filter: None,
    })
}

fn is_well_known_url(input: &str) -> bool {
    if !(input.starts_with("http://") || input.starts_with("https://")) {
        return false;
    }
    let (host, _) = split_url(input);
    if matches!(
        host.as_str(),
        "github.com" | "gitlab.com" | "raw.githubusercontent.com"
    ) {
        return false;
    }
    !input.ends_with(".git")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shorthand() {
        let parsed = parse_source("vercel-labs/agent-skills");
        assert_eq!(parsed.kind, SourceKind::GitHub);
        assert_eq!(
            parsed.url,
            "https://github.com/vercel-labs/agent-skills.git"
        );
    }

    #[test]
    fn parses_shorthand_with_skill() {
        let parsed = parse_source("vercel-labs/agent-skills@vercel-optimize");
        assert_eq!(parsed.skill_filter.as_deref(), Some("vercel-optimize"));
        assert_eq!(parsed.kind, SourceKind::GitHub);
    }

    #[test]
    fn version_pin_is_a_ref() {
        let parsed = parse_source("owner/repo@v1.2.3");
        assert_eq!(parsed.ref_.as_deref(), Some("v1.2.3"));
        assert!(parsed.skill_filter.is_none());
        let sha = parse_source("owner/repo@0123456789abcdef0123456789abcdef01234567");
        assert_eq!(
            sha.ref_.as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    #[test]
    fn parses_github_tree_url() {
        let parsed = parse_source("https://github.com/owner/repo/tree/branch/skills/web");
        assert_eq!(parsed.kind, SourceKind::GitHub);
        assert_eq!(parsed.ref_.as_deref(), Some("branch"));
        assert_eq!(parsed.subpath.as_deref(), Some("skills/web"));
    }

    #[test]
    fn parses_local_paths() {
        let parsed = parse_source("./skills");
        assert_eq!(parsed.kind, SourceKind::Local);
        assert!(parsed.local_path.is_some());
        let parsed = parse_source("/abs/path");
        assert_eq!(parsed.kind, SourceKind::Local);
    }

    #[test]
    fn hosted_artifacts_are_downloads() {
        let parsed = parse_source("https://github.com/owner/repo/archive/refs/heads/main.zip");
        assert_eq!(parsed.kind, SourceKind::Download);
        let parsed = parse_source("https://raw.githubusercontent.com/o/r/main/skills/x/SKILL.md");
        assert_eq!(parsed.kind, SourceKind::Download);
    }

    #[test]
    fn gitlab_urls() {
        let parsed = parse_source("https://gitlab.com/group/sub/repo");
        assert_eq!(parsed.kind, SourceKind::GitLab);
        assert_eq!(parsed.url, "https://gitlab.com/group/sub/repo.git");
        let parsed = parse_source("https://gitlab.com/group/repo/-/tree/branch/sub");
        assert_eq!(parsed.ref_.as_deref(), Some("branch"));
        assert_eq!(parsed.subpath.as_deref(), Some("sub"));
    }

    #[test]
    fn owner_repo_extraction() {
        let parsed = parse_source("owner/repo");
        assert_eq!(get_owner_repo(&parsed).as_deref(), Some("owner/repo"));
        let parsed = parse_source("https://gitlab.com/group/sub/repo.git");
        assert_eq!(get_owner_repo(&parsed).as_deref(), Some("group/sub/repo"));
        let local = parse_source("./x");
        assert_eq!(get_owner_repo(&local), None);
    }

    #[test]
    fn well_known_urls() {
        let parsed = parse_source("https://example.com/skills");
        assert_eq!(parsed.kind, SourceKind::WellKnown);
        let parsed = parse_source("https://example.com/repo.git");
        assert_eq!(parsed.kind, SourceKind::Git);
    }

    #[test]
    fn fragment_refs() {
        let parsed = parse_source("owner/repo#main@skill-a");
        assert_eq!(parsed.ref_.as_deref(), Some("main"));
        assert_eq!(parsed.skill_filter.as_deref(), Some("skill-a"));
    }
}
