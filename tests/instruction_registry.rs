#![allow(unused_must_use)]

//! Public-API smoke test for the instruction surface — parallel to
//! `tests/registry.rs`, `tests/mcp_registry.rs`, and `tests/skill_registry.rs`.
//!
//! Covers:
//!
//! * registry exposure (every implementer is also in `all()`)
//! * `instruction_by_id` lookup
//! * round-trip install / idempotent re-install / uninstall in each agent's
//!   natural placement mode
//! * scope refusal: calling `Scope::Global` on a local-only agent
//! * owner-mismatch refusal under `Scope::Local`

use agent_config::{
    instruction_by_id, instruction_capable, AgentConfigError, InstructionPlacement,
    InstructionSpec, Scope, ScopeKind,
};
use std::collections::HashSet;

const INSTRUCTION_CAPABLE: &[&str] = &[
    "claude",
    "cline",
    "roo",
    "kilocode",
    "windsurf",
    "antigravity",
    "gemini",
    "codex",
    "copilot",
    "codebuddy",
    "amp",
    "forge",
    "qodercli",
    "qwen",
    "junie",
    "trae",
    "openclaw",
    "hermes",
];

/// Agents that take both `Scope::Global` and `Scope::Local`.
const GLOBAL_AND_LOCAL: &[&str] = &[
    "claude",
    "gemini",
    "codex",
    "codebuddy",
    "amp",
    "forge",
    "qodercli",
    "qwen",
];

/// Agents whose instruction surface is local-scoped only.
const LOCAL_ONLY: &[&str] = &[
    "cline",
    "roo",
    "kilocode",
    "windsurf",
    "antigravity",
    "copilot",
    "junie",
    "trae",
    "openclaw",
    "hermes",
];

/// Pick the placement mode each agent natively supports. Picking a placement
/// the agent doesn't pass paths for would surface `MissingSpecField` from the
/// shared helper instead of exercising the round trip.
fn placement_for(id: &str) -> InstructionPlacement {
    match id {
        "claude" => InstructionPlacement::ReferencedFile,
        "cline" | "roo" | "kilocode" | "windsurf" | "antigravity" => {
            InstructionPlacement::StandaloneFile
        }
        _ => InstructionPlacement::InlineBlock,
    }
}

fn build_spec(name: &str, owner: &str, placement: InstructionPlacement) -> InstructionSpec {
    InstructionSpec::builder(name)
        .owner(owner)
        .placement(placement)
        .body("# Smoke\n\nA test instruction body.\n")
        .try_build()
        .unwrap()
}

#[test]
fn instruction_capable_lists_every_prompt_capable_agent() {
    let ids: HashSet<_> = instruction_capable().into_iter().map(|i| i.id()).collect();
    for &expected in INSTRUCTION_CAPABLE {
        assert!(
            ids.contains(expected),
            "missing instruction-capable integration: {expected}"
        );
    }
}

#[test]
fn instruction_capable_subset_of_all_integrations() {
    let main_ids: HashSet<_> = agent_config::all().into_iter().map(|i| i.id()).collect();
    for agent in instruction_capable() {
        assert!(
            main_ids.contains(agent.id()),
            "{} appears in instruction_capable but not in registry::all",
            agent.id()
        );
    }
}

#[test]
fn instruction_by_id_returns_each_agent() {
    for &id in INSTRUCTION_CAPABLE {
        let agent = instruction_by_id(id).expect(id);
        assert_eq!(agent.id(), id);
    }
}

#[test]
fn instruction_by_id_returns_none_for_non_instruction_agents() {
    // Cursor, OpenCode, iFlow, Tabnine have no prompt surface so they do not
    // implement `InstructionSurface`.
    assert!(instruction_by_id("cursor").is_none());
    assert!(instruction_by_id("opencode").is_none());
    assert!(instruction_by_id("iflow").is_none());
    assert!(instruction_by_id("tabnine").is_none());
    assert!(instruction_by_id("does-not-exist").is_none());
}

#[test]
fn full_instruction_round_trip_per_agent() {
    for &id in INSTRUCTION_CAPABLE {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        let agent = instruction_by_id(id).expect(id);
        let spec = build_spec("smoketest", "smoketest-app", placement_for(id));

        let report = agent.install_instruction(&scope, &spec).unwrap();
        assert!(
            !report.created.is_empty() || !report.patched.is_empty(),
            "{id} install_instruction wrote nothing"
        );

        let r2 = agent.install_instruction(&scope, &spec).unwrap();
        assert!(
            r2.already_installed,
            "{id} re-install_instruction not idempotent"
        );

        assert!(
            agent.is_instruction_installed(&scope, "smoketest").unwrap(),
            "{id} should detect installed instruction"
        );

        let urep = agent
            .uninstall_instruction(&scope, "smoketest", "smoketest-app")
            .unwrap();
        assert!(
            !urep.removed.is_empty() || !urep.patched.is_empty(),
            "{id} uninstall_instruction wrote nothing"
        );

        assert!(
            !agent.is_instruction_installed(&scope, "smoketest").unwrap(),
            "{id} should not detect uninstalled instruction"
        );
    }
}

#[test]
fn instruction_install_refuses_owner_mismatch_per_agent() {
    for &id in INSTRUCTION_CAPABLE {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        let agent = instruction_by_id(id).expect(id);
        let placement = placement_for(id);
        let owned = build_spec("owned", "app-a", placement);
        let stolen = build_spec("owned", "app-b", placement);

        agent.install_instruction(&scope, &owned).unwrap();
        let err = agent.install_instruction(&scope, &stolen).unwrap_err();
        assert!(
            matches!(err, AgentConfigError::NotOwnedByCaller { .. }),
            "{id} should refuse another owner, got {err:?}"
        );
        agent
            .uninstall_instruction(&scope, "owned", "app-a")
            .unwrap();
    }
}

#[test]
fn instruction_global_install_refuses_for_local_only_agents() {
    let spec = build_spec("smoke", "myapp", InstructionPlacement::StandaloneFile);
    for &id in LOCAL_ONLY {
        let agent = instruction_by_id(id).expect(id);
        if !agent
            .supported_instruction_scopes()
            .contains(&ScopeKind::Local)
            || agent
                .supported_instruction_scopes()
                .contains(&ScopeKind::Global)
        {
            continue;
        }
        let err = agent
            .install_instruction(&Scope::Global, &spec)
            .err()
            .unwrap_or_else(|| panic!("{id} should refuse Global scope"));
        assert!(
            matches!(
                err,
                AgentConfigError::UnsupportedScope {
                    scope: ScopeKind::Global,
                    ..
                }
            ),
            "{id} should refuse Global scope, got {err:?}"
        );
    }
}

#[test]
fn instruction_supported_scopes_reflect_capabilities() {
    for &id in GLOBAL_AND_LOCAL {
        let agent = instruction_by_id(id).expect(id);
        let scopes = agent.supported_instruction_scopes();
        assert!(
            scopes.contains(&ScopeKind::Global) && scopes.contains(&ScopeKind::Local),
            "{id} should support both scopes"
        );
    }
    for &id in LOCAL_ONLY {
        let agent = instruction_by_id(id).expect(id);
        let scopes = agent.supported_instruction_scopes();
        assert!(
            scopes.contains(&ScopeKind::Local) && !scopes.contains(&ScopeKind::Global),
            "{id} should be local-only"
        );
    }
}
