//! Public dry-run plan API coverage.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

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

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

struct IsolatedGlobalEnv {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
    home: tempfile::TempDir,
}

impl IsolatedGlobalEnv {
    fn new() -> Self {
        let lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let appdata = home.path().join("AppData").join("Roaming");
        let localappdata = home.path().join("AppData").join("Local");
        let xdg_config = home.path().join(".config");
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

        env::set_var("HOME", home.path());
        env::set_var("USERPROFILE", home.path());
        env::set_var("APPDATA", &appdata);
        env::set_var("LOCALAPPDATA", &localappdata);
        env::set_var("XDG_CONFIG_HOME", &xdg_config);
        env::set_var("CODEX_HOME", home.path().join(".codex"));

        Self {
            _lock: lock,
            saved,
            home,
        }
    }

    fn home_path(&self) -> &Path {
        self.home.path()
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

fn scope_label(kind: ScopeKind) -> &'static str {
    if kind == ScopeKind::Global {
        "global"
    } else {
        "local"
    }
}

fn run_for_supported_scopes(kinds: &[ScopeKind], mut run: impl FnMut(ScopeKind, Scope, &Path)) {
    for &kind in kinds {
        if kind == ScopeKind::Local {
            let dir = tempfile::tempdir().unwrap();
            let scope = temp_scope(&dir);
            run(kind, scope, dir.path());
        } else if kind == ScopeKind::Global {
            #[cfg(not(windows))]
            {
                let env = IsolatedGlobalEnv::new();
                run(kind, Scope::Global, env.home_path());
            }
        }
    }
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

fn has_create_backup(changes: &[PlannedChange]) -> bool {
    changes
        .iter()
        .any(|change| matches!(change, PlannedChange::CreateBackup { .. }))
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

fn assert_hook_plan_matches_actual(
    agent: &dyn ai_hooker::Integration,
    kind: ScopeKind,
    scope: &Scope,
) {
    let tag = format!("parity-{}-{}", agent.id(), scope_label(kind));
    let spec = hook_spec(&tag);

    let initial = agent.plan_install(scope, &spec).unwrap();
    assert!(
        matches!(initial.status, InstallStatus::WillChange),
        "{} {} hook install plan should change: {:?}",
        agent.id(),
        scope_label(kind),
        initial.changes
    );

    agent.install(scope, &spec).unwrap();
    let reinstall = agent.plan_install(scope, &spec).unwrap();
    assert!(
        matches!(reinstall.status, InstallStatus::NoOp),
        "{} {} hook install plan should match installed state: {:?}",
        agent.id(),
        scope_label(kind),
        reinstall.changes
    );

    let uninstall = agent.plan_uninstall(scope, &tag).unwrap();
    assert!(
        matches!(uninstall.status, InstallStatus::WillChange),
        "{} {} hook uninstall plan should see installed state: {:?}",
        agent.id(),
        scope_label(kind),
        uninstall.changes
    );

    agent.uninstall(scope, &tag).unwrap();
    let already_absent = agent.plan_uninstall(scope, &tag).unwrap();
    assert!(
        matches!(already_absent.status, InstallStatus::NoOp),
        "{} {} hook uninstall plan should match absent state: {:?}",
        agent.id(),
        scope_label(kind),
        already_absent.changes
    );
}

fn assert_mcp_plan_matches_actual(
    agent: &dyn ai_hooker::McpSurface,
    kind: ScopeKind,
    scope: &Scope,
) {
    let name = format!("parity-{}-{}", agent.id(), scope_label(kind));
    let owner = "plan-app";
    let spec = mcp_spec(&name, owner);

    let initial = agent.plan_install_mcp(scope, &spec).unwrap();
    assert!(
        matches!(initial.status, InstallStatus::WillChange),
        "{} {} MCP install plan should change: {:?}",
        agent.id(),
        scope_label(kind),
        initial.changes
    );

    agent.install_mcp(scope, &spec).unwrap();
    let reinstall = agent.plan_install_mcp(scope, &spec).unwrap();
    assert!(
        matches!(reinstall.status, InstallStatus::NoOp),
        "{} {} MCP install plan should match installed state: {:?}",
        agent.id(),
        scope_label(kind),
        reinstall.changes
    );

    let uninstall = agent.plan_uninstall_mcp(scope, &name, owner).unwrap();
    assert!(
        matches!(uninstall.status, InstallStatus::WillChange),
        "{} {} MCP uninstall plan should see installed state: {:?}",
        agent.id(),
        scope_label(kind),
        uninstall.changes
    );

    agent.uninstall_mcp(scope, &name, owner).unwrap();
    let already_absent = agent.plan_uninstall_mcp(scope, &name, owner).unwrap();
    assert!(
        matches!(already_absent.status, InstallStatus::NoOp),
        "{} {} MCP uninstall plan should match absent state: {:?}",
        agent.id(),
        scope_label(kind),
        already_absent.changes
    );
}

fn assert_skill_plan_matches_actual(
    agent: &dyn ai_hooker::SkillSurface,
    kind: ScopeKind,
    scope: &Scope,
) {
    let name = format!("parity-{}-{}", agent.id(), scope_label(kind));
    let owner = "plan-app";
    let spec = skill_spec(&name, owner);

    let initial = agent.plan_install_skill(scope, &spec).unwrap();
    assert!(
        matches!(initial.status, InstallStatus::WillChange),
        "{} {} skill install plan should change: {:?}",
        agent.id(),
        scope_label(kind),
        initial.changes
    );

    agent.install_skill(scope, &spec).unwrap();
    let reinstall = agent.plan_install_skill(scope, &spec).unwrap();
    assert!(
        matches!(reinstall.status, InstallStatus::NoOp),
        "{} {} skill install plan should match installed state: {:?}",
        agent.id(),
        scope_label(kind),
        reinstall.changes
    );

    let uninstall = agent.plan_uninstall_skill(scope, &name, owner).unwrap();
    assert!(
        matches!(uninstall.status, InstallStatus::WillChange),
        "{} {} skill uninstall plan should see installed state: {:?}",
        agent.id(),
        scope_label(kind),
        uninstall.changes
    );

    agent.uninstall_skill(scope, &name, owner).unwrap();
    let already_absent = agent.plan_uninstall_skill(scope, &name, owner).unwrap();
    assert!(
        matches!(already_absent.status, InstallStatus::NoOp),
        "{} {} skill uninstall plan should match absent state: {:?}",
        agent.id(),
        scope_label(kind),
        already_absent.changes
    );
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
fn existing_backup_allows_patch_without_new_backup_in_plan() {
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

    assert!(matches!(plan.status, InstallStatus::WillChange));
    assert!(has_patch_file(&plan.changes, &cfg));
    assert!(!has_create_backup(&plan.changes));
    assert!(!has_refusal(
        &plan.changes,
        RefusalReason::BackupAlreadyExists
    ));
}

#[test]
fn copilot_mcp_plan_matches_installed_shape() {
    let dir = tempfile::tempdir().unwrap();
    let scope = temp_scope(&dir);
    let copilot = mcp_by_id("copilot").unwrap();
    let spec = mcp_spec("github", "plan-app");

    copilot.install_mcp(&scope, &spec).unwrap();

    let reinstall = copilot.plan_install_mcp(&scope, &spec).unwrap();
    assert!(matches!(reinstall.status, InstallStatus::NoOp));

    let uninstall = copilot
        .plan_uninstall_mcp(&scope, "github", "plan-app")
        .unwrap();
    assert!(matches!(uninstall.status, InstallStatus::WillChange));
    assert!(has_remove_file(
        &uninstall.changes,
        &dir.path().join(".mcp.json")
    ));
}

#[test]
fn hook_plan_matches_actual_for_every_supported_agent_scope() {
    for agent in all() {
        run_for_supported_scopes(agent.supported_scopes(), |kind, scope, _root| {
            assert_hook_plan_matches_actual(agent.as_ref(), kind, &scope);
        });
    }
}

#[test]
fn mcp_plan_matches_actual_for_every_supported_agent_scope() {
    for agent in mcp_capable() {
        run_for_supported_scopes(agent.supported_mcp_scopes(), |kind, scope, _root| {
            assert_mcp_plan_matches_actual(agent.as_ref(), kind, &scope);
        });
    }
}

#[test]
fn skill_plan_matches_actual_for_every_supported_agent_scope() {
    for agent in skill_capable() {
        run_for_supported_scopes(agent.supported_skill_scopes(), |kind, scope, _root| {
            assert_skill_plan_matches_actual(agent.as_ref(), kind, &scope);
        });
    }
}

#[test]
fn mcp_plan_rejects_hostile_names_without_mutation() {
    let claude = mcp_by_id("claude").unwrap();

    for bad in ["", "../escape", "bad/name", "C:\\escape"] {
        let dir = tempfile::tempdir().unwrap();
        let scope = temp_scope(&dir);
        let err = claude
            .plan_uninstall_mcp(&scope, bad, "plan-app")
            .unwrap_err();
        assert!(
            matches!(err, ai_hooker::HookerError::InvalidTag { .. }),
            "expected invalid MCP name for {bad:?}"
        );
        assert_empty_dir(dir.path());
    }
}

#[test]
fn skill_plan_rejects_hostile_asset_paths_without_mutation() {
    let claude = skill_by_id("claude").unwrap();

    for bad in [PathBuf::from("../escape.txt"), PathBuf::from("/etc/passwd")] {
        let dir = tempfile::tempdir().unwrap();
        let scope = temp_scope(&dir);
        let spec = SkillSpec::builder("path-test")
            .owner("plan-app")
            .description("Use during path safety tests.")
            .body("## Goal\nStay inside the skill directory.\n")
            .asset(SkillAsset {
                relative_path: bad.clone(),
                bytes: b"nope".to_vec(),
                executable: false,
            })
            .build();

        let err = claude.plan_install_skill(&scope, &spec).unwrap_err();
        assert!(
            matches!(err, ai_hooker::HookerError::Other(_)),
            "expected unsafe asset path rejection for {bad:?}"
        );
        assert_empty_dir(dir.path());
    }
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

#[test]
#[cfg(unix)]
fn local_hook_install_rejects_symlinked_config_parent_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, project.join(".claude")).unwrap();

    let scope = Scope::Local(project.clone());
    let err = ai_hooker::by_id("claude")
        .unwrap()
        .install(&scope, &hook_spec("symlink-escape"))
        .unwrap_err();

    assert!(matches!(err, ai_hooker::HookerError::PathResolution(_)));
    assert!(!outside.join("settings.json").exists());
    assert!(!project.join("CLAUDE.md").exists());
}

#[test]
#[cfg(unix)]
fn local_mcp_install_rejects_symlinked_config_parent_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, project.join(".cursor")).unwrap();

    let scope = Scope::Local(project);
    let err = mcp_by_id("cursor")
        .unwrap()
        .install_mcp(&scope, &mcp_spec("symlink-escape", "plan-app"))
        .unwrap_err();

    assert!(matches!(err, ai_hooker::HookerError::PathResolution(_)));
    assert!(!outside.join("mcp.json").exists());
    assert!(!outside.join(".ai-hooker-mcp.json").exists());
}

#[test]
#[cfg(unix)]
fn local_skill_install_rejects_symlinked_skill_parent_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, project.join(".claude")).unwrap();

    let scope = Scope::Local(project);
    let err = skill_by_id("claude")
        .unwrap()
        .install_skill(&scope, &skill_spec("symlink-escape", "plan-app"))
        .unwrap_err();

    assert!(matches!(err, ai_hooker::HookerError::PathResolution(_)));
    assert!(!outside.join("skills").exists());
}
