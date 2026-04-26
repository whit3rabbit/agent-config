//! Cross-agent regression: every MCP-capable and skill-capable integration
//! must reject symlinked Local config/skill paths before doing any write.
//!
//! Phase A1 of the release-blockers plan. We use the public dry-run planner
//! (`plan_install_mcp` / `plan_install_skill`) to discover the agent-specific
//! target path on a clean project, then replace the *first* directory
//! component under the Local root with a symlink to an outside directory.
//! The mutating call must reject the symlinked component with
//! `AgentConfigError::PathResolution` before touching disk.
//!
//! Why not symlink the Local root itself? `util::fs_atomic::ensure_contained`
//! canonicalizes the root before walking, so a symlinked root canonicalizes
//! to its real target and silently passes containment. The first interior
//! component is the right symlink trap: `canonicalize(root)` is real, but
//! `<root>/.cursor` (etc.) is a symlink and must be rejected.
//!
//! Initial expectations:
//! - Agents that correctly call `scope.ensure_contained` PASS today
//!   (Cursor, Gemini, Codex, Copilot, OpenCode, Windsurf, Amp, IFlow, Forge,
//!   Junie, QoderCli, Qwen, Tabnine, ...).
//! - Agents that skip the check FAIL today (Antigravity, Roo MCP-Local,
//!   OpenClaw skills-Local, KiloCode skills-Local). These tests encode the
//!   bug; A2-A7 will make them pass.
//! - Cline / Hermes / OpenClaw MCP are Global-only and are filtered out.
//! - Claude / Copilot MCP-Local write `<root>/.mcp.json` directly under
//!   the root, with no first subdir to trap. Those are skipped by this
//!   harness; per-agent tests in `tests/plan_api.rs` cover their containment.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

use agent_config::{
    registry, AgentConfigError, McpSpec, PlannedChange, Scope, ScopeKind, SkillSpec,
};
use tempfile::tempdir;

