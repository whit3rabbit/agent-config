//! Cline instructions surface. Standalone files under `.clinerules/`.

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, InstructionSurface, UninstallReport};
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::InstructionSpec;
use crate::status::StatusReport;
use crate::util::instructions_dir;

use super::{ClineAgent, LEGACY_RULES_DIR, RULES_DIR};

impl ClineAgent {
    pub(super) fn standalone_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        let root = self.project_root(scope)?;
        let rules_dir = root.join(RULES_DIR);
        Ok(instructions_dir::StandaloneLayout {
            config_dir: rules_dir.clone(),
            instruction_dir: rules_dir,
        })
    }

    pub(super) fn legacy_standalone_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        let root = self.project_root(scope)?;
        let rules_dir = root.join(LEGACY_RULES_DIR);
        Ok(instructions_dir::StandaloneLayout {
            config_dir: rules_dir.clone(),
            instruction_dir: rules_dir,
        })
    }
}

impl InstructionSurface for ClineAgent {
    fn id(&self) -> &'static str {
        "cline"
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
        let primary_status = instructions_dir::standalone_status(
            self.standalone_layout(scope)?,
            name,
            expected_owner,
        )?;
        if matches!(
            primary_status.status,
            crate::status::InstallStatus::InstalledOwned { .. }
                | crate::status::InstallStatus::InstalledOtherOwner { .. }
        ) {
            Ok(primary_status)
        } else {
            let legacy_layout = self.legacy_standalone_layout(scope)?;
            let legacy_status =
                instructions_dir::standalone_status(legacy_layout, name, expected_owner)?;
            if matches!(
                legacy_status.status,
                crate::status::InstallStatus::InstalledOwned { .. }
                    | crate::status::InstallStatus::InstalledOtherOwner { .. }
            ) {
                Ok(legacy_status)
            } else {
                Ok(primary_status)
            }
        }
    }

    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        instructions_dir::standalone_plan_install(
            InstructionSurface::id(self),
            scope,
            self.standalone_layout(scope),
            spec,
        )
    }

    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        let mut plan = instructions_dir::standalone_plan_uninstall(
            InstructionSurface::id(self),
            scope,
            self.standalone_layout(scope),
            name,
            owner_tag,
        )?;
        if let Ok(legacy_layout) = self.legacy_standalone_layout(scope) {
            let legacy_plan = instructions_dir::standalone_plan_uninstall(
                InstructionSurface::id(self),
                scope,
                Ok(legacy_layout),
                name,
                owner_tag,
            )?;
            plan.changes.extend(legacy_plan.changes);
        }
        Ok(plan)
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        instructions_dir::standalone_install(scope, self.standalone_layout(scope)?, spec)
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        let mut report = instructions_dir::standalone_uninstall(
            scope,
            self.standalone_layout(scope)?,
            name,
            owner_tag,
        )?;
        if let Ok(legacy_layout) = self.legacy_standalone_layout(scope) {
            let legacy_report =
                instructions_dir::standalone_uninstall(scope, legacy_layout, name, owner_tag)?;
            report.merge(legacy_report);
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
    use tempfile::tempdir;

    fn instruction_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::StandaloneFile)
            .body("# Test instruction\n")
            .build()
    }

    #[test]
    fn instruction_writes_to_rules_dir() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("test-rule", "myapp"))
            .unwrap();
        assert!(dir.path().join(".cline/rules/test-rule.md").exists());
    }

    #[test]
    fn instruction_uninstall_removes_file() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("test-rule", "myapp"))
            .unwrap();
        agent
            .uninstall_instruction(&scope, "test-rule", "myapp")
            .unwrap();
        assert!(!dir.path().join(".cline/rules/test-rule.md").exists());
    }

    #[test]
    fn instruction_rejects_global_scope() {
        let agent = ClineAgent::new();
        let spec = instruction_spec("test-rule", "myapp");
        let plan = agent
            .plan_install_instruction(&Scope::Global, &spec)
            .unwrap();
        assert!(matches!(plan.status, crate::plan::PlanStatus::Refused));
    }
}
