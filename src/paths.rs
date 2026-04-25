//! Cross-platform resolution of the per-user directories that AI harnesses use.
//!
//! Most harnesses on macOS/Linux store config under `$HOME/.<name>` (dotdir
//! convention) rather than the XDG config dir. On Windows, `dirs::home_dir`
//! returns `%USERPROFILE%`, which is what those harnesses ship with too.

use std::path::PathBuf;

use crate::error::HookerError;

/// Returns the user's home directory or a [`HookerError::PathResolution`] if
/// the platform doesn't expose one.
pub fn home_dir() -> Result<PathBuf, HookerError> {
    dirs::home_dir().ok_or_else(|| {
        HookerError::PathResolution("could not determine user home directory".into())
    })
}

/// Returns `$XDG_CONFIG_HOME` (or its platform default) — used by OpenCode.
///
/// On macOS this is `~/Library/Application Support`. OpenCode, however, uses
/// `~/.config/opencode` even on macOS, so callers that need OpenCode's path
/// should prefer [`opencode_plugins_dir`] which encodes that quirk.
pub fn config_dir() -> Result<PathBuf, HookerError> {
    dirs::config_dir().ok_or_else(|| {
        HookerError::PathResolution("could not determine user config directory".into())
    })
}

/// `~/.claude` (all platforms).
pub fn claude_home() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?.join(".claude"))
}

/// `~/.cursor` (all platforms).
pub fn cursor_home() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?.join(".cursor"))
}

/// `~/.gemini` (all platforms).
pub fn gemini_home() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?.join(".gemini"))
}

/// `$CODEX_HOME` if set, else `~/.codex`.
pub fn codex_home() -> Result<PathBuf, HookerError> {
    if let Some(h) = std::env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(h));
    }
    Ok(home_dir()?.join(".codex"))
}

/// `~/.openclaw` (all platforms).
pub fn openclaw_home() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?.join(".openclaw"))
}

/// OpenCode forces its plugin directory under `~/.config/opencode/plugins`
/// regardless of platform conventions. Returns that path.
pub fn opencode_plugins_dir() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?.join(".config").join("opencode").join("plugins"))
}

/// `~/.config/opencode/opencode.json` — OpenCode's main config (where the
/// `mcp` array lives).
pub fn opencode_config_file() -> Result<PathBuf, HookerError> {
    Ok(home_dir()?
        .join(".config")
        .join("opencode")
        .join("opencode.json"))
}

/// `~/.claude/mcp.json` — the dedicated MCP user-config file.
///
/// Note: Anthropic's CLI also writes a `~/.claude.json` that contains
/// conversation transcripts plus arbitrary metadata; this library deliberately
/// keeps MCP config in the dedicated `mcp.json` file and does **not** touch
/// `~/.claude.json`.
pub fn claude_mcp_user_file() -> Result<PathBuf, HookerError> {
    Ok(claude_home()?.join("mcp.json"))
}

/// `~/.cursor/mcp.json` — Cursor's MCP user-config file.
pub fn cursor_mcp_user_file() -> Result<PathBuf, HookerError> {
    Ok(cursor_home()?.join("mcp.json"))
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
        let cases: Vec<(Result<PathBuf, HookerError>, &str)> = vec![
            (claude_home(), ".claude"),
            (cursor_home(), ".cursor"),
            (gemini_home(), ".gemini"),
            (openclaw_home(), ".openclaw"),
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
        assert!(p.to_string_lossy().ends_with(".config/opencode/plugins"));
    }
}
