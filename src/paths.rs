//! Cross-platform resolution of the per-user directories that AI harnesses use.
//!
//! Most harnesses on macOS/Linux store config under `$HOME/.<name>` (dotdir
//! convention) rather than the XDG config dir. On Windows, `dirs::home_dir`
//! returns `%USERPROFILE%`, which is what those harnesses ship with too.

use std::path::PathBuf;

use crate::error::AgentConfigError;

/// Returns the user's home directory or a [`AgentConfigError::PathResolution`] if
/// the platform doesn't expose one.
pub fn home_dir() -> Result<PathBuf, AgentConfigError> {
    dirs::home_dir().ok_or_else(|| {
        AgentConfigError::PathResolution("could not determine user home directory".into())
    })
}

/// Returns `$XDG_CONFIG_HOME` (or its platform default) — used by OpenCode.
///
/// On macOS this is `~/Library/Application Support`. OpenCode, however, uses
/// `~/.config/opencode` even on macOS, so callers that need OpenCode's path
/// should prefer [`opencode_plugins_dir`] which encodes that quirk.
pub fn config_dir() -> Result<PathBuf, AgentConfigError> {
    dirs::config_dir().ok_or_else(|| {
        AgentConfigError::PathResolution("could not determine user config directory".into())
    })
}

/// `~/.claude` (all platforms).
pub fn claude_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".claude"))
}

/// `~/.cursor` (all platforms).
pub fn cursor_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".cursor"))
}

/// `~/.gemini` (all platforms).
pub fn gemini_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".gemini"))
}

/// `$CODEX_HOME` if set, else `~/.codex`.
pub fn codex_home() -> Result<PathBuf, AgentConfigError> {
    if let Some(h) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(h));
    }
    Ok(home_dir()?.join(".codex"))
}

/// `~/.openclaw` (all platforms).
pub fn openclaw_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".openclaw"))
}

/// `~/.hermes` (all platforms).
pub fn hermes_home() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".hermes"))
}

/// OpenCode forces its plugin directory under `~/.config/opencode/plugins`
/// regardless of platform conventions. Returns that path.
pub fn opencode_plugins_dir() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".config").join("opencode").join("plugins"))
}

/// `~/.config/opencode/opencode.json` — OpenCode's main config, where the
/// object-based `mcp` map lives.
pub fn opencode_config_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?
        .join(".config")
        .join("opencode")
        .join("opencode.json"))
}

/// `~/.config/kilo/kilo.jsonc` — Kilo Code's global JSONC config.
pub fn kilo_config_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".config").join("kilo").join("kilo.jsonc"))
}

/// `~/.claude.json` — Claude Code's user/local MCP config file.
pub fn claude_mcp_user_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?.join(".claude.json"))
}

/// `~/.cursor/mcp.json` — Cursor's MCP user-config file.
pub fn cursor_mcp_user_file() -> Result<PathBuf, AgentConfigError> {
    Ok(cursor_home()?.join("mcp.json"))
}

/// VS Code globalStorage directory for an extension in the stable `Code`
/// profile.
pub fn vscode_global_storage(extension_id: &str) -> Result<PathBuf, AgentConfigError> {
    Ok(config_dir()?
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join(extension_id))
}

/// Cline's global MCP settings file inside VS Code globalStorage.
pub fn cline_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(vscode_global_storage("saoudrizwan.claude-dev")?
        .join("settings")
        .join("cline_mcp_settings.json"))
}

/// Roo Code's global MCP settings file inside VS Code globalStorage.
pub fn roo_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(vscode_global_storage("rooveterinaryinc.roo-cline")?
        .join("settings")
        .join("mcp_settings.json"))
}

/// `~/.gemini/antigravity/mcp_config.json` — Antigravity's global MCP config.
pub fn antigravity_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(gemini_home()?.join("antigravity").join("mcp_config.json"))
}

/// `~/.codeium/windsurf/mcp_config.json` — Windsurf's global MCP config.
pub fn windsurf_mcp_global_file() -> Result<PathBuf, AgentConfigError> {
    Ok(home_dir()?
        .join(".codeium")
        .join("windsurf")
        .join("mcp_config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_is_resolvable_in_tests() {
        // CI environments always have $HOME set; smoke check that we don't panic.
        let _ = home_dir().expect("home dir on test host");
    }

    #[test]
    fn codex_home_respects_env_var() {
        // Mutating env vars races with parallel tests; CODEX_HOME has no other
        // reader in this crate, so the race is benign here.
        std::env::set_var("CODEX_HOME", "/tmp/codex-test-home");
        assert_eq!(codex_home().unwrap(), PathBuf::from("/tmp/codex-test-home"));
        std::env::remove_var("CODEX_HOME");
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
    }

    #[test]
    fn opencode_plugins_dir_ends_correctly() {
        let p = opencode_plugins_dir().expect("path resolved");
        assert!(p.ends_with(PathBuf::from(".config").join("opencode").join("plugins")));
    }

    #[test]
    fn mcp_paths_end_correctly() {
        assert!(claude_mcp_user_file()
            .unwrap()
            .to_string_lossy()
            .ends_with(".claude.json"));
        assert!(kilo_config_file()
            .unwrap()
            .ends_with(PathBuf::from(".config").join("kilo").join("kilo.jsonc")));
        assert!(cline_mcp_global_file().unwrap().ends_with(
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
                .join("antigravity")
                .join("mcp_config.json")
        ));
        assert!(windsurf_mcp_global_file().unwrap().ends_with(
            PathBuf::from(".codeium")
                .join("windsurf")
                .join("mcp_config.json")
        ));
    }
}
