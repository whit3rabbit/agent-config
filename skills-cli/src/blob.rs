//! Blob fast-path installs, ported from `src/blob.ts`: GitHub Trees API
//! discovery, raw.githubusercontent.com frontmatter reads, and skills.sh
//! snapshot downloads.

use std::time::Duration;

use skills_store::sanitize::{sanitize_metadata, to_skill_slug};

use crate::http;

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_BASE_URL_DEFAULT: &str = "https://skills.sh";

fn download_base_url() -> String {
    std::env::var("SKILLS_DOWNLOAD_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DOWNLOAD_BASE_URL_DEFAULT.to_string())
}

/// One entry of a GitHub tree.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TreeEntry {
    /// Path within the repo.
    pub path: String,
    /// `blob` or `tree`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Git SHA.
    pub sha: String,
}

/// A fetched repo tree.
#[derive(Debug, Clone)]
pub struct RepoTree {
    /// Tree SHA.
    pub sha: String,
    /// Branch the tree was fetched from.
    pub branch: String,
    /// Flattened entries.
    pub tree: Vec<TreeEntry>,
}

fn api_base() -> String {
    let host = skills_store::paths::github_host();
    if host == "github.com" {
        "https://api.github.com".to_string()
    } else {
        format!("https://{host}/api/v3")
    }
}

fn fetch_tree_branch(owner_repo: &str, branch: &str, token: Option<&str>) -> Result<RepoTree, (bool, bool)> {
    // (rate_limited, auth_retryable)
    let url = format!(
        "{}/repos/{}/git/trees/{}?recursive=1",
        api_base(),
        owner_repo,
        url_encode_component(branch)
    );
    let token_owned = token.map(str::to_string);
    let mut headers: Vec<(&str, &str)> =
        vec![("Accept", "application/vnd.github.v3+json"), ("User-Agent", "skills-cli")];
    let auth_header;
    if let Some(token) = &token_owned {
        auth_header = format!("Bearer {token}");
        headers.push(("Authorization", auth_header.as_str()));
    }
    match http::get(&url, FETCH_TIMEOUT, &headers) {
        Ok(response) => {
            if (200..300).contains(&response.status) {
                let Ok(value) = response.json() else {
                    return Err((false, false));
                };
                let sha = value.get("sha").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let tree: Vec<TreeEntry> = value
                    .get("tree")
                    .and_then(|t| serde_json::from_value(t.clone()).ok())
                    .unwrap_or_default();
                return Ok(RepoTree { sha, branch: branch.to_string(), tree });
            }
            let rate_limited = response.status == 403
                && response.header("x-ratelimit-remaining") == Some("0");
            let auth_retryable = response.status == 401 || response.status == 404;
            Err((rate_limited, auth_retryable))
        }
        Err(_) => Err((false, false)),
    }
}

fn fetch_tree_with_gh_cli(owner_repo: &str, branches: &[String]) -> Option<RepoTree> {
    use std::process::Command;
    for branch in branches {
        let endpoint = format!("repos/{owner_repo}/git/trees/{}?recursive=1", url_encode_component(branch));
        let output = Command::new("gh")
            .args(["api", &endpoint, "--method", "GET", "--hostname", &skills_store::paths::github_host()])
            .env("GH_PROMPT_DISABLED", "1")
            .output()
            .ok()?;
        if !output.status.success() {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
            continue;
        };
        let (Some(sha), Some(tree)) = (
            value.get("sha").and_then(|s| s.as_str()),
            value.get("tree").and_then(|t| t.as_array()),
        ) else {
            continue;
        };
        let entries: Vec<TreeEntry> = tree
            .iter()
            .map(|entry| TreeEntry {
                path: entry.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string(),
                kind: entry.get("type").and_then(|t| t.as_str()).unwrap_or("blob").to_string(),
                sha: entry.get("sha").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            })
            .collect();
        return Some(RepoTree { sha: sha.to_string(), branch: branch.clone(), tree: entries });
    }
    None
}

