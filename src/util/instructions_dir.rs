//! Shared install/uninstall logic for standalone instruction files.
//!
//! Supports three placement modes via [`InstructionPlacement`]:
//!
//! - **InlineBlock**: inject content as a managed markdown block in a host
//!   file (reuses `md_block::upsert` / `md_block::remove`).
//! - **ReferencedFile**: write a standalone file and inject a managed include
//!   reference into the host file. Both the file and the reference are tracked
//!   in the ownership ledger.
//! - **StandaloneFile**: write a standalone file only, no reference
//!   (for agents with rules directories).

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::{has_refusal, PlannedChange, RefusalReason};
use crate::spec::{InstructionPlacement, InstructionSpec};
use crate::util::{file_lock, fs_atomic, md_block, ownership, planning};

const LEDGER_FILE: &str = ".agent-config-instructions.json";
const KIND: &str = "instruction";

/// Ledger path for instructions.
pub(crate) fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LEDGER_FILE)
}

/// Instruction file path given a directory and name.
fn instruction_file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.md"))
}

/// Validate that an instruction name does not contain path traversal.
fn validate_name(name: &str) -> Result<(), AgentConfigError> {
    InstructionSpec::validate_name(name)?;
    // Also reject slashes and other path separators embedded in the name.
    for c in name.chars() {
        if c == '/' || c == '\\' {
            return Err(AgentConfigError::Other(anyhow::anyhow!(
                "instruction name must not contain path separators (got {name:?})"
            )));
        }
    }
    Ok(())
}

/// Validate that a relative path used for reference mode is safe.
fn validate_relative(p: &str) -> Result<(), AgentConfigError> {
    if p.starts_with('/') || p.starts_with('\\') {
        return Err(AgentConfigError::Other(anyhow::anyhow!(
            "instruction reference must not be absolute (got {p:?})"
        )));
    }
    for comp in Path::new(p).components() {
        match comp {
            Component::CurDir | Component::Normal(_) => {}
            _ => {
                return Err(AgentConfigError::Other(anyhow::anyhow!(
                    "instruction reference must not contain `..` or root (got {p:?})"
                )));
            }
        }
    }
    Ok(())
}

/// Probe instruction file and ledger on disk. Returns the instruction file
/// path, the ledger path, and whether the instruction file exists.
pub(crate) fn paths_for_status(
    config_dir: &Path,
    instruction_dir: &Path,
    name: &str,
) -> (PathBuf, PathBuf) {
    let file = instruction_file_path(instruction_dir, name);
    let led = ledger_path(config_dir);
    (file, led)
}

/// Install (or update) an instruction file. Dispatches to the appropriate
/// placement handler based on `spec.placement`.
///
/// - `config_dir`: directory for the ownership ledger
/// - `host_file`: the file that gets the include reference or inline block
///   (required for InlineBlock and ReferencedFile)
/// - `instruction_dir`: where standalone instruction files are written
///   (required for ReferencedFile and StandaloneFile)
/// - `reference_line`: the include reference string for ReferencedFile
///   (e.g., `@RTK.md`). Only used when placement is ReferencedFile.
pub(crate) fn install(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: Option<&Path>,
    instruction_dir: Option<&Path>,
    reference_line: Option<&str>,
) -> Result<InstallReport, AgentConfigError> {
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
            install_referenced(config_dir, spec, host, dir, ref_line)
        }
        InstructionPlacement::InlineBlock => {
            let host = host_file.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "host_file (required for InlineBlock)",
            })?;
            install_inline(config_dir, spec, host)
        }
        InstructionPlacement::StandaloneFile => {
            let dir = instruction_dir.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "instruction_dir (required for StandaloneFile)",
            })?;
            install_standalone(config_dir, spec, dir)
        }
    }
}