fn dummy_mcp() -> McpSpec {
    McpSpec::builder("symlink_test_server")
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

/// Find the first directory component of `target` under `root`. Returns
/// `None` when `target` sits directly under `root` (no subdir to trap),
/// when `target` is not under `root`, or when the first component is not a
/// normal name (`./`, `../`, `/`).
///
/// On macOS, `tempfile::tempdir()` lives under `/var/folders/...` which is a
/// symlink to `/private/var/folders/...`. If an agent's planner canonicalizes
/// the root before returning the target, `target.strip_prefix(root)` returns
/// `None` and the agent is silently skipped (defeating the regression). Try
/// stripping the canonical root as a fallback.
fn first_subdir_under(root: &Path, target: &Path) -> Option<PathBuf> {
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let rel = target
        .strip_prefix(root)
        .or_else(|_| target.strip_prefix(&canonical_root))
        .ok()?;
    let first = rel.components().next()?;
    if rel.components().count() < 2 {
        // Target is a direct child of root (e.g. `<root>/.mcp.json`); there
        // is no subdir to symlink. Skip.
        return None;
    }
    if let Component::Normal(name) = first {
        Some(root.join(name))
    } else {
        None
    }
}

/// Pick the first plannable target path from `changes`. Refused/no-op plans
/// have no `CreateFile`/`PatchFile` and are skipped by the caller.
fn first_target(changes: &[PlannedChange]) -> Option<PathBuf> {
    changes.iter().find_map(|c| match c {
        PlannedChange::CreateFile { path } | PlannedChange::PatchFile { path } => {
            Some(path.clone())
        }
        _ => None,
    })
}

/// Outcome of probing one agent. Aggregated across the registry so the test
/// reports every offender, not just the first one to fail.
#[derive(Debug)]
struct Outcome {
    id: &'static str,
    kind: &'static str, // "passed" | "failed" | "skipped"
    detail: String,
}

fn assert_no_failures(label: &str, outcomes: &[Outcome]) {
    let failures: Vec<_> = outcomes.iter().filter(|o| o.kind == "failed").collect();
    let passed: Vec<_> = outcomes
        .iter()
        .filter(|o| o.kind == "passed")
        .map(|o| o.id)
        .collect();
    let skipped: Vec<_> = outcomes
        .iter()
        .filter(|o| o.kind == "skipped")
        .map(|o| (o.id, o.detail.as_str()))
        .collect();
    eprintln!("[{label}] probed PASS: {passed:?}");
    eprintln!("[{label}] skipped: {skipped:?}");
    if !failures.is_empty() {
        let detail: Vec<String> = failures
            .iter()
            .map(|o| format!("{}: {}", o.id, o.detail))
            .collect();
        panic!(
            "[{label}] {} agent(s) did not reject symlinked Local subdir:\n  {}",
            failures.len(),
            detail.join("\n  ")
        );
    }
}

#[test]
fn every_local_mcp_install_rejects_symlinked_subdir() {
    let mut outcomes = Vec::new();
    for integration in registry::mcp_capable() {
        let id = integration.id();
        if !integration.supported_mcp_scopes().contains(&ScopeKind::Local) {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "global-only".into(),
            });
            continue;
        }

        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = project.path().to_path_buf();
        let scope = Scope::Local(root.clone());

        // Discover the agent's intended Local-scope target on a clean root.
        let plan = match integration.plan_install_mcp(&scope, &dummy_mcp()) {
            Ok(p) => p,
            Err(e) => {
                outcomes.push(Outcome {
                    id,
                    kind: "skipped",
                    detail: format!("plan_install_mcp errored: {e:?}"),
                });
                continue;
            }
        };
        let Some(target) = first_target(&plan.changes) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "no CreateFile/PatchFile in plan".into(),
            });
            continue;
        };
        let Some(first_dir) = first_subdir_under(&root, &target) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: format!("target {} sits at Local root", target.display()),
            });
            continue;
        };

        // Trap: replace `<root>/<first_dir>` with a symlink to `outside`.
        if first_dir.exists() {
            // Plans must not create directories. Stay defensive.
            fs::remove_dir_all(&first_dir).unwrap();
        }
        symlink(outside.path(), &first_dir).unwrap();

        let result = integration.install_mcp(&scope, &dummy_mcp());
        // Containment must run before any write. Even if a future regression
        // weakens the error type, no bytes should land in `outside`. We
        // record this as a failed outcome rather than asserting so the
        // aggregator still reports every offender in one run.
        let outside_empty = fs::read_dir(outside.path()).unwrap().next().is_none();
        if !outside_empty {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "wrote into outside dir despite returning {:?}",
                    result
                ),
            });
            continue;
        }
        if matches!(result, Err(AgentConfigError::PathResolution(_))) {
            outcomes.push(Outcome {
                id,
                kind: "passed",
                detail: String::new(),
            });
        } else {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "trap={} got={}",
                    first_dir.display(),
                    match result {
                        Ok(_) => "Ok(_)".to_string(),
                        Err(e) => format!("Err({e:?})"),
                    }
                ),
            });
        }
    }
    assert_no_failures("install_mcp", &outcomes);
}

