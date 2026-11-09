//! DeepSeek Harness (`dsh`) integration.
//!
//! DeepSeek Harness is an open-source, plugin-driven coding agent framework
//! by DeepSeek AI (<https://deepseekdocs.com/en/docs/getting-started/quickstart>).
//! Built on the Cordis architecture, DSH treats tools, subagents, and extensions
//! as composable plugins.
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: fenced HTML-comment block in `AGENTS.md`. DSH's
//!    instructions subsystem discovers `instructionFileCandidates` prioritizing
//!    `AGENTS.md` (and `CLAUDE.md`), as well as `$DSH_HOME/AGENTS.md`.
//! 2. **Skills**: directory-scoped `SKILL.md` folders at `<root>/.dsh/skills/`
//!    (Local) and `$DSH_HOME/skills/` (Global, default `~/.dsh/skills`).
//! 3. **Instructions**: standalone instruction blocks in `AGENTS.md`
//!    ([`crate::spec::InstructionPlacement::InlineBlock`]), with sidecar ownership ledger at
//!    `<root>/.dsh/.agent-config-instructions.json` or
//!    `$DSH_HOME/.agent-config-instructions.json`.
//!
//! Hooks and MCP are handled natively via Cordis plugins (such as
//! `@deepseek-ai/dsh-hooks-claude-code` or `@deepseek-ai/dsh-mcp-client`)
//! and profile patch manifests (`cordis.patch.yml`), not as standalone
//! file-backed hook/MCP configurations, so they are deferred in `agent-config`.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, md_block, ownership, safe_fs, skills_dir,
};

/// DeepSeek Harness installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct DshAgent {
    _private: (),
}

impl DshAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::dsh_home()?.join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::dsh_home()?.join("skills"),
            Scope::Local(p) => p.join(".dsh").join("skills"),
        })
    }

    /// Directory holding the instruction ownership ledger.
    /// Global: `$DSH_HOME` (default `~/.dsh`). Local: `<root>/.dsh`.
    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::dsh_home()?,
            Scope::Local(p) => p.join(".dsh"),
        })
    }
}

impl Integration for DshAgent {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn display_name(&self) -> &'static str {
        "DeepSeek Harness"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::rules_path(scope)?;
        StatusReport::for_markdown_block_hook(tag, path)
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::markdown_install(
            Integration::id(self),
            scope,
            spec,
            Self::rules_path(scope),
            true,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::markdown_uninstall(
            Integration::id(self),
            scope,
            tag,
            Self::rules_path(scope),
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        agent_planning::validate_prompt_only_event(Integration::id(self), &spec.event)?;
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "dsh",
                field: "rules",
            })?;
        let path = Self::rules_path(scope)?;
        let mut report = InstallReport::default();
        scope.ensure_contained(&path)?;
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = safe_fs::write(scope, &path, new_host.as_bytes(), true)?;
            if outcome.no_change {
                report.already_installed = true;
            } else if outcome.existed {
                report.patched.push(outcome.path.clone());
            } else {
                report.created.push(outcome.path.clone());
            }
            if let Some(b) = outcome.backup {
                report.backed_up.push(b);
            }
            Ok::<(), AgentConfigError>(())
        })?;
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::rules_path(scope)?;
        let mut report = UninstallReport::default();
        scope.ensure_contained(&path)?;
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let (stripped, removed) = md_block::remove(&host, tag);

            if !removed {
                report.not_installed = true;
                return Ok(());
            }

            if stripped.trim().is_empty() {
                if safe_fs::restore_backup_if_matches(scope, &path, stripped.as_bytes())? {
                    report.restored.push(path.clone());
                } else {
                    safe_fs::remove_file(scope, &path)?;
                    report.removed.push(path.clone());
                }
            } else {
                safe_fs::write(scope, &path, stripped.as_bytes(), false)?;
                report.patched.push(path.clone());
            }
            Ok::<(), AgentConfigError>(())
        })?;
        Ok(report)
    }
}

impl SkillSurface for DshAgent {
    fn id(&self) -> &'static str {
        "dsh"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn skill_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        let (dir, manifest, ledger) = skills_dir::paths_for_status(&root, name);
        let recorded = ownership::owner_of(&ledger, name)?;
        Ok(StatusReport::for_skill(
            name,
            dir,
            manifest,
            ledger,
            expected_owner,
            recorded,
        ))
    }

    fn plan_install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::skill_install(
            SkillSurface::id(self),
            scope,
            spec,
            Self::skills_root(scope),
        )
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl DshAgent {
    fn inline_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::InlineLayout, AgentConfigError> {
        Ok(instructions_dir::InlineLayout {
            config_dir: Self::instruction_config_dir(scope)?,
            host_file: Self::rules_path(scope)?,
        })
    }
}

impl InstructionSurface for DshAgent {
    fn id(&self) -> &'static str {
        "dsh"
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
        instructions_dir::inline_status(self.inline_layout(scope)?, name, expected_owner)
    }

    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_install(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            spec,
        )
    }

    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_uninstall(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            name,
            owner_tag,
        )
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        instructions_dir::inline_install(scope, self.inline_layout(scope)?, spec)
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        instructions_dir::inline_uninstall(scope, self.inline_layout(scope)?, name, owner_tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn skill_spec(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Test DSH skill.")
            .body("## Goal\nRun DSH skill task.\n")
            .build()
    }

    fn instruction_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::InlineBlock)
            .body("# DSH instruction\n\nRun instruction.\n")
            .try_build()
            .unwrap()
    }

    #[test]
    fn install_writes_agents_md_block() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use DSH rules."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(body.contains("Use DSH rules."));
        assert!(body.contains("AGENT-CONFIG:alpha"));
    }

    #[test]
    fn rules_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "rules");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "rules"))
            .unwrap();
        assert!(agent.is_installed(&scope, "alpha").unwrap());
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
    }

    #[test]
    fn install_requires_rules() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .build();
        let err = agent.install(&scope, &spec).unwrap_err();
        assert!(matches!(err, AgentConfigError::MissingSpecField { .. }));
    }

    #[test]
    fn install_skill_writes_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill_spec("alpha-skill", "myapp"))
            .unwrap();
        assert!(dir.path().join(".dsh/skills/alpha-skill/SKILL.md").exists());
    }

    #[test]
    fn skill_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill_spec("alpha-skill", "app-a"))
            .unwrap();
        let err = agent
            .install_skill(&scope, &skill_spec("alpha-skill", "app-b"))
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn instruction_install_and_uninstall() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = instruction_spec("my-instr", "app");
        agent.install_instruction(&scope, &spec).unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(md.contains("AGENT-CONFIG-INSTR:my-instr"));
        assert!(md.contains("Run instruction."));

        // Idempotent
        let again = agent.install_instruction(&scope, &spec).unwrap();
        assert!(again.already_installed);

        // Uninstall
        agent
            .uninstall_instruction(&scope, "my-instr", "app")
            .unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(!md.contains("AGENT-CONFIG-INSTR:my-instr"));
    }

    #[test]
    fn plan_install_does_not_write() {
        let dir = tempdir().unwrap();
        let agent = DshAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "rules");
        let _plan = agent.plan_install(&scope, &spec).unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
    }
}
