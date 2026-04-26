//! Sidecar JSON ledger that records which consumer owns which MCP server
//! (or skill) installed by `agent-config`.
//!
//! The harness configs themselves (`mcp.json`, `config.toml`, etc.) are
//! caller-visible files we share with both the user and other consumers, so we
//! cannot safely embed `_agent_config_tag`-style markers inside every entry —
//! some harnesses reject unknown keys, and the TOML/JSON-array variants do
//! not all round-trip extra fields cleanly.
//!
//! Instead, every install records `{ "name": { "owner": "<tag>", "content_hash": "<sha256-hex>" } }`
//! in a sibling file (e.g., `~/.cursor/.agent-config-mcp.json`). Uninstall
//! consults the ledger first; if the recorded owner differs from the caller
//! (or the entry exists in the harness config but not in the ledger), we
//! refuse with [`AgentConfigError::NotOwnedByCaller`] rather than risk clobbering
//! work this caller did not perform. If the content hash no longer matches
//! the config file on disk, we refuse with [`AgentConfigError::ConfigDrifted`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::error::AgentConfigError;
use crate::util::{file_lock, fs_atomic, json_patch};

/// Sidecar ledger filename, sibling to the harness's MCP config file.
const MCP_LEDGER_FILE: &str = ".agent-config-mcp.json";

/// Ledger path for an MCP-config file. Lives in the same directory as
/// `config_path` so it travels with the harness's config when copied or
/// scoped to a project.
pub(crate) fn mcp_ledger_for(config_path: &Path) -> PathBuf {
    let mut p = config_path.to_path_buf();
    p.set_file_name(MCP_LEDGER_FILE);
    p
}

/// Top-level shape of the ledger file.
const VERSION_KEY: &str = "version";
const ENTRIES_KEY: &str = "entries";
const OWNER_KEY: &str = "owner";
const CONTENT_HASH_KEY: &str = "content_hash";

/// Current ledger schema version. Bumped on incompatible changes.
const CURRENT_VERSION: u64 = 2;

/// Compute a SHA-256 hex digest of `data`.
pub(crate) fn content_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Read the file at `path` and return its SHA-256 hex digest.
/// Returns `None` if the file does not exist.
pub(crate) fn file_content_hash(path: &Path) -> Result<Option<String>, AgentConfigError> {
    // Pre-stat so a missing file remains `None` (not `Some(hash(""))`) — the
    // bounded reader returns `Vec::new()` for both missing and empty files.
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs_atomic::read_capped(path)?;
    Ok(Some(content_hash(&bytes)))
}

/// Record an install. Creates the ledger file if missing.
///
/// `content_hash` is an optional SHA-256 hex digest of the config file
/// content as it should appear after this install. When provided, it enables
/// drift detection on uninstall: if the config file no longer matches, the
/// caller receives [`AgentConfigError::ConfigDrifted`] instead of a silent backup
/// restore.
pub(crate) fn record_install(
    ledger_path: &Path,
    name: &str,
    owner: &str,
    content_hash: Option<&str>,
) -> Result<(), AgentConfigError> {
    fs_atomic::reject_symlink(ledger_path)?;
    file_lock::with_lock(ledger_path, || {
        let mut root = json_patch::read_or_empty(ledger_path)?;
        let entries = ensure_shape(&mut root);
        let mut entry = Map::new();
        entry.insert(OWNER_KEY.to_string(), Value::String(owner.to_string()));
        if let Some(hash) = content_hash {
            entry.insert(
                CONTENT_HASH_KEY.to_string(),
                Value::String(hash.to_string()),
            );
        }
        entries.insert(name.to_string(), Value::Object(entry));
        fs_atomic::write_atomic(ledger_path, &json_patch::to_pretty(&root), false)?;
        Ok(())
    })
}

/// Forget an entry. Returns the previously recorded owner, if any. Removes the
/// ledger file entirely when no entries remain.
pub(crate) fn record_uninstall(
    ledger_path: &Path,
    name: &str,
) -> Result<Option<String>, AgentConfigError> {
    fs_atomic::reject_symlink(ledger_path)?;
    file_lock::with_lock(ledger_path, || {
        let mut root = json_patch::read_or_empty(ledger_path)?;
        let prev_owner = {
            let Some(entries) = root.get_mut(ENTRIES_KEY).and_then(Value::as_object_mut) else {
                return Ok(None);
            };
            let removed = entries.remove(name);
            let owner = removed
                .as_ref()
                .and_then(|v| v.get(OWNER_KEY))
                .and_then(Value::as_str)
                .map(str::to_owned);
            if entries.is_empty() {
                // Drop the ledger entirely so a clean uninstall leaves no trace.
                fs_atomic::remove_if_exists(ledger_path)?;
                return Ok(owner);
            }
            owner
        };
        fs_atomic::write_atomic(ledger_path, &json_patch::to_pretty(&root), false)?;
        Ok(prev_owner)
    })
}

