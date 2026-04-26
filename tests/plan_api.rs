//! Public dry-run plan API coverage.

use std::fs;
use std::path::{Path, PathBuf};

use ai_hooker::{
    all, mcp_by_id, mcp_capable, skill_by_id, skill_capable, Event, HookSpec, InstallStatus,
    Matcher, McpSpec, PlannedChange, RefusalReason, Scope, ScopeKind, SkillAsset, SkillSpec,
};

fn hook_spec(tag: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command("echo plan")
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Use the dry-run plan test rules.")
        .build()
}

fn bare_hook_spec(tag: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command("echo plan")
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .build()
}

fn mcp_spec(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .env("FOO", "bar")
        .build()
}

fn skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during dry-run plan tests.")
        .body("## Goal\nDo the thing.\n")
        .build()
}

fn executable_skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during dry-run plan tests.")
        .body("## Goal\nDo the thing.\n")
        .asset(SkillAsset {
            relative_path: PathBuf::from("scripts/run.sh"),
            bytes: b"#!/bin/sh\necho ok\n".to_vec(),
            executable: true,
        })
        .build()
}

fn temp_scope(dir: &tempfile::TempDir) -> Scope {
    Scope::Local(dir.path().to_path_buf())
}

fn assert_empty_dir(path: &Path) {
    assert!(
        fs::read_dir(path).unwrap().next().is_none(),
        "{} should remain empty after planning",
        path.display()
    );
}

fn has_create_dir(changes: &[PlannedChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::CreateDir { .. }))
}

fn has_create_file(changes: &[PlannedChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::CreateFile { .. }))
}

fn has_patch_file(changes: &[PlannedChange], expected: &Path) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::PatchFile { path, .. } if path == expected))
}

fn has_remove_file(changes: &[PlannedChange], expected: &Path) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::RemoveFile { path, .. } if path == expected))
}

fn has_set_permissions(changes: &[PlannedChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::SetPermissions { mode: 0o755, .. }))
}

fn has_refusal(changes: &[PlannedChange], expected: RefusalReason) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::Refuse { reason, .. } if *reason == expected))
}

#[test]
fn hook_plan_methods_are_exposed_for_every_registered_agent() {
    for agent in all() {
        let dir = tempfile::tempdir().unwrap();
        let scope = temp_scope(&dir);
        let tag = format!("plan-{}", agent.id());

        let install = agent.plan_install(&scope, &hook_spec(&tag)).unwrap();
        assert!(
            !matches!(install.status, InstallStatus::Refused),
            "{} hook install plan was refused: {:?}",
            agent.id(),
            install.changes
        );

        let uninstall = agent.plan_uninstall(&scope, &tag).unwrap();
        assert!(
            !matches!(uninstall.status, InstallStatus::Refused),
            "{} hook uninstall plan was refused: {:?}",
            agent.id(),
            uninstall.changes
        );
        assert_empty_dir(dir.path());
    }
}

#[test]
fn mcp_plan_methods_are_exposed_for_every_capable_agent() {
    for agent in mcp_capable() {
        let dir = tempfile::tempdir().unwrap();
        let scope = temp_scope(&dir);
        let plan = agent
            .plan_install_mcp(&scope, &mcp_spec("plan-server", "plan-app"))
            .unwrap();

        if agent.supported_mcp_scopes().contains(&ScopeKind::Local) {
            assert!(
                !matches!(plan.status, InstallStatus::Refused),
                "{} local MCP plan was refused: {:?}",
                agent.id(),
                plan.changes
            );
        } else {
            assert!(
                matches!(plan.status, InstallStatus::Refused),
                "{}",
                agent.id()
            );
            assert!(has_refusal(&plan.changes, RefusalReason::UnsupportedScope));
        }

        let uninstall = agent
            .plan_uninstall_mcp(&scope, "plan-server", "plan-app")
            .unwrap();
        if agent.supported_mcp_scopes().contains(&ScopeKind::Local) {
            assert!(
                !matches!(uninstall.status, InstallStatus::Refused),
                "{} local MCP uninstall plan was refused: {:?}",
                agent.id(),
                uninstall.changes
            );
        } else {
            assert!(
                matches!(uninstall.status, InstallStatus::Refused),
                "{}",
                agent.id()
            );
            assert!(has_refusal(
                &uninstall.changes,
                RefusalReason::UnsupportedScope
            ));
        }
        assert_empty_dir(dir.path());
    }
}

#[test]
fn skill_plan_methods_are_exposed_for_every_capable_agent() {
    for agent in skill_capable() {
        let dir = tempfile::tempdir().unwrap();
        let scope = temp_scope(&dir);
        let plan = agent
            .plan_install_skill(&scope, &skill_spec("plan-skill", "plan-app"))
            .unwrap();

        if agent.supported_skill_scopes().contains(&ScopeKind::Local) {
            assert!(
                !matches!(plan.status, InstallStatus::Refused),
                "{} local skill plan was refused: {:?}",
                agent.id(),
                plan.changes
            );
        } else {
            assert!(
                matches!(plan.status, InstallStatus::Refused),
                "{}",
                agent.id()
            );
            assert!(has_refusal(&plan.changes, RefusalReason::UnsupportedScope));
        }

        let uninstall = agent
            .plan_uninstall_skill(&scope, "plan-skill", "plan-app")
            .unwrap();
        if agent.supported_skill_scopes().contains(&ScopeKind::Local) {
            assert!(
                !matches!(uninstall.status, InstallStatus::Refused),
                "{} local skill uninstall plan was refused: {:?}",
                agent.id(),
                uninstall.changes
            );
        } else {
            assert!(
                matches!(uninstall.status, InstallStatus::Refused),
                "{}",
                agent.id()
            );
            assert!(has_refusal(
                &uninstall.changes,
                RefusalReason::UnsupportedScope
            ));
        }
        assert_empty_dir(dir.path());
    }
}

