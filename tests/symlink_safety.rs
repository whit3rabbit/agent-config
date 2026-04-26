//! Cross-agent regression: every MCP-capable and skill-capable integration
//! must reject symlinked Local config/root paths before doing any write.
//!
//! Phase A1 of the release-blockers plan: this file enumerates registry
//! members and asserts containment is enforced. Initially expected to fail
//! for the agents that skip `scope.ensure_contained` (antigravity, cline,
//! kilocode, roo, openclaw, hermes); subsequent A2-A7 tasks make them pass.

#![cfg(unix)]

use std::os::unix::fs::symlink;

use agent_config::{
    registry, AgentConfigError, McpSpec, Scope, ScopeKind, SkillSpec,
};
use tempfile::tempdir;

fn dummy_mcp() -> McpSpec {
    McpSpec::builder("symlink-test-server")
        .owner("symlink-test")
        .stdio("/bin/true", [] as [&str; 0])
        .build()
}

fn dummy_skill() -> SkillSpec {
    SkillSpec::builder("symlink-test-skill")
        .owner("symlink-test")
        .description("Symlink containment regression skill.")
        .body("body\n")
        .build()
}

#[test]
fn every_local_mcp_install_rejects_symlinked_root() {
    for integration in registry::mcp_capable() {
        // Skip Global-only MCP agents.
        if !integration.supported_mcp_scopes().contains(&ScopeKind::Local) {
            continue;
        }
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let aliased = project.path().join("aliased");
        symlink(outside.path(), &aliased).unwrap();

        let scope = Scope::Local(aliased);
        let result = integration.install_mcp(&scope, &dummy_mcp());

        assert!(
            matches!(result, Err(AgentConfigError::PathResolution(_))),
            "{} did not reject symlinked Local root on install_mcp: {:?}",
            integration.id(),
            result.map(|_| "ok").map_err(|e| format!("{e:?}")),
        );
    }
}

#[test]
fn every_local_mcp_uninstall_rejects_symlinked_root() {
    for integration in registry::mcp_capable() {
        if !integration.supported_mcp_scopes().contains(&ScopeKind::Local) {
            continue;
        }
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let aliased = project.path().join("aliased");
        symlink(outside.path(), &aliased).unwrap();

        let scope = Scope::Local(aliased);
        let result = integration.uninstall_mcp(&scope, "symlink-test-server", "symlink-test");

        assert!(
            matches!(result, Err(AgentConfigError::PathResolution(_))),
            "{} did not reject symlinked Local root on uninstall_mcp: {:?}",
            integration.id(),
            result.map(|_| "ok").map_err(|e| format!("{e:?}")),
        );
    }
}

#[test]
fn every_local_skill_install_rejects_symlinked_root() {
    for integration in registry::skill_capable() {
        if !integration.supported_skill_scopes().contains(&ScopeKind::Local) {
            continue;
        }
        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let aliased = project.path().join("aliased");
        symlink(outside.path(), &aliased).unwrap();

        let scope = Scope::Local(aliased);
        let result = integration.install_skill(&scope, &dummy_skill());

        assert!(
            matches!(result, Err(AgentConfigError::PathResolution(_))),
            "{} did not reject symlinked Local root on install_skill: {:?}",
            integration.id(),
            result.map(|_| "ok").map_err(|e| format!("{e:?}")),
        );
    }
}
