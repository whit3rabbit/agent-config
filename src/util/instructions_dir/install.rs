//! Install handlers for each [`InstructionPlacement`] variant and the public
//! `install` dispatcher.

use std::fs;
use std::path::Path;

use crate::error::AgentConfigError;
use crate::integration::InstallReport;
use crate::scope::Scope;
use crate::spec::{InstructionPlacement, InstructionSpec};
use crate::util::{file_lock, fs_atomic, md_block, ownership, safe_fs};

use super::{
    ensure_trailing_newline, instruction_file_path, ledger_path, record_outcome, validate_name,
    validate_relative, KIND,
};

/// Install (or update) an instruction file. Dispatches to the appropriate
/// placement handler based on `spec.placement`.
///
/// - `scope`: the install scope; used for symlink-aware file mutation
/// - `config_dir`: directory for the ownership ledger
/// - `host_file`: the file that gets the include reference or inline block
///   (required for InlineBlock and ReferencedFile)
/// - `instruction_dir`: where standalone instruction files are written
///   (required for ReferencedFile and StandaloneFile)
/// - `reference_line`: the include reference string for ReferencedFile
///   (e.g., `@MYAPP.md`). Only used when placement is ReferencedFile.
pub(crate) fn install(
    scope: &Scope,
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
            install_referenced(scope, config_dir, spec, host, dir, ref_line)
        }
        InstructionPlacement::InlineBlock => {
            let host = host_file.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "host_file (required for InlineBlock)",
            })?;
            install_inline(scope, config_dir, spec, host)
        }
        InstructionPlacement::StandaloneFile => {
            let dir = instruction_dir.ok_or(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "instruction_dir (required for StandaloneFile)",
            })?;
            install_standalone(scope, config_dir, spec, dir)
        }
    }
}

fn install_referenced(
    scope: &Scope,
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

        let prior = ownership::owner_of(&led, &spec.name)?;
        let instr_existed = instr_path.exists();
        let adopting = spec.adopt_unowned && instr_existed && prior.is_none();
        ownership::require_owner_with_policy(
            &led,
            &spec.name,
            &spec.owner_tag,
            KIND,
            instr_existed,
            spec.adopt_unowned,
        )?;

        // Ensure instruction and ledger directories exist.
        fs::create_dir_all(instruction_dir)
            .map_err(|e| AgentConfigError::io(instruction_dir, e))?;
        fs::create_dir_all(config_dir).map_err(|e| AgentConfigError::io(config_dir, e))?;
        fs_atomic::ensure_contained(&instr_path, instruction_dir)?;

        // Write instruction file.
        let body = ensure_trailing_newline(&spec.body);
        let outcome = safe_fs::write(scope, &instr_path, body.as_bytes(), false)?;
        record_outcome(&mut report, outcome);

        // Upsert include reference in host file. If our ledger already
        // records this name as an instruction (a pre-rename install), drain
        // its legacy AGENT-CONFIG:<name> block before writing the new
        // AGENT-CONFIG-INSTR fence so old installs migrate cleanly. We only
        // prune when `prior.is_some()` because a legacy-prefixed block with
        // a matching name could otherwise belong to a hook with the same
        // tag and must be left alone.
        let host_content = fs_atomic::read_to_string_or_empty(host_file)?;
        let host_content_pruned = if prior.is_some()
            && md_block::contains_legacy_instruction(&host_content, &spec.name)
        {
            let (stripped, _) = md_block::remove_legacy_instruction(&host_content, &spec.name);
            stripped
        } else {
            host_content.clone()
        };
        let new_host =
            md_block::upsert_instruction(&host_content_pruned, &spec.name, reference_line);
        if new_host != host_content {
            let host_outcome = safe_fs::write(scope, host_file, new_host.as_bytes(), true)?;
            record_outcome(&mut report, host_outcome);
        }

        // Record ownership.
        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() || adopting {
            let hash = ownership::file_content_hash(&instr_path)?;
            ownership::record_install(&led, &spec.name, &spec.owner_tag, hash.as_deref())?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed && !adopting {
            report.already_installed = true;
        }
        Ok(report)
    })
}