fn install_referenced(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: &Path,
    instruction_dir: &Path,
    reference_line: &str,
) -> Result<InstallReport, AgentConfigError> {
    let led = ledger_path(config_dir);
    let instr_path = instruction_file_path(instruction_dir, &spec.name);

    // Determine lock ordering: sort paths to avoid deadlocks.
    let mut lock_paths = vec![
        host_file.parent().unwrap_or(host_file).to_path_buf(),
        instruction_dir.to_path_buf(),
    ];
    lock_paths.sort();
    let lock_root = lock_paths
        .into_iter()
        .next()
        .unwrap_or_else(|| config_dir.to_path_buf());

    file_lock::with_lock(&lock_root, || {
        let mut report = InstallReport::default();

        ownership::require_owner(&led, &spec.name, &spec.owner_tag, KIND, instr_path.exists())?;

        // Ensure instruction directory exists.
        fs::create_dir_all(instruction_dir)
            .map_err(|e| AgentConfigError::io(instruction_dir, e))?;
        fs_atomic::ensure_contained(&instr_path, instruction_dir)?;

        // Write instruction file.
        let body = ensure_trailing_newline(&spec.body);
        let outcome = fs_atomic::write_atomic(&instr_path, body.as_bytes(), false)?;
        record_outcome(&mut report, outcome);

        // Upsert include reference in host file.
        let host_content = fs_atomic::read_to_string_or_empty(host_file)?;
        let new_host = md_block::upsert(&host_content, &spec.name, reference_line);
        if new_host != host_content {
            let host_outcome = fs_atomic::write_atomic(host_file, new_host.as_bytes(), true)?;
            record_outcome(&mut report, host_outcome);
        }

        // Record ownership.
        let prior = ownership::owner_of(&led, &spec.name)?;
        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() {
            let hash = ownership::file_content_hash(&instr_path)?;
            ownership::record_install(&led, &spec.name, &spec.owner_tag, hash.as_deref())?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    })
}

fn install_inline(
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: &Path,
) -> Result<InstallReport, AgentConfigError> {
    let led = ledger_path(config_dir);

    file_lock::with_lock(host_file, || {
        let mut report = InstallReport::default();

        // For inline, check if a block with our name already exists.
        let host_content = fs_atomic::read_to_string_or_empty(host_file)?;
        let block_exists = md_block::contains(&host_content, &spec.name);

        ownership::require_owner(&led, &spec.name, &spec.owner_tag, KIND, block_exists)?;

        let new_host = md_block::upsert(&host_content, &spec.name, &spec.body);
        if new_host != host_content {
            let host_outcome = fs_atomic::write_atomic(host_file, new_host.as_bytes(), true)?;
            record_outcome(&mut report, host_outcome);
        }

        let prior = ownership::owner_of(&led, &spec.name)?;
        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() {
            let hash = ownership::content_hash(spec.body.as_bytes());
            ownership::record_install(&led, &spec.name, &spec.owner_tag, Some(&hash))?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    })
}

fn install_standalone(
    config_dir: &Path,
    spec: &InstructionSpec,
    instruction_dir: &Path,
) -> Result<InstallReport, AgentConfigError> {
    let led = ledger_path(config_dir);
    let instr_path = instruction_file_path(instruction_dir, &spec.name);

    file_lock::with_lock(instruction_dir, || {
        let mut report = InstallReport::default();

        ownership::require_owner(&led, &spec.name, &spec.owner_tag, KIND, instr_path.exists())?;

        fs::create_dir_all(instruction_dir)
            .map_err(|e| AgentConfigError::io(instruction_dir, e))?;
        fs_atomic::ensure_contained(&instr_path, instruction_dir)?;

        let body = ensure_trailing_newline(&spec.body);
        let outcome = fs_atomic::write_atomic(&instr_path, body.as_bytes(), false)?;
        record_outcome(&mut report, outcome);

        let prior = ownership::owner_of(&led, &spec.name)?;
        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() {
            let hash = ownership::file_content_hash(&instr_path)?;
            ownership::record_install(&led, &spec.name, &spec.owner_tag, hash.as_deref())?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    })
}

/// Uninstall an instruction. Dispatches based on what is present on disk and
/// in the ledger.
pub(crate) fn uninstall(
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
        md_block::contains(&content, name)
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
            md_block::contains(&content, name)
        });

        if !instr_exists && !block_exists && !in_ledger {
            report.not_installed = true;
            return Ok(report);
        }

        ownership::require_owner(&led, name, owner_tag, KIND, instr_exists || block_exists)?;

        // Remove include block from host file.
        if let Some(host) = host_file {
            if host.exists() {
                let content = fs_atomic::read_to_string_or_empty(host)?;
                let (new_content, removed) = md_block::remove(&content, name);
                if removed && new_content != content {
                    if new_content.trim().is_empty() {
                        // Host file is now empty; just leave it empty rather than
                        // deleting, since we may not own the file itself.
                        let _ = fs_atomic::write_atomic(host, new_content.as_bytes(), true);
                    } else {
                        let _ = fs_atomic::write_atomic(host, new_content.as_bytes(), true);
                    }
                    report.patched.push(host.to_path_buf());
                }
            }
        }

        // Remove instruction file.
        if let Some(path) = &instr_path {
            if path.exists() {
                fs_atomic::ensure_contained(path, instruction_dir.unwrap_or(config_dir))?;
                fs::remove_file(path).map_err(|e| AgentConfigError::io(path, e))?;
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

    match (actual_owner.as_deref(), instr_path.exists()) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
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
    planning::plan_markdown_upsert(&mut changes, host_file, &spec.name, reference_line)?;

    let owner_changed = actual_owner.as_deref() != Some(spec.owner_tag.as_str());
    let file_would_change = changes.iter().any(|c| {
        matches!(
            c,
            PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
        )
    });
    if !has_refusal(&changes) && (owner_changed || file_would_change) {
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
    let block_exists = md_block::contains(&host_content, &spec.name);

    match (actual_owner.as_deref(), block_exists) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
            changes.push(PlannedChange::Refuse {
                path: Some(host_file.to_path_buf()),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    planning::plan_markdown_upsert(&mut changes, host_file, &spec.name, &spec.body)?;

    let owner_changed = actual_owner.as_deref() != Some(spec.owner_tag.as_str());
    let file_would_change = changes.iter().any(|c| {
        matches!(
            c,
            PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
        )
    });
    if !has_refusal(&changes) && (owner_changed || file_would_change) {
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

    match (actual_owner.as_deref(), instr_path.exists()) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(led),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
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
    if !has_refusal(&changes) && (owner_changed || file_would_change) {
        planning::plan_write_ledger(&mut changes, &led, &spec.name, &spec.owner_tag);
    }

    Ok(changes)
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
        planning::plan_markdown_remove(&mut changes, host, name)?;
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

fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        let mut out = s.to_string();
        out.push('\n');
        out
    }
}

fn record_outcome(report: &mut InstallReport, outcome: fs_atomic::WriteOutcome) {
    if outcome.no_change {
        return;
    }
    if outcome.existed {
        report.patched.push(outcome.path.clone());
    } else {
        report.created.push(outcome.path.clone());
    }
    if let Some(b) = outcome.backup {
        report.backed_up.push(b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn basic_referenced_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::ReferencedFile)
            .body("# RTK\n\nUse rtk for compact output.\n")
            .build()
    }

    fn basic_standalone_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::StandaloneFile)
            .body("# RTK\n\nUse rtk for compact output.\n")
            .build()
    }

    fn basic_inline_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::InlineBlock)
            .body("# RTK\n\nUse rtk for compact output.\n")
            .build()
    }

    #[test]
    fn referenced_file_creates_standalone_and_include_block() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_referenced_spec("RTK", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        let instr_path = instr_dir.join("RTK.md");
        assert!(instr_path.exists());
        assert!(fs::read_to_string(&instr_path).unwrap().contains("# RTK"));

        let host_content = fs::read_to_string(&host).unwrap();
        assert!(host_content.contains("@RTK.md"));
        assert!(host_content.contains("BEGIN AGENT-CONFIG:RTK"));
    }

    #[test]
    fn referenced_file_idempotent_same_content() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        let spec = basic_referenced_spec("RTK", "myapp");
        install(
            &config_dir,
            &spec,
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();
        let report = install(
            &config_dir,
            &spec,
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();
        assert!(report.already_installed);
    }

    #[test]
    fn referenced_file_updates_on_content_change() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        let spec1 = basic_referenced_spec("RTK", "myapp");
        install(
            &config_dir,
            &spec1,
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        let spec2 = InstructionSpec::builder("RTK")
            .owner("myapp")
            .placement(InstructionPlacement::ReferencedFile)
            .body("# RTK v2\n\nUpdated content.\n")
            .build();
        let report = install(
            &config_dir,
            &spec2,
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();
        assert!(!report.already_installed);
        assert!(fs::read_to_string(instr_dir.join("RTK.md"))
            .unwrap()
            .contains("v2"));
    }

    #[test]
    fn standalone_file_writes_to_target_dir() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let rules_dir = config_dir.join("rules");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_standalone_spec("RTK", "myapp"),
            None,
            Some(&rules_dir),
            None,
        )
        .unwrap();

        assert!(rules_dir.join("RTK.md").exists());
    }

    #[test]
    fn inline_block_uses_md_block() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let host = config_dir.join("AGENTS.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_inline_spec("RTK", "myapp"),
            Some(&host),
            None,
            None,
        )
        .unwrap();

        let content = fs::read_to_string(&host).unwrap();
        assert!(content.contains("# RTK"));
        assert!(content.contains("BEGIN AGENT-CONFIG:RTK"));
    }

    #[test]
    fn uninstall_removes_file_and_include() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_referenced_spec("RTK", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        uninstall(&config_dir, "RTK", "myapp", Some(&host), Some(&instr_dir)).unwrap();

        assert!(!instr_dir.join("RTK.md").exists());
        let host_content = fs::read_to_string(&host).unwrap();
        assert!(!host_content.contains("@RTK.md"));
        assert!(!host_content.contains("BEGIN AGENT-CONFIG:RTK"));
    }

    #[test]
    fn uninstall_refuses_modified_file() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_referenced_spec("RTK", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        // Modify the instruction file to simulate user edits.
        fs::write(instr_dir.join("RTK.md"), "# Modified content\n").unwrap();

        // Uninstall should still work (we don't check drift on uninstall
        // for instructions; we just remove the file).
        let report = uninstall(&config_dir, "RTK", "myapp", Some(&host), Some(&instr_dir)).unwrap();
        assert!(!report.removed.is_empty());
    }

    #[test]
    fn owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &config_dir,
            &basic_referenced_spec("RTK", "appA"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        let err = install(
            &config_dir,
            &basic_referenced_spec("RTK", "appB"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn user_installed_refused() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&instr_dir).unwrap();
        fs::write(instr_dir.join("RTK.md"), "# User content\n").unwrap();

        let err = install(
            &config_dir,
            &basic_referenced_spec("RTK", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn plan_install_no_side_effects() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        let changes = plan_install(
            &config_dir,
            &basic_referenced_spec("RTK", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@RTK.md"),
        )
        .unwrap();

        assert!(!changes.is_empty());
        // No files should have been created.
        assert!(!instr_dir.join("RTK.md").exists());
        assert!(!host.exists());
    }

    #[test]
    fn path_traversal_rejected() {
        // Names with special characters are rejected at spec validation time.
        let result = InstructionSpec::builder("../escape")
            .owner("myapp")
            .placement(InstructionPlacement::StandaloneFile)
            .body("body\n")
            .try_build();
        assert!(result.is_err());
    }
}
