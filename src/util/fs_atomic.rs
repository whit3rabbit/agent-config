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
use std::path::{Component, Path, PathBuf};

use crate::error::AgentConfigError;

/// Hard ceiling on bytes the library will read from any harness config or
/// rules file. Set high enough to never trip in practice (typical configs
/// are <100 KiB) and low enough to keep memory bounded on a malicious
/// or runaway file.
pub(crate) const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) fn read_capped(path: &Path) -> Result<Vec<u8>, AgentConfigError> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(AgentConfigError::io(path, e)),
    };
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(AgentConfigError::ConfigTooLarge {
            path: path.to_path_buf(),
            size: meta.len(),
            limit: MAX_CONFIG_BYTES,
        });
    }
    std::fs::read(path).map_err(|e| AgentConfigError::io(path, e))
}

pub(crate) fn read_to_string_capped(path: &Path) -> Result<String, AgentConfigError> {
    let bytes = read_capped(path)?;
    String::from_utf8(bytes).map_err(|e| {
        AgentConfigError::io(
            path,
            std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        )
    })
}

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
pub(super) fn write_atomic(
    path: &Path,
    content: &[u8],
    make_backup: bool,
) -> Result<WriteOutcome, AgentConfigError> {
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
        Err(e) => return Err(AgentConfigError::io(path, e)),
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| AgentConfigError::io(parent, e))?;
        }
    }

    let backup_path = if existed && make_backup && !backup_path(path).exists() {
        match create_backup(path) {
            Ok(backup) => Some(backup),
            Err(AgentConfigError::BackupExists(_)) => None,
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
        .ok_or_else(|| {
            AgentConfigError::PathResolution(format!("path has no file name: {path:?}"))
        })?
        .to_string_lossy()
        .into_owned();

    // Use NamedTempFile in the destination's parent so rename is same-filesystem.
    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_name}."))
        .suffix(".tmp")
        .tempfile_in(&parent)
        .map_err(|e| AgentConfigError::io(&parent, e))?;

    tmp.write_all(content)
        .map_err(|e| AgentConfigError::io(tmp.path(), e))?;
    tmp.as_file_mut()
        .sync_all()
        .map_err(|e| AgentConfigError::io(tmp.path(), e))?;

    tmp.persist(path)
        .map_err(|e| AgentConfigError::io(path, e.error))?;

    Ok(WriteOutcome {
        path: path.to_path_buf(),
        existed,
        backup: backup_path,
        no_change: false,
    })
}

/// Copy `path` → `<path>.bak`, refusing if a backup already exists.
fn create_backup(path: &Path) -> Result<PathBuf, AgentConfigError> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);

    let mut src = fs::File::open(path).map_err(|e| AgentConfigError::io(path, e))?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&bak)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                AgentConfigError::BackupExists(bak.clone())
            } else {
                AgentConfigError::io(&bak, e)
            }
        })?;
    std::io::copy(&mut src, &mut dst).map_err(|e| AgentConfigError::io(&bak, e))?;
    dst.sync_all().map_err(|e| AgentConfigError::io(&bak, e))?;
    Ok(bak)
}

/// Set Unix mode on a file. No-op on non-Unix platforms.
#[cfg(unix)]
pub(super) fn chmod(path: &Path, mode: u32) -> Result<(), AgentConfigError> {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path)
        .map_err(|e| AgentConfigError::io(path, e))?
        .permissions();
    p.set_mode(mode);
    fs::set_permissions(path, p).map_err(|e| AgentConfigError::io(path, e))?;
    Ok(())
}

/// No-op on Windows.
#[cfg(not(unix))]
pub(super) fn chmod(_path: &Path, _mode: u32) -> Result<(), AgentConfigError> {
    Ok(())
}

/// Remove a file, ignoring NotFound. Returns true if a file was actually removed.
pub(super) fn remove_if_exists(path: &Path) -> Result<bool, AgentConfigError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(AgentConfigError::io(path, e)),
    }
}