fn fetch_tree_with_auth(owner_repo: &str, branches: &[String]) -> Option<RepoTree> {
    if let Some(token) = http::github_token() {
        for branch in branches {
            if let Ok(tree) = fetch_tree_branch(owner_repo, branch, Some(&token)) {
                return Some(tree);
            }
        }
    }
    fetch_tree_with_gh_cli(owner_repo, branches)
}

/// Fetch the full recursive tree for a repo (upstream `fetchRepoTree`).
#[must_use]
pub fn fetch_repo_tree(owner_repo: &str, ref_: Option<&str>) -> Option<RepoTree> {
    let branches: Vec<String> = match ref_ {
        Some(ref_) => vec![ref_.to_string()],
        None => vec!["HEAD".to_string(), "main".to_string(), "master".to_string()],
    };
    let mut rate_limited = false;
    let mut auth_retryable = false;
    for branch in &branches {
        match fetch_tree_branch(owner_repo, branch, None) {
            Ok(tree) => return Some(tree),
            Err((limited, retryable)) => {
                if limited {
                    rate_limited = true;
                    break;
                }
                if retryable {
                    auth_retryable = true;
                    break;
                }
            }
        }
    }
    if rate_limited || auth_retryable {
        return fetch_tree_with_auth(owner_repo, &branches);
    }
    None
}

/// Folder hash (tree SHA) for a skill path from a repo tree
/// (upstream `getSkillFolderHashFromTree`).
#[must_use]
pub fn get_skill_folder_hash_from_tree(tree: &RepoTree, skill_path: &str) -> Option<String> {
    let mut folder_path = skill_path.replace('\\', "/");
    let lower = folder_path.to_lowercase();
    if lower.ends_with("/skill.md") {
        folder_path.truncate(folder_path.len() - 9);
    } else if lower.ends_with("skill.md") {
        folder_path.truncate(folder_path.len() - 8);
    }
    if folder_path.ends_with('/') {
        folder_path.pop();
    }
    if folder_path.is_empty() {
        return Some(tree.sha.clone());
    }
    tree.tree
        .iter()
        .find(|entry| entry.kind == "tree" && entry.path == folder_path)
        .map(|entry| entry.sha.clone())
}

const PRIORITY_PREFIXES: &[&str] = &[
    "",
    "skills/",
    "skills/.curated/",
    "skills/.experimental/",
    "skills/.system/",
];

