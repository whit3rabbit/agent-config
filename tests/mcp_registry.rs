//! Public-API smoke test for the MCP surface — parallel to `tests/registry.rs`.

use ai_hooker::{mcp_by_id, mcp_capable, McpSpec, Scope, ScopeKind};
use std::collections::HashSet;

#[test]
fn mcp_capable_includes_every_file_backed_agent() {
    let ids: HashSet<_> = mcp_capable().into_iter().map(|i| i.id()).collect();
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
    ] {
        assert!(
            ids.contains(expected),
            "missing MCP-capable integration: {expected}"
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
    ] {
        let agent = mcp_by_id(id).expect(id);
        assert_eq!(agent.id(), id);
    }
}

#[test]
fn mcp_by_id_returns_none_for_unsupported() {
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

    for id in [
        "claude",
        "cursor",
        "gemini",
        "codex",
        "copilot",
        "opencode",
        "roo",
        "windsurf",
        "kilocode",
        "antigravity",
    ] {
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
fn mcp_scope_sets_match_agent_contracts() {
    let cline = mcp_by_id("cline").unwrap();
    assert_eq!(cline.supported_mcp_scopes(), &[ScopeKind::Global]);

    for id in ["openclaw", "hermes"] {
        let agent = mcp_by_id(id).unwrap();
        assert_eq!(agent.supported_mcp_scopes(), &[ScopeKind::Global], "{id}");
    }

    let copilot = mcp_by_id("copilot").unwrap();
    assert!(copilot.supported_mcp_scopes().contains(&ScopeKind::Global));
    assert!(copilot.supported_mcp_scopes().contains(&ScopeKind::Local));

    for id in ["roo", "kilocode", "antigravity", "windsurf"] {
        let agent = mcp_by_id(id).unwrap();
        let scopes = agent.supported_mcp_scopes();
        assert!(scopes.contains(&ScopeKind::Global), "{id}");
        assert!(scopes.contains(&ScopeKind::Local), "{id}");
    }
}

#[test]
fn mcp_install_rejects_unsupported_scope() {
    let dir = tempfile::tempdir().unwrap();
    let local = Scope::Local(dir.path().to_path_buf());
    let spec = McpSpec::builder("smoketest-server")
        .owner("smoketest-app")
        .stdio("npx", ["-y", "@example/server"])
        .build();

    for id in ["cline", "openclaw", "hermes"] {
        let agent = mcp_by_id(id).unwrap();
        let err = agent.install_mcp(&local, &spec).unwrap_err();
        assert!(
            matches!(
                err,
                ai_hooker::HookerError::UnsupportedScope {
                    scope: ScopeKind::Local,
                    ..
                }
            ),
            "{id} should reject local MCP install"
        );
    }

    assert!(mcp_by_id("copilot")
        .unwrap()
        .supported_mcp_scopes()
        .contains(&ScopeKind::Global));
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
