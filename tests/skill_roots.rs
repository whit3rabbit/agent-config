//! Skills-root regression tests for the 2026-11 path migrations
//! (FINDINGS.md F1–F5, F7): every moved root installs into its canonical
//! location and still drains pre-migration installs via status/uninstall.
//!
//! Legacy locations are seeded by hand (skill directory plus a minimal
//! ownership ledger) because the legacy roots are no longer reachable as
//! install targets.

#![allow(unused_must_use)]

mod common;

use std::fs;
use std::path::Path;

use agent_config::{skill_by_id, Scope, SkillSpec};
use common::IsolatedGlobalEnv;

fn skill(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Path migration probe.")
        .body("## Goal\nLoad from the canonical root.\n")
        .build()
}

/// Seed a pre-migration install: `<root>/<name>/SKILL.md` plus an ownership
/// ledger entry without a content hash (v1-style), which uninstall accepts.
fn seed_legacy_skill(root: &Path, name: &str, owner: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        "---\nname: probe\n---\npre-migration body\n",
    )
    .unwrap();
    let ledger = root.join(".agent-config-skills.json");
    fs::write(
        &ledger,
        format!("{{\"version\":2,\"entries\":{{\"{name}\":{{\"owner\":\"{owner}\"}}}}}}"),
    )
    .unwrap();
}

// --- F1: Amp ---------------------------------------------------------------

#[test]
fn amp_global_skills_install_uses_config_agents_dir() {
    let env = IsolatedGlobalEnv::new();
    let amp = skill_by_id("amp").unwrap();

    amp.install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    let canonical = env.home_path().join(".config/agents/skills/probe");
    assert!(canonical.join("SKILL.md").exists());
    assert!(!env.home_path().join(".config/amp/skills/probe").exists());
    assert!(amp.is_skill_installed(&Scope::Global, "probe").unwrap());

    amp.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!canonical.exists());
}

#[test]
fn amp_global_status_and_uninstall_drain_legacy_amp_dir() {
    let env = IsolatedGlobalEnv::new();
    let legacy_root = env.home_path().join(".config/amp/skills");
    seed_legacy_skill(&legacy_root, "probe", "app");
    let amp = skill_by_id("amp").unwrap();

    assert!(amp.is_skill_installed(&Scope::Global, "probe").unwrap());
    amp.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!legacy_root.join("probe").exists());
    assert!(!legacy_root.join(".agent-config-skills.json").exists());
}

// --- F2: Antigravity -------------------------------------------------------

#[test]
fn antigravity_global_skills_install_uses_gemini_config_dir() {
    let env = IsolatedGlobalEnv::new();
    let antigravity = skill_by_id("antigravity").unwrap();

    antigravity
        .install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    let canonical = env.home_path().join(".gemini/config/skills/probe");
    assert!(canonical.join("SKILL.md").exists());
    assert!(!env
        .home_path()
        .join(".gemini/antigravity/skills/probe")
        .exists());
    assert!(antigravity
        .is_skill_installed(&Scope::Global, "probe")
        .unwrap());

    antigravity
        .uninstall_skill(&Scope::Global, "probe", "app")
        .unwrap();
    assert!(!canonical.exists());
}

#[test]
fn antigravity_global_status_and_uninstall_drain_legacy_dir() {
    let env = IsolatedGlobalEnv::new();
    let legacy_root = env.home_path().join(".gemini/antigravity/skills");
    seed_legacy_skill(&legacy_root, "probe", "app");
    let antigravity = skill_by_id("antigravity").unwrap();

    assert!(antigravity
        .is_skill_installed(&Scope::Global, "probe")
        .unwrap());
    antigravity
        .uninstall_skill(&Scope::Global, "probe", "app")
        .unwrap();
    assert!(!legacy_root.join("probe").exists());
}

// --- F3: Antigravity CLI ---------------------------------------------------

