//! Cross-platform resolution of the per-user directories that AI harnesses use.
//!
//! Most harnesses on macOS/Linux store config under `$HOME/.<name>` (dotdir
//! convention) rather than the XDG config dir. On Windows, explicit
//! `%USERPROFILE%`/`%APPDATA%` overrides are honored before falling back to
//! shell-known folders, which is what those harnesses ship with too.

use std::fs;
use std::path::PathBuf;

use crate::error::AgentConfigError;

/// Returns the user's home directory or a [`AgentConfigError::PathResolution`] if
/// the platform doesn't expose one.
///
/// # Errors
///
/// Returns [`AgentConfigError::PathResolution`] when neither `$HOME`
/// (`%USERPROFILE%` on Windows) nor [`dirs::home_dir`] yields a value.
pub fn home_dir() -> Result<PathBuf, AgentConfigError> {
    #[cfg(windows)]
    if let Some(home) = env_path("USERPROFILE") {
        return Ok(home);
    }

    #[cfg(not(windows))]
    if let Some(home) = env_path("HOME") {
        return Ok(home);
    }

    dirs::home_dir().ok_or_else(|| {
        AgentConfigError::PathResolution("could not determine user home directory".into())
    })
}

/// Returns `$XDG_CONFIG_HOME` (or its platform default) — used by OpenCode.
///
/// On macOS this is `~/Library/Application Support`. OpenCode, however, uses
/// `~/.config/opencode` even on macOS, so callers that need OpenCode's path
/// should prefer [`opencode_plugins_dir`] which encodes that quirk.
///
/// # Errors
///
/// Returns [`AgentConfigError::PathResolution`] when neither
/// `$XDG_CONFIG_HOME` (`%APPDATA%` on Windows) nor [`dirs::config_dir`]
/// yields a value.
pub fn config_dir() -> Result<PathBuf, AgentConfigError> {
    if let Some(config) = env_path("XDG_CONFIG_HOME") {
        return Ok(config);
    }

    #[cfg(windows)]
    if let Some(config) = env_path("APPDATA") {
        return Ok(config);
    }

    dirs::config_dir().ok_or_else(|| {
        AgentConfigError::PathResolution("could not determine user config directory".into())
    })
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `~/.claude` (all platforms).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn claude_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".claude"))
}

/// `~/.cursor` (all platforms).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn cursor_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".cursor"))
}

/// `~/.gemini` (all platforms).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn gemini_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".gemini"))
}

/// `$CODEX_HOME` if set, else `~/.codex`.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`] when
/// `CODEX_HOME` is unset and the home directory cannot be resolved.
pub fn codex_home() -> Result<PathBuf, AgentConfigError> {
    if let Some(h) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(h));
    }
    Ok(home_dir()?.join(".codex"))
}

/// `~/.openclaw` (all platforms).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn openclaw_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".openclaw"))
}

/// `~/.hermes` (all platforms).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn hermes_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".hermes"))
}

/// OpenCode forces its plugin directory under `~/.config/opencode/plugins`
/// regardless of platform conventions. Returns that path.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn opencode_plugins_dir() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".config").join("opencode").join("plugins"))
}

/// `~/.config/opencode/opencode.json` — OpenCode's main config, where the
/// object-based `mcp` map lives.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn opencode_config_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?
        .join(".config")
        .join("opencode")
        .join("opencode.json"))
}

/// `~/.config/kilo/kilo.jsonc` — Kilo Code's global JSONC config.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn kilo_config_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".config").join("kilo").join("kilo.jsonc"))
}

/// `~/.claude.json` — Claude Code's user/local MCP config file.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn claude_mcp_user_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".claude.json"))
}

/// `~/.cursor/mcp.json` — Cursor's MCP user-config file.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`cursor_home`].
pub fn cursor_mcp_user_file() -> Result<PathBuf, AgentConfigError> {
    Ok(cursor_home()?.join("mcp.json"))
}

/// VS Code globalStorage directory for an extension in the stable `Code`
/// profile. `extension_id` is appended verbatim and is not validated; pass the
/// publisher.name string used in the VS Code marketplace
/// (e.g. `"saoudrizwan.claude-dev"`).
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`config_dir`].
pub fn vscode_global_storage(extension_id: &str) -> Result<PathBuf, AgentConfigError> {
    Ok(config_dir()?
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join(extension_id))
}

