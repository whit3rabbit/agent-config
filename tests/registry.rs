//! End-to-end smoke tests against the public API surface.

use ai_hooker::{by_id, Event, HookSpec, Matcher, Scope};
use std::collections::HashSet;

#[test]
fn registry_exposes_every_supported_integration() {
    let ids: HashSet<_> = ai_hooker::all().into_iter().map(|i| i.id()).collect();

    for expected in [
        "claude",
        "cursor",
        "gemini",
        "openclaw",
        "hermes",
        "codex",
        "copilot",
        "opencode",
        "cline",
        "roo",
        "windsurf",
        "kilocode",
        "antigravity",
        "amp",
        "codebuddy",
        "forge",
        "iflow",
        "junie",
        "qodercli",
        "qwen",
        "tabnine",
        "trae",
    ] {
        assert!(ids.contains(expected), "missing integration: {expected}");
    }
}

#[test]
fn by_id_returns_each_registered_integration() {
    for id in [
        "claude",
        "cursor",
        "gemini",
        "openclaw",
        "hermes",
        "codex",
        "copilot",
        "opencode",
        "cline",
        "roo",
        "windsurf",
        "kilocode",
        "antigravity",
        "amp",
        "codebuddy",
        "forge",
        "iflow",
        "junie",
        "qodercli",
        "qwen",
        "tabnine",
        "trae",
    ] {
        let agent = by_id(id).expect(id);
        assert_eq!(agent.id(), id);
        assert!(!agent.display_name().is_empty());
    }
}

#[test]
fn by_id_returns_none_for_unknown() {
    assert!(by_id("does-not-exist").is_none());
}

#[test]
fn full_round_trip_against_a_local_project_for_three_agents() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    let spec = HookSpec::builder("smoketest")
        .command("smoketest hook")
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Use the smoketest harness for everything.")
        .build();

    for id in ["claude", "cursor", "cline"] {
        let agent = by_id(id).expect(id);
        let install = agent.install(&scope, &spec).unwrap();
        assert!(
            !install.created.is_empty() || !install.patched.is_empty(),
            "{id} should have written something"
        );

        // Idempotent re-install.
        let again = agent.install(&scope, &spec).unwrap();
        assert!(
            again.already_installed || (again.created.is_empty() && again.patched.is_empty()),
            "{id} re-install should be a no-op"
        );

        // Detection.
        assert!(agent.is_installed(&scope, "smoketest").unwrap(), "{id}");

        // Uninstall.
        let report = agent.uninstall(&scope, "smoketest").unwrap();
        assert!(
            !report.removed.is_empty() || !report.patched.is_empty() || !report.restored.is_empty(),
            "{id} uninstall should report at least one action"
        );

        // No longer detected.
        assert!(!agent.is_installed(&scope, "smoketest").unwrap(), "{id}");
    }
}

#[test]
fn custom_event_hooks_round_trip_for_json_hook_agents() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    for id in ["claude", "cursor", "gemini", "codex", "windsurf"] {
        let agent = by_id(id).expect(id);
        let spec = HookSpec::builder("customtest")
            .command("customtest hook")
            .matcher(Matcher::Bash)
            .event(Event::Custom("customEvent".into()))
            .build();

        agent.install(&scope, &spec).unwrap();
        assert!(
            agent.is_installed(&scope, "customtest").unwrap(),
            "{id} should detect custom-event hook"
        );

        let report = agent.uninstall(&scope, "customtest").unwrap();
        assert!(
            !report.removed.is_empty() || !report.patched.is_empty() || !report.restored.is_empty(),
            "{id} uninstall should remove custom-event hook"
        );
        assert!(
            !agent.is_installed(&scope, "customtest").unwrap(),
            "{id} should not detect removed custom-event hook"
        );
    }
}

#[test]
fn invalid_tag_is_rejected_at_install_time() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = by_id("cline").unwrap();

    let bad_spec = HookSpec::builder("not valid because spaces")
        .command("noop")
        .rules("x")
        .try_build();
    assert!(matches!(
        bad_spec.unwrap_err(),
        ai_hooker::HookerError::InvalidTag { .. }
    ));

    // And via direct uninstall path.
    let err = agent
        .uninstall(&scope, "ghost tag with spaces")
        .unwrap_err();
    assert!(matches!(err, ai_hooker::HookerError::InvalidTag { .. }));
}
