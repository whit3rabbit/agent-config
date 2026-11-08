//! End-to-end installer tests: canonical copy + agent symlinks, scope rules,
//! and removal.

use std::path::Path;

use skills_store::installer::{
    default_install_mode, get_canonical_path, get_install_path, install_skill_dir,
    list_installed_skills, remove_skills, InstallOptions, InstallMode, RemoveOptions, Scope,
};
use skills_store::registry::agent_by_id;

fn skill_dir_with(name: &str, parent: &Path) -> std::path::PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill {name}\n---\nBody\n"),
    )
    .unwrap();
    dir
}

#[test]
fn universal_agent_installs_into_canonical_without_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    let skill = skill_dir_with("my-skill", tmp.path());
    let cursor = agent_by_id("cursor").unwrap();

    let mut warnings = Vec::new();
    let outcome = install_skill_dir(
        &skill,
        cursor,
        &InstallOptions { global: Scope::Project, cwd: Some(cwd.to_path_buf()), ..Default::default() },
        &mut warnings,
    );
    assert!(outcome.success, "{:?}", outcome.error);
    assert!(outcome.skipped || outcome.path.ends_with("my-skill"));
    let canonical = cwd.join(".agents/skills/my-skill");
    assert!(canonical.join("SKILL.md").exists());

    // Listed in project scope. Agent attribution depends on which agents the
    // host detects, so only assert the skill surfaced.
    let listed = list_installed_skills(Some(false), cwd, None);
    assert!(listed.iter().any(|s| s.name == "my-skill" && s.scope == Scope::Project));
}

#[test]
fn non_universal_agent_gets_symlink_when_root_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    // The agent root must exist or the project symlink is skipped.
    std::fs::create_dir_all(cwd.join(".windsurf")).unwrap();
    let skill = skill_dir_with("linked-skill", tmp.path());
    let windsurf = agent_by_id("windsurf").unwrap();

    let mut warnings = Vec::new();
    let outcome = install_skill_dir(
        &skill,
        windsurf,
        &InstallOptions {
            global: Scope::Project,
            cwd: Some(cwd.to_path_buf()),
            mode: Some(InstallMode::Symlink),
            ..Default::default()
        },
        &mut warnings,
    );
    assert!(outcome.success, "{:?}", outcome.error);
    assert!(!outcome.symlink_failed);
    let agent_dir = cwd.join(".windsurf/skills/linked-skill");
    assert!(agent_dir.join("SKILL.md").exists());
    let meta = std::fs::symlink_metadata(&agent_dir).unwrap();
    assert!(meta.file_type().is_symlink());
    let target = std::fs::read_link(&agent_dir).unwrap();
    assert!(target.ends_with(".agents/skills/linked-skill"), "{target:?}");
}

#[test]
fn project_symlink_skipped_without_agent_root() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    let skill = skill_dir_with("no-root-skill", tmp.path());
    let windsurf = agent_by_id("windsurf").unwrap();
    assert!(!windsurf.is_universal());

    let mut warnings = Vec::new();
    let outcome = install_skill_dir(
        &skill,
        windsurf,
        &InstallOptions { global: Scope::Project, cwd: Some(cwd.to_path_buf()), ..Default::default() },
        &mut warnings,
    );
    assert!(outcome.success);
    assert_eq!(outcome.skip_reason, Some("missing-agent-project-directory"));
    assert!(!cwd.join(".windsurf").exists());
}

#[test]
fn copy_mode_writes_every_agent_dir_independently() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    std::fs::create_dir_all(cwd.join(".windsurf")).unwrap();
    std::fs::create_dir_all(cwd.join(".roo")).unwrap();
    let skill = skill_dir_with("copied-skill", tmp.path());

    let mut warnings = Vec::new();
    for id in ["windsurf", "roo"] {
        let agent = agent_by_id(id).unwrap();
        let outcome = install_skill_dir(
            &skill,
            agent,
            &InstallOptions {
                global: Scope::Project,
                cwd: Some(cwd.to_path_buf()),
                mode: Some(InstallMode::Copy),
                ..Default::default()
            },
            &mut warnings,
        );
        assert!(outcome.success, "{:?}", outcome.error);
        assert_eq!(outcome.mode, InstallMode::Copy);
    }
    assert!(cwd.join(".windsurf/skills/copied-skill/SKILL.md").exists());
    assert!(cwd.join(".roo/skills/copied-skill/SKILL.md").exists());
    assert!(!cwd.join(".agents/skills/copied-skill").exists());
}