/// Look up the owner of `name` without mutating the ledger.
pub(crate) fn owner_of(ledger_path: &Path, name: &str) -> Result<Option<String>, AgentConfigError> {
    fs_atomic::reject_symlink(ledger_path)?;
    let root = json_patch::read_or_empty(ledger_path)?;
    Ok(root
        .get(ENTRIES_KEY)
        .and_then(Value::as_object)
        .and_then(|m| m.get(name))
        .and_then(|v| v.get(OWNER_KEY))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Strict, read-only ledger parse result for validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StrictLedgerRead {
    /// The ledger file is absent.
    Missing,
    /// The ledger file is present and has the expected schema.
    Valid {
        /// Map of ledger entry name to (owner tag, optional content hash).
        entries: BTreeMap<String, LedgerEntry>,
    },
    /// The ledger file is present but not valid ledger JSON.
    Malformed {
        /// Human-readable parse or shape error.
        reason: String,
    },
}

/// One entry parsed from the sidecar ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LedgerEntry {
    /// The consumer tag that owns this entry.
    pub owner: String,
    /// SHA-256 hex digest of the config file content at install time, if
    /// recorded (v2 ledgers). `None` for v1 ledgers.
    pub content_hash: Option<String>,
}

/// Read a ledger without repairing or normalizing it.
///
/// This is intentionally stricter than the mutating install/uninstall path:
/// validation must report malformed ledgers as drift and must never rewrite
/// them as a side effect of checking state.
pub(crate) fn read_strict(ledger_path: &Path) -> Result<StrictLedgerRead, AgentConfigError> {
    fs_atomic::reject_symlink(ledger_path)?;
    // Pre-stat so we can distinguish a missing ledger from a 0-byte ledger;
    // the bounded reader collapses both to `Vec::new()`.
    if !ledger_path.exists() {
        return Ok(StrictLedgerRead::Missing);
    }
    let bytes = fs_atomic::read_capped(ledger_path)?;
    if bytes.is_empty() || bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(StrictLedgerRead::Malformed {
            reason: "ledger file is empty".into(),
        });
    }

    let root: Value = match serde_json::from_slice(&bytes) {
        Ok(root) => root,
        Err(e) => {
            return Ok(StrictLedgerRead::Malformed {
                reason: e.to_string(),
            });
        }
    };

    let Some(root_obj) = root.as_object() else {
        return Ok(StrictLedgerRead::Malformed {
            reason: "ledger root must be a JSON object".into(),
        });
    };
    if !root_obj.get(VERSION_KEY).is_some_and(Value::is_number) {
        return Ok(StrictLedgerRead::Malformed {
            reason: "ledger version must be a number".into(),
        });
    }
    let Some(entries_obj) = root_obj.get(ENTRIES_KEY).and_then(Value::as_object) else {
        return Ok(StrictLedgerRead::Malformed {
            reason: "ledger entries must be an object".into(),
        });
    };

    let mut entries = BTreeMap::new();
    for (name, entry) in entries_obj {
        let Some(owner) = entry.get(OWNER_KEY).and_then(Value::as_str) else {
            return Ok(StrictLedgerRead::Malformed {
                reason: format!("ledger entry {name:?} must contain string owner"),
            });
        };
        let content_hash = entry
            .get(CONTENT_HASH_KEY)
            .and_then(Value::as_str)
            .map(str::to_owned);
        entries.insert(
            name.clone(),
            LedgerEntry {
                owner: owner.to_string(),
                content_hash,
            },
        );
    }

    Ok(StrictLedgerRead::Valid { entries })
}

/// True if `name` has any owner recorded in the ledger.
pub(crate) fn contains(ledger_path: &Path, name: &str) -> Result<bool, AgentConfigError> {
    Ok(owner_of(ledger_path, name)?.is_some())
}

