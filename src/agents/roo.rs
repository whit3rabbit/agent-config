//! Roo Code integration.
//!
//! Two surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.roo/rules/<tag>.md`.
//!
//! 2. **MCP servers** — global VS Code extension config at
//!    `Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`
//!    or project config at `.roo/mcp.json`, keyed by server name under
//!    `mcpServers`.

use std::path::Path;
use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, McpSpec};
use crate::status::StatusReport;
use crate::util::{instructions_dir, mcp_json_object, ownership, rules_dir};

const RULES_DIR: &str = ".roo/rules";

/// Roo Code integration.
#[derive(Debug, Clone, Copy, Default)]
pub struct RooAgent {
    _private: (),
}

impl RooAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn require_local<'a>(&self, scope: &'a Scope) -> Result<&'a Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "roo",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::roo_mcp_global_file()?,
            Scope::Local(p) => p.join(".roo").join("mcp.json"),
        })
    }
}

impl Integration for RooAgent {
    fn id(&self) -> &'static str {
        "roo"
    }

    fn display_name(&self) -> &'static str {
        "Roo Code"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.require_local(scope)?;
        let path = rules_dir::target_path(root, RULES_DIR, tag);
        Ok(StatusReport::for_file_hook(tag, path))
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::rules_install(
            Integration::id(self),
            scope,
            spec,
            self.require_local(scope),
            RULES_DIR,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::rules_uninstall(
            Integration::id(self),
            scope,
            tag,
            self.require_local(scope),
            RULES_DIR,
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let _ = self.require_local(scope)?;
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "roo",
                field: "rules",
            })?;
        rules_dir::install(scope, RULES_DIR, &spec.tag, &rules.content)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let _ = self.require_local(scope)?;
        rules_dir::uninstall(scope, RULES_DIR, tag)
    }
}

impl McpSurface for RooAgent {
    fn id(&self) -> &'static str {
        "roo"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence = mcp_json_object::config_presence(&cfg, name)?;
        let recorded = ownership::owner_of(&ledger, name)?;
        Ok(StatusReport::for_mcp(
            name,
            cfg,
            ledger,
            presence,
            expected_owner,
            recorded,
        ))
    }

    fn plan_install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::mcp_json_object_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::mcp_path(scope),
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::mcp_json_object_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
        )
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        spec.validate_local_secret_policy(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::install(&cfg, &ledger, spec)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

impl RooAgent {
    fn standalone_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        let root = self.require_local(scope)?;
        Ok(instructions_dir::StandaloneLayout {
            config_dir: root.join(".roo"),
            instruction_dir: root.join(".roo/rules"),
        })
    }
}

impl InstructionSurface for RooAgent {
    fn id(&self) -> &'static str {
        "roo"
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
        instructions_dir::standalone_status(self.standalone_layout(scope)?, name, expected_owner)
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
        instructions_dir::standalone_plan_uninstall(
            InstructionSurface::id(self),
            scope,
            self.standalone_layout(scope),
            name,
            owner_tag,
        )
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
        instructions_dir::standalone_uninstall(
            scope,
            self.standalone_layout(scope)?,
            name,
            owner_tag,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_rules_writes_dot_roo_rules() {
        let dir = tempdir().unwrap();
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".roo/rules/alpha.md").exists());
    }

    #[test]
    fn install_mcp_writes_project_mcp_json() {
        let dir = tempdir().unwrap();
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".roo/mcp.json");
        let v = read_json(&cfg);
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }
}

#[cfg(test)]
mod instruction_tests {
    use super::*;
    use crate::integration::InstructionSurface;
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
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("test-rule", "myapp"))
            .unwrap();
        assert!(dir.path().join(".roo/rules/test-rule.md").exists());
    }

    #[test]
    fn instruction_uninstall_removes_file() {
        let dir = tempdir().unwrap();
        let agent = RooAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("test-rule", "myapp"))
            .unwrap();
        agent
            .uninstall_instruction(&scope, "test-rule", "myapp")
            .unwrap();
        assert!(!dir.path().join(".roo/rules/test-rule.md").exists());
    }

    #[test]
    fn instruction_rejects_global_scope() {
        let agent = RooAgent::new();
        let spec = instruction_spec("test-rule", "myapp");
        let plan = agent
            .plan_install_instruction(&Scope::Global, &spec)
            .expect("plan should refuse, not error");
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
    }
}
