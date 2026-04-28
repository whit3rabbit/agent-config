//! Per-placement shim helpers used by agents' `InstructionSurface`
//! implementations. Each agent provides an [`InlineLayout`] or
//! [`StandaloneLayout`] (the resolved per-scope paths) and these helpers
//! handle scope containment, status detection, plan construction, and
//! delegation to the placement-specific install/uninstall.
//!
//! `_plan_*` helpers accept a `Result<Layout, _>` so a path-resolution
//! `UnsupportedScope` error becomes a refused plan; other errors and the
//! mutating `_install` / `_uninstall` helpers keep error semantics.

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::{InstallPlan, PlanTarget, RefusalReason, UninstallPlan};
use crate::scope::Scope;
use crate::spec::{HookSpec, InstructionSpec};
use crate::status::{ConfigPresence, StatusReport};
use crate::util::{fs_atomic, md_block, ownership};

use super::{
    install::install, ledger_path, paths_for_status, plan::plan_install, uninstall::plan_uninstall,
    uninstall::uninstall, InlineLayout, StandaloneLayout,
};

/// Build a [`StatusReport`] for an InlineBlock instruction agent.
pub(crate) fn inline_status(
    layout: InlineLayout,
    name: &str,
    expected_owner: &str,
) -> Result<StatusReport, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    let led = ledger_path(&layout.config_dir);
    let content = fs_atomic::read_to_string_or_empty(&layout.host_file)?;
    let block_in_host = md_block::contains(&content, name);
    let presence = if block_in_host {
        ConfigPresence::Single
    } else {
        ConfigPresence::Absent
    };
    let recorded = ownership::owner_of(&led, name)?;
    Ok(StatusReport::for_instruction(
        name,
        layout.host_file,
        led,
        presence,
        expected_owner,
        recorded,
    ))
}

/// Plan an install for an InlineBlock instruction agent. An `UnsupportedScope`
/// path-resolution error is converted to a refused plan; other errors propagate.
pub(crate) fn inline_plan_install(
    integration_id: &'static str,
    scope: &Scope,
    layout: Result<InlineLayout, AgentConfigError>,
    spec: &InstructionSpec,
) -> Result<InstallPlan, AgentConfigError> {
    spec.validate()?;
    let target = PlanTarget::Instruction {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let layout = match layout {
        Ok(l) => l,
        Err(AgentConfigError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = plan_install(
        &layout.config_dir,
        spec,
        Some(&layout.host_file),
        None,
        None,
    )?;
    Ok(InstallPlan::from_changes(target, changes))
}

/// Plan an uninstall for an InlineBlock instruction agent.
pub(crate) fn inline_plan_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    layout: Result<InlineLayout, AgentConfigError>,
    name: &str,
    owner_tag: &str,
) -> Result<UninstallPlan, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Instruction {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let layout = match layout {
        Ok(l) => l,
        Err(AgentConfigError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = plan_uninstall(
        &layout.config_dir,
        name,
        owner_tag,
        Some(&layout.host_file),
        None,
    )?;
    Ok(UninstallPlan::from_changes(target, changes))
}

/// Install (or update) an instruction for an InlineBlock agent.
pub(crate) fn inline_install(
    scope: &Scope,
    layout: InlineLayout,
    spec: &InstructionSpec,
) -> Result<InstallReport, AgentConfigError> {
    spec.validate()?;
    scope.ensure_contained(&layout.host_file)?;
    install(
        scope,
        &layout.config_dir,
        spec,
        Some(&layout.host_file),
        None,
        None,
    )
}

/// Uninstall an instruction for an InlineBlock agent.
pub(crate) fn inline_uninstall(
    scope: &Scope,
    layout: InlineLayout,
    name: &str,
    owner_tag: &str,
) -> Result<UninstallReport, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    scope.ensure_contained(&layout.host_file)?;
    uninstall(
        scope,
        &layout.config_dir,
        name,
        owner_tag,
        Some(&layout.host_file),
        None,
    )
}

/// Build a [`StatusReport`] for a StandaloneFile instruction agent.
pub(crate) fn standalone_status(
    layout: StandaloneLayout,
    name: &str,
    expected_owner: &str,
) -> Result<StatusReport, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    let (instr_path, led) = paths_for_status(&layout.config_dir, &layout.instruction_dir, name);
    let presence = if instr_path.exists() {
        ConfigPresence::Single
    } else {
        ConfigPresence::Absent
    };
    let recorded = ownership::owner_of(&led, name)?;
    Ok(StatusReport::for_instruction(
        name,
        instr_path,
        led,
        presence,
        expected_owner,
        recorded,
    ))
}

/// Plan an install for a StandaloneFile instruction agent. An
/// `UnsupportedScope` path-resolution error is converted to a refused plan.
pub(crate) fn standalone_plan_install(
    integration_id: &'static str,
    scope: &Scope,
    layout: Result<StandaloneLayout, AgentConfigError>,
    spec: &InstructionSpec,
) -> Result<InstallPlan, AgentConfigError> {
    spec.validate()?;
    let target = PlanTarget::Instruction {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let layout = match layout {
        Ok(l) => l,
        Err(AgentConfigError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = plan_install(
        &layout.config_dir,
        spec,
        None,
        Some(&layout.instruction_dir),
        None,
    )?;
    Ok(InstallPlan::from_changes(target, changes))
}

/// Plan an uninstall for a StandaloneFile instruction agent.
pub(crate) fn standalone_plan_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    layout: Result<StandaloneLayout, AgentConfigError>,
    name: &str,
    owner_tag: &str,
) -> Result<UninstallPlan, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Instruction {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let layout = match layout {
        Ok(l) => l,
        Err(AgentConfigError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = plan_uninstall(
        &layout.config_dir,
        name,
        owner_tag,
        None,
        Some(&layout.instruction_dir),
    )?;
    Ok(UninstallPlan::from_changes(target, changes))
}

/// Install (or update) an instruction for a StandaloneFile agent.
pub(crate) fn standalone_install(
    scope: &Scope,
    layout: StandaloneLayout,
    spec: &InstructionSpec,
) -> Result<InstallReport, AgentConfigError> {
    spec.validate()?;
    let target_file = layout.instruction_dir.join(format!("{}.md", spec.name));
    scope.ensure_contained(&target_file)?;
    install(
        scope,
        &layout.config_dir,
        spec,
        None,
        Some(&layout.instruction_dir),
        None,
    )
}

/// Uninstall an instruction for a StandaloneFile agent.
pub(crate) fn standalone_uninstall(
    scope: &Scope,
    layout: StandaloneLayout,
    name: &str,
    owner_tag: &str,
) -> Result<UninstallReport, AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    scope.ensure_contained(&layout.instruction_dir)?;
    uninstall(
        scope,
        &layout.config_dir,
        name,
        owner_tag,
        None,
        Some(&layout.instruction_dir),
    )
}
