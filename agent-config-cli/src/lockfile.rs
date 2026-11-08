//! The `agent.config.lock.json` lockfile: per-package provenance and the
//! exact items installed into which agents.
//!
//! Two lockfiles exist, one per scope: the project lockfile sits next to the
//! working directory (`./agent.config.lock.json`) and is meant to be
//! committed; the global lockfile lives under the user config dir. The CLI
//! writes only these files; everything inside harness config dirs stays
//! under the library's control.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diag::{codes, CliResult, Diag};

/// Project-scope lockfile name.
pub const LOCK_FILE: &str = "agent.config.lock.json";

/// Lockfile schema version.
pub const LOCK_VERSION: u32 = 1;

/// The whole lockfile.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    /// Schema version; currently always `1`.
    pub version: u32,
    /// Installed packages keyed by package name.
    #[serde(default)]
    pub packages: BTreeMap<String, LockPackage>,
}

/// One installed package.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockPackage {
    /// Source string as the user typed it (normalized).
    pub source: String,
    /// `git` or `local`.
    pub source_type: String,
    /// Clone URL for git sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Pinned ref, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ref: Option<String>,
    /// Resolved commit SHA for git sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Repo subpath, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    /// `--instruction-file` used at install time, so `update` can replay the
    /// same manifest-less mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_file: Option<String>,
    /// SHA-256 over every file the package contributed.
    pub content_hash: String,
    /// RFC 3339 timestamp of the last install.
    pub installed_at: String,
    /// Scope this entry was installed under: `local` or `global`.
    pub scope: String,
    /// `--agent` filter recorded from the original install (replayed by
    /// `update`).
    #[serde(default)]
    pub agent_filter: Vec<String>,
    /// `--only` surfaces recorded from the original install.
    #[serde(default)]
    pub only: Vec<String>,
    /// Concrete items installed per agent.
    pub items: Vec<LockItem>,
}

/// One installed (surface, name, agent) triple.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct LockItem {
    /// Surface kind: `hook`, `instruction`, `mcp`, or `skill`.
    pub surface: String,
    /// Hook tag / instruction name / MCP name / skill name.
    pub name: String,
    /// Agent id from the registry.
    pub agent: String,
}

/// Resolve the lockfile path for a scope.
pub fn lockfile_path(global: bool) -> CliResult<PathBuf> {
    if global {
        let base = agent_config::paths::config_dir().map_err(|e| {
            Diag::new(
                codes::IO,
                format!("cannot resolve the user config dir for the global lockfile: {e}"),
            )
        })?;
        Ok(base.join("agentconfig").join("lock.json"))
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(LOCK_FILE))
            .map_err(|e| {
                Diag::new(
                    codes::IO,
                    format!("cannot resolve the working directory: {e}"),
                )
            })
    }
}

/// Load a lockfile. A missing file reads as an empty lockfile; a malformed
/// one is an error.
pub fn load(path: &Path) -> CliResult<Lockfile> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Lockfile {
                version: LOCK_VERSION,
                packages: BTreeMap::new(),
            });
        }
        Err(e) => {
            return Err(Diag::new(
                codes::LOCK_INVALID,
                format!("cannot read {}: {e}", path.display()),
            ))
        }
    };
    let lock: Lockfile = serde_json::from_str(&text).map_err(|e| {
        Diag::with_fix(
            codes::LOCK_INVALID,
            format!("{} is not a valid lockfile: {e}", path.display()),
            "delete the lockfile and re-run `agentconfig add` for your packages",
        )
    })?;
    Ok(lock)
}

/// Persist a lockfile (temp write + rename, trailing newline).
pub fn save(path: &Path, lock: &Lockfile) -> CliResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            Diag::new(
                codes::IO,
                format!("cannot create {}: {e}", parent.display()),
            )
        })?;
    }
    let mut body = serde_json::to_string_pretty(lock).expect("lockfile serialization cannot fail");
    body.push('\n');
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)
        .map_err(|e| Diag::new(codes::IO, format!("cannot write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        Diag::new(
            codes::IO,
            format!("cannot finalize {}: {e}", path.display()),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Lockfile {
        let mut packages = BTreeMap::new();
        packages.insert(
            "acme".to_string(),
            LockPackage {
                source: "acme/toolkit".to_string(),
                source_type: "git".to_string(),
                url: Some("https://github.com/acme/toolkit".to_string()),
                git_ref: None,
                commit: Some("abc".to_string()),
                subpath: None,
                instruction_file: None,
                content_hash: "deadbeef".to_string(),
                installed_at: "2026-11-08T00:00:00Z".to_string(),
                scope: "local".to_string(),
                agent_filter: vec![],
                only: vec![],
                items: vec![
                    LockItem {
                        surface: "mcp".to_string(),
                        name: "ctx7".to_string(),
                        agent: "claude".to_string(),
                    },
                    LockItem {
                        surface: "hook".to_string(),
                        name: "guard".to_string(),
                        agent: "codex".to_string(),
                    },
                ],
            },
        );
        Lockfile {
            version: LOCK_VERSION,
            packages,
        }
    }

    #[test]
    fn round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.config.lock.json");
        save(&path, &sample()).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded, sample());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn missing_file_is_empty_lockfile() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("nope.json")).unwrap();
        assert!(loaded.packages.is_empty());
        assert_eq!(loaded.version, LOCK_VERSION);
    }

    #[test]
    fn malformed_lockfile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent.config.lock.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = load(&path).unwrap_err();
        assert_eq!(err.code, codes::LOCK_INVALID);
    }

    #[test]
    fn items_sort_deterministically() {
        let mut items = sample().packages.get("acme").unwrap().items.clone();
        items.reverse();
        items.sort();
        assert_eq!(items[0].surface, "hook");
        assert_eq!(items[1].surface, "mcp");
    }
}
