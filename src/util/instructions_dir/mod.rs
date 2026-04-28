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
//!
//! Layout: this module is a directory with internal submodules that split
//! the dispatcher and per-placement bodies (`install`, `plan`, `uninstall`)
//! and the shim helpers (`shims`) used by `InstructionSurface` impls. All
//! callers consume this module via the re-exported flat API below.

use std::path::{Component, Path, PathBuf};

use crate::error::AgentConfigError;
use crate::integration::InstallReport;
use crate::spec::InstructionSpec;
use crate::util::fs_atomic;

mod install;
mod plan;
mod shims;
mod uninstall;

pub(crate) use install::install;
pub(crate) use plan::plan_install;
pub(crate) use shims::{
    inline_install, inline_plan_install, inline_plan_uninstall, inline_status, inline_uninstall,
    standalone_install, standalone_plan_install, standalone_plan_uninstall, standalone_status,
    standalone_uninstall,
};
pub(crate) use uninstall::{plan_uninstall, uninstall};

const LEDGER_FILE: &str = ".agent-config-instructions.json";
pub(super) const KIND: &str = "instruction";

/// Resolved per-scope path layout for an InlineBlock instruction agent.
///
/// Agents construct this once from their per-scope path resolution and pass
/// it to the [`inline_status`] / [`inline_install`] / etc. shim helpers,
/// which collapse the previously-duplicated InstructionSurface bodies.
pub(crate) struct InlineLayout {
    /// Directory holding the ownership ledger.
    pub config_dir: PathBuf,
    /// Memory file the inline block is upserted into.
    pub host_file: PathBuf,
}

/// Resolved per-scope path layout for a StandaloneFile instruction agent.
pub(crate) struct StandaloneLayout {
    /// Directory holding the ownership ledger.
    pub config_dir: PathBuf,
    /// Directory the standalone `<name>.md` file is written under.
    pub instruction_dir: PathBuf,
}

/// Ledger path for instructions.
pub(crate) fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LEDGER_FILE)
}

/// Instruction file path given a directory and name.
pub(super) fn instruction_file_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.md"))
}

/// Validate that an instruction name does not contain path traversal.
pub(super) fn validate_name(name: &str) -> Result<(), AgentConfigError> {
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
pub(super) fn validate_relative(p: &str) -> Result<(), AgentConfigError> {
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

pub(super) fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        let mut out = s.to_string();
        out.push('\n');
        out
    }
}

