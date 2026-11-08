//! Lock files: the global `.skill-lock.json` (schema v3) and the project
//! `skills-lock.json` (schema v1), byte-compatible with the upstream CLI.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{diag, SkillsError};

const GLOBAL_LOCK_FILE: &str = ".skill-lock.json";
const GLOBAL_LOCK_VERSION: u32 = 3;
const LOCAL_LOCK_FILE: &str = "skills-lock.json";
const LOCAL_LOCK_VERSION: u32 = 1;

/// A successful placement of a skill within the lockfile's scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillPlacement {
    /// Harness registry ID.
    pub agent: String,
    /// Eve subagent, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<String>,
    /// Effective install mode.
    pub mode: String,
}

/// One entry of the global lock (`src/skill-lock.ts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillLockEntry {
    /// Normalized source identifier (e.g. `owner/repo`).
    pub source: String,
    /// Provider/source type (`github`, `gitlab`, `git`, `local`, `well-known`, `download`).
    #[serde(rename = "sourceType")]
    pub source_type: String,
    /// The original URL used to install the skill.
    #[serde(rename = "sourceUrl")]
    pub source_url: String,
    /// Branch or tag ref used for installation.
    #[serde(
        rename = "ref",
        alias = "ref_",
        skip_serializing_if = "Option::is_none"
    )]
    pub ref_: Option<String>,
    /// Subpath within the source repo, if applicable.
    #[serde(rename = "skillPath", skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// GitHub tree SHA for the entire skill folder (empty string when unknown).
    #[serde(rename = "skillFolderHash")]
    pub skill_folder_hash: String,
    /// ISO timestamp when the skill was first installed.
    #[serde(rename = "installedAt")]
    pub installed_at: String,
    /// ISO timestamp when the skill was last updated.
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    /// Name of the plugin this skill belongs to (if any).
    #[serde(rename = "pluginName", skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    /// Base URL for well-known skills.
    #[serde(rename = "sourceBaseUrl", skip_serializing_if = "Option::is_none")]
    pub source_base_url: Option<String>,
    /// Digest recorded for well-known skills.
    #[serde(rename = "wellKnownDigest", skip_serializing_if = "Option::is_none")]
    pub well_known_digest: Option<String>,
    /// Set when the user chose copies over symlinks.
    #[serde(rename = "installMode", skip_serializing_if = "Option::is_none")]
    pub install_mode: Option<String>,
    /// Placements recorded by skillz. Older lockfiles can infer them from disk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<SkillPlacement>,
    /// Content fingerprint for clone or snapshot based update checks.
    #[serde(
        default,
        rename = "computedHash",
        skip_serializing_if = "Option::is_none"
    )]
    pub computed_hash: Option<String>,
}

/// Dismissed-prompt tracking in the global lock.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DismissedPrompts {
    /// Dismissed the find-skills installation prompt.
    #[serde(rename = "findSkillsPrompt", skip_serializing_if = "Option::is_none")]
    pub find_skills_prompt: Option<bool>,
}

/// The global lock file structure (`.skill-lock.json`, version 3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillLock {
    /// Schema version.
    pub version: u32,
    /// Map of skill name to its lock entry.
    pub skills: BTreeMap<String, SkillLockEntry>,
    /// Tracks dismissed prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed: Option<DismissedPrompts>,
    /// Last selected agents for installation.
    #[serde(rename = "lastSelectedAgents", skip_serializing_if = "Option::is_none")]
    pub last_selected_agents: Option<Vec<String>>,
}

impl Default for SkillLock {
    fn default() -> Self {
        Self {
            version: GLOBAL_LOCK_VERSION,
            skills: BTreeMap::new(),
            dismissed: Some(DismissedPrompts::default()),
            last_selected_agents: None,
        }
    }
}

