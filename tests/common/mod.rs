//! Shared helpers for integration tests.
//!
//! Each test file that needs these helpers should declare `mod common;` at
//! the top. Items not used by a particular test binary are tolerated via
//! `#[allow(dead_code)]`; without it, Rust warns once per item per binary.

#![allow(dead_code)]

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use tempfile::TempDir;

/// Process-wide lock that serializes any test that mutates path-related
/// environment variables. Tests that need [`IsolatedGlobalEnv`] hold this
/// for their entire body, so only one such test runs at a time.
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Build a controlled environment for a global-scope install: a fresh tempdir
/// rooted at `HOME`/`USERPROFILE`, plus matching `APPDATA`, `LOCALAPPDATA`,
/// `XDG_CONFIG_HOME`, and `CODEX_HOME`. Holds the [`env_lock`] for the
/// duration of the test, so the env-var state cannot race with other tests.
///
/// The home directory is canonicalized so the strict global-scope symlink
/// policy in `safe_fs::write` does not trip on OS-level symlinks (e.g.
/// macOS's `/var` -> `/private/var`).
pub struct IsolatedGlobalEnv {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
    _home: TempDir,
    home_canon: PathBuf,
}

impl IsolatedGlobalEnv {
    pub fn new() -> Self {
        let lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let home_canon = fs::canonicalize(home.path()).unwrap();
        let appdata = home_canon.join("AppData").join("Roaming");
        let localappdata = home_canon.join("AppData").join("Local");
        let xdg_config = home_canon.join(".config");
        fs::create_dir_all(&appdata).unwrap();
        fs::create_dir_all(&localappdata).unwrap();
        fs::create_dir_all(&xdg_config).unwrap();

        let vars = [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "CODEX_HOME",
        ];
        let saved = vars
            .into_iter()
            .map(|key| (key, env::var_os(key)))
            .collect();

        env::set_var("HOME", &home_canon);
        env::set_var("USERPROFILE", &home_canon);
        env::set_var("APPDATA", &appdata);
        env::set_var("LOCALAPPDATA", &localappdata);
        env::set_var("XDG_CONFIG_HOME", &xdg_config);
        env::set_var("CODEX_HOME", home_canon.join(".codex"));

        Self {
            _lock: lock,
            saved,
            _home: home,
            home_canon,
        }
    }

    pub fn home_path(&self) -> &Path {
        &self.home_canon
    }

    pub fn appdata_path(&self) -> PathBuf {
        self.home_canon.join("AppData").join("Roaming")
    }

    pub fn xdg_config_path(&self) -> PathBuf {
        self.home_canon.join(".config")
    }
}

impl Drop for IsolatedGlobalEnv {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => env::set_var(key, value),
                None => env::remove_var(key),
            }
        }
    }
}
