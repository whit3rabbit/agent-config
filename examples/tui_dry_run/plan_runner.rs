//! Dispatches the canonical specs against the right surface trait per tab,
//! and renders `PlannedChange` variants for display.

use agent_config::{
    by_id, instruction_capable, mcp_capable, skill_capable, AgentConfigError, InstallPlan,
    PlanStatus, PlannedChange, Scope,
};

use crate::specs::{self, Tab, HOOK_AGENTS};

/// One row in the agent list: stable id + display name.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub id: &'static str,
    pub display: &'static str,
}

/// Enumerate the agents shown in the left pane for a given tab.
///
/// SKILLS / MCP / INSTRUCTIONS read from the matching `*_capable()`
/// registry. HOOKS uses the hand-maintained `HOOK_AGENTS` slice (see the
/// note on that constant in `specs.rs`). The Surface traits only expose
/// `id()`, so the display name comes from `by_id(id)` (which routes
/// through the `Integration` trait where `display_name()` lives).
pub fn agents_for(tab: Tab) -> Vec<AgentRow> {
    let ids: Vec<&'static str> = match tab {
        Tab::Skills => skill_capable().into_iter().map(|a| a.id()).collect(),
        Tab::Mcp => mcp_capable().into_iter().map(|a| a.id()).collect(),
        Tab::Instructions => instruction_capable().into_iter().map(|a| a.id()).collect(),
        Tab::Hooks => HOOK_AGENTS.to_vec(),
    };
    ids.into_iter()
        .map(|id| AgentRow {
            id,
            display: by_id(id).map(|a| a.display_name()).unwrap_or(id),
        })
        .collect()
}

/// Build a plan for one (tab, agent_id, scope) combination by invoking the
/// matching `plan_install_*` method on the right trait.
///
/// Returns `Err` if the agent id is not registered for the tab's surface;
/// callers should treat that as "row went stale" and skip it.
pub fn plan_for(tab: Tab, agent_id: &str, scope: &Scope) -> Result<InstallPlan, AgentConfigError> {
    match tab {
        Tab::Skills => {
            let agent =
                agent_config::skill_by_id(agent_id).ok_or(AgentConfigError::MissingSpecField {
                    id: "<tui_dry_run>",
                    field: "skill_capable agent",
                })?;
            agent.plan_install_skill(scope, &specs::skill_spec())
        }
        Tab::Mcp => {
            let agent =
                agent_config::mcp_by_id(agent_id).ok_or(AgentConfigError::MissingSpecField {
                    id: "<tui_dry_run>",
                    field: "mcp_capable agent",
                })?;
            agent.plan_install_mcp(scope, &specs::mcp_spec())
        }
        Tab::Instructions => {
            let agent = agent_config::instruction_by_id(agent_id).ok_or(
                AgentConfigError::MissingSpecField {
                    id: "<tui_dry_run>",
                    field: "instruction_capable agent",
                },
            )?;
            agent.plan_install_instruction(scope, &specs::instruction_spec_for(agent_id))
        }
        Tab::Hooks => {
            let agent = by_id(agent_id).ok_or(AgentConfigError::MissingSpecField {
                id: "<tui_dry_run>",
                field: "registered agent",
            })?;
            agent.plan_install(scope, &specs::hook_spec())
        }
    }
}

/// One displayable line for a single `PlannedChange`. Mirrors the format
/// used in `examples/dry_run_plan.rs::summarise_changes`.
pub fn render_change(change: &PlannedChange) -> String {
    match change {
        PlannedChange::CreateFile { path } => format!("create  : {}", path.display()),
        PlannedChange::PatchFile { path } => format!("patch   : {}", path.display()),
        PlannedChange::RemoveFile { path } => format!("remove  : {}", path.display()),
        PlannedChange::CreateDir { path } => format!("mkdir   : {}", path.display()),
        PlannedChange::RemoveDir { path } => format!("rmdir   : {}", path.display()),
        PlannedChange::WriteLedger { path, key, owner } => {
            format!("ledger  : {} <- {key}={owner}", path.display())
        }
        PlannedChange::RemoveLedgerEntry { path, key } => {
            format!("ledger- : {} <- {key}", path.display())
        }
        PlannedChange::CreateBackup { backup, target } => {
            format!("backup  : {} <- {}", backup.display(), target.display())
        }
        PlannedChange::RestoreBackup { backup, target } => {
            format!("restore : {} -> {}", backup.display(), target.display())
        }
        PlannedChange::SetPermissions { path, mode } => {
            format!("chmod   : {} {:o}", path.display(), mode)
        }
        PlannedChange::NoOp { path, reason } => {
            format!("noop    : {} ({reason})", path.display())
        }
        PlannedChange::Refuse { reason, path } => match path {
            Some(p) => format!("refuse  : {reason:?} ({})", p.display()),
            None => format!("refuse  : {reason:?}"),
        },
        other => format!("other   : {other:?}"),
    }
}

/// Aggregate counts across the plans produced by a bulk-run pass.
#[derive(Debug, Default, Clone)]
pub struct Aggregate {
    pub agents: usize,
    pub creates: usize,
    pub patches: usize,
    pub ledger_writes: usize,
    pub perms: usize,
    pub no_ops: usize,
    pub refused: usize,
    pub errored: usize,
    /// First error message encountered, for the toast's second line.
    pub first_error: Option<String>,
}

impl Aggregate {
    pub fn record(&mut self, plan: &Result<InstallPlan, AgentConfigError>) {
        self.agents += 1;
        match plan {
            Ok(plan) => {
                if matches!(plan.status, PlanStatus::Refused) {
                    self.refused += 1;
                    return;
                }
                for change in &plan.changes {
                    match change {
                        PlannedChange::CreateFile { .. } | PlannedChange::CreateDir { .. } => {
                            self.creates += 1
                        }
                        PlannedChange::PatchFile { .. } => self.patches += 1,
                        PlannedChange::WriteLedger { .. } => self.ledger_writes += 1,
                        PlannedChange::SetPermissions { .. } => self.perms += 1,
                        PlannedChange::NoOp { .. } => self.no_ops += 1,
                        PlannedChange::Refuse { .. } => self.refused += 1,
                        _ => {}
                    }
                }
            }
            Err(e) => {
                self.errored += 1;
                if self.first_error.is_none() {
                    self.first_error = Some(format!("{e}"));
                }
            }
        }
    }

    /// First-line summary suitable for a toast.
    pub fn summary_line(&self) -> String {
        format!(
            "Planned: {creates} create, {patches} patch, {ledger} ledger, {refused} refused ({agents} agents)",
            creates = self.creates,
            patches = self.patches,
            ledger = self.ledger_writes,
            refused = self.refused,
            agents = self.agents,
        )
    }
}