const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// Find SKILL.md paths in a repo tree (upstream `findSkillMdPaths`).
#[must_use]
pub fn find_skill_md_paths(tree: &RepoTree, subpath: Option<&str>, extra_containers: &[String]) -> Vec<String> {
    let all_skill_mds: Vec<String> = tree
        .tree
        .iter()
        .filter(|entry| entry.kind == "blob")
        .map(|entry| entry.path.clone())
        .filter(|path| {
            path.rsplit('/').next().map(|last| last.to_lowercase() == "skill.md").unwrap_or(false)
        })
        .collect();

    let prefix = match subpath {
        Some(subpath) => {
            if subpath.ends_with('/') {
                subpath.to_string()
            } else {
                format!("{subpath}/")
            }
        }
        None => String::new(),
    };
    let filtered: Vec<String> = if prefix.is_empty() {
        all_skill_mds
    } else {
        all_skill_mds
            .into_iter()
            .filter(|path| path.starts_with(&prefix) || path.as_str() == format!("{prefix}SKILL.md"))
            .collect()
    };
    if filtered.is_empty() {
        return Vec::new();
    }

    let lower_skill_md_set: std::collections::HashSet<String> =
        filtered.iter().map(|p| p.to_lowercase()).collect();

    let mut priority_prefixes: Vec<String> = PRIORITY_PREFIXES.iter().map(|s| s.to_string()).collect();
    for dir in skills_store::registry::agent_project_skill_dirs() {
        priority_prefixes.push(format!("{dir}/"));
    }
    for dir in extra_containers {
        let candidate = format!("{dir}/");
        if !priority_prefixes.contains(&candidate) {
            priority_prefixes.push(candidate);
        }
    }

    let mut results: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for priority_prefix in &priority_prefixes {
        let full_prefix = format!("{prefix}{priority_prefix}");
        let is_container = !priority_prefix.is_empty();
        let is_extra_container = extra_containers
            .iter()
            .any(|dir| format!("{dir}/") == *priority_prefix);

        for skill_md in &filtered {
            if !skill_md.starts_with(&full_prefix) {
                continue;
            }
            let rest = &skill_md[full_prefix.len()..];
            if rest.to_lowercase() == "skill.md" {
                if is_extra_container {
                    continue;
                }
                if seen.insert(skill_md.clone()) {
                    results.push(skill_md.clone());
                }
                continue;
            }
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() == 2 && parts[1].to_lowercase() == "skill.md" {
                if seen.insert(skill_md.clone()) {
                    results.push(skill_md.clone());
                }
                continue;
            }
            let skill_dirs = &parts[..parts.len().saturating_sub(1)];
            let has_ancestor_skill = (0..skill_dirs.len().saturating_sub(1)).any(|index| {
                let ancestor_path = skill_dirs[..index + 1].join("/");
                lower_skill_md_set
                    .contains(&format!("{full_prefix}{ancestor_path}/SKILL.md").to_lowercase())
            });
            if is_container
                && parts.len() >= 3
                && parts.len() <= skills_store::skills::DEFAULT_SKILL_CONTAINER_DEPTH + 1
                && parts.last().map(|p| p.to_lowercase() == "skill.md").unwrap_or(false)
                && skill_dirs.iter().all(|part| !SKIP_DIRS.contains(part))
                && !has_ancestor_skill
                && seen.insert(skill_md.clone()) {
                    results.push(skill_md.clone());
                }
        }
    }

    if !results.is_empty() {
        return results;
    }
    filtered
        .into_iter()
        .filter(|path| path.split('/').count() <= 6)
        .collect()
}

fn fetch_raw_content(owner_repo: &str, branch: &str, file_path: &str) -> Option<String> {
    let url = format!("https://raw.githubusercontent.com/{owner_repo}/{branch}/{file_path}");
    http::get_ok(&url, FETCH_TIMEOUT).ok().map(|response| response.text())
}

/// Parse the `paths` field of a skills.sh.json file (upstream
/// `parseSkillsShPaths`).
#[must_use]
pub fn parse_skills_sh_paths(content: &str) -> Vec<String> {
    let Ok(config) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(paths) = config.get("paths").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    let mut result: Vec<String> = Vec::new();
    for entry in paths {
        let Some(entry) = entry.as_str() else { continue };
        if !entry.starts_with("./") || entry.contains('\\') {
            continue;
        }
        let mut parts: Vec<&str> = Vec::new();
        for segment in entry.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        let normalized = parts.join("/");
        if normalized.is_empty() || normalized == ".." || normalized.starts_with("../") {
            continue;
        }
        if !result.contains(&normalized) {
            result.push(normalized);
        }
    }
    result
}

fn fetch_skills_sh_paths(owner_repo: &str, tree: &RepoTree, subpath: Option<&str>) -> Vec<String> {
    let root = match subpath {
        Some(subpath) => format!("{}/", subpath.trim_end_matches('/')),
        None => String::new(),
    };
    let config_path = format!("{root}skills.sh.json");
    if !tree.tree.iter().any(|entry| entry.kind == "blob" && entry.path == config_path) {
        return Vec::new();
    }
    fetch_raw_content(owner_repo, &tree.branch, &config_path)
        .map(|content| parse_skills_sh_paths(&content))
        .unwrap_or_default()
}

