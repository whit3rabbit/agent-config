//! Trae agent integration (ByteDance).
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: project-local fenced block in `.trae/project_rules.md`.
//!    Trae also reads `.trae/user_rules.md`; this crate writes the
//!    project-scoped file.
//! 2. **Skills**: directory-scoped `SKILL.md` folders at `.trae/skills/<name>/`.
//!
//! MCP and hooks are not part of Trae's documented file-config surface.

use std::path::{Path, PathBuf};

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

/// Trae agent installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraeAgent {
    _private: (),
}

impl TraeAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn require_local(scope: &Scope) -> Result<&Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "trae",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(Self::require_local(scope)?
            .join(".trae")
            .join("project_rules.md"))
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".trae").join("skills"),
            Scope::Local(p) => p.join(".trae").join("skills"),
        })
    }

    /// Directory holding the instruction ownership ledger. Local-only;
    /// lives next to the host file under `<root>/.trae/`.
    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(Self::require_local(scope)?.join(".trae"))
    }
}

impl Integration for TraeAgent {
    fn id(&self) -> &'static str {
        "trae"
    }

    fn display_name(&self) -> &'static str {
        "Trae"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
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
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "trae",
                field: "rules",
            })?;
        let path = Self::rules_path(scope)?;
        scope.ensure_contained(&path)?;
        let mut report = InstallReport::default();
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
        scope.ensure_contained(&path)?;
        let mut report = UninstallReport::default();
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

impl SkillSurface for TraeAgent {
    fn id(&self) -> &'static str {
        "trae"
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

impl TraeAgent {
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

impl InstructionSurface for TraeAgent {
    fn id(&self) -> &'static str {
        "trae"
    }

    fn supported_instruction_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
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
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn skill(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Test Trae skill.")
            .body("## Goal\nDo it.\n")
            .build()
    }

    #[test]
    fn install_writes_project_rules_md() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use Trae."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join(".trae/project_rules.md")).unwrap();
        assert!(body.contains("Use Trae."));
    }

    #[test]
    fn global_prompt_scope_rejected() {
        let agent = TraeAgent::new();
        let err = agent
            .install(&Scope::Global, &rules_spec("alpha", "x"))
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "x")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".trae/project_rules.md").exists());
    }

    #[test]
    fn install_skill_writes_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha-skill", "myapp"))
            .unwrap();
        assert!(dir
            .path()
            .join(".trae/skills/alpha-skill/SKILL.md")
            .exists());
    }
}