/// One entry of the project lock (`src/local-lock.ts`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalSkillLockEntry {
    /// Where the skill came from: npm package name, `owner/repo`, local path, etc.
    pub source: String,
    /// Original remote URL, when source was normalized for lock readability.
    #[serde(rename = "sourceUrl", skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    /// Branch or tag ref used for installation.
    #[serde(
        rename = "ref",
        alias = "ref_",
        skip_serializing_if = "Option::is_none"
    )]
    pub ref_: Option<String>,
    /// The provider/source type.
    #[serde(rename = "sourceType")]
    pub source_type: String,
    /// Path to the skill's SKILL.md within the source repo.
    #[serde(rename = "skillPath", skip_serializing_if = "Option::is_none")]
    pub skill_path: Option<String>,
    /// Version of the npm package that shipped the skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Package whose package.json `skills` field requested the skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// SHA-256 hash computed from all files in the skill folder.
    #[serde(rename = "computedHash")]
    pub computed_hash: String,
    /// Eve subagent targets this skill was installed into (`''` = root agent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagents: Option<Vec<String>>,
    /// Digest recorded for well-known skills.
    #[serde(rename = "wellKnownDigest", skip_serializing_if = "Option::is_none")]
    pub well_known_digest: Option<String>,
    /// Set when the user chose copies over symlinks.
    #[serde(rename = "installMode", skip_serializing_if = "Option::is_none")]
    pub install_mode: Option<String>,
    /// Placements recorded by skillz. Older lockfiles can infer them from disk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub placements: Vec<SkillPlacement>,
}

/// The project lock file structure (`skills-lock.json`, version 1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LocalSkillLock {
    /// Schema version.
    pub version: u32,
    /// Map of skill name to its lock entry (sorted alphabetically on write).
    pub skills: BTreeMap<String, LocalSkillLockEntry>,
}

impl Default for LocalSkillLock {
    fn default() -> Self {
        Self {
            version: LOCAL_LOCK_VERSION,
            skills: BTreeMap::new(),
        }
    }
}

/// Path of the global skill lock: `$XDG_STATE_HOME/skills/.skill-lock.json`
/// when set, else `~/.agents/.skill-lock.json`.
#[must_use]
pub fn get_skill_lock_path() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(state).join("skills").join(GLOBAL_LOCK_FILE);
    }
    crate::paths::home_dir()
        .join(".agents")
        .join(GLOBAL_LOCK_FILE)
}

/// Path of the local (project) lock file for `cwd`.
#[must_use]
pub fn get_local_lock_path(cwd: &Path) -> PathBuf {
    cwd.join(LOCAL_LOCK_FILE)
}

/// Read the global skill lock; missing or older-format files read as empty.
pub fn read_skill_lock() -> Result<SkillLock, SkillsError> {
    read_skill_lock_at(&get_skill_lock_path())
}

/// Read the global lock from an explicit path (testing).
pub fn read_skill_lock_at(path: &Path) -> Result<SkillLock, SkillsError> {
    let invalid = |detail: &str| diag::invalid_local_lock(&path.to_string_lossy(), detail);
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(SkillLock::default()),
        Err(err) => return Err(invalid(&err.to_string())),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&content).map_err(|err| invalid(&err.to_string()))?;
    let version = parsed.get("version").and_then(|v| v.as_u64());
    let has_skills = parsed.get("skills").is_some_and(|s| s.is_object());
    if version.is_none() || !has_skills {
        return Err(invalid("expected a version and skills object"));
    }
    // Old formats are wiped (v3 adds skillFolderHash; fresh installs repopulate).
    if version.is_none_or(|v| v < GLOBAL_LOCK_VERSION as u64) {
        return Ok(SkillLock::default());
    }
    serde_json::from_value::<SkillLock>(parsed)
        .map_err(|err| diag::invalid_local_lock(&path.to_string_lossy(), &err.to_string()))
}

/// Write the global skill lock (pretty-printed, 2-space indent).
pub fn write_skill_lock(lock: &SkillLock) -> Result<(), SkillsError> {
    write_skill_lock_at(&get_skill_lock_path(), lock)
}