#[test]
fn default_install_mode_depends_on_target_span() {
    let windsurf = agent_by_id("windsurf").unwrap();
    let roo = agent_by_id("roo").unwrap();
    let claude = agent_by_id("claude-code").unwrap();
    assert_eq!(default_install_mode(&[windsurf]), InstallMode::Copy);
    assert_eq!(default_install_mode(&[windsurf, roo]), InstallMode::Symlink);
    assert_eq!(default_install_mode(&[claude]), InstallMode::Copy);
}

#[test]
fn global_install_lands_in_home_agents_skills_for_universal() {
    // We can't easily fake $HOME here (std::env::set_var is process-global and
    // racy under the test harness), so assert the path math instead.
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    let canonical = get_canonical_path("some-skill", true, cwd);
    assert!(canonical.starts_with(skills_store::paths::home_dir()));
    assert!(canonical.to_string_lossy().contains(".agents/skills/some-skill"));
}

#[test]
fn remove_cleans_agent_links_then_canonical_and_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    std::fs::create_dir_all(cwd.join(".windsurf")).unwrap();
    let skill = skill_dir_with("removable", tmp.path());
    let windsurf = agent_by_id("windsurf").unwrap();

    let mut warnings = Vec::new();
    let outcome = install_skill_dir(
        &skill,
        windsurf,
        &InstallOptions {
            global: Scope::Project,
            cwd: Some(cwd.to_path_buf()),
            mode: Some(InstallMode::Symlink),
            ..Default::default()
        },
        &mut warnings,
    );
    assert!(outcome.success);

    // Record a lock entry like `add` would.
    let mut lock = skills_store::read_local_lock(cwd).unwrap();
    lock.skills.insert(
        "removable".into(),
        skills_store::LocalSkillLockEntry {
            source: "owner/repo".into(),
            source_url: None,
            ref_: None,
            source_type: "github".into(),
            skill_path: Some("skills/removable/SKILL.md".into()),
            version: None,
            via: None,
            computed_hash: "hash".into(),
            subagents: None,
            well_known_digest: None,
            install_mode: None,
        },
    );
    skills_store::write_local_lock(&lock, cwd).unwrap();

    let results = remove_skills(
        &["removable".to_string()],
        &RemoveOptions {
            global: Scope::Project,
            cwd: Some(cwd.to_path_buf()),
            lockfile: Some(true),
            ..Default::default()
        },
        &mut warnings,
    )
    .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
    assert_eq!(results[0].source, "owner/repo");
    assert!(!cwd.join(".windsurf/skills/removable").exists());
    assert!(!cwd.join(".agents/skills/removable").exists());
    let lock = skills_store::read_local_lock(cwd).unwrap();
    assert!(!lock.skills.contains_key("removable"));
}

#[test]
fn resolve_skills_to_remove_matches_lock_keys_and_sources() {
    use skills_store::installer::resolve_skills_to_remove;
    use std::collections::HashMap;

    let mut source_by_key = HashMap::new();
    source_by_key.insert("ce:review".to_string(), "owner/repo".to_string());
    let resolved = resolve_skills_to_remove(
        &["ce-review".to_string(), "owner/repo".to_string()],
        &[],
        &["ce:review".to_string()],
        &source_by_key,
    );
    assert_eq!(resolved, vec!["ce:review".to_string()]);
}

#[test]
fn get_install_path_respects_agent_layout() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path();
    let windsurf = agent_by_id("windsurf").unwrap();
    let amp = agent_by_id("amp").unwrap();
    // Project: non-universal agents read their own dir; universal read canonical.
    assert!(get_install_path("x", windsurf, false, cwd, None)
        .starts_with(cwd.join(".windsurf/skills")));
    assert!(get_install_path("x", amp, false, cwd, None).starts_with(cwd.join(".agents/skills")));
}