// NOTE: This test runs against a fresh project with no existing config and
// no ledger entry, so a correctly-implemented `uninstall_mcp` rejects the
// symlinked subdir via `ensure_contained` *before* any disk read. Buggy
// agents that skip `ensure_contained` reach a no-op early-return path
// (nothing to remove) and return `Ok(_)`. Either way, observing
// `Err(PathResolution(_))` proves containment ran ahead of state probing.
#[test]
fn every_local_mcp_uninstall_rejects_symlinked_subdir() {
    let mut outcomes = Vec::new();
    for integration in registry::mcp_capable() {
        let id = integration.id();
        if !integration.supported_mcp_scopes().contains(&ScopeKind::Local) {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "global-only".into(),
            });
            continue;
        }

        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = project.path().to_path_buf();
        let scope = Scope::Local(root.clone());

        // Use the install plan to discover the path; uninstall on a fresh
        // project would yield a no-op or refused plan with no path.
        let plan = match integration.plan_install_mcp(&scope, &dummy_mcp()) {
            Ok(p) => p,
            Err(e) => {
                outcomes.push(Outcome {
                    id,
                    kind: "skipped",
                    detail: format!("plan_install_mcp errored: {e:?}"),
                });
                continue;
            }
        };
        let Some(target) = first_target(&plan.changes) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "no CreateFile/PatchFile in plan".into(),
            });
            continue;
        };
        let Some(first_dir) = first_subdir_under(&root, &target) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: format!("target {} sits at Local root", target.display()),
            });
            continue;
        };

        if first_dir.exists() {
            fs::remove_dir_all(&first_dir).unwrap();
        }
        symlink(outside.path(), &first_dir).unwrap();

        let result = integration.uninstall_mcp(&scope, "symlink_test_server", "symlink-test");
        // Containment must run before any disk read or write. Even on the
        // no-op early-return path, no bytes should land in `outside`.
        let outside_empty = fs::read_dir(outside.path()).unwrap().next().is_none();
        if !outside_empty {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "wrote into outside dir despite returning {:?}",
                    result
                ),
            });
            continue;
        }
        if matches!(result, Err(AgentConfigError::PathResolution(_))) {
            outcomes.push(Outcome {
                id,
                kind: "passed",
                detail: String::new(),
            });
        } else {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "trap={} got={}",
                    first_dir.display(),
                    match result {
                        Ok(_) => "Ok(_)".to_string(),
                        Err(e) => format!("Err({e:?})"),
                    }
                ),
            });
        }
    }
    assert_no_failures("uninstall_mcp", &outcomes);
}

#[test]
fn every_local_skill_install_rejects_symlinked_subdir() {
    let mut outcomes = Vec::new();
    for integration in registry::skill_capable() {
        let id = integration.id();
        if !integration.supported_skill_scopes().contains(&ScopeKind::Local) {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "global-only".into(),
            });
            continue;
        }

        let project = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let root = project.path().to_path_buf();
        let scope = Scope::Local(root.clone());

        let plan = match integration.plan_install_skill(&scope, &dummy_skill()) {
            Ok(p) => p,
            Err(e) => {
                outcomes.push(Outcome {
                    id,
                    kind: "skipped",
                    detail: format!("plan_install_skill errored: {e:?}"),
                });
                continue;
            }
        };
        let Some(target) = first_target(&plan.changes) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: "no CreateFile/PatchFile in plan".into(),
            });
            continue;
        };
        let Some(first_dir) = first_subdir_under(&root, &target) else {
            outcomes.push(Outcome {
                id,
                kind: "skipped",
                detail: format!("target {} sits at Local root", target.display()),
            });
            continue;
        };

        if first_dir.exists() {
            fs::remove_dir_all(&first_dir).unwrap();
        }
        symlink(outside.path(), &first_dir).unwrap();

        let result = integration.install_skill(&scope, &dummy_skill());
        // Containment must run before any write. Even if a future regression
        // weakens the error type, no bytes should land in `outside`.
        let outside_empty = fs::read_dir(outside.path()).unwrap().next().is_none();
        if !outside_empty {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "wrote into outside dir despite returning {:?}",
                    result
                ),
            });
            continue;
        }
        if matches!(result, Err(AgentConfigError::PathResolution(_))) {
            outcomes.push(Outcome {
                id,
                kind: "passed",
                detail: String::new(),
            });
        } else {
            outcomes.push(Outcome {
                id,
                kind: "failed",
                detail: format!(
                    "trap={} got={}",
                    first_dir.display(),
                    match result {
                        Ok(_) => "Ok(_)".to_string(),
                        Err(e) => format!("Err({e:?})"),
                    }
                ),
            });
        }
    }
    assert_no_failures("install_skill", &outcomes);
}
