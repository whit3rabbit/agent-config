//! Shared install/uninstall logic for "rules-only" harnesses that load a
//! directory of project-local markdown files (Cline, Roo Code, Windsurf,
//! Kilo Code, Google Antigravity).
//!
//! Each consumer owns one file outright (`<dir>/<tag>.md`), so multiple
//! callers coexist without fence markers or shared arrays.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::PlannedChange;
use crate::util::planning;
use crate::util::{file_lock, fs_atomic};

/// Compute the per-tag rule-file path inside `<root>/<rules_dir>/`.
pub(crate) fn target_path(root: &Path, rules_dir: &str, tag: &str) -> PathBuf {
    root.join(rules_dir).join(format!("{tag}.md"))
}

/// Returns true if a per-tag rule file already exists.
pub(crate) fn is_installed(root: &Path, rules_dir: &str, tag: &str) -> Result<bool, HookerError> {
    Ok(target_path(root, rules_dir, tag).exists())
}

/// Atomically write the rules markdown file. Idempotent on identical content.
pub(crate) fn install(
    root: &Path,
    rules_dir: &str,
    tag: &str,
    body: &str,
) -> Result<InstallReport, HookerError> {
    let lock_target = lock_target(root);
    file_lock::with_lock(&lock_target, || {
        let path = target_path(root, rules_dir, tag);
        let body = fs_atomic::ensure_trailing_newline(body);
        let outcome = fs_atomic::write_atomic(&path, body.as_bytes(), true)?;

        let mut report = InstallReport::default();
        if outcome.no_change {
            report.already_installed = true;
        } else if outcome.existed {
            report.patched.push(outcome.path.clone());
        } else {
            report.created.push(outcome.path.clone());
        }
        if let Some(b) = outcome.backup {
            report.backed_up.push(b);
        }
        Ok(report)
    })
}

/// Plan writing the per-tag rule file. Idempotent on identical content.
pub(crate) fn plan_install(
    root: &Path,
    rules_dir: &str,
    tag: &str,
    body: &str,
) -> Result<Vec<PlannedChange>, HookerError> {
    let path = target_path(root, rules_dir, tag);
    let body = fs_atomic::ensure_trailing_newline(body);
    let mut changes = Vec::new();
    planning::plan_write_file(&mut changes, &path, body.as_bytes(), true)?;
    Ok(changes)
}

/// Remove the per-tag rule file, then walk parent directories upward, pruning
/// any that became empty. Stops at `root`.
pub(crate) fn uninstall(
    root: &Path,
    rules_dir: &str,
    tag: &str,
) -> Result<UninstallReport, HookerError> {
    let lock_target = lock_target(root);
    file_lock::with_lock(&lock_target, || {
        let path = target_path(root, rules_dir, tag);
        let mut report = UninstallReport::default();
        if !path.exists() {
            report.not_installed = true;
            return Ok(report);
        }
        fs_atomic::remove_if_exists(&path)?;
        report.removed.push(path.clone());

        let mut parent = path.parent();
        while let Some(p) = parent {
            if p == root {
                break;
            }
            match fs::read_dir(p) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        break;
                    }
                }
                Err(_) => break,
            }
            if fs::remove_dir(p).is_err() {
                break;
            }
            parent = p.parent();
        }
        Ok(report)
    })
}

fn lock_target(root: &Path) -> PathBuf {
    root.join(".ai-hooker-rules")
}

/// Plan removal of the per-tag rule file and any empty parent directories.
pub(crate) fn plan_uninstall(
    root: &Path,
    rules_dir: &str,
    tag: &str,
) -> Result<Vec<PlannedChange>, HookerError> {
    let path = target_path(root, rules_dir, tag);
    let mut changes = Vec::new();
    if !path.exists() {
        changes.push(PlannedChange::NoOp {
            path,
            reason: "rule file is already absent".into(),
        });
        return Ok(changes);
    }
    changes.push(PlannedChange::RemoveFile { path: path.clone() });
    planning::plan_remove_empty_parents(&mut changes, &path, root);
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_writes_under_rules_dir() {
        let dir = tempdir().unwrap();
        let r = install(dir.path(), ".clinerules", "alpha", "rule body").unwrap();
        let expected = dir.path().join(".clinerules/alpha.md");
        assert_eq!(r.created, vec![expected.clone()]);
        assert_eq!(fs::read_to_string(&expected).unwrap(), "rule body\n");
    }

    #[test]
    fn install_is_idempotent_on_same_body() {
        let dir = tempdir().unwrap();
        install(dir.path(), ".clinerules", "alpha", "x").unwrap();
        let r = install(dir.path(), ".clinerules", "alpha", "x").unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_removes_file_and_empty_parents() {
        let dir = tempdir().unwrap();
        install(dir.path(), ".kilocode/rules", "alpha", "x").unwrap();
        uninstall(dir.path(), ".kilocode/rules", "alpha").unwrap();
        assert!(!dir.path().join(".kilocode").exists());
    }

    #[test]
    fn uninstall_keeps_other_consumers_files() {
        let dir = tempdir().unwrap();
        install(dir.path(), ".clinerules", "alpha", "a").unwrap();
        install(dir.path(), ".clinerules", "beta", "b").unwrap();
        uninstall(dir.path(), ".clinerules", "alpha").unwrap();
        assert!(!dir.path().join(".clinerules/alpha.md").exists());
        assert!(dir.path().join(".clinerules/beta.md").exists());
    }

    #[test]
    fn uninstall_unknown_is_noop() {
        let dir = tempdir().unwrap();
        let r = uninstall(dir.path(), ".clinerules", "ghost").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn is_installed_reflects_state() {
        let dir = tempdir().unwrap();
        assert!(!is_installed(dir.path(), ".clinerules", "alpha").unwrap());
        install(dir.path(), ".clinerules", "alpha", "x").unwrap();
        assert!(is_installed(dir.path(), ".clinerules", "alpha").unwrap());
    }
}
