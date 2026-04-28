//! Shared install/uninstall logic for "rules-only" harnesses that load a
//! directory of project-local markdown files (Cline, Roo Code, Windsurf,
//! Kilo Code, Google Antigravity).
//!
//! Each consumer owns one file outright (`<dir>/<tag>.md`), so multiple
//! callers coexist without fence markers or shared arrays.

use std::path::{Path, PathBuf};

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::PlannedChange;
use crate::scope::Scope;
use crate::util::planning;
use crate::util::{file_lock, fs_atomic, safe_fs};

/// Compute the per-tag rule-file path inside `<root>/<rules_dir>/`.
pub(crate) fn target_path(root: &Path, rules_dir: &str, tag: &str) -> PathBuf {
    root.join(rules_dir).join(format!("{tag}.md"))
}

fn require_local_root(scope: &Scope) -> Result<&Path, AgentConfigError> {
    scope.local_root().ok_or_else(|| {
        AgentConfigError::PathResolution(
            "rules_dir requires a local-scope project root; caller must enforce this".into(),
        )
    })
}

/// Returns true if a per-tag rule file already exists.
pub(crate) fn is_installed(
    root: &Path,
    rules_dir: &str,
    tag: &str,
) -> Result<bool, AgentConfigError> {
    Ok(target_path(root, rules_dir, tag).exists())
}

/// Atomically write the rules markdown file. Idempotent on identical content.
/// Routes through `safe_fs::write` so symlink components under the project
/// root are rejected even when the caller did not pre-check the path.
pub(crate) fn install(
    scope: &Scope,
    rules_dir: &str,
    tag: &str,
    body: &str,
) -> Result<InstallReport, AgentConfigError> {
    let root = require_local_root(scope)?;
    let lock_target = lock_target(root);
    file_lock::with_lock(&lock_target, || {
        let path = target_path(root, rules_dir, tag);
        let body = fs_atomic::ensure_trailing_newline(body);
        let outcome = safe_fs::write(scope, &path, body.as_bytes(), true)?;

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
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    let path = target_path(root, rules_dir, tag);
    let body = fs_atomic::ensure_trailing_newline(body);
    let mut changes = Vec::new();
    planning::plan_write_file(&mut changes, &path, body.as_bytes(), true)?;
    Ok(changes)
}

/// Remove the per-tag rule file, then walk parent directories upward, pruning
/// any that became empty. Stops at the local-scope root.
pub(crate) fn uninstall(
    scope: &Scope,
    rules_dir: &str,
    tag: &str,
) -> Result<UninstallReport, AgentConfigError> {
    let root = require_local_root(scope)?;
    let lock_target = lock_target(root);
    file_lock::with_lock(&lock_target, || {
        let path = target_path(root, rules_dir, tag);
        let mut report = UninstallReport::default();
        if !path.exists() {
            report.not_installed = true;
            return Ok(report);
        }
        safe_fs::remove_file(scope, &path)?;
        report.removed.push(path.clone());

        let mut parent = path.parent();
        while let Some(p) = parent {
            if p == root {
                break;
            }
            match std::fs::read_dir(p) {
                Ok(mut entries) => {
                    if entries.next().is_some() {
                        break;
                    }
                }
                Err(_) => break,
            }
            // remove_empty_dir routes through the same scope/symlink checks as
            // every other mutation in this helper. Stop pruning on first error
            // (race with a concurrent writer or permission issue) — leftover
            // empty directories are harmless.
            match safe_fs::remove_empty_dir(scope, p) {
                Ok(true) => {}
                _ => break,
            }
            parent = p.parent();
        }
        Ok(report)
    })
}

fn lock_target(root: &Path) -> PathBuf {
    root.join(".agent-config-rules")
}

/// Plan removal of the per-tag rule file and any empty parent directories.
pub(crate) fn plan_uninstall(
    root: &Path,
    rules_dir: &str,
    tag: &str,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
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
    use std::fs;
    use tempfile::tempdir;

    fn local(root: &Path) -> Scope {
        Scope::Local(root.to_path_buf())
    }

    #[test]
    fn install_writes_under_rules_dir() {
        let dir = tempdir().unwrap();
        let r = install(&local(dir.path()), ".clinerules", "alpha", "rule body").unwrap();
        let expected = dir.path().join(".clinerules/alpha.md");
        assert_eq!(r.created, vec![expected.clone()]);
        assert_eq!(fs::read_to_string(&expected).unwrap(), "rule body\n");
    }

    #[test]
    fn install_is_idempotent_on_same_body() {
        let dir = tempdir().unwrap();
        install(&local(dir.path()), ".clinerules", "alpha", "x").unwrap();
        let r = install(&local(dir.path()), ".clinerules", "alpha", "x").unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_removes_file_and_empty_parents() {
        let dir = tempdir().unwrap();
        install(&local(dir.path()), ".kilocode/rules", "alpha", "x").unwrap();
        uninstall(&local(dir.path()), ".kilocode/rules", "alpha").unwrap();
        assert!(!dir.path().join(".kilocode").exists());
    }

    #[test]
    fn uninstall_keeps_other_consumers_files() {
        let dir = tempdir().unwrap();
        install(&local(dir.path()), ".clinerules", "alpha", "a").unwrap();
        install(&local(dir.path()), ".clinerules", "beta", "b").unwrap();
        uninstall(&local(dir.path()), ".clinerules", "alpha").unwrap();
        assert!(!dir.path().join(".clinerules/alpha.md").exists());
        assert!(dir.path().join(".clinerules/beta.md").exists());
    }

    #[test]
    fn uninstall_unknown_is_noop() {
        let dir = tempdir().unwrap();
        let r = uninstall(&local(dir.path()), ".clinerules", "ghost").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn install_rejects_global_scope() {
        let err = install(&Scope::Global, ".clinerules", "alpha", "x").unwrap_err();
        assert!(matches!(err, AgentConfigError::PathResolution(_)));
    }

    #[test]
    #[cfg(unix)]
    fn install_rejects_symlinked_target() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        let outside_target = outside_dir.path().join("escape.md");
        fs::write(&outside_target, "user content").unwrap();

        let rules_subdir = dir.path().join(".clinerules");
        fs::create_dir_all(&rules_subdir).unwrap();
        let alpha = rules_subdir.join("alpha.md");
        symlink(&outside_target, &alpha).unwrap();

        let err = install(&local(dir.path()), ".clinerules", "alpha", "body").unwrap_err();
        assert!(matches!(err, AgentConfigError::PathResolution(_)));
        // Symlinked target must remain untouched.
        assert_eq!(fs::read_to_string(&outside_target).unwrap(), "user content");
    }

    #[test]
    #[cfg(unix)]
    fn install_rejects_symlinked_rules_subdir() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let outside_dir = tempdir().unwrap();
        // Symlink the entire rules subdir to an outside location.
        symlink(outside_dir.path(), dir.path().join(".clinerules")).unwrap();

        let err = install(&local(dir.path()), ".clinerules", "alpha", "body").unwrap_err();
        assert!(matches!(err, AgentConfigError::PathResolution(_)));
        // Must not have written into the symlinked-out directory.
        assert!(!outside_dir.path().join("alpha.md").exists());
    }

    #[test]
    fn is_installed_reflects_state() {
        let dir = tempdir().unwrap();
        assert!(!is_installed(dir.path(), ".clinerules", "alpha").unwrap());
        install(&local(dir.path()), ".clinerules", "alpha", "x").unwrap();
        assert!(is_installed(dir.path(), ".clinerules", "alpha").unwrap());
    }
}