/// Read `path` as UTF-8, returning an empty string if the file does not exist.
/// Routes through [`read_to_string_capped`] so an oversized config surfaces
/// [`AgentConfigError::ConfigTooLarge`] instead of consuming unbounded memory.
/// `read_to_string_capped` already returns an empty buffer for missing paths,
/// preserving the original "or_empty" semantics.
pub(crate) fn read_to_string_or_empty(path: &Path) -> Result<String, AgentConfigError> {
    read_to_string_capped(path)
}

/// Restore `<path>.bak` over `path` only when the backup already matches the
/// desired post-uninstall bytes. No-op if no backup exists or the backup is
/// stale for the desired state.
pub(super) fn restore_backup_if_matches(
    path: &Path,
    desired_content: &[u8],
) -> Result<bool, AgentConfigError> {
    let bak = backup_path(path);
    if !bak.exists() {
        return Ok(false);
    }
    let backup_content = fs::read(&bak).map_err(|e| AgentConfigError::io(&bak, e))?;
    if backup_content != desired_content {
        return Ok(false);
    }
    fs::rename(&bak, path).map_err(|e| AgentConfigError::io(&bak, e))?;
    Ok(true)
}

/// `<path>.bak`, the path [`write_atomic`] uses for first-touch backups.
pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    PathBuf::from(bak)
}

/// Append a newline if `s` does not already end with one.
pub(crate) fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Verify that `path` cannot escape `root`.
///
/// This canonicalizes `root`, then walks the existing components between the
/// local root and `path`, rejecting symlink components before following them.
/// Missing tail components are allowed, but the deepest existing ancestor must
/// still canonicalize under `root`. Existing symlink targets are rejected too,
/// since backup/write paths would otherwise follow the link.
pub(crate) fn ensure_contained(path: &Path, root: &Path) -> Result<(), AgentConfigError> {
    let canonical_root = fs::canonicalize(root).map_err(|e| AgentConfigError::io(root, e))?;
    let root_abs = lexical_absolute(root)?;
    let path_abs = lexical_absolute(path)?;

    let deepest_existing = if let Ok(relative) = path_abs.strip_prefix(&root_abs) {
        deepest_existing_descendant(&root_abs, relative)?
    } else if let Ok(relative) = path_abs.strip_prefix(&canonical_root) {
        deepest_existing_descendant(&canonical_root, relative)?
    } else {
        deepest_existing_ancestor(&path_abs)?
    };

    let canonical_existing = fs::canonicalize(&deepest_existing)
        .map_err(|e| AgentConfigError::io(&deepest_existing, e))?;
    if !canonical_existing.starts_with(&canonical_root) {
        return Err(AgentConfigError::PathResolution(format!(
            "resolved path {} escapes project root {}",
            canonical_existing.display(),
            canonical_root.display()
        )));
    }
    Ok(())
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, AgentConfigError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().map_err(|e| AgentConfigError::io(Path::new("."), e))?;
        Ok(cwd.join(path))
    }
}

fn validate_relative_components(relative: &Path) -> Result<(), AgentConfigError> {
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(AgentConfigError::PathResolution(format!(
                    "path {} escapes project root via parent directory component",
                    relative.display()
                )));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(AgentConfigError::PathResolution(format!(
                    "expected relative path beneath project root, got {}",
                    relative.display()
                )));
            }
        }
    }
    Ok(())
}

