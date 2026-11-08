//! Agent home-directory resolution, mirroring `src/agents.ts` of the upstream
//! `skills` CLI (including its XDG-literal conventions on every platform).

use std::path::PathBuf;

/// The user's home directory (`$USERPROFILE` on Windows, `$HOME` elsewhere),
/// falling back to a best-effort default.
#[must_use]
pub fn home_dir() -> PathBuf {
    if let Some(home) = env_path("USERPROFILE").or_else(|| env_path("HOME")) {
        return home;
    }
    #[cfg(windows)]
    if let Some(profile) = home_fallback_windows() {
        return profile;
    }
    PathBuf::from("/")
}

#[cfg(windows)]
fn home_fallback_windows() -> Option<PathBuf> {
    // shutil-style fallback: %HOMEDRIVE%%HOMEPATH%
    match (env_path("HOMEDRIVE"), env_path("HOMEPATH")) {
        (Some(drive), Some(path)) => Some(drive.join(path.trim_start_matches('\\'))),
        _ => None,
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Trimmed string env var, as the upstream CLI reads overrides.
#[must_use]
pub fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// XDG configuration home: `$XDG_CONFIG_HOME` when set and absolute, else
/// `~/.config` (never `~/Library/Application Support`, matching `xdg-basedir`).
#[must_use]
pub fn config_home() -> PathBuf {
    if let Some(xdg) = env_path("XDG_CONFIG_HOME") {
        if xdg.is_absolute() {
            return xdg;
        }
    }
    home_dir().join(".config")
}

/// `$CODEX_HOME` or `~/.codex`.
#[must_use]
pub fn codex_home() -> PathBuf {
    env_path("CODEX_HOME").unwrap_or_else(|| home_dir().join(".codex"))
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
#[must_use]
pub fn claude_home() -> PathBuf {
    env_path("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home_dir().join(".claude"))
}

/// `$VIBE_HOME` or `~/.vibe`.
#[must_use]
pub fn vibe_home() -> PathBuf {
    env_path("VIBE_HOME").unwrap_or_else(|| home_dir().join(".vibe"))
}

/// `$HERMES_HOME` or `~/.hermes`.
#[must_use]
pub fn hermes_home() -> PathBuf {
    env_path("HERMES_HOME").unwrap_or_else(|| home_dir().join(".hermes"))
}

/// `$AUTOHAND_HOME` or `~/.autohand`.
#[must_use]
pub fn autohand_home() -> PathBuf {
    env_path("AUTOHAND_HOME").unwrap_or_else(|| home_dir().join(".autohand"))
}

/// `$GROK_HOME` or `~/.grok`.
#[must_use]
pub fn grok_home() -> PathBuf {
    env_path("GROK_HOME").unwrap_or_else(|| home_dir().join(".grok"))
}

/// `$SARVAM_HOME` or `~/.sarvam`.
#[must_use]
pub fn sarvam_home() -> PathBuf {
    env_path("SARVAM_HOME").unwrap_or_else(|| home_dir().join(".sarvam"))
}

/// `$APPDATA` or `~/AppData/Roaming`.
#[must_use]
pub fn appdata() -> PathBuf {
    env_path("APPDATA").unwrap_or_else(|| home_dir().join("AppData").join("Roaming"))
}

/// Reasonix home: `%APPDATA%/reasonix` on Windows, `~/.reasonix` elsewhere.
#[must_use]
pub fn reasonix_home() -> PathBuf {
    if cfg!(windows) {
        appdata().join("reasonix")
    } else {
        home_dir().join(".reasonix")
    }
}

/// `$FLATPAK_XDG_CONFIG_HOME`, if set.
#[must_use]
pub fn flatpak_config_home() -> Option<PathBuf> {
    env_path("FLATPAK_XDG_CONFIG_HOME")
}

/// The GitHub host selected by `GH_HOST` (validated), else `github.com`.
#[must_use]
pub fn github_host() -> String {
    let Some(configured) = env_trimmed("GH_HOST") else {
        return "github.com".to_string();
    };
    // Reject anything that is not a bare hostname.
    let lowered = configured.to_lowercase();
    let valid = !lowered.contains('/')
        && !lowered.contains(':')
        && !lowered.contains('?')
        && !lowered.contains('#')
        && !lowered.contains('@')
        && !lowered.contains('\\');
    if valid {
        lowered
    } else {
        "github.com".to_string()
    }
}

/// Whether `host` is github.com or the configured `GH_HOST`.
#[must_use]
pub fn is_github_host(host: &str) -> bool {
    let normalized = host.to_lowercase();
    normalized == "github.com" || normalized == github_host()
}

/// Home of the canonical skills store: `~/.agents/skills`.
#[must_use]
pub fn global_agents_skills_dir() -> PathBuf {
    home_dir().join(".agents").join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_host_validation() {
        // The env-dependent branches are exercised indirectly; the default is stable.
        assert!(is_github_host("github.com"));
        assert!(is_github_host("GitHub.COM"));
    }
}
