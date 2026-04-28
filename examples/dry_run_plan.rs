//! Preview install and uninstall changes before mutating the filesystem.
//!
//! Demonstrates:
//!
//! - `plan_install` returns an `InstallPlan` describing every file, ledger,
//!   and permission change without writing anything
//! - `PlanStatus::WillChange`, `NoOp`, and `Refused` discriminate the three
//!   high-level outcomes
//! - The same plan API exists on hooks, MCP, skills, and instructions
//!
//! Run: `cargo run --example dry_run_plan`

use agent_config::{
    by_id, mcp_by_id, Event, HookSpec, Matcher, McpSpec, PlanStatus, PlannedChange, Result, Scope,
};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());

    // 1. Plan a hook install. Nothing is written yet.
    let hook_spec = HookSpec::builder("myapp")
        .command_program("myapp", ["hook"])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Use myapp.")
        .try_build()?;
    let claude = by_id("claude").expect("claude is registered");
    let plan = claude.plan_install(&scope, &hook_spec)?;
    println!("hook plan_install");
    println!("  status  : {:?}", plan.status);
    summarise_changes(&plan.changes);
    assert!(matches!(plan.status, PlanStatus::WillChange));
    assert!(
        !project
            .path()
            .join(".claude")
            .join("settings.json")
            .exists(),
        "plan must not mutate the filesystem"
    );

    // 2. Apply, then re-plan: the same spec is now a no-op.
    let _ = claude.install(&scope, &hook_spec)?;
    let plan_again = claude.plan_install(&scope, &hook_spec)?;
    println!("\nhook plan_install (after apply)");
    println!("  status  : {:?}", plan_again.status);
    assert!(matches!(plan_again.status, PlanStatus::NoOp));

    // 3. Plan an MCP install with a likely-secret env value. The default
    //    secret policy refuses local-scope inline secrets, and the planner
    //    surfaces that as a `Refused` plan instead of pretending to succeed.
    let mcp = mcp_by_id("claude").expect("claude supports MCP");
    let bad_spec = McpSpec::builder("github")
        .owner("myapp")
        .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
        .env("GITHUB_TOKEN", "ghp_redacted")
        .try_build()?;
    let bad_plan = mcp.plan_install_mcp(&scope, &bad_spec)?;
    println!("\nmcp plan_install (inline secret)");
    println!("  status  : {:?}", bad_plan.status);
    summarise_changes(&bad_plan.changes);
    assert!(matches!(bad_plan.status, PlanStatus::Refused));

    // 4. Plan an uninstall that has nothing to remove. NoOp, not Refused.
    let uninstall_plan = claude.plan_uninstall(&scope, "never-installed")?;
    println!("\nhook plan_uninstall (absent)");
    println!("  status  : {:?}", uninstall_plan.status);
    assert!(matches!(uninstall_plan.status, PlanStatus::NoOp));

    let _ = claude.uninstall(&scope, "myapp")?;
    Ok(())
}

fn summarise_changes(changes: &[PlannedChange]) {
    for change in changes {
        match change {
            PlannedChange::CreateFile { path } => {
                println!("  create  : {}", path.display());
            }
            PlannedChange::PatchFile { path } => {
                println!("  patch   : {}", path.display());
            }
            PlannedChange::CreateDir { path } => {
                println!("  mkdir   : {}", path.display());
            }
            PlannedChange::WriteLedger { path, key, owner } => {
                println!("  ledger  : {} <- {key}={owner}", path.display());
            }
            PlannedChange::CreateBackup { backup, target } => {
                println!("  backup  : {} <- {}", backup.display(), target.display());
            }
            PlannedChange::SetPermissions { path, mode } => {
                println!("  chmod   : {} {:o}", path.display(), mode);
            }
            PlannedChange::Refuse { reason, path } => {
                println!("  refuse  : {reason:?} ({path:?})");
            }
            PlannedChange::NoOp { path, reason } => {
                println!("  noop    : {} ({reason})", path.display());
            }
            other => println!("  other   : {other:?}"),
        }
    }
}