fn deepest_existing_descendant(root: &Path, relative: &Path) -> Result<PathBuf, AgentConfigError> {
    validate_relative_components(relative)?;

    let mut current = root.to_path_buf();
    let mut deepest = current.clone();
    let components: Vec<Component<'_>> = relative.components().collect();

    for (index, component) in components.iter().enumerate() {
        match component {
            Component::Normal(part) => current.push(part),
            Component::CurDir => continue,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => unreachable!(),
        }

        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentConfigError::PathResolution(format!(
                    "refusing to access path through symlink at {}",
                    current.display()
                )));
            }
            Ok(meta) => {
                if index + 1 < components.len() && !meta.is_dir() {
                    return Err(AgentConfigError::PathResolution(format!(
                        "path component {} is not a directory",
                        current.display()
                    )));
                }
                deepest = current.clone();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(AgentConfigError::io(&current, e)),
        }
    }

    Ok(deepest)
}

fn deepest_existing_ancestor(path: &Path) -> Result<PathBuf, AgentConfigError> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(AgentConfigError::PathResolution(format!(
                    "refusing to access path through symlink at {}",
                    current.display()
                )));
            }
            Ok(_) => return Ok(current),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !current.pop() {
                    return Err(AgentConfigError::PathResolution(format!(
                        "could not find existing ancestor for {}",
                        path.display()
                    )));
                }
            }
            Err(e) => return Err(AgentConfigError::io(&current, e)),
        }
    }
}