#[test]
fn missing_hook_config_reports_create_dir_and_create_file_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let claude = ai_hooker::by_id("claude").unwrap();

    let plan = claude.plan_install(&scope, &hook_spec("dryrun")).unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_create_dir(&plan.changes));
    assert!(has_create_file(&plan.changes));
    assert!(!dir.path().join(".claude").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
}

#[test]
fn existing_mcp_config_with_unrelated_entries_reports_patch_not_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cfg = dir.path().join(".mcp.json");
    let original = r#"{
  "mcpServers": {
    "user": { "command": "uvx", "args": [] }
  }
}
"#;
    fs::write(&cfg, original).unwrap();

    let claude = mcp_by_id("claude").unwrap();
    let plan = claude
        .plan_install_mcp(&scope, &mcp_spec("github", "plan-app"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_patch_file(&plan.changes, &cfg));
    assert!(!has_remove_file(&plan.changes, &cfg));
    assert_eq!(fs::read_to_string(&cfg).unwrap(), original);
    assert!(!dir.path().join(".ai-hooker-mcp.json").exists());
}

#[test]
fn identical_mcp_install_reports_noop() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let claude = mcp_by_id("claude").unwrap();
    let spec = mcp_spec("github", "plan-app");

    claude.install_mcp(&scope, &spec).unwrap();
    let plan = claude.plan_install_mcp(&scope, &spec).unwrap();

    assert!(matches!(plan.status, InstallStatus::NoOp));
    assert!(plan
        .changes
        .iter()
        .all(|change| matches!(change, PlannedChange::NoOp { .. })));
}

#[test]
fn mcp_owner_mismatch_is_refused_in_plan() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let claude = mcp_by_id("claude").unwrap();

    claude
        .install_mcp(&scope, &mcp_spec("github", "app-a"))
        .unwrap();
    let plan = claude
        .plan_install_mcp(&scope, &mcp_spec("github", "app-b"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::Refused));
    assert!(has_refusal(&plan.changes, RefusalReason::OwnerMismatch));
}

#[test]
fn hand_installed_mcp_entry_is_refused_in_plan() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cfg = dir.path().join(".mcp.json");
    fs::write(
        &cfg,
        r#"{"mcpServers":{"github":{"command":"npx","args":[]}}}"#,
    )
    .unwrap();

    let claude = mcp_by_id("claude").unwrap();
    let plan = claude
        .plan_install_mcp(&scope, &mcp_spec("github", "plan-app"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::Refused));
    assert!(has_refusal(
        &plan.changes,
        RefusalReason::UserInstalledEntry
    ));
}

#[test]
fn existing_backup_collision_is_refused_in_plan() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cfg = dir.path().join(".mcp.json");
    fs::write(
        &cfg,
        r#"{"mcpServers":{"user":{"command":"uvx","args":[]}}}"#,
    )
    .unwrap();
    fs::write(dir.path().join(".mcp.json.bak"), b"backup").unwrap();

    let claude = mcp_by_id("claude").unwrap();
    let plan = claude
        .plan_install_mcp(&scope, &mcp_spec("github", "plan-app"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::Refused));
    assert!(has_refusal(
        &plan.changes,
        RefusalReason::BackupAlreadyExists
    ));
}

#[test]
fn uninstall_final_mcp_entry_reports_config_removal() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cfg = dir.path().join(".mcp.json");
    let claude = mcp_by_id("claude").unwrap();

    claude
        .install_mcp(&scope, &mcp_spec("github", "plan-app"))
        .unwrap();
    let plan = claude
        .plan_uninstall_mcp(&scope, "github", "plan-app")
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_remove_file(&plan.changes, &cfg));
}

#[test]
fn uninstall_one_of_many_mcp_entries_reports_patch_not_removal() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cfg = dir.path().join(".mcp.json");
    let claude = mcp_by_id("claude").unwrap();

    claude
        .install_mcp(&scope, &mcp_spec("alpha", "plan-app"))
        .unwrap();
    claude
        .install_mcp(&scope, &mcp_spec("beta", "plan-app"))
        .unwrap();
    let plan = claude
        .plan_uninstall_mcp(&scope, "alpha", "plan-app")
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_patch_file(&plan.changes, &cfg));
    assert!(!has_remove_file(&plan.changes, &cfg));
}

#[test]
fn cline_hook_script_plan_reports_set_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let cline = ai_hooker::by_id("cline").unwrap();

    let plan = cline
        .plan_install(&scope, &bare_hook_spec("cline-script"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_set_permissions(&plan.changes));
    assert_empty_dir(dir.path());
}

#[test]
fn executable_skill_asset_plan_reports_set_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let claude = skill_by_id("claude").unwrap();

    let plan = claude
        .plan_install_skill(&scope, &executable_skill_spec("plan-skill", "plan-app"))
        .unwrap();

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_set_permissions(&plan.changes));
    assert_empty_dir(dir.path());
}
