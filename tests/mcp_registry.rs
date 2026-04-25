//! Public-API smoke test for the MCP surface — parallel to `tests/registry.rs`.

use ai_hooker::{mcp_by_id, mcp_capable, McpSpec, Scope};
use std::collections::HashSet;

#[test]
fn mcp_capable_includes_every_pr1_agent() {
    let ids: HashSet<_> = mcp_capable().into_iter().map(|i| i.id()).collect();
    for expected in ["claude", "cursor", "gemini", "codex", "opencode"] {
        assert!(
            ids.contains(expected),
            "missing MCP-capable integration: {expected}"
        );
    }
}

#[test]
fn mcp_capable_excludes_prompt_only_agents() {
    let ids: HashSet<_> = mcp_capable().into_iter().map(|i| i.id()).collect();
    for not_expected in ["copilot", "cline", "roo", "kilocode", "antigravity"] {
        assert!(
            !ids.contains(not_expected),
            "{not_expected} unexpectedly appears in mcp_capable"
        );
    }
}

#[test]
fn mcp_capable_subset_of_all_integrations() {
    // Every MCP-capable id must also exist in the main registry.
    let main_ids: HashSet<_> = ai_hooker::all().into_iter().map(|i| i.id()).collect();
    for agent in mcp_capable() {
        assert!(
            main_ids.contains(agent.id()),
            "{} is in mcp_capable but not in registry::all",
            agent.id()
        );
    }
}

#[test]
fn mcp_by_id_returns_each_agent() {
    for id in ["claude", "cursor", "gemini", "codex", "opencode"] {
        let agent = mcp_by_id(id).expect(id);
        assert_eq!(agent.id(), id);
    }
}

#[test]
fn mcp_by_id_returns_none_for_unsupported() {
    // Cline doesn't implement MCP.
    assert!(mcp_by_id("cline").is_none());
    // Copilot doesn't implement MCP.
    assert!(mcp_by_id("copilot").is_none());
    // Bogus id.
    assert!(mcp_by_id("does-not-exist").is_none());
}

#[test]
fn full_mcp_round_trip_per_agent() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    let spec = McpSpec::builder("smoketest-server")
        .owner("smoketest-app")
        .stdio("npx", ["-y", "@example/server"])
        .env("FOO", "bar")
        .build();

    for id in ["claude", "cursor", "gemini", "codex", "opencode"] {
        let agent = mcp_by_id(id).expect(id);

        // First install.
        let report = agent.install_mcp(&scope, &spec).unwrap();
        assert!(
            !report.created.is_empty() || !report.patched.is_empty(),
            "{id} install_mcp wrote nothing"
        );

        // Idempotent re-install.
        let r2 = agent.install_mcp(&scope, &spec).unwrap();
        assert!(r2.already_installed, "{id} re-install_mcp not idempotent");

        // Detection.
        assert!(
            agent.is_mcp_installed(&scope, "smoketest-server").unwrap(),
            "{id} should detect installed server"
        );

        // Uninstall.
        let unreport = agent
            .uninstall_mcp(&scope, "smoketest-server", "smoketest-app")
            .unwrap();
        assert!(
            !unreport.removed.is_empty()
                || !unreport.patched.is_empty()
                || !unreport.restored.is_empty(),
            "{id} uninstall_mcp wrote nothing"
        );

        // No longer detected.
        assert!(
            !agent.is_mcp_installed(&scope, "smoketest-server").unwrap(),
            "{id} should not detect uninstalled server"
        );
    }
}

#[test]
fn invalid_mcp_name_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    let err = agent
        .uninstall_mcp(&scope, "bad name with spaces", "myapp")
        .unwrap_err();
    assert!(matches!(err, ai_hooker::HookerError::InvalidTag { .. }));
}

#[test]
fn invalid_owner_tag_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    let err = agent
        .uninstall_mcp(&scope, "ok-name", "bad owner")
        .unwrap_err();
    assert!(matches!(err, ai_hooker::HookerError::InvalidTag { .. }));
}