/// Reject the path if it is a symlink.
///
/// Uses `fs::symlink_metadata` to detect symlinks without following them.
/// Returns [`AgentConfigError::PathResolution`] if `path` is a symlink.
#[allow(dead_code)]
pub(crate) fn reject_symlink(path: &Path) -> Result<(), AgentConfigError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(AgentConfigError::PathResolution(
            format!("refusing to write through symlink at {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AgentConfigError::io(path, e)),
    }
}

/// Like [`write_atomic`], but first verifies that `path` stays within `root`
/// and is not a symlink. Intended for `Scope::Local` writes where the caller
/// supplies a project root that must contain all writes.
#[allow(dead_code)]
pub(crate) fn write_atomic_contained(
    path: &Path,
    content: &[u8],
    make_backup: bool,
    root: &Path,
) -> Result<WriteOutcome, AgentConfigError> {
    reject_symlink(path)?;
    ensure_contained(path, root)?;
    write_atomic(path, content, make_backup)
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

    fn write_succeeded_or_windows_replace_race(
        result: Result<WriteOutcome, AgentConfigError>,
    ) -> bool {
        match result {
            Ok(_) => true,
            Err(AgentConfigError::Io { source, .. })
                if cfg!(windows) && source.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                false
            }
            Err(e) => panic!("unexpected write_atomic error: {e:?}"),
        }
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

    #[cfg(not(unix))]
    #[test]
    fn chmod_is_noop_on_non_unix() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.sh");
        chmod(&path, 0o755).unwrap();
        assert!(!path.exists());
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

        // Windows can reject one simultaneous replace with AccessDenied. Shared
        // integration paths serialize writes with file locks; this lower-level
        // helper only needs to leave a complete final file when raced directly.
        let a_succeeded = write_succeeded_or_windows_replace_race(ra);
        let b_succeeded = write_succeeded_or_windows_replace_race(rb);
        assert!(
            a_succeeded || b_succeeded,
            "at least one concurrent writer should succeed"
        );
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

        let a_succeeded = write_succeeded_or_windows_replace_race(ra);
        let b_succeeded = write_succeeded_or_windows_replace_race(rb);
        assert!(
            a_succeeded || b_succeeded,
            "at least one concurrent writer should succeed"
        );
        assert_eq!(fs::read(backup_path(&path)).unwrap(), b"original");
        let final_bytes = fs::read(&path).unwrap();
        assert!(final_bytes == b"new-a" || final_bytes == b"new-b");
    }

    #[test]
    fn ensure_contained_allows_path_under_root() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sub").join("config.json");
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(&file, b"{}").unwrap();
        ensure_contained(&file, dir.path()).unwrap();
    }

    #[test]
    fn ensure_contained_rejects_path_outside_root() {
        let dir = tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let file = outside.join("config.json");
        fs::write(&file, b"{}").unwrap();
        let root = dir.path().join("inside");
        fs::create_dir_all(&root).unwrap();
        let err = ensure_contained(&file, &root).unwrap_err();
        assert!(matches!(err, AgentConfigError::PathResolution(_)));
    }

    #[test]
    fn ensure_contained_allows_nonexistent_parent() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("new").join("config.json");
        assert!(!file.parent().unwrap().exists());
        ensure_contained(&file, dir.path()).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn ensure_contained_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let escape = dir.path().join("escape");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&escape).unwrap();
        fs::write(escape.join("secret"), b"leaked").unwrap();
        let link = project.join("evil-link");
        symlink(&escape, &link).unwrap();
        let target = link.join("config.json");
        let err = ensure_contained(&target, &project).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::PathResolution(ref msg) if msg.contains("symlink"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn ensure_contained_rejects_symlink_escape_with_missing_deeper_parent() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let escape = dir.path().join("escape");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&escape).unwrap();
        let link = project.join(".opencode");
        symlink(&escape, &link).unwrap();

        let target = link.join("plugins").join("hook.ts");
        assert!(!target.parent().unwrap().exists());
        let err = ensure_contained(&target, &project).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::PathResolution(ref msg) if msg.contains("symlink"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn ensure_contained_rejects_symlink_target() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let escape = dir.path().join("escape");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&escape).unwrap();
        let outside_file = escape.join("config.json");
        fs::write(&outside_file, b"outside").unwrap();
        let link = project.join("config.json");
        symlink(&outside_file, &link).unwrap();

        let err = ensure_contained(&link, &project).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::PathResolution(ref msg) if msg.contains("symlink"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn write_atomic_contained_rejects_symlinked_missing_parent() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let escape = dir.path().join("escape");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&escape).unwrap();
        let link = project.join(".opencode");
        symlink(&escape, &link).unwrap();

        let target = link.join("plugins").join("hook.ts");
        let err = write_atomic_contained(&target, b"export {}", true, &project).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::PathResolution(ref msg) if msg.contains("symlink"))
        );
        assert!(!escape.join("plugins").exists());
    }

    #[test]
    #[cfg(unix)]
    fn reject_symlink_detects_symlink() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let target = dir.path().join("real");
        fs::write(&target, b"real").unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        let err = reject_symlink(&link).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::PathResolution(ref msg) if msg.contains("symlink"))
        );
    }

    #[test]
    fn reject_symlink_allows_regular_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("config.json");
        fs::write(&file, b"{}").unwrap();
        reject_symlink(&file).unwrap();
    }

    #[test]
    fn reject_symlink_allows_nonexistent() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("ghost");
        reject_symlink(&file).unwrap();
    }

    #[test]
    fn read_capped_rejects_oversized_files() {
        let dir = tempdir().unwrap();
        let big = dir.path().join("big.json");
        let f = std::fs::File::create(&big).unwrap();
        // sparse: set logical length above the cap without actually writing 8 MiB.
        f.set_len(MAX_CONFIG_BYTES + 1).unwrap();
        let err = read_capped(&big).unwrap_err();
        assert!(matches!(err, AgentConfigError::ConfigTooLarge { size, limit, .. } if size > limit));
    }

    #[test]
    fn read_capped_returns_empty_for_missing_file() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        let bytes = read_capped(&missing).unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn read_capped_reads_small_file() {
        let dir = tempdir().unwrap();
        let small = dir.path().join("small.json");
        std::fs::write(&small, b"{}").unwrap();
        let bytes = read_capped(&small).unwrap();
        assert_eq!(bytes, b"{}");
    }

    #[test]
    fn read_to_string_capped_rejects_invalid_utf8() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("bad.txt");
        std::fs::write(&bad, [0xFF, 0xFE, 0xFD]).unwrap();
        let err = read_to_string_capped(&bad).unwrap_err();
        assert!(matches!(err, AgentConfigError::Io { .. }));
    }
}
