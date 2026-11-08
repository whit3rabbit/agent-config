//! RFC 8615 well-known skill discovery, ported from
//! `src/providers/wellknown.ts`.

use std::time::Duration;

use skills_store::error::{diag, SkillsError};
use skills_store::sanitize::sanitize_metadata;

use crate::archive::{read_tar_gz, read_zip_archive, ArchiveLimits};

const DISCOVERY_SCHEMA_V2: &str = "https://schemas.agentskills.io/discovery/0.2.0/schema.json";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
const WELL_KNOWN_PATHS: [&str; 2] = [".well-known/agent-skills", ".well-known/skills"];

/// A skill fetched from a well-known endpoint.
#[allow(dead_code)] // source_url/is_v2/description mirror the upstream shape
#[derive(Debug, Clone)]
pub struct WellKnownSkill {
    /// Frontmatter name (sanitized).
    pub name: String,
    /// Frontmatter description (sanitized).
    pub description: String,
    /// Full SKILL.md content including frontmatter.
    pub content: String,
    /// The index entry name (used as the install directory).
    pub install_name: String,
    /// URL the SKILL.md / artifact came from.
    pub source_url: String,
    /// Frontmatter metadata object.
    pub metadata: Option<serde_json::Value>,
    /// All files, keyed by relative path (`SKILL.md` is UTF-8 text).
    pub files: Vec<(String, FileContent)>,
    /// Digest recorded in the index (v2 entries).
    pub index_digest: Option<String>,
    /// Whether the entry came from a v0.2.0 index.
    pub is_v2: bool,
}

/// File content: text or binary.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum FileContent {
    /// UTF-8 text.
    Text(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
}

impl FileContent {
    /// The raw bytes of the file content.
    #[must_use]
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            FileContent::Text(text) => text.clone().into_bytes(),
            FileContent::Bytes(bytes) => bytes.clone(),
        }
    }
}

#[allow(dead_code)]
enum NormalizedEntry {
    V1 {
        name: String,
        description: String,
        files: Vec<String>,
        base_url: String,
        well_known_path: String,
    },
    V2 {
        name: String,
        description: String,
        artifact_url: String,
        digest: String,
    },
}

struct Candidate {
    entries: Vec<NormalizedEntry>,
    resolved_base_url: String,
}

fn url_join(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    if let Some((scheme_rest, _)) = base.split_once("://") {
        let _ = scheme_rest;
    }
    // Resolve against the base's directory when relative.
    if relative.starts_with('/') {
        if let Some(scheme_end) = base.find("://") {
            let authority_end = base[scheme_end + 3..].find('/').map(|i| i + scheme_end + 3);
            if let Some(end) = authority_end {
                return format!("{}{}", &base[..end], relative);
            }
        }
        return relative.to_string();
    }
    let base_dir = base.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(base);
    format!("{base_dir}/{relative}")
}

fn hostname_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority.split('@').next_back().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    Some(host.to_string())
}

fn is_valid_skill_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

fn is_safe_legacy_file_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains("..")
        && !path.contains('\0')
}

