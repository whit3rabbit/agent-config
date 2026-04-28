//! Pi coding-agent integration.
//!
//! Pi (`@mariozechner/pi-coding-agent`) is a minimal terminal harness whose
//! built-in surface is intentionally small: four file-system tools and an
//! extension system implemented in TypeScript. There is no JSON-config-driven
//! hook surface — Pi extensions register `pi.on("tool_call", ...)` handlers
//! programmatically — so this integration refuses any [`HookSpec`] that lacks
//! a `rules` body.
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: tagged HTML-comment fence in `AGENTS.md` (Pi reads
//!    `~/.pi/agent/AGENTS.md` globally and walks up from cwd locally).
//! 2. **MCP servers**: standard `mcpServers` JSON map at `~/.pi/agent/mcp.json`
//!    (Global) or `<root>/.pi/mcp.json` (Local). Pi's optional
//!    `pi-mcp-adapter` extension reads this shape. We intentionally write to
//!    Pi-owned files only, never to the cross-host `~/.config/mcp/mcp.json`
//!    or top-level `.mcp.json`.
//! 3. **Skills**: directory-scoped skills at `~/.pi/agent/skills/<name>/`
//!    (Global) or `<root>/.pi/skills/<name>/` (Local), each a folder with
//!    `SKILL.md` plus optional `scripts/`/`references/`/`assets/`.
//! 4. **Instructions**: `InlineBlock` placement inside the same `AGENTS.md`
//!    file as rules.
//!
//! Pi has no documented config-file hook contract; mapping `Event::PreToolUse`
//! and friends to a JSON envelope would be speculation. If upstream ever ships
//! one, register it here.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, mcp_json_object, md_block, ownership, safe_fs,
    skills_dir,
};

/// Pi coding-agent installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct PiAgent {
    _private: (),
}

impl PiAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// `~/.pi/agent/AGENTS.md` (Global) or `<root>/AGENTS.md` (Local). The
    /// project-local path is the standard cross-harness memory file; the
    /// fenced HTML-comment block keeps our content scoped so other agents
    /// reading the same file don't see the fence as their content.
    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::pi_home()?.join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::pi_home()?.join("mcp.json"),
            Scope::Local(p) => p.join(".pi").join("mcp.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::pi_home()?.join("skills"),
            Scope::Local(p) => p.join(".pi").join("skills"),
        })
    }

    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::pi_home()?,
            Scope::Local(p) => p.join(".pi"),
        })
    }
}

impl Integration for PiAgent {
    fn id(&self) -> &'static str {
        "pi"
    }

    fn display_name(&self) -> &'static str {
        "Pi"
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
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "pi",
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

impl McpSurface for PiAgent {
    fn id(&self) -> &'static str {
        "pi"
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

impl SkillSurface for PiAgent {
    fn id(&self) -> &'static str {
        "pi"
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
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl PiAgent {
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

impl InstructionSurface for PiAgent {
    fn id(&self) -> &'static str {
        "pi"
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
    use serde_json::{json, Value};
    use std::path::Path;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn command_spec(tag: &str) -> HookSpec {
        // Hook command without rules — Pi has no config-file hook surface, so
        // installing this should refuse with `MissingSpecField`.
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
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
    fn install_writes_agents_md_at_root() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use Pi."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(body.contains("Use Pi."));
        assert!(body.contains("BEGIN AGENT-CONFIG:alpha"));
    }

    #[test]
    fn install_refuses_hook_without_rules() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let err = agent.install(&scope, &command_spec("alpha")).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::MissingSpecField {
                id: "pi",
                field: "rules"
            }
        ));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = rules_spec("alpha", "x");
        agent.install(&scope, &s).unwrap();
        let r2 = agent.install(&scope, &s).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "x")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn install_mcp_writes_dot_pi_mcp_json() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&dir.path().join(".pi/mcp.json"));
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_mcp_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_skill_writes_dot_pi_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = SkillSpec::builder("my-skill")
            .owner("myapp")
            .description("test")
            .body("Body.")
            .build();
        agent.install_skill(&scope, &spec).unwrap();
        assert!(dir.path().join(".pi/skills/my-skill/SKILL.md").exists());
    }

    #[test]
    fn plan_install_then_install_matches() {
        let dir = tempdir().unwrap();
        let agent = PiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "Use Pi.");

        let plan = agent.plan_install(&scope, &spec).unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
        assert!(!plan.changes.is_empty());

        agent.install(&scope, &spec).unwrap();
        assert!(dir.path().join("AGENTS.md").exists());
    }
}