/// Write the global lock to an explicit path (testing).
pub fn write_skill_lock_at(path: &Path, lock: &SkillLock) -> Result<(), SkillsError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            SkillsError::new(
                "SKILLS_IO",
                format!("failed to create {}: {err}", parent.display()),
            )
        })?;
    }
    let content = serde_json::to_string_pretty(lock)
        .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
    write_atomic(path, &content)
}

/// Read the local (project) lock. Missing files read as empty; unusable files
/// are an error (upstream `SKILLS_INVALID_LOCAL_LOCK`).
pub fn read_local_lock(cwd: &Path) -> Result<LocalSkillLock, SkillsError> {
    read_local_lock_at(&get_local_lock_path(cwd), cwd)
}

/// Read the local lock from an explicit path (testing).
pub fn read_local_lock_at(path: &Path, lock_dir: &Path) -> Result<LocalSkillLock, SkillsError> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LocalSkillLock::default());
        }
        Err(err) => {
            return Err(diag::invalid_local_lock(
                &path.to_string_lossy(),
                &err.to_string(),
            ));
        }
    };
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|err| diag::invalid_local_lock(&path.to_string_lossy(), &err.to_string()))?;
    let shape_ok = parsed.is_object()
        && parsed.get("version").is_some_and(|v| v.is_number())
        && parsed.get("skills").is_some_and(|s| s.is_object());
    if !shape_ok {
        return Err(diag::invalid_local_lock(
            &path.to_string_lossy(),
            "expected an object with \"version\" and \"skills\"",
        ));
    }
    let version = parsed["version"].as_u64().unwrap_or(0);
    if version != LOCAL_LOCK_VERSION as u64 {
        return Err(diag::invalid_local_lock(
            &path.to_string_lossy(),
            &format!("unsupported lock version {version} (expected {LOCAL_LOCK_VERSION})"),
        ));
    }
    let mut lock: LocalSkillLock = serde_json::from_value(parsed)
        .map_err(|err| diag::invalid_local_lock(&path.to_string_lossy(), &err.to_string()))?;
    // Local sources are stored portable (`./rel`) and resolved on read.
    for entry in lock.skills.values_mut() {
        if entry.source_type == "local" && !Path::new(&entry.source).is_absolute() {
            // Lexical resolution strips `./` segments, like JS `resolve()`.
            entry.source = lexical_abs(&lock_dir.join(&entry.source))
                .to_string_lossy()
                .to_string();
        }
    }
    Ok(lock)
}

/// Write the local (project) lock: entries sorted by name, local sources made
/// portable, trailing newline.
pub fn write_local_lock(lock: &LocalSkillLock, cwd: &Path) -> Result<(), SkillsError> {
    write_local_lock_at(&get_local_lock_path(cwd), lock, cwd)
}

/// Write the local lock to an explicit path (testing).
pub fn write_local_lock_at(
    path: &Path,
    lock: &LocalSkillLock,
    lock_dir: &Path,
) -> Result<(), SkillsError> {
    let mut portable = lock.clone();
    for entry in portable.skills.values_mut() {
        if entry.source_type == "local" {
            entry.source = get_portable_local_source(&entry.source, lock_dir);
        }
    }
    let content = serde_json::to_string_pretty(&portable)
        .map_err(|err| SkillsError::new("SKILLS_IO", err.to_string()))?;
    write_atomic(path, &(content + "\n"))
}

fn write_atomic(path: &Path, content: &str) -> Result<(), SkillsError> {
    use std::io::Write as _;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let error = |err| {
        SkillsError::new(
            "SKILLS_IO",
            format!("failed to write {}: {err}", path.display()),
        )
    };
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    temp.write_all(content.as_bytes()).map_err(error)?;
    temp.as_file().sync_all().map_err(error)?;
    temp.persist(path).map_err(|err| error(err.error))?;
    Ok(())
}

