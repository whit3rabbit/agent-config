//! Atomic file writes, first-touch backups, and Unix permission helpers.
//!
//! Every write through this module:
//! 1. Writes to `<path>.<rand>.tmp` in the same directory.
//! 2. Calls `fsync` on the temp file's contents.
//! 3. Renames temp → `<path>` (POSIX atomic; on Windows uses `MOVEFILE_REPLACE_EXISTING`).
//!
//! If the target file already exists and a backup is requested, we copy
//! `<path>` → `<path>.bak` *first*. Existing backups are treated as the
//! first-touch snapshot and are left in place.

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
/// first unless that backup already exists.
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

    let backup_path = if existed && make_backup && !backup_path(path).exists() {
        match create_backup(path) {
            Ok(backup) => Some(backup),
            Err(HookerError::BackupExists(_)) => None,
            Err(e) => return Err(e),
        }
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

    let mut src = fs::File::open(path).map_err(|e| HookerError::io(path, e))?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&bak)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                HookerError::BackupExists(bak.clone())
            } else {
                HookerError::io(&bak, e)
            }
        })?;
    std::io::copy(&mut src, &mut dst).map_err(|e| HookerError::io(&bak, e))?;
    dst.sync_all().map_err(|e| HookerError::io(&bak, e))?;
    Ok(bak)
}

/// Set Unix mode on a file. No-op on non-Unix platforms.
#[cfg(unix)]
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

/// Restore `<path>.bak` over `path` only when the backup already matches the
/// desired post-uninstall bytes. No-op if no backup exists or the backup is
/// stale for the desired state.
pub(crate) fn restore_backup_if_matches(
    path: &Path,
    desired_content: &[u8],
) -> Result<bool, HookerError> {
    let bak = backup_path(path);
    if !bak.exists() {
        return Ok(false);
    }
    let backup_content = fs::read(&bak).map_err(|e| HookerError::io(&bak, e))?;
    if backup_content != desired_content {
        return Ok(false);
    }
    fs::rename(&bak, path).map_err(|e| HookerError::io(&bak, e))?;
    Ok(true)
}

/// `<path>.bak`, the path [`write_atomic`] uses for first-touch backups.
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    PathBuf::from(bak)
}

/// Remove `<path>.bak` if it exists. Returns true if a file was actually removed.
pub(crate) fn remove_backup_if_exists(path: &Path) -> Result<bool, HookerError> {
    remove_if_exists(&backup_path(path))
}

/// Append a newline if `s` does not already end with one.
pub(crate) fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
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
            a_thread.join().expect("first writer panicked"),
            b_thread.join().expect("second writer panicked"),
        )
    }

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
        assert!(!backup_path(&path).exists(), "no backup on no-op");
    }

    #[test]
    fn existing_backup_is_preserved_while_target_updates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let bak = backup_path(&path);

        fs::write(&path, b"original").unwrap();
        fs::write(&bak, b"important-old-backup").unwrap();

        let out = write_atomic(&path, b"new", true).unwrap();
        assert!(out.backup.is_none());
        assert_eq!(fs::read(&bak).unwrap(), b"important-old-backup");
        assert_eq!(fs::read(&path).unwrap(), b"new");
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
        assert!(restore_backup_if_matches(&path, b"v1").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"v1");
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn restore_backup_skips_stale_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f.txt");
        fs::write(&path, b"v1").unwrap();
        write_atomic(&path, b"v2", true).unwrap();

        assert!(!restore_backup_if_matches(&path, b"").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"v2");
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"v1");
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

    #[test]
    fn concurrent_writes_without_backup_leave_complete_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        let path_a = path.clone();
        let path_b = path.clone();
        let a = br#"{"winner":"a","padding":"aaaaaaaaaaaaaaaa"}"#.to_vec();
        let b = br#"{"winner":"b","padding":"bbbbbbbbbbbbbbbb"}"#.to_vec();
        let expect_a = a.clone();
        let expect_b = b.clone();

        let (ra, rb) = run_two(
            move || write_atomic(&path_a, &a, false),
            move || write_atomic(&path_b, &b, false),
        );

        ra.unwrap();
        rb.unwrap();
        let final_bytes = fs::read(&path).unwrap();
        assert!(
            final_bytes == expect_a || final_bytes == expect_b,
            "final file should be one complete write, got {:?}",
            String::from_utf8_lossy(&final_bytes)
        );
    }

    #[test]
    fn concurrent_backup_creation_keeps_original_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, b"original").unwrap();
        let path_a = path.clone();
        let path_b = path.clone();

        let (ra, rb) = run_two(
            move || write_atomic(&path_a, b"new-a", true),
            move || write_atomic(&path_b, b"new-b", true),
        );

        ra.unwrap();
        rb.unwrap();
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"original");
        let final_bytes = fs::read(&path).unwrap();
        assert!(final_bytes == b"new-a" || final_bytes == b"new-b");
    }
}
