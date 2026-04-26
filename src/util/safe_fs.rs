//! Scope-aware filesystem mutation entrypoints for integrations.
//!
//! Agent modules should use this facade for writes and removals. Lower-level
//! patching helpers inside `util` may still use `fs_atomic` directly after
//! their callers have resolved and validated the target paths.

use std::fs;
use std::path::Path;

use crate::error::AgentConfigError;
use crate::scope::Scope;
use crate::util::fs_atomic::{self, WriteOutcome};

/// Atomically replace `path` after applying scope safety checks.
pub(crate) fn write(
    scope: &Scope,
    path: &Path,
    content: &[u8],
    make_backup: bool,
) -> Result<WriteOutcome, AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    fs_atomic::write_atomic(path, content, make_backup)
}

/// Remove a file after applying scope safety checks.
pub(crate) fn remove_file(scope: &Scope, path: &Path) -> Result<bool, AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    fs_atomic::remove_if_exists(path)
}

/// Remove a directory tree after applying scope safety checks.
#[allow(dead_code)]
pub(crate) fn remove_dir_all(scope: &Scope, path: &Path) -> Result<bool, AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AgentConfigError::io(path, e)),
    }
}

/// Remove an empty directory after applying scope safety checks.
pub(crate) fn remove_empty_dir(scope: &Scope, path: &Path) -> Result<bool, AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AgentConfigError::io(path, e)),
    }
}

/// Restore `<path>.bak` over `path` when the backup matches the desired bytes.
pub(crate) fn restore_backup_if_matches(
    scope: &Scope,
    path: &Path,
    desired_content: &[u8],
) -> Result<bool, AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    ensure_mutation_target(scope, &fs_atomic::backup_path(path))?;
    fs_atomic::restore_backup_if_matches(path, desired_content)
}

/// Remove `<path>.bak` after applying scope safety checks.
pub(crate) fn remove_backup_if_exists(
    scope: &Scope,
    path: &Path,
) -> Result<bool, AgentConfigError> {
    remove_file(scope, &fs_atomic::backup_path(path))
}

/// Set Unix permissions after applying scope safety checks.
pub(crate) fn chmod(scope: &Scope, path: &Path, mode: u32) -> Result<(), AgentConfigError> {
    ensure_mutation_target(scope, path)?;
    fs_atomic::chmod(path, mode)
}

fn ensure_mutation_target(scope: &Scope, path: &Path) -> Result<(), AgentConfigError> {
    match scope {
        Scope::Global => fs_atomic::reject_symlink(path),
        Scope::Local(_) => scope.ensure_contained(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    #[cfg(unix)]
    fn global_write_rejects_symlinked_target() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let real = dir.path().join("real-config.json");
        let link = dir.path().join("config.json");
        fs::write(&real, b"outside").unwrap();
        symlink(&real, &link).unwrap();

        let err = write(&Scope::Global, &link, b"{}", false).unwrap_err();

        assert!(matches!(err, AgentConfigError::PathResolution(_)));
        assert_eq!(fs::read(&real).unwrap(), b"outside");
    }
}