fn lexical_abs(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
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

fn get_portable_local_source(source: &str, lock_dir: &Path) -> String {
    let absolute = Path::new(source);
    let absolute = if absolute.is_absolute() {
        absolute.to_path_buf()
    } else {
        lock_dir.join(absolute)
    };
    let Ok(relative) = absolute.strip_prefix(lock_dir) else {
        return absolute.to_string_lossy().replace('\\', "/");
    };
    let portable = relative.to_string_lossy().replace('\\', "/");
    if portable.is_empty() {
        ".".to_string()
    } else if portable == ".." || portable.starts_with("../") {
        portable
    } else {
        format!("./{portable}")
    }
}

/// SHA-256 over every file in `skill_dir`, sorted by relative path
/// (`computeSkillFolderHash`). `.git` and `node_modules` directories are
/// skipped; paths are `/`-separated and hashed together with file contents.
pub fn compute_folder_hash(skill_dir: &Path) -> Result<String, SkillsError> {
    use sha2::{Digest, Sha256};
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    collect_files(skill_dir, skill_dir, &mut files)?;
    files.sort_by(|(a, _), (b, _)| locale_compare(a, b));
    let mut hasher = Sha256::new();
    for (relative_path, content) in files {
        hasher.update(relative_path.as_bytes());
        hasher.update(&content);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_files(
    base_dir: &Path,
    current_dir: &Path,
    results: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), SkillsError> {
    let entries = std::fs::read_dir(current_dir).map_err(|err| {
        SkillsError::new("SKILLS_IO", format!("{}: {err}", current_dir.display()))
    })?;
    for entry in entries.flatten() {
        let full_path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            if name == ".git" || name == "node_modules" {
                continue;
            }
            collect_files(base_dir, &full_path, results)?;
        } else if file_type.is_file() {
            let mut content = Vec::new();
            std::fs::File::open(&full_path)
                .and_then(|mut f| f.read_to_end(&mut content))
                .map_err(|err| {
                    SkillsError::new("SKILLS_IO", format!("{}: {err}", full_path.display()))
                })?;
            let relative = full_path
                .strip_prefix(base_dir)
                .unwrap_or(&full_path)
                .to_string_lossy()
                .replace('\\', "/");
            results.push((relative, content));
        }
    }
    Ok(())
}

/// Approximate JS `localeCompare` ordering: base letters compare first, then
/// case (`a` < `A` < `b` < `B`).
#[must_use]
pub fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a_chars = a.chars();
    let mut b_chars = b.chars();
    loop {
        match (a_chars.next(), b_chars.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => {
                let lx = x.to_lowercase().next().unwrap_or(x);
                let ly = y.to_lowercase().next().unwrap_or(y);
                if lx != ly {
                    return lx.cmp(&ly);
                }
                if x != y {
                    return x.cmp(&y);
                }
            }
        }
    }
}

/// Why recording `entry` for `skill_name` would change the lock, or `None`
/// when it matches (drives `--frozen-lockfile`).
#[must_use]
pub fn lock_change_reason(
    lock: &LocalSkillLock,
    skill_name: &str,
    entry: (&str, Option<&str>),
) -> Option<String> {
    let (computed_hash, version) = entry;
    let Some(locked) = lock.skills.get(skill_name) else {
        return Some(format!("{skill_name} is not in skills-lock.json"));
    };
    if locked.computed_hash != computed_hash {
        return Some(format!(
            "{skill_name} has changed since skills-lock.json was written"
        ));
    }
    if locked.version.as_deref() != version {
        return Some(format!(
            "{skill_name} comes from version {}, skills-lock.json has {}",
            version.unwrap_or("unknown"),
            locked.version.as_deref().unwrap_or("unknown")
        ));
    }
    None
}

/// The `--frozen-lockfile` refusal message.
#[must_use]
pub fn frozen_lockfile_error(changes: &[String]) -> String {
    let mut out = String::from(
        "Cannot install with --frozen-lockfile because skills-lock.json is not up to date:",
    );
    for change in changes {
        out.push_str(&format!("\n  {change}"));
    }
    out.push_str("\nRun again with --no-frozen-lockfile to update it.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn global_lock_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".skill-lock.json");
        let mut lock = SkillLock::default();
        lock.skills.insert(
            "my-skill".into(),
            SkillLockEntry {
                source: "owner/repo".into(),
                source_type: "github".into(),
                source_url: "https://github.com/owner/repo.git".into(),
                ref_: None,
                skill_path: Some("skills/my-skill/SKILL.md".into()),
                skill_folder_hash: "abc".into(),
                installed_at: "2026-11-07T00:00:00.000Z".into(),
                updated_at: "2026-11-07T00:00:00.000Z".into(),
                plugin_name: None,
                source_base_url: None,
                well_known_digest: None,
                install_mode: None,
                placements: vec![],
                computed_hash: None,
            },
        );
        write_skill_lock_at(&path, &lock).unwrap();
        let read = read_skill_lock_at(&path).unwrap();
        assert_eq!(read, lock);
    }

    #[test]
    fn old_global_lock_versions_wipe() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".skill-lock.json");
        std::fs::write(&path, r#"{"version":2,"skills":{"a":{}}}"#).unwrap();
        let lock = read_skill_lock_at(&path).unwrap();
        assert!(lock.skills.is_empty());
        assert_eq!(lock.version, 3);
    }

    #[test]
    fn local_lock_invalid_version_errors() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("skills-lock.json");
        std::fs::write(&path, r#"{"version":2,"skills":{}}"#).unwrap();
        let err = read_local_lock_at(&path, tmp.path()).unwrap_err();
        assert_eq!(err.code, "SKILLS_INVALID_LOCAL_LOCK");
        assert!(err.why.contains("unsupported lock version 2 (expected 1)"));
    }

    #[test]
    fn local_lock_missing_is_empty() {
        let tmp = TempDir::new().unwrap();
        let lock = read_local_lock(tmp.path()).unwrap();
        assert!(lock.skills.is_empty());
    }

    #[test]
    fn local_lock_portable_sources() {
        let tmp = TempDir::new().unwrap();
        let mut lock = LocalSkillLock::default();
        lock.skills.insert(
            "s".into(),
            LocalSkillLockEntry {
                source: "./vendor/skill".into(),
                source_url: None,
                ref_: None,
                source_type: "local".into(),
                skill_path: None,
                version: None,
                via: None,
                computed_hash: "h".into(),
                subagents: None,
                well_known_digest: None,
                install_mode: None,
                placements: vec![],
            },
        );
        write_local_lock(&lock, tmp.path()).unwrap();
        let content = std::fs::read_to_string(tmp.path().join("skills-lock.json")).unwrap();
        assert!(content.ends_with('\n'));
        let read = read_local_lock(tmp.path()).unwrap();
        assert_eq!(
            Path::new(&read.skills["s"].source),
            tmp.path().join("vendor/skill")
        );
    }

    #[test]
    fn folder_hash_is_stable_and_path_sensitive() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("skill");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("SKILL.md"), "hello").unwrap();
        std::fs::write(dir.join("sub").join("b.txt"), "b").unwrap();
        let h1 = compute_folder_hash(&dir).unwrap();
        let h2 = compute_folder_hash(&dir).unwrap();
        assert_eq!(h1, h2);
        std::fs::write(dir.join("SKILL.md"), "changed").unwrap();
        let h3 = compute_folder_hash(&dir).unwrap();
        assert_ne!(h1, h3);
        // Deterministic hex output.
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn lock_change_reasons() {
        let mut lock = LocalSkillLock::default();
        lock.skills.insert(
            "a".into(),
            LocalSkillLockEntry {
                source: "x".into(),
                source_url: None,
                ref_: None,
                source_type: "github".into(),
                skill_path: None,
                version: Some("1.0.0".into()),
                via: None,
                computed_hash: "h1".into(),
                subagents: None,
                well_known_digest: None,
                install_mode: None,
                placements: vec![],
            },
        );
        assert_eq!(
            lock_change_reason(&lock, "b", ("h1", None)).as_deref(),
            Some("b is not in skills-lock.json")
        );
        assert_eq!(
            lock_change_reason(&lock, "a", ("h2", None)).as_deref(),
            Some("a has changed since skills-lock.json was written")
        );
        assert!(lock_change_reason(&lock, "a", ("h1", Some("1.0.0"))).is_none());
    }
}
