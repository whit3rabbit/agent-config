//! Uninstall and uninstall-plan logic. The dispatcher cleans up whatever it
//! finds on disk (instruction file, host inline block, ledger entry) without
//! needing the original placement mode.

use std::path::Path;

use crate::error::AgentConfigError;
use crate::integration::UninstallReport;
use crate::plan::{PlannedChange, RefusalReason};
use crate::scope::Scope;
use crate::util::{file_lock, fs_atomic, md_block, ownership, planning, safe_fs};

use super::{instruction_file_path, ledger_path, validate_name, KIND};

/// Uninstall an instruction. Dispatches based on what is present on disk and
/// in the ledger.
pub(crate) fn uninstall(
    scope: &Scope,
    config_dir: &Path,
    name: &str,
    owner_tag: &str,
    host_file: Option<&Path>,
    instruction_dir: Option<&Path>,
) -> Result<UninstallReport, AgentConfigError> {
    validate_name(name)?;

    let led = ledger_path(config_dir);
    let instr_path = instruction_dir.map(|d| instruction_file_path(d, name));

    let instr_exists = instr_path.as_ref().is_some_and(|p| p.exists());
    let in_ledger = ownership::contains(&led, name)?;
    let block_exists = host_file.is_some_and(|h| {
        let content = fs_atomic::read_to_string_or_empty(h).unwrap_or_default();
        md_block::contains_instruction(&content, name)
            || md_block::contains_legacy_instruction(&content, name)
    });
    let anything_present = instr_exists || block_exists;

    if !anything_present && !in_ledger {
        return Ok(UninstallReport {
            not_installed: true,
            ..UninstallReport::default()
        });
    }

    // For uninstall, we need to know the placement mode to know what to clean
    // up. We just clean up everything we find: instruction file, host block,
    // and ledger entry.
    let lock_root = match (host_file, instruction_dir) {
        (Some(h), Some(_)) => h.parent().unwrap_or(h).to_path_buf(),
        (Some(h), None) => h.parent().unwrap_or(h).to_path_buf(),
        (None, Some(d)) => d.to_path_buf(),
        (None, None) => config_dir.to_path_buf(),
    };

    file_lock::with_lock(&lock_root, || {
        let mut report = UninstallReport::default();

        let instr_exists = instr_path.as_ref().is_some_and(|p| p.exists());
        let in_ledger = ownership::contains(&led, name)?;
        let block_exists = host_file.is_some_and(|h| {
            let content = fs_atomic::read_to_string_or_empty(h).unwrap_or_default();
            md_block::contains_instruction(&content, name)
                || md_block::contains_legacy_instruction(&content, name)
        });

        if !instr_exists && !block_exists && !in_ledger {
            report.not_installed = true;
            return Ok(report);
        }

        ownership::require_owner(&led, name, owner_tag, KIND, instr_exists || block_exists)?;

        // Order matters: host write must succeed before we drop the ledger
        // entry, otherwise a failed write leaves a dangling include with no
        // ledger record to repair against.
        if let (Some(host), true) = (host_file, block_exists) {
            let content = fs_atomic::read_to_string_or_empty(host)?;
            let (new_content, removed) = md_block::remove_instruction(&content, name);
            let (new_content, removed) = if removed {
                (new_content, true)
            } else {
                md_block::remove_legacy_instruction(&content, name)
            };
            if removed && new_content != content {
                safe_fs::write(scope, host, new_content.as_bytes(), true)?;
                report.patched.push(host.to_path_buf());
            }
        }

        if let Some(path) = &instr_path {
            if instr_exists {
                fs_atomic::ensure_contained(path, instruction_dir.unwrap_or(config_dir))?;
                safe_fs::remove_file(scope, path)?;
                report.removed.push(path.clone());
            }
        }

        ownership::record_uninstall(&led, name)?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    })
}

/// Plan uninstalling an instruction without mutating disk.
pub(crate) fn plan_uninstall(
    config_dir: &Path,
    name: &str,
    owner_tag: &str,
    host_file: Option<&Path>,
    instruction_dir: Option<&Path>,
) -> Result<Vec<PlannedChange>, AgentConfigError> {
    validate_name(name)?;

    let mut changes = Vec::new();
    let led = ledger_path(config_dir);
    let instr_path = instruction_dir.map(|d| instruction_file_path(d, name));

    let instr_exists = instr_path.as_ref().is_some_and(|p| p.exists());
    let actual_owner = ownership::owner_of(&led, name)?;

    if !instr_exists && actual_owner.is_none() {
        changes.push(PlannedChange::NoOp {
            path: instr_path.unwrap_or_else(|| led.clone()),
            reason: "instruction is already absent".into(),
        });
        return Ok(changes);
    }

    match (actual_owner.as_deref(), instr_exists) {
        (Some(owner), _) if owner != owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
            changes.push(PlannedChange::Refuse {
                path: instr_path.clone(),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    // Plan removing include block from host file.
    if let Some(host) = host_file {
        planning::plan_markdown_remove_instruction(&mut changes, host, name)?;
    }

    // Plan removing instruction file.
    if let Some(path) = &instr_path {
        if path.exists() {
            changes.push(PlannedChange::RemoveFile { path: path.clone() });
        }
    }

    if actual_owner.is_some() {
        planning::plan_remove_ledger_entry(&mut changes, &led, name);
    }

    Ok(changes)
}
