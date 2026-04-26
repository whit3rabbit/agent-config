//! Google Antigravity integration.
//!
//! Two surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.agent/rules/<tag>.md`.
//!    Note the directory is singular `.agent/`, not `.agents/`.
//!
//! 2. **Skills** — directory-scoped skills at `.agent/skills/<name>/` (Local)
//!    or `~/.gemini/antigravity/skills/<name>/` (Global). Each skill is a
//!    folder with `SKILL.md` plus optional `scripts/`/`references/`/`assets/`.
//!
//! 3. **MCP servers** — JSON config at `.agent/mcp_config.json` (Local) or
//!    `~/.gemini/antigravity/mcp_config.json` (Global), keyed by server name
//!    under `mcpServers`.
//!
//! Antigravity does not yet expose a hooks surface in the way other harnesses
//! do.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{mcp_json_object, ownership, rules_dir, skills_dir};

const RULES_DIR: &str = ".agent/rules";

/// Google Antigravity integration.
#[derive(Debug, Clone, Copy, Default)]
pub struct AntigravityAgent {
    _private: (),
}

impl AntigravityAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "antigravity",
                scope: ScopeKind::Global,
            }),
        }
    }

    /// Skills root: `<root>/.agent/skills/` (Local) or
    /// `~/.gemini/antigravity/skills/` (Global). Both scopes are supported
    /// for skills.
    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("antigravity").join("skills"),
            Scope::Local(p) => p.join(".agent").join("skills"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_mcp_global_file()?,
            Scope::Local(p) => p.join(".agent").join("mcp_config.json"),
        })
    }
}

impl Integration for AntigravityAgent {
    fn id(&self) -> &'static str {
        "antigravity"
    }

    fn display_name(&self) -> &'static str {
        "Google Antigravity"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
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
            self.project_root(scope),
            RULES_DIR,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::rules_uninstall(
            Integration::id(self),
            scope,
            tag,
            self.project_root(scope),
            RULES_DIR,
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "antigravity",
                field: "rules",
            })?;
        rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        rules_dir::uninstall(root, RULES_DIR, tag)
    }
}

impl McpSurface for AntigravityAgent {
    fn id(&self) -> &'static str {
        "antigravity"
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

impl SkillSurface for AntigravityAgent {
    fn id(&self) -> &'static str {
        "antigravity"
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
        spec.validate()?;
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
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = Self::skills_root(scope)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            .description("Format Git commits.")
            .body("## Goal\nFormat them.\n")
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    #[test]
    fn install_rules_uses_singular_dot_agent() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".agent/rules/alpha.md").exists());
        assert!(!dir.path().join(".agents").exists());
    }

    #[test]
    fn rules_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = rules_spec("alpha", "body");
        agent.install(&scope, &s).unwrap();
        let r = agent.install(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_skill_writes_under_dot_agent_skills() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha", "myapp"))
            .unwrap();
        assert!(dir.path().join(".agent/skills/alpha/SKILL.md").exists());
        let s = fs::read_to_string(dir.path().join(".agent/skills/alpha/SKILL.md")).unwrap();
        assert!(s.contains("name: alpha"));
        assert!(s.contains("description: Format Git commits."));
    }

    #[test]
    fn skill_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = skill("alpha", "myapp");
        agent.install_skill(&scope, &s).unwrap();
        let r = agent.install_skill(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn skill_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha", "myapp"))
            .unwrap();
        agent.uninstall_skill(&scope, "alpha", "myapp").unwrap();
        assert!(!dir.path().join(".agent/skills/alpha").exists());
    }

    #[test]
    fn skill_uninstall_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha", "appA"))
            .unwrap();
        let err = agent.uninstall_skill(&scope, "alpha", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn skill_supports_both_scopes() {
        let agent = AntigravityAgent::new();
        let scopes = agent.supported_skill_scopes();
        assert!(scopes.contains(&ScopeKind::Local));
        assert!(scopes.contains(&ScopeKind::Global));
    }

    #[test]
    fn rules_install_requires_rules_field() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let no_rules = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .build();
        let err = agent.install(&scope, &no_rules).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::MissingSpecField { field: "rules", .. }
        ));
    }

    #[test]
    fn install_mcp_writes_dot_agent_mcp_config() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".agent/mcp_config.json");
        let v: serde_json::Value = serde_json::from_slice(&fs::read(cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["github"]["command"],
            serde_json::json!("npx")
        );
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }
}
