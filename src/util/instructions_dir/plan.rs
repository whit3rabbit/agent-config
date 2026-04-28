//! Side-effect-free planners for installing instructions per
//! [`InstructionPlacement`] variant.

use std::path::Path;

use crate::error::AgentConfigError;
use crate::plan::{has_refusal, PlannedChange, RefusalReason};
use crate::spec::{InstructionPlacement, InstructionSpec};
use crate::util::{fs_atomic, md_block, ownership, planning};

use super::{
    ensure_trailing_newline, instruction_file_path, ledger_path, validate_name, validate_relative,
};

/// Plan installing an instruction without mutating disk.
pub(crate) fn plan_install(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: Option<&Path>,
    instruction_dir: Option<&Path>,
    reference_line: Option<&str>,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    spec.validate()?;
    validate_name(&spec.name)?;

    match spec.placement {
        InstructionPlacement::ReferencedFile => {
            let host = host_file.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "host_file (required for ReferencedFile)",
            })?;
            let dir = instruction_dir.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "instruction_dir (required for ReferencedFile)",
            })?;
            let ref_line = reference_line.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "reference_line (required for ReferencedFile)",
            })?;
            validate_relative(ref_line)?;
            plan_referenced(config_dir, spec, host, dir, ref_line)
        }
        InstructionPlacement::InlineBlock => {
            let host = host_file.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "host_file (required for InlineBlock)",
            })?;
            plan_inline(config_dir, spec, host)
        }
        InstructionPlacement::StandaloneFile => {
            let dir = instruction_dir.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "instruction_dir (required for StandaloneFile)",
            })?;
            plan_standalone(config_dir, spec, dir)
        }
    }
}

fn plan_referenced(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: &Path,
    instruction_dir: &Path,
    reference_line: &str,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    let mut changes = Vec::new();
    let led = ledger_path(config_dir);
    let instr_path = instruction_file_path(instruction_dir, &spec.name);

    let actual_owner = ownership::owner_of(&led, &spec.name)?;
    let instr_existed = instr_path.exists();
    let adopting = spec.adopt_unowned && instr_existed && actual_owner.is_none();

    match (actual_owner.as_deref(), instr_existed) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) if !spec.adopt_unowned => {
            changes.push(PlannedChange::Refuse {
                path: Some(instr_path),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    let body = ensure_trailing_newline(&spec.body);
    planning::plan_write_file(&mut changes, &instr_path, body.as_bytes(), false)?;

    // Plan the include reference in host file.
    planning::plan_markdown_upsert_instruction(
        &mut changes,
        host_file,
        &spec.name,
        reference_line,
    )?;

    let owner_changed = actual_owner.as_deref() != Some(spec.owner_tag.as_str());
    let file_would_change = changes.iter().any(|c| {
        matches!(
            c,
            PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
        )
    });
    if !has_refusal(&changes) && (owner_changed || file_would_change || adopting) {
        planning::plan_write_ledger(&mut changes, &led, &spec.name, &spec.owner_tag);
    }

    Ok(changes)
}

fn plan_inline(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: &Path,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    let mut changes = Vec::new();
    let led = ledger_path(config_dir);

    let actual_owner = ownership::owner_of(&led, &spec.name)?;
    let host_content = fs_atomic::read_to_string_or_empty(host_file)?;
    let block_exists = md_block::contains_instruction(&host_content, &spec.name)
        || md_block::contains_legacy_instruction(&host_content, &spec.name);
    let adopting = spec.adopt_unowned && block_exists && actual_owner.is_none();

    match (actual_owner.as_deref(), block_exists) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) if !spec.adopt_unowned => {
            changes.push(PlannedChange::Refuse {
                path: Some(host_file.to_path_buf()),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    planning::plan_markdown_upsert_instruction(&mut changes, host_file, &spec.name, &spec.body)?;

    let owner_changed = actual_owner.as_deref() != Some(spec.owner_tag.as_str());
    let file_would_change = changes.iter().any(|c| {
        matches!(
            c,
            PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
        )
    });
    if !has_refusal(&changes) && (owner_changed || file_would_change || adopting) {
        planning::plan_write_ledger(&mut changes, &led, &spec.name, &spec.owner_tag);
    }

    Ok(changes)
}

fn plan_standalone(
    config_dir: &Path,
    spec: &InstructionSpec,
    instruction_dir: &Path,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    let mut changes = Vec::new();
    let led = ledger_path(config_dir);
    let instr_path = instruction_file_path(instruction_dir, &spec.name);

    let actual_owner = ownership::owner_of(&led, &spec.name)?;
    let instr_existed = instr_path.exists();
    let adopting = spec.adopt_unowned && instr_existed && actual_owner.is_none();

    match (actual_owner.as_deref(), instr_existed) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) if !spec.adopt_unowned => {
            changes.push(PlannedChange::Refuse {
                path: Some(instr_path),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    let body = ensure_trailing_newline(&spec.body);
    planning::plan_write_file(&mut changes, &instr_path, body.as_bytes(), false)?;

    let owner_changed = actual_owner.as_deref() != Some(spec.owner_tag.as_str());
    let file_would_change = changes.iter().any(|c| {
        matches!(
            c,
            PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
        )
    });
    if !has_refusal(&changes) && (owner_changed || file_would_change || adopting) {
        planning::plan_write_ledger(&mut changes, &led, &spec.name, &spec.owner_tag);
    }

    Ok(changes)
}