#[test]
fn antigravitycli_global_skills_install_uses_gemini_config_dir() {
    let env = IsolatedGlobalEnv::new();
    let cli = skill_by_id("antigravitycli").unwrap();

    cli.install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    let canonical = env.home_path().join(".gemini/config/skills/probe");
    assert!(canonical.join("SKILL.md").exists());
    assert!(!env
        .home_path()
        .join(".gemini/antigravity-cli/skills/probe")
        .exists());
    assert!(cli.is_skill_installed(&Scope::Global, "probe").unwrap());

    cli.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!canonical.exists());
}

#[test]
fn antigravitycli_global_status_and_uninstall_drain_legacy_cli_dir() {
    let env = IsolatedGlobalEnv::new();
    let legacy_root = env.home_path().join(".gemini/antigravity-cli/skills");
    seed_legacy_skill(&legacy_root, "probe", "app");
    let cli = skill_by_id("antigravitycli").unwrap();

    assert!(cli.is_skill_installed(&Scope::Global, "probe").unwrap());
    cli.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!legacy_root.join("probe").exists());
}

/// Both Antigravity integrations share the canonical global root, so an
/// install recorded by one is visible to the other through the shared
/// per-root ledger.
#[test]
fn antigravity_and_cli_share_canonical_global_skills_root() {
    let env = IsolatedGlobalEnv::new();
    let antigravity = skill_by_id("antigravity").unwrap();
    let cli = skill_by_id("antigravitycli").unwrap();

    antigravity
        .install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    assert!(cli.is_skill_installed(&Scope::Global, "probe").unwrap());
    // Same owner: uninstall through the sibling integration succeeds.
    cli.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!env.home_path().join(".gemini/config/skills/probe").exists());
}

// --- F4: Cline -------------------------------------------------------------

#[test]
fn cline_global_skills_install_uses_standard_agents_dir() {
    let env = IsolatedGlobalEnv::new();
    let cline = skill_by_id("cline").unwrap();

    cline.install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    let canonical = env.home_path().join(".agents/skills/probe");
    assert!(canonical.join("SKILL.md").exists());
    assert!(!env.home_path().join(".cline/skills/probe").exists());
    assert!(cline.is_skill_installed(&Scope::Global, "probe").unwrap());

    cline.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!canonical.exists());
}

#[test]
fn cline_local_skills_install_uses_standard_agents_dir() {
    let env = IsolatedGlobalEnv::new();
    let project = env.home_path().join("project");
    fs::create_dir_all(&project).unwrap();
    let cline = skill_by_id("cline").unwrap();
    let scope = Scope::Local(project.clone());

    cline.install_skill(&scope, &skill("probe", "app")).unwrap();

    assert!(project.join(".agents/skills/probe/SKILL.md").exists());
    assert!(!project.join(".cline/skills/probe").exists());
    assert!(cline.is_skill_installed(&scope, "probe").unwrap());

    cline.uninstall_skill(&scope, "probe", "app").unwrap();
    assert!(!project.join(".agents/skills/probe").exists());
}

#[test]
fn cline_local_status_and_uninstall_drain_legacy_cline_dir() {
    let env = IsolatedGlobalEnv::new();
    let project = env.home_path().join("project");
    let legacy_root = project.join(".cline/skills");
    seed_legacy_skill(&legacy_root, "probe", "app");
    let cline = skill_by_id("cline").unwrap();
    let scope = Scope::Local(project);

    assert!(cline.is_skill_installed(&scope, "probe").unwrap());
    cline.uninstall_skill(&scope, "probe", "app").unwrap();
    assert!(!legacy_root.join("probe").exists());
}

// --- F5: Kilo Code ---------------------------------------------------------

#[test]
fn kilocode_local_skills_install_uses_standard_agents_dir() {
    let env = IsolatedGlobalEnv::new();
    let project = env.home_path().join("project");
    fs::create_dir_all(&project).unwrap();
    let kilo = skill_by_id("kilocode").unwrap();
    let scope = Scope::Local(project.clone());

    kilo.install_skill(&scope, &skill("probe", "app")).unwrap();

    assert!(project.join(".agents/skills/probe/SKILL.md").exists());
    assert!(!project.join(".kilo/skills/probe").exists());
    assert!(kilo.is_skill_installed(&scope, "probe").unwrap());

    kilo.uninstall_skill(&scope, "probe", "app").unwrap();
    assert!(!project.join(".agents/skills/probe").exists());
}

