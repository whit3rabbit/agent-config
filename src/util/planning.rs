//! Shared side-effect-free planning helpers.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::AgentConfigError;
use crate::plan::{PlannedChange, RefusalReason};
use crate::util::{fs_atomic, json_patch, md_block};

/// Plan an atomic file write without touching disk.
pub(crate) fn plan_write_file(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    content: &[u8],
    make_backup: bool,
) -> Result<(), AgentConfigError> {
    // Pre-stat preserves the original NotFound→CreateFile branch, while the
    // capped read on the existing-file path bounds memory if the user's
    // harness config is pathologically large.
    if !path.exists() {
        plan_parent_dirs(changes, path);
        changes.push(PlannedChange::CreateFile {
            path: path.to_path_buf(),
        });
        return Ok(());
    }
    let current = fs_atomic::read_capped(path)?;
    if current == content {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "already up to date".into(),
        });
        return Ok(());
    }
    if make_backup {
        let backup = fs_atomic::backup_path(path);
        if !backup.exists() {
            changes.push(PlannedChange::CreateBackup {
                backup,
                target: path.to_path_buf(),
            });
        }
    }
    changes.push(PlannedChange::PatchFile {
        path: path.to_path_buf(),
    });
    Ok(())
}

/// Plan removal of a file if it exists.
pub(crate) fn plan_remove_file(changes: &mut Vec<PlannedChange>, path: &Path) {
    if path.exists() {
        changes.push(PlannedChange::RemoveFile {
            path: path.to_path_buf(),
        });
    } else {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "file is already absent".into(),
        });
    }
}