pub(super) fn record_outcome(report: &mut InstallReport, outcome: fs_atomic::WriteOutcome) {
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
    use crate::scope::Scope;
    use crate::spec::InstructionPlacement;
    use std::fs;
    use tempfile::tempdir;

    fn local_scope(p: &Path) -> Scope {
        Scope::Local(p.to_path_buf())
    }

    fn basic_referenced_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::ReferencedFile)
            .body("# MyApp\n\nProject-specific guidance.\n")
            .build()
    }

    fn basic_standalone_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::StandaloneFile)
            .body("# MyApp\n\nProject-specific guidance.\n")
            .build()
    }

    fn basic_inline_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::InlineBlock)
            .body("# MyApp\n\nProject-specific guidance.\n")
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
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        let instr_path = instr_dir.join("MYAPP.md");
        assert!(instr_path.exists());
        assert!(fs::read_to_string(&instr_path).unwrap().contains("# MyApp"));

        let host_content = fs::read_to_string(&host).unwrap();
        assert!(host_content.contains("@MYAPP.md"));
        assert!(host_content.contains("BEGIN AGENT-CONFIG:MYAPP"));
    }

    #[test]
    fn referenced_file_idempotent_same_content() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        let spec = basic_referenced_spec("MYAPP", "myapp");
        install(
            &local_scope(dir.path()),
            &config_dir,
            &spec,
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();
        let report = install(
            &local_scope(dir.path()),
            &config_dir,
            &spec,
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
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

        let spec1 = basic_referenced_spec("MYAPP", "myapp");
        install(
            &local_scope(dir.path()),
            &config_dir,
            &spec1,
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        let spec2 = InstructionSpec::builder("MYAPP")
            .owner("myapp")
            .placement(InstructionPlacement::ReferencedFile)
            .body("# MyApp v2\n\nUpdated content.\n")
            .build();
        let report = install(
            &local_scope(dir.path()),
            &config_dir,
            &spec2,
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();
        assert!(!report.already_installed);
        assert!(fs::read_to_string(instr_dir.join("MYAPP.md"))
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
            &local_scope(dir.path()),
            &config_dir,
            &basic_standalone_spec("MYAPP", "myapp"),
            None,
            Some(&rules_dir),
            None,
        )
        .unwrap();

        assert!(rules_dir.join("MYAPP.md").exists());
    }

    #[test]
    fn inline_block_uses_md_block() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let host = config_dir.join("AGENTS.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_inline_spec("MYAPP", "myapp"),
            Some(&host),
            None,
            None,
        )
        .unwrap();

        let content = fs::read_to_string(&host).unwrap();
        assert!(content.contains("# MyApp"));
        assert!(content.contains("BEGIN AGENT-CONFIG:MYAPP"));
    }

    #[test]
    fn uninstall_removes_file_and_include() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        uninstall(
            &local_scope(dir.path()),
            &config_dir,
            "MYAPP",
            "myapp",
            Some(&host),
            Some(&instr_dir),
        )
        .unwrap();

        assert!(!instr_dir.join("MYAPP.md").exists());
        let host_content = fs::read_to_string(&host).unwrap();
        assert!(!host_content.contains("@MYAPP.md"));
        assert!(!host_content.contains("BEGIN AGENT-CONFIG:MYAPP"));
    }

    #[test]
    fn uninstall_refuses_modified_file() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        // Modify the instruction file to simulate user edits.
        fs::write(instr_dir.join("MYAPP.md"), "# Modified content\n").unwrap();

        // Uninstall should still work (we don't check drift on uninstall
        // for instructions; we just remove the file).
        let report = uninstall(
            &local_scope(dir.path()),
            &config_dir,
            "MYAPP",
            "myapp",
            Some(&host),
            Some(&instr_dir),
        )
        .unwrap();
        assert!(!report.removed.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn uninstall_propagates_host_write_failure_and_preserves_ledger() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        let ledger = config_dir.join(LEDGER_FILE);
        let ledger_before = fs::read_to_string(&ledger).unwrap();
        let host_content_before = fs::read_to_string(&host).unwrap();

        // Replace the regular host file with a symlink pointing outside the
        // local scope. Reads still succeed (they follow the symlink); the
        // safe_fs::write call must refuse it via Scope::Local containment so
        // the uninstall error propagates and the ledger entry is preserved.
        let outside = dir.path().parent().unwrap().join("escape.md");
        fs::write(&outside, &host_content_before).unwrap();
        fs::remove_file(&host).unwrap();
        symlink(&outside, &host).unwrap();

        let result = uninstall(
            &local_scope(dir.path()),
            &config_dir,
            "MYAPP",
            "myapp",
            Some(&host),
            Some(&instr_dir),
        );

        let _ = fs::remove_file(&host);
        let _ = fs::remove_file(&outside);

        assert!(
            result.is_err(),
            "uninstall must propagate host write failure"
        );
        assert!(
            ledger.exists(),
            "ledger file must remain when host write failed"
        );
        let ledger_after = fs::read_to_string(&ledger).unwrap();
        assert_eq!(
            ledger_before, ledger_after,
            "ledger entry must be preserved when host write failed"
        );
    }

    #[test]
    fn owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&config_dir).unwrap();

        install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "appA"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        let err = install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "appB"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
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
        fs::write(instr_dir.join("MYAPP.md"), "# User content\n").unwrap();

        let err = install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn adopt_unowned_takes_over_orphan_instruction_file() {
        // Crash window: instruction file written, ledger never recorded.
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let instr_dir = config_dir.join("instructions");
        let host = config_dir.join("CLAUDE.md");
        fs::create_dir_all(&instr_dir).unwrap();
        fs::write(
            instr_dir.join("MYAPP.md"),
            "# MyApp\n\nProject-specific guidance.\n",
        )
        .unwrap();

        let adopt_spec = InstructionSpec::builder("MYAPP")
            .owner("myapp")
            .placement(InstructionPlacement::ReferencedFile)
            .body("# MyApp\n\nProject-specific guidance.\n")
            .adopt_unowned(true)
            .build();
        install(
            &local_scope(dir.path()),
            &config_dir,
            &adopt_spec,
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        // Ledger now exists with the right owner; subsequent plain install is
        // a no-op.
        let r = install(
            &local_scope(dir.path()),
            &config_dir,
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();
        assert!(r.already_installed);
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
            &basic_referenced_spec("MYAPP", "myapp"),
            Some(&host),
            Some(&instr_dir),
            Some("@MYAPP.md"),
        )
        .unwrap();

        assert!(!changes.is_empty());
        // No files should have been created.
        assert!(!instr_dir.join("MYAPP.md").exists());
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

    // ---- Shim tests (inline_* / standalone_*) ----

    #[test]
    fn inline_shim_round_trip_and_status() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("cfg");
        let host = config_dir.join("AGENTS.md");
        fs::create_dir_all(&config_dir).unwrap();

        let layout = || InlineLayout {
            config_dir: config_dir.clone(),
            host_file: host.clone(),
        };

        let report = inline_install(
            &local_scope(dir.path()),
            layout(),
            &basic_inline_spec("MYAPP", "myapp"),
        )
        .unwrap();
        assert!(!report.already_installed);

        let status = inline_status(layout(), "MYAPP", "myapp").unwrap();
        assert!(matches!(
            status.status,
            crate::status::InstallStatus::InstalledOwned { .. }
        ));

        let r = inline_uninstall(&local_scope(dir.path()), layout(), "MYAPP", "myapp").unwrap();
        assert!(!r.patched.is_empty() || !r.removed.is_empty());

        let after = inline_status(layout(), "MYAPP", "myapp").unwrap();
        assert!(matches!(after.status, crate::status::InstallStatus::Absent));
    }

    #[test]
    fn standalone_shim_round_trip_and_status() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join("cfg");
        let rules = config_dir.join("rules");
        fs::create_dir_all(&config_dir).unwrap();

        let layout = || StandaloneLayout {
            config_dir: config_dir.clone(),
            instruction_dir: rules.clone(),
        };

        standalone_install(
            &local_scope(dir.path()),
            layout(),
            &basic_standalone_spec("MYAPP", "myapp"),
        )
        .unwrap();
        assert!(rules.join("MYAPP.md").exists());

        let status = standalone_status(layout(), "MYAPP", "myapp").unwrap();
        assert!(matches!(
            status.status,
            crate::status::InstallStatus::InstalledOwned { .. }
        ));

        standalone_uninstall(&local_scope(dir.path()), layout(), "MYAPP", "myapp").unwrap();
        assert!(!rules.join("MYAPP.md").exists());
    }

    #[test]
    fn inline_plan_install_unsupported_scope_refuses() {
        let scope = local_scope(Path::new("/tmp/whatever"));
        let layout_err: Result<InlineLayout, AgentConfigError> =
            Err(AgentConfigError::UnsupportedScope {
                id: "test",
                scope: crate::scope::ScopeKind::Global,
            });
        let plan = inline_plan_install(
            "test",
            &scope,
            layout_err,
            &basic_inline_spec("MYAPP", "myapp"),
        )
        .unwrap();
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
        assert!(plan.changes.iter().any(|c| matches!(
            c,
            crate::plan::PlannedChange::Refuse {
                reason: crate::plan::RefusalReason::UnsupportedScope,
                ..
            }
        )));
    }

    #[test]
    fn standalone_plan_install_unsupported_scope_refuses() {
        let scope = local_scope(Path::new("/tmp/whatever"));
        let layout_err: Result<StandaloneLayout, AgentConfigError> =
            Err(AgentConfigError::UnsupportedScope {
                id: "test",
                scope: crate::scope::ScopeKind::Global,
            });
        let plan = standalone_plan_install(
            "test",
            &scope,
            layout_err,
            &basic_standalone_spec("MYAPP", "myapp"),
        )
        .unwrap();
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
    }

    #[test]
    fn inline_plan_uninstall_unsupported_scope_refuses() {
        let scope = local_scope(Path::new("/tmp/whatever"));
        let layout_err: Result<InlineLayout, AgentConfigError> =
            Err(AgentConfigError::UnsupportedScope {
                id: "test",
                scope: crate::scope::ScopeKind::Global,
            });
        let plan = inline_plan_uninstall("test", &scope, layout_err, "MYAPP", "myapp").unwrap();
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
    }

    #[test]
    fn standalone_plan_uninstall_unsupported_scope_refuses() {
        let scope = local_scope(Path::new("/tmp/whatever"));
        let layout_err: Result<StandaloneLayout, AgentConfigError> =
            Err(AgentConfigError::UnsupportedScope {
                id: "test",
                scope: crate::scope::ScopeKind::Global,
            });
        let plan = standalone_plan_uninstall("test", &scope, layout_err, "MYAPP", "myapp").unwrap();
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
    }

    #[test]
    fn inline_plan_install_propagates_non_scope_error() {
        let scope = local_scope(Path::new("/tmp/whatever"));
        // Use a generic error variant; if it's not UnsupportedScope, it should
        // propagate rather than convert to a refused plan.
        let layout_err: Result<InlineLayout, AgentConfigError> = Err(AgentConfigError::Other(
            anyhow::anyhow!("synthetic resolution failure"),
        ));
        let result = inline_plan_install(
            "test",
            &scope,
            layout_err,
            &basic_inline_spec("MYAPP", "myapp"),
        );
        assert!(result.is_err());
    }
}
