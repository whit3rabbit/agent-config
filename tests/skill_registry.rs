//! Public-API smoke test for the skills surface — parallel to
//! `tests/registry.rs` and `tests/mcp_registry.rs`.

use ai_hooker::{skill_by_id, skill_capable, Scope, SkillSpec};
use std::collections::HashSet;

#[test]
fn skill_capable_lists_claude_and_antigravity() {
    let ids: HashSet<_> = skill_capable().into_iter().map(|i| i.id()).collect();
    for expected in ["claude", "antigravity"] {
        assert!(
            ids.contains(expected),
            "missing skill-capable integration: {expected}"
        );
    }
}

#[test]
fn skill_capable_excludes_non_skill_agents() {
    let ids: HashSet<_> = skill_capable().into_iter().map(|i| i.id()).collect();
    for not_expected in [
        "cursor", "gemini", "codex", "copilot", "opencode", "cline", "roo", "kilocode", "windsurf",
    ] {
        assert!(
            !ids.contains(not_expected),
            "{not_expected} unexpectedly appears in skill_capable"
        );
    }
}

#[test]
fn skill_capable_subset_of_all_integrations() {
    let main_ids: HashSet<_> = ai_hooker::all().into_iter().map(|i| i.id()).collect();
    for agent in skill_capable() {
        assert!(
            main_ids.contains(agent.id()),
            "{} is in skill_capable but not in registry::all",
            agent.id()
        );
    }
}

#[test]
fn skill_by_id_returns_each_agent() {
    for id in ["claude", "antigravity"] {
        let agent = skill_by_id(id).expect(id);
        assert_eq!(agent.id(), id);
    }
}

#[test]
fn skill_by_id_returns_none_for_unsupported() {
    assert!(skill_by_id("cursor").is_none());
    assert!(skill_by_id("does-not-exist").is_none());
}

#[test]
fn full_skill_round_trip_per_agent() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    let spec = SkillSpec::builder("smoketest-skill")
        .owner("smoketest-app")
        .description("A test skill for end-to-end verification.")
        .body("## Goal\nDo the thing.\n")
        .build();

    for id in ["claude", "antigravity"] {
        let agent = skill_by_id(id).expect(id);

        let report = agent.install_skill(&scope, &spec).unwrap();
        assert!(
            !report.created.is_empty() || !report.patched.is_empty(),
            "{id} install_skill wrote nothing"
        );

        // Idempotent re-install.
        let r2 = agent.install_skill(&scope, &spec).unwrap();
        assert!(r2.already_installed, "{id} re-install_skill not idempotent");

        // Detection.
        assert!(
            agent.is_skill_installed(&scope, "smoketest-skill").unwrap(),
            "{id} should detect installed skill"
        );

        // Uninstall.
        let u = agent
            .uninstall_skill(&scope, "smoketest-skill", "smoketest-app")
            .unwrap();
        assert!(
            !u.removed.is_empty(),
            "{id} uninstall_skill should report removed paths"
        );

        // No longer detected.
        assert!(
            !agent.is_skill_installed(&scope, "smoketest-skill").unwrap(),
            "{id} should not detect uninstalled skill"
        );
    }
}

#[test]
fn invalid_skill_name_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = skill_by_id("claude").unwrap();
    let err = agent
        .uninstall_skill(&scope, "bad name with spaces", "myapp")
        .unwrap_err();
    assert!(matches!(err, ai_hooker::HookerError::InvalidTag { .. }));
}