fn install_inline(
    scope: &Scope,
    config_dir: &Path,
    spec: &InstructionSpec,
    host_file: &Path,
) -> Result<InstallReport, AgentConfigError> {
    let led = ledger_path(config_dir);

    file_lock::with_lock(host_file, || {
        let mut report = InstallReport::default();

        // For inline, check if a block with our name already exists.
        // Accept the legacy AGENT-CONFIG:<name> fence so adoption logic also
        // recognizes pre-rename installs as already-present.
        let host_content = fs_atomic::read_to_string_or_empty(host_file)?;
        let block_exists = md_block::contains_instruction(&host_content, &spec.name)
            || md_block::contains_legacy_instruction(&host_content, &spec.name);

        let prior = ownership::owner_of(&led, &spec.name)?;
        let adopting = spec.adopt_unowned && block_exists && prior.is_none();
        ownership::require_owner_with_policy(
            &led,
            &spec.name,
            &spec.owner_tag,
            KIND,
            block_exists,
            spec.adopt_unowned,
        )?;

        // Ensure host directory exists so the first inline upsert can write it.
        if let Some(parent) = host_file.parent() {
            fs::create_dir_all(parent).map_err(|e| AgentConfigError::io(parent, e))?;
        }
        // Ensure ledger directory exists. The ledger may live in a different
        // directory than the host (e.g. host at `<root>/AGENTS.md`, ledger at
        // `<root>/.amp/.agent-config-instructions.json`).
        fs::create_dir_all(config_dir).map_err(|e| AgentConfigError::io(config_dir, e))?;

        // Drain a legacy AGENT-CONFIG:<name> block (from a pre-rename
        // install) before writing the new fence so we never end up with
        // both. Only prune when our ledger says this name is ours
        // (`prior.is_some()`); otherwise a hook with a matching tag would
        // be erased.
        let host_content_pruned = if prior.is_some()
            && md_block::contains_legacy_instruction(&host_content, &spec.name)
        {
            let (stripped, _) = md_block::remove_legacy_instruction(&host_content, &spec.name);
            stripped
        } else {
            host_content.clone()
        };
        let new_host = md_block::upsert_instruction(&host_content_pruned, &spec.name, &spec.body);
        if new_host != host_content {
            let host_outcome = safe_fs::write(scope, host_file, new_host.as_bytes(), true)?;
            record_outcome(&mut report, host_outcome);
        }

        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() || adopting {
            let hash = ownership::content_hash(spec.body.as_bytes());
            ownership::record_install(&led, &spec.name, &spec.owner_tag, Some(&hash))?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed && !adopting {
            report.already_installed = true;
        }
        Ok(report)
    })
}

fn install_standalone(
    scope: &Scope,
    config_dir: &Path,
    spec: &InstructionSpec,
    instruction_dir: &Path,
) -> Result<InstallReport, AgentConfigError> {
    let led = ledger_path(config_dir);
    let instr_path = instruction_file_path(instruction_dir, &spec.name);

    file_lock::with_lock(instruction_dir, || {
        let mut report = InstallReport::default();

        let prior = ownership::owner_of(&led, &spec.name)?;
        let instr_existed = instr_path.exists();
        let adopting = spec.adopt_unowned && instr_existed && prior.is_none();
        ownership::require_owner_with_policy(
            &led,
            &spec.name,
            &spec.owner_tag,
            KIND,
            instr_existed,
            spec.adopt_unowned,
        )?;

        fs::create_dir_all(instruction_dir)
            .map_err(|e| AgentConfigError::io(instruction_dir, e))?;
        fs::create_dir_all(config_dir).map_err(|e| AgentConfigError::io(config_dir, e))?;
        fs_atomic::ensure_contained(&instr_path, instruction_dir)?;

        let body = ensure_trailing_newline(&spec.body);
        let outcome = safe_fs::write(scope, &instr_path, body.as_bytes(), false)?;
        record_outcome(&mut report, outcome);

        let owner_changed = prior.as_deref() != Some(spec.owner_tag.as_str());

        if owner_changed || !report.created.is_empty() || !report.patched.is_empty() || adopting {
            let hash = ownership::file_content_hash(&instr_path)?;
            ownership::record_install(&led, &spec.name, &spec.owner_tag, hash.as_deref())?;
        }

        if report.created.is_empty() && report.patched.is_empty() && !owner_changed && !adopting {
            report.already_installed = true;
        }
        Ok(report)
    })
}
