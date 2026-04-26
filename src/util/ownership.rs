//! Sidecar JSON ledger that records which consumer owns which MCP server
//! (or skill) installed by `ai-hooker`.
//!
//! The harness configs themselves (`mcp.json`, `config.toml`, etc.) are
//! caller-visible files we share with both the user and other consumers, so we
//! cannot safely embed `_ai_hooker_tag`-style markers inside every entry —
//! some harnesses reject unknown keys, and the TOML/JSON-array variants do
//! not all round-trip extra fields cleanly.
//!
//! Instead, every install records `{ "name": { "owner": "<tag>" } }` in a
//! sibling file (e.g., `~/.cursor/.ai-hooker-mcp.json`). Uninstall consults
//! the ledger first; if the recorded owner differs from the caller (or the
//! entry exists in the harness config but not in the ledger), we refuse with
//! [`HookerError::NotOwnedByCaller`] rather than risk clobbering work this
//! caller did not perform.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::error::HookerError;
use crate::util::{fs_atomic, json_patch};

/// Sidecar ledger filename, sibling to the harness's MCP config file.
const MCP_LEDGER_FILE: &str = ".ai-hooker-mcp.json";

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

/// Current ledger schema version. Bumped on incompatible changes.
const CURRENT_VERSION: u64 = 1;

/// Record an install. Creates the ledger file if missing.
pub(crate) fn record_install(
    ledger_path: &Path,
    name: &str,
    owner: &str,
) -> Result<(), HookerError> {
    let mut root = json_patch::read_or_empty(ledger_path)?;
    let entries = ensure_shape(&mut root);
    entries.insert(name.to_string(), json!({ OWNER_KEY: owner }));
    fs_atomic::write_atomic(ledger_path, &json_patch::to_pretty(&root), false)?;
    Ok(())
}

/// Forget an entry. Returns the previously recorded owner, if any. Removes the
/// ledger file entirely when no entries remain.
pub(crate) fn record_uninstall(
    ledger_path: &Path,
    name: &str,
) -> Result<Option<String>, HookerError> {
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
}

/// Look up the owner of `name` without mutating the ledger.
pub(crate) fn owner_of(ledger_path: &Path, name: &str) -> Result<Option<String>, HookerError> {
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
        /// Map of ledger entry name to owner tag.
        entries: BTreeMap<String, String>,
    },
    /// The ledger file is present but not valid ledger JSON.
    Malformed {
        /// Human-readable parse or shape error.
        reason: String,
    },
}

/// Read a ledger without repairing or normalizing it.
///
/// This is intentionally stricter than the mutating install/uninstall path:
/// validation must report malformed ledgers as drift and must never rewrite
/// them as a side effect of checking state.
pub(crate) fn read_strict(ledger_path: &Path) -> Result<StrictLedgerRead, HookerError> {
    let bytes = match fs::read(ledger_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StrictLedgerRead::Missing);
        }
        Err(e) => return Err(HookerError::io(ledger_path, e)),
    };
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
        entries.insert(name.clone(), owner.to_string());
    }

    Ok(StrictLedgerRead::Valid { entries })
}

/// True if `name` has any owner recorded in the ledger.
pub(crate) fn contains(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    Ok(owner_of(ledger_path, name)?.is_some())
}

/// Verify that `name` is owned by `expected`. Returns
/// [`HookerError::NotOwnedByCaller`] otherwise. `kind` is a short label for
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
) -> Result<(), HookerError> {
    let actual = owner_of(ledger_path, name)?;
    match (actual.as_deref(), present_in_config) {
        // Standard case: ledger recorded owner; must match.
        (Some(o), _) if o == expected => Ok(()),
        // Mismatch — recorded owner differs.
        (Some(_), _) => Err(HookerError::NotOwnedByCaller {
            kind,
            name: name.into(),
            expected: expected.into(),
            actual,
        }),
        // No ledger entry but the harness config has it: user-installed.
        (None, true) => Err(HookerError::NotOwnedByCaller {
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
    use tempfile::tempdir;

    fn ledger_path(dir: &Path) -> std::path::PathBuf {
        dir.join(".ai-hooker-mcp.json")
    }

    #[test]
    fn record_install_creates_ledger() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "myapp").unwrap();
        assert!(path.exists());
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn second_install_overwrites_owner() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "appA").unwrap();
        record_install(&path, "github", "appB").unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("appB"));
    }

    #[test]
    fn record_uninstall_returns_prior_owner() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "github", "myapp").unwrap();
        let prev = record_uninstall(&path, "github").unwrap();
        assert_eq!(prev.as_deref(), Some("myapp"));
        assert_eq!(owner_of(&path, "github").unwrap(), None);
    }

    #[test]
    fn uninstall_removes_empty_ledger_file() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "alpha", "myapp").unwrap();
        record_uninstall(&path, "alpha").unwrap();
        assert!(!path.exists(), "ledger should be removed when empty");
    }

    #[test]
    fn uninstall_keeps_ledger_with_other_entries() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "alpha", "appA").unwrap();
        record_install(&path, "beta", "appB").unwrap();
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
        record_install(&path, "x", "myapp").unwrap();
        require_owner(&path, "x", "myapp", "mcp server", true).unwrap();
    }

    #[test]
    fn require_owner_fails_on_mismatch() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        record_install(&path, "x", "appA").unwrap();
        let err = require_owner(&path, "x", "appB", "mcp server", true).unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: Some(ref a), .. } if a == "appA"
        ));
    }

    #[test]
    fn require_owner_fails_when_present_but_unrecorded() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        let err = require_owner(&path, "x", "myapp", "mcp server", true).unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
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
        record_install(&path, "github", "myapp").unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn hand_edited_non_object_root_is_recovered() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        std::fs::write(&path, r#"["not","an","object"]"#).unwrap();
        record_install(&path, "github", "myapp").unwrap();
        assert_eq!(owner_of(&path, "github").unwrap().as_deref(), Some("myapp"));
    }

    #[test]
    fn contains_reflects_state() {
        let dir = tempdir().unwrap();
        let path = ledger_path(dir.path());
        assert!(!contains(&path, "x").unwrap());
        record_install(&path, "x", "myapp").unwrap();
        assert!(contains(&path, "x").unwrap());
    }
}
