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
        // Strict: every existing component must be a regular dir/file. A
        // symlinked `~/.claude` or `~/.cursor` would otherwise redirect
        // writes outside the user's intended config tree.
        Scope::Global => fs_atomic::reject_symlink_components(path),
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

    #[test]
    #[cfg(unix)]
    fn global_write_rejects_symlinked_parent_directory() {
        use std::os::unix::fs::symlink;

        let real_root = tempdir().unwrap();
        let stage = tempdir().unwrap();
        // Canonicalize the staging path so this test exercises a symlink we
        // introduced (`<stage>/.claude` → real_root) rather than incidental
        // OS-level symlinks like macOS's `/var` → `/private/var`.
        let stage_canon = fs::canonicalize(stage.path()).unwrap();
        let claude_dir = stage_canon.join(".claude");
        symlink(real_root.path(), &claude_dir).unwrap();
        let target = claude_dir.join("settings.json");

        let err = write(&Scope::Global, &target, b"{}", false).unwrap_err();
        assert!(matches!(err, AgentConfigError::PathResolution(_)));
        // The symlink target must not have been touched.
        assert!(!real_root.path().join("settings.json").exists());
    }

    #[test]
    #[cfg(unix)]
    fn global_write_allows_real_parent_directory() {
        let dir = tempdir().unwrap();
        // Canonicalize: macOS tempdirs live under `/var/folders/...`, where
        // `/var` is itself a symlink. The strict check walks lexical
        // components, so user-facing paths (canonical) are what we test.
        let canonical = fs::canonicalize(dir.path()).unwrap();
        let nested = canonical.join("config").join("settings.json");
        write(&Scope::Global, &nested, b"{}", false).unwrap();
        assert_eq!(fs::read(&nested).unwrap(), b"{}");
    }
}