#[test]
fn kilocode_local_status_and_uninstall_drain_legacy_kilo_dir() {
    let env = IsolatedGlobalEnv::new();
    let project = env.home_path().join("project");
    let legacy_root = project.join(".kilo/skills");
    seed_legacy_skill(&legacy_root, "probe", "app");
    let kilo = skill_by_id("kilocode").unwrap();
    let scope = Scope::Local(project);

    assert!(kilo.is_skill_installed(&scope, "probe").unwrap());
    kilo.uninstall_skill(&scope, "probe", "app").unwrap();
    assert!(!legacy_root.join("probe").exists());
}

/// Global stays at `~/.kilo/skills` — current Kilo builds still look there.
#[test]
fn kilocode_global_skills_root_is_unchanged() {
    let env = IsolatedGlobalEnv::new();
    let kilo = skill_by_id("kilocode").unwrap();

    kilo.install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    let expected = env.home_path().join(".kilo/skills/probe");
    assert!(expected.join("SKILL.md").exists());
    assert!(kilo.is_skill_installed(&Scope::Global, "probe").unwrap());

    kilo.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!expected.exists());
}

// --- F6: CLAUDE_CONFIG_DIR -------------------------------------------------

#[test]
fn claude_global_skills_respect_claude_config_dir() {
    let env = IsolatedGlobalEnv::new();
    let custom = env.home_path().join("claude-custom");
    std::env::set_var("CLAUDE_CONFIG_DIR", &custom);
    let claude = skill_by_id("claude").unwrap();

    claude
        .install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    assert!(custom.join("skills/probe/SKILL.md").exists());
    assert!(!env.home_path().join(".claude/skills/probe").exists());
    assert!(claude.is_skill_installed(&Scope::Global, "probe").unwrap());

    claude.uninstall_skill(&Scope::Global, "probe", "app").unwrap();
    assert!(!custom.join("skills/probe").exists());

    // Restore the IsolatedGlobalEnv-pinned value before the guard drops.
    std::env::set_var("CLAUDE_CONFIG_DIR", env.home_path().join(".claude"));
}

// --- F7: HERMES_HOME + project scope ---------------------------------------

#[test]
fn hermes_global_skills_respect_hermes_home() {
    let env = IsolatedGlobalEnv::new();
    let custom = env.home_path().join("hermes-custom");
    std::env::set_var("HERMES_HOME", &custom);
    let hermes = skill_by_id("hermes").unwrap();

    hermes
        .install_skill(&Scope::Global, &skill("probe", "app"))
        .unwrap();

    assert!(custom.join("skills/agent-config/probe/SKILL.md").exists());
    assert!(hermes.is_skill_installed(&Scope::Global, "probe").unwrap());

    hermes
        .uninstall_skill(&Scope::Global, "probe", "app")
        .unwrap();
    assert!(!custom.join("skills/agent-config/probe").exists());

    // Restore the IsolatedGlobalEnv-pinned value before the guard drops.
    std::env::set_var("HERMES_HOME", env.home_path().join(".hermes"));
}

#[test]
fn hermes_local_skills_install_under_project() {
    let env = IsolatedGlobalEnv::new();
    let project = env.home_path().join("project");
    fs::create_dir_all(&project).unwrap();
    let hermes = skill_by_id("hermes").unwrap();
    let scope = Scope::Local(project.clone());

    hermes.install_skill(&scope, &skill("probe", "app")).unwrap();

    assert!(project
        .join(".hermes/skills/agent-config/probe/SKILL.md")
        .exists());
    assert!(hermes.is_skill_installed(&scope, "probe").unwrap());

    hermes.uninstall_skill(&scope, "probe", "app").unwrap();
    assert!(!project.join(".hermes/skills/agent-config/probe").exists());
}