/// A skill resolved from blob storage.
#[derive(Clone)]
pub struct BlobSkill {
    /// Skill name (sanitized frontmatter name).
    pub name: String,
    /// Description.
    pub description: String,
    /// Raw SKILL.md content.
    pub raw_content: String,
    /// Snapshot files (`{ path, contents }`).
    pub files: Vec<(String, String)>,
    /// Local snapshot hash (upstream `snapshotHash`).
    pub snapshot_hash: String,
    /// Repo-relative SKILL.md path.
    pub repo_path: String,
    /// Frontmatter metadata.
    pub metadata: Option<serde_json::Value>,
}

/// The blob install result: skills + tree.
#[derive(Clone)]
pub struct BlobInstallResult {
    /// Resolved skills.
    pub skills: Vec<BlobSkill>,
    /// The fetched tree.
    #[allow(dead_code)] // read by update flows that re-fetch the tree
    pub tree: RepoTree,
}

fn url_encode_component(value: &str) -> String {
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

fn compute_snapshot_hash(files: &[(String, String)]) -> String {
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&(String, String)> = files.iter().collect();
    sorted.sort_by(|a, b| skills_store::locks::locale_compare(&a.0, &b.0));
    let mut hasher = Sha256::new();
    for (path, contents) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(contents.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn get_skill_folder_path(skill_md_path: &str) -> String {
    let lower = skill_md_path.to_lowercase();
    if lower.ends_with("/skill.md") {
        return skill_md_path[..skill_md_path.len() - 9].to_string();
    }
    if lower == "skill.md" {
        return String::new();
    }
    skill_md_path[..skill_md_path.len() - 9].to_string()
}

const SNAPSHOT_EXCLUDED_FILES: &[&str] = &["metadata.json"];
const SNAPSHOT_EXCLUDED_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];

fn is_installable_snapshot_path(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let Some(file_name) = parts.last() else { return false };
    if SNAPSHOT_EXCLUDED_FILES.contains(file_name) {
        return false;
    }
    parts[..parts.len() - 1].iter().all(|part| !SNAPSHOT_EXCLUDED_DIRS.contains(part))
}

fn has_complete_nested_snapshot(tree: &RepoTree, skill_md_path: &str, files: &[(String, String)]) -> bool {
    let folder_path = get_skill_folder_path(skill_md_path);
    if folder_path.is_empty() {
        return true;
    }
    let folder_prefix = format!("{folder_path}/");
    let snapshot_paths: std::collections::HashSet<&str> =
        files.iter().map(|(path, _)| path.as_str()).collect();
    tree.tree.iter().all(|entry| {
        if entry.kind != "blob" || !entry.path.starts_with(&folder_prefix) {
            return true;
        }
        let relative_path = &entry.path[folder_prefix.len()..];
        !is_installable_snapshot_path(relative_path) || snapshot_paths.contains(relative_path)
    })
}

/// Attempt a blob install; `None` means the caller must fall back to a clone
/// (upstream `tryBlobInstall`).
#[allow(clippy::too_many_lines)]
pub fn try_blob_install(
    owner_repo: &str,
    subpath: Option<&str>,
    skill_filter: Option<&str>,
    ref_: Option<&str>,
    include_internal: bool,
) -> Option<BlobInstallResult> {
    if ref_.is_some() {
        return None;
    }
    let tree = fetch_repo_tree(owner_repo, ref_)?;
    let extra_containers = fetch_skills_sh_paths(owner_repo, &tree, subpath);
    let mut skill_md_paths = find_skill_md_paths(&tree, subpath, &extra_containers);
    if skill_md_paths.is_empty() {
        return None;
    }

    if let Some(skill_filter) = skill_filter {
        let filter_slug = to_skill_slug(skill_filter);
        let by_folder: Vec<String> = skill_md_paths
            .iter()
            .filter(|path| {
                let parts: Vec<&str> = path.split('/').collect();
                parts.len() >= 2
                    && to_skill_slug(parts[parts.len() - 2]) == filter_slug
            })
            .cloned()
            .collect();
        if !by_folder.is_empty() {
            skill_md_paths = by_folder;
        }
    }

    /// (mdPath, name, description, content, metadata)
    type ParsedSkill = (String, String, String, String, Option<serde_json::Value>);
    let mut parsed: Vec<ParsedSkill> = Vec::new();
    for md_path in &skill_md_paths {
        let Some(content) = fetch_raw_content(owner_repo, &tree.branch, md_path) else {
            continue;
        };
        let fm = skills_store::frontmatter::parse_frontmatter(&content);
        let (Some(name), Some(description)) = (
            fm.data.get("name").and_then(|v| v.as_str()),
            fm.data.get("description").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let internal = fm
            .data
            .get("metadata")
            .and_then(|m| m.get("internal"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if internal && !include_internal {
            continue;
        }
        let safe_name = sanitize_metadata(name);
        let safe_description = sanitize_metadata(description);
        parsed.push((
            md_path.clone(),
            safe_name,
            safe_description,
            content,
            fm.data.get("metadata").filter(|m| m.is_object()).cloned(),
        ));
    }
    if parsed.is_empty() {
        return None;
    }

    let mut filtered = parsed;
    if let Some(skill_filter) = skill_filter {
        let filter_slug = to_skill_slug(skill_filter);
        let by_name: Vec<_> = filtered
            .iter()
            .filter(|(_, name, _, _, _)| to_skill_slug(name) == filter_slug)
            .cloned()
            .collect();
        if by_name.is_empty() {
            return None;
        }
        filtered = by_name;
    }

    let source = owner_repo.to_lowercase();
    type SnapshotDownload = Option<(Vec<(String, String)>, String)>;
    let mut downloads: Vec<(ParsedSkill, SnapshotDownload)> = Vec::new();
    for skill in &filtered {
        let slug = to_skill_slug(&skill.1);
        let (owner, repo) = source.split_once('/')?;
        let url = format!(
            "{}/api/download/{}/{}/{}",
            download_base_url(),
            url_encode_component(owner),
            url_encode_component(repo),
            url_encode_component(&slug)
        );
        let download = http::get_ok(&url, FETCH_TIMEOUT)
            .ok()
            .and_then(|response| response.json().ok())
            .and_then(|value| {
                let files: Vec<(String, String)> = value
                    .get("files")?
                    .as_array()?
                    .iter()
                    .map(|file| {
                        Some((
                            file.get("path")?.as_str()?.to_string(),
                            file.get("contents")?.as_str()?.to_string(),
                        ))
                    })
                    .collect::<Option<Vec<_>>>()?;
                let hash = value.get("hash").and_then(|h| h.as_str()).unwrap_or("").to_string();
                Some((files, hash))
            });
        downloads.push((skill.clone(), download));
    }
    if downloads.iter().any(|(_, download)| download.is_none()) {
        return None;
    }

    let mut blob_skills = Vec::new();
    for (skill, download) in downloads {
        let (files, _server_hash) = download.unwrap();
        let md_path = &skill.0;
        if !has_complete_nested_snapshot(&tree, md_path, &files) {
            return None;
        }
        let folder_path = get_skill_folder_path(md_path);
        let files = if folder_path.is_empty() {
            files
                .into_iter()
                .filter(|(path, _)| path.to_lowercase() == "skill.md")
                .collect()
        } else {
            files
        };
        let snapshot_hash = compute_snapshot_hash(&files);
        blob_skills.push(BlobSkill {
            name: skill.1,
            description: skill.2,
            raw_content: skill.3,
            files,
            snapshot_hash,
            repo_path: md_path.clone(),
            metadata: skill.4,
        });
    }
    Some(BlobInstallResult { skills: blob_skills, tree })
}


