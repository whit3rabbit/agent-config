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

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, SkillSpec};
use crate::util::{mcp_json_object, ownership, rules_dir, skills_dir};

const RULES_DIR: &str = ".agent/rules";

/// Google Antigravity integration.
pub struct AntigravityAgent;

impl AntigravityAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "antigravity",
                scope: ScopeKind::Global,
            }),
        }
    }

    /// Skills root: `<root>/.agent/skills/` (Local) or
    /// `~/.gemini/antigravity/skills/` (Global). Both scopes are supported
    /// for skills.
    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("antigravity").join("skills"),
            Scope::Local(p) => p.join(".agent").join("skills"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_mcp_global_file()?,
            Scope::Local(p) => p.join(".agent").join("mcp_config.json"),
        })
    }
}

impl Default for AntigravityAgent {
    fn default() -> Self {
        Self::new()
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

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let root = self.project_root(scope)?;
        rules_dir::is_installed(root, RULES_DIR, tag)
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let rules = spec.rules.as_ref().ok_or(HookerError::MissingSpecField {
            id: "antigravity",
            field: "rules",
        })?;
        rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
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

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&Self::mcp_path(scope)?);
        mcp_json_object::is_installed(&ledger, name)
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::install(&cfg, &ledger, spec)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
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

    fn is_skill_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        skills_dir::is_installed(&root, name)
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let root = Self::skills_root(scope)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
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
        HookSpec::builder(tag).command("noop").rules(body).build()
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
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
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
        let no_rules = HookSpec::builder("alpha").command("noop").build();
        let err = agent.install(&scope, &no_rules).unwrap_err();
        assert!(matches!(
            err,
            HookerError::MissingSpecField { field: "rules", .. }
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
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }
}
