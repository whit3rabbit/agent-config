//! Claude instruction surface. The only `ReferencedFile` placement in the
//! crate: writes a standalone `<NAME>.md` and injects an `@<NAME>.md` import
//! line into the host memory file (`CLAUDE.md`).

use std::path::PathBuf;

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, InstructionSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec};
use crate::status::StatusReport;
use crate::util::{fs_atomic, instructions_dir, md_block, ownership};

use super::ClaudeAgent;

impl ClaudeAgent {
    /// Instruction layout for the given scope.
    ///
    /// Returns `(config_dir, host_file, instruction_dir, reference_line)`:
    /// - `config_dir`: where the ledger lives
    /// - `host_file`: the file that gets the include reference (`CLAUDE.md`)
    /// - `instruction_dir`: where instruction files are written
    /// - `reference_line`: the include reference string (e.g., `@MYAPP.md`)
    fn instruction_layout(
        scope: &Scope,
        name: &str,
    ) -> Result<(PathBuf, PathBuf, PathBuf, String), AgentConfigError> {
        Ok(match scope {
            Scope::Global => {
                let claude_home = paths::claude_home()?;
                let host = claude_home.join("CLAUDE.md");
                let ref_line = format!("@{name}.md");
                (claude_home.clone(), host, claude_home, ref_line)
            }
            Scope::Local(p) => {
                let config_dir = p.join(".claude");
                let host = p.join("CLAUDE.md");
                let instr_dir = config_dir.join("instructions");
                let ref_line = format!("@.claude/instructions/{name}.md");
                (config_dir, host, instr_dir, ref_line)
            }
        })
    }
}

impl InstructionSurface for ClaudeAgent {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn supported_instruction_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn instruction_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        let (config_dir, host_file, instr_dir, _prefix) = Self::instruction_layout(scope, name)?;
        let led = instructions_dir::ledger_path(&config_dir);
        let instr_path = instr_dir.join(format!("{name}.md"));

        let instr_exists = instr_path.exists();
        let block_in_host = if host_file.exists() {
            let host = fs_atomic::read_to_string_or_empty(&host_file)?;
            md_block::contains_instruction(&host, name)
                || md_block::contains_legacy_instruction(&host, name)
        } else {
            false
        };

        let presence = if instr_exists || block_in_host {
            crate::status::ConfigPresence::Single
        } else {
            crate::status::ConfigPresence::Absent
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

    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        spec.validate()?;
        let target = PlanTarget::Instruction {
            integration_id: InstructionSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let (config_dir, host_file, instr_dir, ref_line) =
            Self::instruction_layout(scope, &spec.name)?;
        let changes = instructions_dir::plan_install(
            &config_dir,
            spec,
            Some(&host_file),
            Some(&instr_dir),
            Some(&ref_line),
        )?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let target = PlanTarget::Instruction {
            integration_id: InstructionSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let (config_dir, host_file, instr_dir, _) = Self::instruction_layout(scope, name)?;
        let changes = instructions_dir::plan_uninstall(
            &config_dir,
            name,
            owner_tag,
            Some(&host_file),
            Some(&instr_dir),
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        scope.ensure_contained(&Self::memory_path(scope)?)?;
        let (config_dir, host_file, instr_dir, ref_line) =
            Self::instruction_layout(scope, &spec.name)?;
        scope.ensure_contained(&host_file)?;
        scope.ensure_contained(&instr_dir.join(&spec.name))?;
        instructions_dir::install(
            scope,
            &config_dir,
            spec,
            Some(&host_file),
            Some(&instr_dir),
            Some(&ref_line),
        )
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let (config_dir, host_file, instr_dir, _) = Self::instruction_layout(scope, name)?;
        instructions_dir::uninstall(
            scope,
            &config_dir,
            name,
            owner_tag,
            Some(&host_file),
            Some(&instr_dir),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
    use std::fs;
    use tempfile::tempdir;

    fn instruction_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::ReferencedFile)
            .body("# MyApp\n\nProject-specific guidance.\n")
            .build()
    }

    #[test]
    fn instruction_global_creates_md_and_reference() {
        let dir = tempdir().unwrap();
        let claude_home = dir.path().join("claude-home");
        fs::create_dir_all(&claude_home).unwrap();

        // Temporarily override home by using Local scope with a path that
        // mimics the global layout. Global scope requires a real home dir,
        // so we test the layout via Local scope instead.
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("MYAPP", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();

        let instr = root.join(".claude/instructions/MYAPP.md");
        assert!(instr.exists());
        assert!(fs::read_to_string(&instr).unwrap().contains("# MyApp"));

        let claude_md = root.join("CLAUDE.md");
        let content = fs::read_to_string(&claude_md).unwrap();
        assert!(content.contains("@.claude/instructions/MYAPP.md"));
        assert!(content.contains("BEGIN AGENT-CONFIG-INSTR:MYAPP"));
    }

    #[test]
    fn instruction_idempotent() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("MYAPP", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();
        let report = agent.install_instruction(&scope, &spec).unwrap();
        assert!(report.already_installed);
    }

    #[test]
    fn instruction_uninstall_removes_both() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("MYAPP", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();

        agent
            .uninstall_instruction(&scope, "MYAPP", "myapp")
            .unwrap();

        assert!(!root.join(".claude/instructions/MYAPP.md").exists());
        let claude_md = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(!claude_md.contains("@.claude/instructions/MYAPP.md"));
    }

    #[test]
    fn instruction_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("MYAPP", "appA");
        agent.install_instruction(&scope, &spec).unwrap();

        let err = agent
            .uninstall_instruction(&scope, "MYAPP", "appB")
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn instruction_plan_does_not_mutate() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("MYAPP", "myapp");
        let plan = agent.plan_install_instruction(&scope, &spec).unwrap();

        assert!(!root.join(".claude/instructions/MYAPP.md").exists());
        assert!(!root.join("CLAUDE.md").exists());
        assert!(!plan.changes.is_empty());
    }
}