/// Cline's global MCP settings file inside VS Code globalStorage.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`vscode_global_storage`].
pub fn cline_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    if let Ok(dir) = std::env::var("CLINE_DATA_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir)
                .join("settings")
                .join("cline_mcp_settings.json"));
        }
    }
    if let Ok(dir) = std::env::var("CLINE_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir)
                .join("data")
                .join("settings")
                .join("cline_mcp_settings.json"));
        }
    }
    Ok(home_dir()?
        .join(".cline")
        .join("data")
        .join("settings")
        .join("cline_mcp_settings.json"))
}

/// Cline's legacy global MCP settings file inside VS Code globalStorage.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`vscode_global_storage`].
pub fn legacy_cline_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(vscode_global_storage("saoudrizwan.claude-dev")?
        .join("settings")
        .join("cline_mcp_settings.json"))
}

/// Roo Code's global MCP settings file inside VS Code globalStorage.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`vscode_global_storage`].
pub fn roo_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(vscode_global_storage("rooveterinaryinc.roo-cline")?
        .join("settings")
        .join("mcp_settings.json"))
}

/// `~/.gemini/antigravity/mcp_config.json` — Antigravity's global MCP config.
///
/// If Antigravity has installed that documented file as a symlink to another
/// file under `~/.gemini`, this returns the resolved target. Antigravity uses
/// this symlink for its shared config store, and resolving only this leaf keeps
/// global write symlink rejection strict for every other path.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`gemini_home`].
pub fn antigravity_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    let gemini = gemini_home()?;
    let documented = gemini.join("config").join("mcp_config.json");
    let metadata = match fs::symlink_metadata(&documented) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(documented),
        Err(error) => return Err(AgentConfigError::io(&documented, error)),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(documented);
    }

    let canonical_gemini = fs::canonicalize(&gemini).map_err(|error| {
        AgentConfigError::PathResolution(format!(
            "could not resolve Antigravity config root {}: {error}",
            gemini.display()
        ))
    })?;
    let target = fs::canonicalize(&documented).map_err(|error| {
        AgentConfigError::PathResolution(format!(
            "could not resolve Antigravity MCP config symlink {}: {error}",
            documented.display()
        ))
    })?;
    if !target.starts_with(&canonical_gemini) {
        return Err(AgentConfigError::PathResolution(format!(
            "refusing to resolve Antigravity MCP config symlink {} outside {}",
            documented.display(),
            canonical_gemini.display()
        )));
    }
    Ok(target)
}

/// `~/.gemini/antigravity-cli` — Antigravity CLI's global config directory.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`gemini_home`].
pub fn antigravity_cli_home() -> Result<PathBuf, AgentConfigError> {
    Ok(gemini_home()?.join("antigravity-cli"))
}

/// `~/.gemini/antigravity-cli/mcp_config.json` — Antigravity CLI's global MCP config.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`antigravity_cli_home`].
pub fn antigravity_cli_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(antigravity_cli_home()?.join("mcp_config.json"))
}

/// `~/.codeium/windsurf/mcp_config.json` — Windsurf's global MCP config.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn windsurf_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json"))
}

/// Charm Crush's per-user config directory.
///
/// Honors `$CRUSH_GLOBAL_CONFIG` if set (Crush's documented override). Falls
/// back to `$XDG_CONFIG_HOME/crush` on Unix and `%APPDATA%\crush` on Windows
/// via [`config_dir`]. The single `crush.json` file lives directly under this
/// directory.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] when no usable directory
/// is found.
pub fn crush_home() -> Result<PathBuf, AgentConfigError> {
    if let Some(p) = env_path("CRUSH_GLOBAL_CONFIG") {
        return Ok(p);
    }
    Ok(config_dir()?.join("crush"))
}

