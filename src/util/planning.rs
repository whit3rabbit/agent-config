//! Shared side-effect-free planning helpers.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::plan::{PlannedChange, RefusalReason};
use crate::util::{fs_atomic, json_patch, md_block};

/// Plan an atomic file write without touching disk.
pub(crate) fn plan_write_file(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    content: &[u8],
    make_backup: bool,
) -> Result<(), HookerError> {
    match fs::read(path) {
        Ok(current) => {
            if current == content {
                changes.push(PlannedChange::NoOp {
                    path: path.to_path_buf(),
                    reason: "already up to date".into(),
                });
                return Ok(());
            }
            if make_backup {
                let backup = fs_atomic::backup_path(path);
                if backup.exists() {
                    changes.push(PlannedChange::Refuse {
                        path: Some(backup),
                        reason: RefusalReason::BackupAlreadyExists,
                    });
                    return Ok(());
                }
                changes.push(PlannedChange::CreateBackup {
                    backup,
                    target: path.to_path_buf(),
                });
            }
            changes.push(PlannedChange::PatchFile {
                path: path.to_path_buf(),
            });
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            plan_parent_dirs(changes, path);
            changes.push(PlannedChange::CreateFile {
                path: path.to_path_buf(),
            });
        }
        Err(e) => return Err(HookerError::io(path, e)),
    }
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

/// Plan restoring `<path>.bak` to `path`, or removing `path` when no backup
/// exists.
pub(crate) fn plan_restore_backup_or_remove(changes: &mut Vec<PlannedChange>, path: &Path) {
    let backup = fs_atomic::backup_path(path);
    if backup.exists() {
        changes.push(PlannedChange::RestoreBackup {
            backup,
            target: path.to_path_buf(),
        });
    } else {
        changes.push(PlannedChange::RemoveFile {
            path: path.to_path_buf(),
        });
    }
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

/// Plan upserting an ai-hooker fenced markdown block.
pub(crate) fn plan_markdown_upsert(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    tag: &str,
    body: &str,
) -> Result<(), HookerError> {
    let host = fs_atomic::read_to_string_or_empty(path)?;
    let new_host = md_block::upsert(&host, tag, body);
    plan_write_file(changes, path, new_host.as_bytes(), true)
}

/// Plan removing an ai-hooker fenced markdown block.
pub(crate) fn plan_markdown_remove(
    changes: &mut Vec<PlannedChange>,
    path: &Path,
    tag: &str,
) -> Result<(), HookerError> {
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
        plan_restore_backup_or_remove(changes, path);
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
) -> Result<(), HookerError>
where
    F: FnOnce(&mut Value),
{
    let mut root = match json_patch::read_or_empty(path) {
        Ok(root) => root,
        Err(HookerError::JsonInvalid { .. }) => {
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
) -> Result<(), HookerError>
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
        Err(HookerError::JsonInvalid { .. }) => {
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
            plan_restore_backup_or_remove(changes, path);
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
