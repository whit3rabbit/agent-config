//! Atomic file writes, first-touch backups, and Unix permission helpers.
//!
//! Every write through this module:
//! 1. Writes to `<path>.<rand>.tmp` in the same directory.
//! 2. Calls `fsync` on the temp file's contents.
//! 3. Renames temp → `<path>` (POSIX atomic; on Windows uses `MOVEFILE_REPLACE_EXISTING`).
//!
//! If the target file already exists and a backup is requested, we copy
//! `<path>` → `<path>.bak` *first* (refusing if `.bak` already exists, to avoid
//! clobbering an older backup the user may rely on).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::error::HookerError;

/// Result of [`write_atomic`] / [`patch_in_place`].
#[derive(Debug, Default, Clone)]
pub(crate) struct WriteOutcome {
    /// The target path that ended up with new content.
    pub path: PathBuf,
    /// Whether the file existed before this call.
    pub existed: bool,
    /// Path of the `.bak` file if one was created (None if file was new or
    /// content was identical).
    pub backup: Option<PathBuf>,
    /// True if the new content is byte-identical to what was already on disk.
    pub no_change: bool,
}

/// Atomically replace the contents of `path` with `content`.
///
/// If the file already exists and `make_backup` is true, copy it to `<path>.bak`
/// first (returning [`HookerError::BackupExists`] if a backup already exists).
/// Creates parent directories if they don't exist. If `content` is byte-equal
/// to the existing file, this is a no-op (no temp file, no backup).
pub(crate) fn write_atomic(
    path: &Path,
    content: &[u8],
    make_backup: bool,
) -> Result<WriteOutcome, HookerError> {
    let existed = match fs::read(path) {
        Ok(current) => {
            if current == content {
                return Ok(WriteOutcome {
                    path: path.to_path_buf(),
                    existed: true,
                    backup: None,
                    no_change: true,
                });
            }
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(HookerError::io(path, e)),
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| HookerError::io(parent, e))?;
        }
    }

    let backup_path = if existed && make_backup {
        Some(create_backup(path)?)
    } else {
        None
    };

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    let file_name = path
        .file_name()
        .ok_or_else(|| HookerError::PathResolution(format!("path has no file name: {path:?}")))?
        .to_string_lossy()
        .into_owned();

    // Use NamedTempFile in the destination's parent so rename is same-filesystem.
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}."))
        .suffix(".tmp")
        .tempfile_in(&parent)
        .map_err(|e| HookerError::io(&parent, e))?;

    tmp.write_all(content)
        .map_err(|e| HookerError::io(tmp.path(), e))?;
    tmp.as_file_mut()
        .sync_all()
        .map_err(|e| HookerError::io(tmp.path(), e))?;

    tmp.persist(path)
        .map_err(|e| HookerError::io(path, e.error))?;

    Ok(WriteOutcome {
        path: path.to_path_buf(),
        existed,
        backup: backup_path,
        no_change: false,
    })
}

/// Copy `path` → `<path>.bak`, refusing if a backup already exists.
fn create_backup(path: &Path) -> Result<PathBuf, HookerError> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);

    if bak.exists() {
        return Err(HookerError::BackupExists(bak));
    }

    fs::copy(path, &bak).map_err(|e| HookerError::io(path, e))?;
    Ok(bak)
}

/// Set Unix mode on a file. No-op on non-Unix platforms.
///
/// Currently used for shell-script delegators (Gemini's optional shell-wrapper
/// path); kept available for future agent additions.
#[cfg(unix)]
#[allow(dead_code)]
pub(crate) fn chmod(path: &Path, mode: u32) -> Result<(), HookerError> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path)
        .map_err(|e| HookerError::io(path, e))?
        .permissions();
    p.set_mode(mode);
    fs::set_permissions(path, p).map_err(|e| HookerError::io(path, e))?;
    Ok(())
}

/// No-op on Windows.
#[cfg(not(unix))]
#[allow(dead_code)]
pub(crate) fn chmod(_path: &Path, _mode: u32) -> Result<(), HookerError> {
    Ok(())
}

/// Remove a file, ignoring NotFound. Returns true if a file was actually removed.
pub(crate) fn remove_if_exists(path: &Path) -> Result<bool, HookerError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(HookerError::io(path, e)),
    }
}

/// Read `path` as UTF-8, returning an empty string if the file does not exist.
/// Avoids the TOCTOU `exists()` + `read_to_string` two-syscall pattern.
pub(crate) fn read_to_string_or_empty(path: &Path) -> Result<String, HookerError> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(HookerError::io(path, e)),
    }
}

/// Restore `<path>.bak` over `path`, then remove the `.bak`. No-op if no backup.
pub(crate) fn restore_backup(path: &Path) -> Result<bool, HookerError> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);
    if !bak.exists() {
        return Ok(false);
    }
    fs::rename(&bak, path).map_err(|e| HookerError::io(&bak, e))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_new_file_no_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let out = write_atomic(&path, b"hello", true).unwrap();
        assert!(!out.existed);
        assert!(out.backup.is_none());
        assert!(!out.no_change);
        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn writes_existing_file_with_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, b"old").unwrap();

        let out = write_atomic(&path, b"new", true).unwrap();
        assert!(out.existed);
        let bak = out.backup.expect("expected backup path");
        assert_eq!(fs::read(&bak).unwrap(), b"old");
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn identical_content_is_noop() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, b"same").unwrap();
        let out = write_atomic(&path, b"same", true).unwrap();
        assert!(out.no_change);
        assert!(out.backup.is_none());
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        assert!(!PathBuf::from(bak).exists(), "no backup on no-op");
    }

    #[test]
    fn refuses_to_clobber_existing_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        let bak = PathBuf::from(bak);

        fs::write(&path, b"original").unwrap();
        fs::write(&bak, b"important-old-backup").unwrap();

        let err = write_atomic(&path, b"new", true).unwrap_err();
        assert!(matches!(err, HookerError::BackupExists(_)));
        assert_eq!(fs::read(&bak).unwrap(), b"important-old-backup");
        assert_eq!(fs::read(&path).unwrap(), b"original", "original untouched");
    }

    #[test]
    fn creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/deeply/under/file.txt");
        write_atomic(&path, b"x", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"x");
    }

    #[test]
    fn restore_backup_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f.txt");
        fs::write(&path, b"v1").unwrap();
        write_atomic(&path, b"v2", true).unwrap();
        assert!(restore_backup(&path).unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"v1");
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        assert!(!PathBuf::from(bak).exists());
    }

    #[test]
    fn remove_if_exists_handles_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ghost");
        assert!(!remove_if_exists(&path).unwrap());
        fs::write(&path, b"x").unwrap();
        assert!(remove_if_exists(&path).unwrap());
    }

    #[test]
    fn read_to_string_or_empty_handles_missing_and_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("maybe.md");
        assert_eq!(read_to_string_or_empty(&path).unwrap(), "");
        fs::write(&path, b"hello").unwrap();
        assert_eq!(read_to_string_or_empty(&path).unwrap(), "hello");
    }
}
