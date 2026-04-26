//! Small lock-file guard for read-modify-write helpers.
//!
//! The lock is advisory but works across threads and processes that use this
//! crate. It is deliberately simple: create a sibling lock file with
//! `create_new`, retry until a short timeout, and remove it on drop. If a
//! process crashes while holding the lock, the timeout error includes the lock
//! path so a user can delete it after confirming no agent-config process is
//! running.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AgentConfigError;

#[cfg(not(test))]
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const LOCK_TIMEOUT: Duration = Duration::from_secs(1);
const RETRY_DELAY: Duration = Duration::from_millis(5);

/// Guard that removes the lock file when dropped.
#[derive(Debug)]
pub(crate) struct FileLock {
    path: PathBuf,
    _file: File,
}

impl FileLock {
    /// Acquire the lock associated with `target`.
    pub(crate) fn acquire(target: &Path) -> Result<Self, AgentConfigError> {
        let lock_path = lock_path_for(target)?;
        if let Some(parent) = lock_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| AgentConfigError::io(parent, e))?;
            }
        }

        let started = Instant::now();
        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    writeln!(file, "pid={}", std::process::id())
                        .map_err(|e| AgentConfigError::io(&lock_path, e))?;
                    file.sync_all()
                        .map_err(|e| AgentConfigError::io(&lock_path, e))?;
                    return Ok(Self {
                        path: lock_path,
                        _file: file,
                    });
                }
                Err(e) if is_lock_contention(&lock_path, &e) => {
                    if started.elapsed() >= LOCK_TIMEOUT {
                        return Err(AgentConfigError::LockTimeout { path: lock_path });
                    }
                    thread::sleep(RETRY_DELAY);
                }
                Err(e) => return Err(AgentConfigError::io(&lock_path, e)),
            }
        }
    }
}

fn is_lock_contention(lock_path: &Path, error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        return true;
    }

    cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied && lock_path.exists()
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Run `f` while holding the lock for `target`.
pub(crate) fn with_lock<T>(
    target: &Path,
    f: impl FnOnce() -> Result<T, AgentConfigError>,
) -> Result<T, AgentConfigError> {
    let _guard = FileLock::acquire(target)?;
    f()
}

fn lock_path_for(target: &Path) -> Result<PathBuf, AgentConfigError> {
    let file_name = target
        .file_name()
        .ok_or_else(|| {
            AgentConfigError::PathResolution(format!("path has no file name: {target:?}"))
        })?
        .to_string_lossy();
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(parent.join(format!(".{file_name}.agent-config.lock")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn with_lock_runs_closure_and_removes_lock() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("config.json");
        let lock = lock_path_for(&target).unwrap();

        let value = with_lock(&target, || Ok::<_, AgentConfigError>(42)).unwrap();

        assert_eq!(value, 42);
        assert!(!lock.exists());
    }

    #[test]
    fn second_lock_times_out_when_lock_file_is_stale() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("config.json");
        let lock = lock_path_for(&target).unwrap();
        fs::write(&lock, b"stale").unwrap();

        let err = FileLock::acquire(&target).unwrap_err();

        assert!(matches!(err, AgentConfigError::LockTimeout { .. }));
    }
}