/// Plan restoring `<path>.bak` to `path` only when that backup already matches
/// the desired post-uninstall bytes, otherwise remove `path`.
pub(crate) fn plan_restore_backup_or_remove(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    desired_content: &[u8],
) -> Result<(), AgentConfigError> {
    let backup = fs_atomic::backup_path(path);
    if backup.exists() {
        // Defense in depth: a hostile process could swap a giant file in for
        // our `.bak`. Treat oversize as non-matching (fall through to
        // RemoveFile) to preserve the planner contract: if the backup doesn't
        // match the desired post-uninstall bytes, plan a remove instead.
        match fs_atomic::read_capped(&backup) {
            Ok(content) if content == desired_content => {
                changes.push(PlannedChange::RestoreBackup {
                    backup,
                    target: path.to_path_buf(),
                });
                return Ok(());
            }
            Ok(_) => {}
            Err(AgentConfigError::ConfigTooLarge { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    changes.push(PlannedChange::RemoveFile {
        path: path.to_path_buf(),
    });
    Ok(())
}

/// Plan ownership ledger creation/update.
pub(crate) fn plan_write_ledger(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    key: &str,
    owner: &str,
) {
    plan_parent_dirs(changes, path);
    changes.push(PlannedChange::WriteLedger {
        path: path.to_path_buf(),
        key: key.to_string(),
        owner: owner.to_string(),
    });
}

/// Plan ownership ledger entry removal.
pub(crate) fn plan_remove_ledger_entry(changes: &mut Vec<PlannedChange>, path: &Path, key: &str) {
    changes.push(PlannedChange::RemoveLedgerEntry {
        path: path.to_path_buf(),
        key: key.to_string(),
    });
}

/// Plan chmod on Unix-like hosts.
pub(crate) fn plan_set_permissions(changes: &mut Vec<PlannedChange>, path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        changes.push(PlannedChange::SetPermissions {
            path: path.to_path_buf(),
            mode,
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (changes, path, mode);
    }
}

/// Plan upserting an agent-config fenced markdown block.
pub(crate) fn plan_markdown_upsert(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    tag: &str,
    body: &str,
) -> Result<(), AgentConfigError> {
    let host = fs_atomic::read_to_string_or_empty(path)?;
    let new_host = md_block::upsert(&host, tag, body);
    plan_write_file(changes, path, new_host.as_bytes(), true)
}

/// Plan removing an agent-config fenced markdown block.
pub(crate) fn plan_markdown_remove(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    tag: &str,
) -> Result<(), AgentConfigError> {
    let host = fs_atomic::read_to_string_or_empty(path)?;
    let (stripped, removed) = md_block::remove(&host, tag);
    if !removed {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "tagged markdown block is already absent".into(),
        });
        return Ok(());
    }
    if stripped.trim().is_empty() {
        plan_restore_backup_or_remove(changes, path, stripped.as_bytes())?;
    } else {
        plan_write_file(changes, path, stripped.as_bytes(), false)?;
    }
    Ok(())
}

/// Plan upserting a tagged JSON array entry at `entry_path`.
pub(crate) fn plan_tagged_json_upsert<F>(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    entry_path: &[&str],
    tag: &str,
    entry: Value,
    configure_root: F,
) -> Result<(), AgentConfigError>
where
    F: FnOnce(&mut Value),
{
    let mut root = match json_patch::read_or_empty(path) {
        Ok(root) => root,
        Err(AgentConfigError::JsonInvalid { .. }) => {
            changes.push(PlannedChange::Refuse {
                path: Some(path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    configure_root(&mut root);
    let changed = json_patch::upsert_tagged_array_entry(&mut root, entry_path, tag, entry)?;
    if changed {
        let bytes = json_patch::to_pretty(&root);
        plan_write_file(changes, path, &bytes, true)?;
    } else {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "tagged JSON entry is already up to date".into(),
        });
    }
    Ok(())
}

/// Plan removing tagged JSON array entries under `parent_path`.
pub(crate) fn plan_tagged_json_remove_under<F>(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    parent_path: &[&str],
    tag: &str,
    is_empty_after: F,
    restore_when_empty: bool,
) -> Result<(), AgentConfigError>
where
    F: FnOnce(&Value) -> bool,
{
    if !path.exists() {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "config file is already absent".into(),
        });
        return Ok(());
    }

    let mut root = match json_patch::read_or_empty(path) {
        Ok(root) => root,
        Err(AgentConfigError::JsonInvalid { .. }) => {
            changes.push(PlannedChange::Refuse {
                path: Some(path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    let changed = json_patch::remove_tagged_array_entries_under(&mut root, parent_path, tag)?;
    if !changed {
        changes.push(PlannedChange::NoOp {
            path: path.to_path_buf(),
            reason: "tagged JSON entry is already absent".into(),
        });
        return Ok(());
    }

    if is_empty_after(&root) {
        if restore_when_empty {
            let bytes = json_patch::to_pretty(&root);
            plan_restore_backup_or_remove(changes, path, &bytes)?;
        } else {
            changes.push(PlannedChange::RemoveFile {
                path: path.to_path_buf(),
            });
            let backup = fs_atomic::backup_path(path);
            if backup.exists() {
                changes.push(PlannedChange::RemoveFile { path: backup });
            }
        }
    } else {
        let bytes = json_patch::to_pretty(&root);
        plan_write_file(changes, path, &bytes, false)?;
    }
    Ok(())
}

/// True when a JSON root object is empty.
pub(crate) fn json_object_empty(root: &Value) -> bool {
    root.as_object().map(Map::is_empty).unwrap_or(true)
}

/// Plan pruning empty parent directories after removing `path`, stopping
/// before `stop_at`.
pub(crate) fn plan_remove_empty_parents(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    stop_at: &Path,
) {
    let Some(mut parent) = path.parent().map(Path::to_path_buf) else {
        return;
    };
    while parent != stop_at {
        let Ok(mut entries) = fs::read_dir(&parent) else {
            break;
        };
        if entries.next().is_some() {
            break;
        }
        changes.push(PlannedChange::RemoveDir {
            path: parent.clone(),
        });
        let Some(next) = parent.parent().map(Path::to_path_buf) else {
            break;
        };
        parent = next;
    }
}

fn plan_parent_dirs(changes: &mut Vec<PlannedChange>, path: &Path) {
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return;
    };
    if parent.exists() {
        return;
    }
    let mut missing = Vec::<PathBuf>::new();
    let mut cur = parent;
    while !cur.exists() {
        missing.push(cur.to_path_buf());
        let Some(next) = cur.parent() else {
            break;
        };
        if next.as_os_str().is_empty() {
            break;
        }
        cur = next;
    }
    missing.reverse();
    for path in missing {
        if !changes
            .iter()
            .any(|c| matches!(c, PlannedChange::CreateDir { path: existing } if existing == &path))
        {
            changes.push(PlannedChange::CreateDir { path });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_write_file_propagates_config_too_large() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("huge.json");
        File::create(&cfg)
            .unwrap()
            .set_len(crate::util::fs_atomic::MAX_CONFIG_BYTES + 1)
            .unwrap();
        let mut changes = Vec::new();
        let err = plan_write_file(&mut changes, &cfg, b"x", true).unwrap_err();
        assert!(matches!(
            err,
            crate::error::AgentConfigError::ConfigTooLarge { .. }
        ));
    }

    #[test]
    fn plan_restore_treats_oversize_backup_as_non_matching() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("settings.json");
        let backup = crate::util::fs_atomic::backup_path(&cfg);
        File::create(&backup)
            .unwrap()
            .set_len(crate::util::fs_atomic::MAX_CONFIG_BYTES + 1)
            .unwrap();
        let mut changes = Vec::new();
        plan_restore_backup_or_remove(&mut changes, &cfg, b"desired").unwrap();
        // Should plan a remove, NOT a restore.
        assert!(
            changes
                .iter()
                .any(|c| matches!(c, crate::plan::PlannedChange::RemoveFile { .. }))
        );
        assert!(
            !changes
                .iter()
                .any(|c| matches!(c, crate::plan::PlannedChange::RestoreBackup { .. }))
        );
    }
}