/// Look up the content hash recorded for `name`, if any.
pub(crate) fn content_hash_of(
    ledger_path: &Path,
    name: &str,
) -> Result<Option<String>, AgentConfigError> {
    fs_atomic::reject_symlink(ledger_path)?;
    let root = json_patch::read_or_empty(ledger_path)?;
    Ok(root
        .get(ENTRIES_KEY)
        .and_then(Value::as_object)
        .and_then(|m| m.get(name))
        .and_then(|v| v.get(CONTENT_HASH_KEY))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Check whether the config file at `config_path` still matches the content
/// hash recorded in the ledger for `name`.
///
/// Returns `Ok(())` if:
/// - no hash was recorded (v1 ledger), or
/// - the hash matches the current file content, or
/// - the config file does not exist (already removed).
///
/// Returns [`AgentConfigError::ConfigDrifted`] if the hash is recorded but the
/// current file content differs.
///
/// Whole-file drift check used for skills (1:1 file per entry). MCP and
/// other multi-entry surfaces use [`check_entry_drift`] instead so a sibling
/// install does not invalidate earlier entries' hashes.
// Wired into `skills_dir::uninstall` in a follow-up commit (Task B4).
#[allow(dead_code)]
pub(crate) fn check_drift(
    ledger_path: &Path,
    name: &str,
    config_path: &Path,
) -> Result<(), AgentConfigError> {
    let expected = content_hash_of(ledger_path, name)?;
    let Some(expected_hash) = expected else {
        return Ok(());
    };
    let actual = file_content_hash(config_path)?;
    match actual {
        None => Ok(()),
        Some(actual_hash) if actual_hash == expected_hash => Ok(()),
        Some(_) => Err(AgentConfigError::ConfigDrifted {
            path: config_path.to_path_buf(),
        }),
    }
}

/// Check whether the canonical hash recorded for `name` matches the hash of
/// the supplied current entry bytes.
///
/// Returns `Ok(())` if no hash was recorded (v1 ledger, or entry installed
/// before per-entry hashing) or if the bytes hash to the recorded value.
/// Returns [`AgentConfigError::ConfigDrifted`] if the hashes differ.
///
/// `config_path` is included only for the error report; the comparison itself
/// is over the entry bytes supplied by the caller.
pub(crate) fn check_entry_drift(
    ledger_path: &Path,
    name: &str,
    config_path: &Path,
    current_entry_bytes: &[u8],
) -> Result<(), AgentConfigError> {
    let Some(expected) = content_hash_of(ledger_path, name)? else {
        return Ok(());
    };
    let actual = content_hash(current_entry_bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(AgentConfigError::ConfigDrifted {
            path: config_path.to_path_buf(),
        })
    }
}

/// Verify that `name` is owned by `expected`. Returns
/// [`AgentConfigError::NotOwnedByCaller`] otherwise. `kind` is a short label for
/// the error message (e.g. `"mcp server"`, `"skill"`).
///
/// The `present_in_config` flag must be `true` if the entry is also present
/// in the harness config; this is how we detect "user installed by hand"
/// (in config but not in ledger).
pub(crate) fn require_owner(
    ledger_path: &Path,
    name: &str,
    expected: &str,
    kind: &'static str,
    present_in_config: bool,
) -> Result<(), AgentConfigError> {
    let actual = owner_of(ledger_path, name)?;
    match (actual.as_deref(), present_in_config) {
        // Standard case: ledger recorded owner; must match.
        (Some(o), _) if o == expected => Ok(()),
        // Mismatch — recorded owner differs.
        (Some(_), _) => Err(AgentConfigError::NotOwnedByCaller {
            kind,
            name: name.into(),
            expected: expected.into(),
            actual,
        }),
        // No ledger entry but the harness config has it: user-installed.
        (None, true) => Err(AgentConfigError::NotOwnedByCaller {
            kind,
            name: name.into(),
            expected: expected.into(),
            actual: None,
        }),
        // Neither in ledger nor config: caller is removing nothing; allow.
        (None, false) => Ok(()),
    }
}

/// Coerce `root` to the canonical ledger shape and return a mutable reference
/// to the entries object. A hand-edited ledger with a non-object root, missing
/// keys, or a non-object `entries` value is silently repaired rather than
/// panicking — we already chose the ledger over harder-to-write markers
/// because users may touch it.
fn ensure_shape(root: &mut Value) -> &mut Map<String, Value> {
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let obj = root.as_object_mut().expect("just coerced to object");
    if !obj.get(VERSION_KEY).is_some_and(Value::is_number) {
        obj.insert(VERSION_KEY.into(), Value::from(CURRENT_VERSION));
    }
    if !obj.get(ENTRIES_KEY).is_some_and(Value::is_object) {
        obj.insert(ENTRIES_KEY.into(), Value::Object(Map::new()));
    }
    obj.get_mut(ENTRIES_KEY)
        .and_then(Value::as_object_mut)
        .expect("just coerced to object")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn run_two<A, B, FA, FB>(a: FA, b: FB) -> (A, B)
    where
        A: Send + 'static,
        B: Send + 'static,
        FA: FnOnce() -> A + Send + 'static,
        FB: FnOnce() -> B + Send + 'static,
    {
        let barrier = Arc::new(Barrier::new(3));
        let a_barrier = Arc::clone(&barrier);
        let b_barrier = Arc::clone(&barrier);
        let a_thread = thread::spawn(move || {
            a_barrier.wait();
            a()
        });
        let b_thread = thread::spawn(move || {
            b_barrier.wait();
            b()
        });
        barrier.wait();
        (
            a_thread.join().expect("first ledger writer panicked"),
            b_thread.join().expect("second ledger writer panicked"),
        )
    }

    fn ledger_path(dir: &Path) -> std::path::PathBuf {
        dir.join(".agent-config-mcp.json")
    }

    #[test]
    fn record_install_creates_ledger() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "myapp", None).unwrap();
        assert!(path.exists());
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn second_install_overwrites_owner() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "appA", None).unwrap();
        record_install(&path, "github", "appB", None).unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("appB"));
    }

    #[test]
    fn record_uninstall_returns_prior_owner() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "myapp", None).unwrap();
        let prev = record_uninstall(&path, "github").unwrap();
        assert_eq!(prev.as_deref(), Some("myapp"));
        assert_eq!(owner_of(&path, "github").unwrap(), None);
    }

    #[test]
    fn uninstall_removes_empty_ledger_file() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "alpha", "myapp", None).unwrap();
        record_uninstall(&path, "alpha").unwrap();
        assert!(!path.exists(), "ledger should be removed when empty");
    }

    #[test]
    fn uninstall_keeps_ledger_with_other_entries() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "alpha", "appA", None).unwrap();
        record_install(&path, "beta", "appB", None).unwrap();
        record_uninstall(&path, "alpha").unwrap();
        assert!(path.exists());
        assert_eq!(owner_of(&path, "beta").unwrap().as_deref(), Some("appB"));
    }

    #[test]
    fn uninstall_missing_entry_is_noop() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let prev = record_uninstall(&path, "ghost").unwrap();
        assert!(prev.is_none());
    }

    #[test]
    fn require_owner_passes_on_match() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "x", "myapp", None).unwrap();
        require_owner(&path, "x", "myapp", "mcp server", true).unwrap();
    }

    #[test]
    fn require_owner_fails_on_mismatch() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "x", "appA", None).unwrap();
        let err = require_owner(&path, "x", "appB", "mcp server", true).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::NotOwnedByCaller { actual: Some(ref a), .. } if a == "appA"
        ));
    }

    #[test]
    fn require_owner_fails_when_present_but_unrecorded() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let err = require_owner(&path, "x", "myapp", "mcp server", true).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn require_owner_passes_when_missing_from_both() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        require_owner(&path, "x", "myapp", "mcp server", false).unwrap();
    }

    #[test]
    fn hand_edited_ledger_with_non_object_entries_is_recovered() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        std::fs::write(&path, r#"{"version":1,"entries":null}"#).unwrap();
        record_install(&path, "github", "myapp", None).unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn hand_edited_non_object_root_is_recovered() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        std::fs::write(&path, r#"["not","an","object"]"#).unwrap();
        record_install(&path, "github", "myapp", None).unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn contains_reflects_state() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        assert!(!contains(&path, "x").unwrap());
        record_install(&path, "x", "myapp", None).unwrap();
        assert!(contains(&path, "x").unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn record_install_rejects_symlinked_ledger() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_ledger = outside.path().join(".agent-config-mcp.json");
        std::fs::write(&outside_ledger, b"{}").unwrap();
        let path = ledger_path(dir.path());
        symlink(&outside_ledger, &path).unwrap();

        let err = record_install(&path, "github", "myapp", None).unwrap_err();

        assert!(matches!(err, AgentConfigError::PathResolution(_)));
        assert_eq!(std::fs::read(&outside_ledger).unwrap(), b"{}");
    }

    #[test]
    #[cfg(unix)]
    fn owner_of_rejects_symlinked_ledger() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_ledger = outside.path().join(".agent-config-mcp.json");
        std::fs::write(&outside_ledger, b"{}").unwrap();
        let path = ledger_path(dir.path());
        symlink(&outside_ledger, &path).unwrap();

        let err = owner_of(&path, "github").unwrap_err();

        assert!(matches!(err, AgentConfigError::PathResolution(_)));
    }

    #[test]
    fn concurrent_record_install_different_names_keeps_both() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let path_a = path.clone();
        let path_b = path.clone();

        let (ra, rb) = run_two(
            move || record_install(&path_a, "alpha", "appA", None),
            move || record_install(&path_b, "beta", "appB", None),
        );

        ra.unwrap();
        rb.unwrap();
        assert_eq!(owner_of(&path, "alpha").unwrap().as_deref(), Some("appA"));
        assert_eq!(owner_of(&path, "beta").unwrap().as_deref(), Some("appB"));
    }

    #[test]
    fn concurrent_record_install_same_name_different_owners_is_valid_last_writer_wins() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let path_a = path.clone();
        let path_b = path.clone();

        let (ra, rb) = run_two(
            move || record_install(&path_a, "shared", "appA", None),
            move || record_install(&path_b, "shared", "appB", None),
        );

        ra.unwrap();
        rb.unwrap();
        let owner = owner_of(&path, "shared").unwrap().unwrap();
        assert!(matches!(owner.as_str(), "appA" | "appB"));
        let StrictLedgerRead::Valid { entries } = read_strict(&path).unwrap() else {
            panic!("ledger should parse after concurrent writes");
        };
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn record_install_stores_content_hash() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let hash = content_hash(b"test content");
        record_install(&path, "github", "myapp", Some(&hash)).unwrap();
        assert_eq!(
            content_hash_of(&path, "github").unwrap().as_deref(),
            Some(hash.as_str())
        );
    }

    #[test]
    fn record_install_without_hash_stores_none() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "myapp", None).unwrap();
        assert_eq!(content_hash_of(&path, "github").unwrap(), None);
    }

    #[test]
    fn v1_ledger_reads_without_hash() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        std::fs::write(
            &path,
            r#"{"version":1,"entries":{"github":{"owner":"myapp"}}}"#,
        )
        .unwrap();
        let StrictLedgerRead::Valid { entries } = read_strict(&path).unwrap() else {
            panic!("v1 ledger should parse");
        };
        let entry = entries.get("github").unwrap();
        assert_eq!(entry.owner, "myapp");
        assert!(entry.content_hash.is_none());
    }

    #[test]
    fn content_hash_is_deterministic() {
        let a = content_hash(b"hello world");
        let b = content_hash(b"hello world");
        assert_eq!(a, b);
        let c = content_hash(b"different");
        assert_ne!(a, c);
    }

    #[test]
    fn check_drift_passes_when_no_hash_recorded() {
        let dir = tempdir().unwrap();
        let ledger = ledger_path(dir.path());
        let config = dir.path().join("config.json");
        std::fs::write(&config, b"{}").unwrap();
        record_install(&ledger, "x", "myapp", None).unwrap();
        check_drift(&ledger, "x", &config).unwrap();
    }

    #[test]
    fn check_drift_passes_when_hash_matches() {
        let dir = tempdir().unwrap();
        let ledger = ledger_path(dir.path());
        let config = dir.path().join("config.json");
        let content = b"{\"mcpServers\":{}}";
        std::fs::write(&config, content).unwrap();
        let hash = content_hash(content);
        record_install(&ledger, "x", "myapp", Some(&hash)).unwrap();
        check_drift(&ledger, "x", &config).unwrap();
    }

    #[test]
    fn check_drift_fails_when_hash_mismatches() {
        let dir = tempdir().unwrap();
        let ledger = ledger_path(dir.path());
        let config = dir.path().join("config.json");
        std::fs::write(&config, b"original").unwrap();
        let hash = content_hash(b"original");
        record_install(&ledger, "x", "myapp", Some(&hash)).unwrap();
        std::fs::write(&config, b"modified").unwrap();
        let err = check_drift(&ledger, "x", &config).unwrap_err();
        assert!(matches!(err, AgentConfigError::ConfigDrifted { .. }));
    }
}