fn normalize_index(
    raw: &serde_json::Value,
    index_url: &str,
    well_known_path: &str,
) -> Option<Vec<NormalizedEntry>> {
    let skills = raw.get("skills")?.as_array()?;
    let schema = raw.get("$schema").and_then(|s| s.as_str());
    if schema == Some(DISCOVERY_SCHEMA_V2) {
        let mut entries = Vec::new();
        for entry in skills {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else { continue };
            let Some(description) = entry.get("description").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(kind) = entry.get("type").and_then(|v| v.as_str()) else { continue };
            if kind != "skill-md" && kind != "archive" {
                continue;
            }
            let Some(url) = entry.get("url").and_then(|v| v.as_str()) else { continue };
            let Some(digest) = entry.get("digest").and_then(|v| v.as_str()) else { continue };
            if !is_valid_skill_name(name)
                || description.is_empty()
                || description.len() > 1024
                || url.is_empty()
                || !digest.starts_with("sha256:")
                || digest.len() != 7 + 64
                || !digest[7..].chars().all(|c| c.is_ascii_hexdigit())
            {
                continue;
            }
            entries.push(NormalizedEntry::V2 {
                name: name.to_string(),
                description: description.to_string(),
                artifact_url: url_join(index_url, url),
                digest: digest.to_string(),
            });
        }
        if entries.is_empty() {
            return None;
        }
        return Some(entries);
    }
    if schema.is_some() {
        return None;
    }
    // Legacy v1: all-or-nothing validation.
    let mut entries = Vec::new();
    for entry in skills {
        let name = entry.get("name").and_then(|v| v.as_str())?;
        let description = entry.get("description").and_then(|v| v.as_str())?;
        let files: Vec<String> = entry
            .get("files")?
            .as_array()?
            .iter()
            .map(|f| f.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()?;
        if !is_valid_skill_name(name)
            || description.is_empty()
            || files.is_empty()
            || !files.iter().all(|f| is_safe_legacy_file_path(f))
            || !files.iter().any(|f| f.to_lowercase() == "skill.md")
        {
            return None;
        }
        let base_url = legacy_skill_base_url(index_url, well_known_path);
        entries.push(NormalizedEntry::V1 {
            name: name.to_string(),
            description: description.to_string(),
            files,
            base_url,
            well_known_path: well_known_path.to_string(),
        });
    }
    Some(entries)
}

fn legacy_skill_base_url(index_url: &str, well_known_path: &str) -> String {
    let marker = format!("/{well_known_path}/index.json");
    index_url
        .rfind(&marker)
        .map(|index| index_url[..index].to_string())
        .unwrap_or_else(|| index_url.to_string())
}

fn fetch_index_candidates(base_url: &str, update_check: bool) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let Some(parsed) = split_origin_and_path(base_url) else {
        return candidates;
    };
    let base_path = parsed.1.trim_end_matches('/').to_string();
    let mut urls_to_try: Vec<(String, String)> = Vec::new();
    for well_known_path in WELL_KNOWN_PATHS {
        // Path-relative first, then the host root fallback.
        urls_to_try.push((
            format!("{}{}/{}/index.json", parsed.0, base_path, well_known_path),
            format!("{}{}", parsed.0, base_path),
        ));
        if !base_path.is_empty() {
            urls_to_try.push((
                format!("{}/{}/index.json", parsed.0, well_known_path),
                parsed.0.clone(),
            ));
        }
    }
    for (index_url, resolved_base) in urls_to_try {
        let mut headers: Vec<(&str, &str)> = Vec::new();
        if update_check {
            headers.push(("X-Skills-Update-Check", "1"));
        }
        let Ok(response) = crate::http::get(&index_url, DISCOVERY_TIMEOUT, &headers) else {
            continue;
        };
        if response.status < 200 || response.status >= 300 {
            continue;
        }
        let Ok(raw) = response.json() else { continue };
        let Some(entries) = normalize_index(&raw, &index_url, well_known_path_of(&index_url))
        else {
            continue;
        };
        candidates.push(Candidate { entries, resolved_base_url: resolved_base });
        if !candidates.is_empty() {
            // Upstream collects all candidates but fetchIndex()/fetchAllSkills
            // only use the first; stop here for identical behavior.
            break;
        }
    }
    candidates
}

fn well_known_path_of(index_url: &str) -> &'static str {
    if index_url.contains("/.well-known/agent-skills/") {
        WELL_KNOWN_PATHS[0]
    } else {
        WELL_KNOWN_PATHS[1]
    }
}