/// Pi coding-agent's per-user config directory: `~/.pi/agent`.
///
/// Pi keeps its global memory file (`AGENTS.md`), MCP file (`mcp.json` for the
/// `pi-mcp-adapter`), skills (`skills/`), and extensions all under this root.
///
/// # Errors
///
/// Propagates [`AgentConfigError::PathResolution`] from [`home_dir`].
pub fn pi_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".pi").join("agent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // Serializes env-var mutations across the tests below. CODEX_HOME is the
    // only var read by these tests, but other tests in the suite that share
    // the process can mutate HOME / USERPROFILE / APPDATA, so any test that
    // mutates env vars must hold this mutex to avoid cross-test interference
    // under parallel execution.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn home_dir_is_resolvable_in_tests() {
        // CI environments always have $HOME set; smoke check that we don't panic.
        let _ = home_dir().expect("home dir on test host");
    }

    #[test]
    fn codex_home_respects_env_var() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let prev = std::env::var_os("CODEX_HOME");
        std::env::set_var("CODEX_HOME", &path);
        let resolved = codex_home().unwrap();
        match prev {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
        assert_eq!(resolved, path);
    }

    #[test]
    fn home_dirs_append_correct_suffix() {
        let cases: Vec<(Result<PathBuf, AgentConfigError>, &str)> = vec![
            (claude_home(), ".claude"),
            (cursor_home(), ".cursor"),
            (gemini_home(), ".gemini"),
            (openclaw_home(), ".openclaw"),
            (hermes_home(), ".hermes"),
        ];
        for (path, suffix) in cases {
            let p = path.expect("path resolved");
            assert!(
                p.to_string_lossy().ends_with(suffix),
                "{p:?} does not end with {suffix}"
            );
        }
        // pi_home is a two-segment suffix.
        let p = pi_home().expect("path resolved");
        assert!(p.ends_with(PathBuf::from(".pi").join("agent")));
        // crush_home ends in `crush` whether sourced from $XDG_CONFIG_HOME or
        // platform default.
        let p = crush_home().expect("path resolved");
        assert!(p.ends_with("crush"));
    }

    #[test]
    fn crush_home_respects_env_var() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let prev = std::env::var_os("CRUSH_GLOBAL_CONFIG");
        std::env::set_var("CRUSH_GLOBAL_CONFIG", &path);
        let resolved = crush_home().unwrap();
        match prev {
            Some(v) => std::env::set_var("CRUSH_GLOBAL_CONFIG", v),
            None => std::env::remove_var("CRUSH_GLOBAL_CONFIG"),
        }
        assert_eq!(resolved, path);
    }

    #[test]
    fn opencode_plugins_dir_ends_correctly() {
        let p = opencode_plugins_dir().expect("path resolved");
        assert!(p.ends_with(PathBuf::from(".config").join("opencode").join("plugins")));
    }

    #[test]
    fn mcp_paths_end_correctly() {
        let _guard = env_lock().lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let home_path = home.path().to_path_buf();
        let prev_home = std::env::var_os("HOME");
        let prev_userprofile = std::env::var_os("USERPROFILE");

        #[cfg(windows)]
        std::env::set_var("USERPROFILE", &home_path);
        #[cfg(not(windows))]
        std::env::set_var("HOME", &home_path);

        assert!(claude_mcp_user_file()
            .unwrap()
            .to_string_lossy()
            .ends_with(".claude.json"));
        assert!(kilo_config_file()
            .unwrap()
            .ends_with(PathBuf::from(".config").join("kilo").join("kilo.jsonc")));
        assert!(cline_mcp_global_file().unwrap().ends_with(
            PathBuf::from(".cline")
                .join("data")
                .join("settings")
                .join("cline_mcp_settings.json")
        ));
        assert!(legacy_cline_mcp_global_file().unwrap().ends_with(
            PathBuf::from("Code")
                .join("User")
                .join("globalStorage")
                .join("saoudrizwan.claude-dev")
                .join("settings")
                .join("cline_mcp_settings.json")
        ));
        assert!(roo_mcp_global_file().unwrap().ends_with(
            PathBuf::from("Code")
                .join("User")
                .join("globalStorage")
                .join("rooveterinaryinc.roo-cline")
                .join("settings")
                .join("mcp_settings.json")
        ));
        assert!(antigravity_mcp_global_file().unwrap().ends_with(
            PathBuf::from(".gemini")
                .join("config")
                .join("mcp_config.json")
        ));
        assert!(antigravity_cli_mcp_global_file().unwrap().ends_with(
            PathBuf::from(".gemini")
                .join("antigravity-cli")
                .join("mcp_config.json")
        ));
        assert!(windsurf_mcp_global_file().unwrap().ends_with(
            PathBuf::from(".codeium")
                .join("windsurf")
                .join("mcp_config.json")
        ));

        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match prev_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
