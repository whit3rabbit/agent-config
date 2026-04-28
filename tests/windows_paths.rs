//! Windows-only path resolution coverage.
//!
//! These tests confirm that `paths::*` honors `%USERPROFILE%` and `%APPDATA%`
//! and that integrations whose surface refuses on Windows do refuse before
//! mutating disk. They are gated on `#[cfg(windows)]`, so non-Windows test
//! runs ignore the file entirely.
//!
//! See README "Supported platforms" for the platform model these tests pin
//! down.

#![cfg(windows)]

mod common;

use std::path::PathBuf;

use agent_config::{
    paths, registry, Event, HookSpec, Matcher, PlanStatus, PlannedChange, RefusalReason, Scope,
};

use common::IsolatedGlobalEnv;

#[test]
fn home_dir_uses_userprofile() {
    let env = IsolatedGlobalEnv::new();
    assert_eq!(paths::home_dir().unwrap(), env.home_path());
}

#[test]
fn config_dir_prefers_xdg_config_home_when_set() {
    // Both XDG_CONFIG_HOME and APPDATA are set by IsolatedGlobalEnv. The
    // documented precedence (XDG first) must apply on Windows too: callers
    // who deliberately set XDG_CONFIG_HOME on Windows expect that override
    // to win.
    let env = IsolatedGlobalEnv::new();
    assert_eq!(paths::config_dir().unwrap(), env.xdg_config_path());
}

#[test]
fn config_dir_falls_back_to_appdata_when_xdg_unset() {
    let env = IsolatedGlobalEnv::new();
    std::env::remove_var("XDG_CONFIG_HOME");
    let resolved = paths::config_dir().unwrap();
    assert_eq!(resolved, env.appdata_path());
}

#[test]
fn vscode_global_storage_lands_under_appdata() {
    let env = IsolatedGlobalEnv::new();
    std::env::remove_var("XDG_CONFIG_HOME");
    let p = paths::vscode_global_storage("ext.id").unwrap();
    let expected = env
        .appdata_path()
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("ext.id");
    assert_eq!(p, expected);
}

#[test]
fn cline_mcp_global_file_resolves_under_appdata() {
    let env = IsolatedGlobalEnv::new();
    std::env::remove_var("XDG_CONFIG_HOME");
    let p = paths::cline_mcp_global_file().unwrap();
    let expected = env
        .appdata_path()
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("saoudrizwan.claude-dev")
        .join("settings")
        .join("cline_mcp_settings.json");
    assert_eq!(p, expected);
}

#[test]
fn roo_mcp_global_file_resolves_under_appdata() {
    let env = IsolatedGlobalEnv::new();
    std::env::remove_var("XDG_CONFIG_HOME");
    let p = paths::roo_mcp_global_file().unwrap();
    let expected = env
        .appdata_path()
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("rooveterinaryinc.roo-cline")
        .join("settings")
        .join("mcp_settings.json");
    assert_eq!(p, expected);
}

#[test]
fn dotdir_homes_resolve_under_userprofile() {
    let env = IsolatedGlobalEnv::new();
    let cases: &[(
        &str,
        fn() -> Result<PathBuf, agent_config::AgentConfigError>,
    )] = &[
        (".claude", paths::claude_home),
        (".cursor", paths::cursor_home),
        (".gemini", paths::gemini_home),
        (".openclaw", paths::openclaw_home),
        (".hermes", paths::hermes_home),
    ];
    for (suffix, resolver) in cases {
        let resolved = resolver().expect("resolver");
        let expected = env.home_path().join(suffix);
        assert_eq!(
            &resolved, &expected,
            "{suffix} should resolve under USERPROFILE"
        );
    }
}

#[test]
fn codex_home_uses_env_override_on_windows() {
    let env = IsolatedGlobalEnv::new();
    let resolved = paths::codex_home().unwrap();
    assert_eq!(resolved, env.home_path().join(".codex"));
}

#[test]
fn windsurf_and_antigravity_mcp_files_resolve_under_userprofile() {
    let env = IsolatedGlobalEnv::new();
    assert_eq!(
        paths::windsurf_mcp_global_file().unwrap(),
        env.home_path()
            .join(".codeium")
            .join("windsurf")
            .join("mcp_config.json")
    );
    assert_eq!(
        paths::antigravity_mcp_global_file().unwrap(),
        env.home_path()
            .join(".gemini")
            .join("antigravity")
            .join("mcp_config.json")
    );
}

#[test]
fn cline_hook_install_is_refused_on_windows() {
    // Cross-references the cline.rs unit tests but exercises the public
    // registry path so a downstream consumer would observe the refusal.
    let _env = IsolatedGlobalEnv::new();
    let project = tempfile::tempdir().unwrap();
    let scope = Scope::Local(project.path().to_path_buf());
    let cline = registry::by_id("cline").expect("cline integration registered");

    let spec = HookSpec::builder("plan-app")
        .command_program("noop", [] as [&str; 0])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .build();

    let plan = cline.plan_install(&scope, &spec).unwrap();
    assert_eq!(plan.status, PlanStatus::Refused);
    let reason = plan
        .changes
        .iter()
        .find_map(|c| match c {
            PlannedChange::Refuse { reason, .. } => Some(*reason),
            _ => None,
        })
        .expect("plan should include a refusal");
    assert!(matches!(reason, RefusalReason::UnsupportedPlatform));

    let err = cline.install(&scope, &spec).unwrap_err();
    assert!(matches!(
        err,
        agent_config::AgentConfigError::UnsupportedPlatform { id: "cline", .. }
    ));
    assert!(!project.path().join(".clinerules/hooks/PreToolUse").exists());
}