fn split_origin_and_path(url: &str) -> Option<(String, String)> {
    let (scheme, rest) = url.split_once("://")?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    Some((format!("{scheme}://{authority}"), path.to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn fetch_v1(entry: &NormalizedEntry) -> Option<WellKnownSkill> {
    let NormalizedEntry::V1 { name, description: _, files, base_url, well_known_path } = entry else {
        return None;
    };
    let skill_base_url = format!(
        "{}/{}/{name}",
        base_url.trim_end_matches('/'),
        well_known_path
    );
    let skill_md_url = format!("{skill_base_url}/SKILL.md");
    let response = crate::http::get_ok(&skill_md_url, DISCOVERY_TIMEOUT).ok()?;
    let content = response.text();
    let fm = skills_store::frontmatter::parse_frontmatter(&content);
    let skill_name = fm.data.get("name")?.as_str()?.to_string();
    let skill_description = fm.data.get("description")?.as_str()?.to_string();

    let mut out_files: Vec<(String, FileContent)> = vec![("SKILL.md".to_string(), FileContent::Text(content.clone()))];
    let other_files: Vec<&String> = files
        .iter()
        .filter(|f| f.to_lowercase() != "skill.md")
        .collect();
    for file_path in other_files {
        let file_url = format!("{skill_base_url}/{file_path}");
        if let Ok(response) = crate::http::get_ok(&file_url, DISCOVERY_TIMEOUT) {
            out_files.push((file_path.clone(), FileContent::Bytes(response.body)));
        }
    }

    Some(WellKnownSkill {
        name: sanitize_metadata(&skill_name),
        description: sanitize_metadata(&skill_description),
        content,
        install_name: name.clone(),
        source_url: skill_md_url,
        metadata: fm.data.get("metadata").filter(|m| m.is_object()).cloned(),
        files: out_files,
        index_digest: None,
        is_v2: false,
    })
}

fn fetch_v2(entry: &NormalizedEntry) -> Result<Option<WellKnownSkill>, SkillsError> {
    let NormalizedEntry::V2 { name, description: _, artifact_url, digest } = entry else {
        return Ok(None);
    };
    let response = crate::http::get_ok(artifact_url, DISCOVERY_TIMEOUT)?;
    let content_type = response.header("content-type").unwrap_or("").to_string();
    let bytes = response.body;
    let actual_digest = format!("sha256:{}", sha256_hex(&bytes));
    if actual_digest != *digest {
        return Err(diag::wellknown_digest_mismatch(name, digest, &actual_digest));
    }

    let files: Vec<(String, FileContent)> = if is_zip_bytes(&bytes, artifact_url, &content_type) {
        let map = read_zip_archive(
            &bytes,
            ArchiveLimits { max_extracted_bytes: 50 * 1024 * 1024, max_entries: 1000 },
        )?;
        map.into_iter()
            .map(|(path, data)| (path, FileContent::Bytes(data)))
            .collect()
    } else if is_tar_gz_bytes(&bytes, artifact_url, &content_type) {
        read_tar_gz(&bytes)?
            .into_iter()
            .map(|(path, data)| (path, FileContent::Bytes(data)))
            .collect()
    } else {
        return Err(diag::archive_unsupported_format());
    };

    let Some((_, skill_md)) = files.iter().find(|(path, _)| path == "SKILL.md") else {
        return Ok(None);
    };
    let content = match skill_md {
        FileContent::Text(text) => text.clone(),
        FileContent::Bytes(bytes) => String::from_utf8_lossy(bytes).to_string(),
    };
    let fm = skills_store::frontmatter::parse_frontmatter(&content);
    let Some(skill_name) = fm.data.get("name").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let Some(skill_description) = fm.data.get("description").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    Ok(Some(WellKnownSkill {
        name: sanitize_metadata(skill_name),
        description: sanitize_metadata(skill_description),
        content,
        install_name: name.clone(),
        source_url: artifact_url.clone(),
        metadata: fm.data.get("metadata").filter(|m| m.is_object()).cloned(),
        files: normalize_file_texts(files),
        index_digest: Some(digest.clone()),
        is_v2: true,
    }))
}

fn normalize_file_texts(files: Vec<(String, FileContent)>) -> Vec<(String, FileContent)> {
    files
        .into_iter()
        .map(|(path, content)| {
            if path.to_lowercase() == "skill.md" {
                match content {
                    FileContent::Bytes(bytes) => {
                        (path, FileContent::Text(String::from_utf8_lossy(&bytes).to_string()))
                    }
                    text => (path, text),
                }
            } else {
                (path, content)
            }
        })
        .collect()
}

fn is_zip_bytes(bytes: &[u8], url: &str, content_type: &str) -> bool {
    content_type.contains("application/zip")
        || url.to_lowercase().ends_with(".zip")
        || (bytes.len() >= 2 && bytes[0] == 0x50 && bytes[1] == 0x4b)
}

fn is_tar_gz_bytes(bytes: &[u8], url: &str, content_type: &str) -> bool {
    content_type.contains("application/gzip")
        || content_type.contains("application/x-gzip")
        || url.to_lowercase().ends_with(".tar.gz")
        || url.to_lowercase().ends_with(".tgz")
        || (bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b)
}

/// Fetch every skill from a well-known endpoint (upstream `fetchAllSkills`).
pub fn fetch_all_skills(url: &str, include_internal: bool) -> Result<Vec<WellKnownSkill>, SkillsError> {
    let candidates = fetch_index_candidates(url, false);
    let scope = scope_of(url);
    let scoped: Vec<&Candidate> = match &scope {
        Some((_, root_base)) => candidates
            .iter()
            .filter(|candidate| candidate.resolved_base_url != *root_base)
            .collect(),
        None => candidates.iter().collect(),
    };
    let include_internal = include_internal
        || matches!(
            std::env::var("INSTALL_INTERNAL_SKILLS").ok().as_deref(),
            Some("1") | Some("true")
        );

    for candidate in &scoped {
        let mut skills = Vec::new();
        let mut digest_error: Option<SkillsError> = None;
        for entry in &candidate.entries {
            let fetched = match entry {
                NormalizedEntry::V1 { .. } => fetch_v1(entry),
                NormalizedEntry::V2 { .. } => match fetch_v2(entry) {
                    Ok(skill) => skill,
                    Err(err) => {
                        if err.code == "SKILLS_WELLKNOWN_DIGEST_MISMATCH" {
                            digest_error = Some(err);
                        }
                        None
                    }
                },
            };
            if let Some(skill) = fetched {
                let internal = skill
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("internal"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !internal || include_internal {
                    skills.push(skill);
                }
            }
        }
        if let Some(err) = digest_error {
            return Err(err);
        }
        if !skills.is_empty() {
            return Ok(skills);
        }
    }

    if let Some((scope_path, root_base)) = scope {
        if scoped.len() < candidates.len() {
            return Err(diag::wellknown_scope_not_found(&scope_path, &root_base));
        }
    }
    Ok(Vec::new())
}

fn scope_of(url: &str) -> Option<(String, String)> {
    let (origin, path) = split_origin_and_path(url)?;
    let trimmed = path.trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return None;
    }
    Some((trimmed, origin))
}

/// The source identifier used in lock files and telemetry
/// (upstream `getSourceIdentifier`).
#[must_use]
#[allow(clippy::inline_fn_without_body)]
pub fn get_source_identifier(url: &str) -> String {
    hostname_of(url)
        .map(|host| host.strip_prefix("www.").unwrap_or(&host).to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Compute the well-known digest recorded in lock files
/// (upstream `computeWellKnownSkillDigest`).
#[must_use]
pub fn compute_well_known_skill_digest(skill: &WellKnownSkill) -> String {
    if let Some(digest) = &skill.index_digest {
        return digest.clone();
    }
    use sha2::{Digest, Sha256};
    let mut sorted: Vec<&(String, FileContent)> = skill.files.iter().collect();
    sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut hasher = Sha256::new();
    for (path, content) in sorted {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(content.as_bytes());
        hasher.update([0]);
    }
    format!("sha256:{:x}", hasher.finalize())
}
