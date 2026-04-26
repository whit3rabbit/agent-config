//! Kilo Code integration.
//!
//! Two surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.kilocode/rules/<tag>.md`.
//!
//! 2. **MCP servers** — JSONC config at `~/.config/kilo/kilo.jsonc`
//!    (Global) or project `kilo.jsonc` / `.kilo/kilo.jsonc` (Local), keyed by
//!    server name under object-based `mcp`.

use std::path::Path;
use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, McpSpec, SkillSpec};
use crate::status::{ConfigPresence, StatusReport};
use crate::util::{instructions_dir, mcp_json_map, ownership, rules_dir, skills_dir};

const RULES_DIR: &str = ".kilocode/rules";

/// Kilo Code integration.
#[derive(Debug, Clone, Copy, Default)]
pub struct KiloCodeAgent {
    _private: (),
}

impl KiloCodeAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn require_local<'a>(&self, scope: &'a Scope) -> Result<&'a Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "kilocode",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn local_mcp_path(root: &Path) -> PathBuf {
        let dot_kilo = root.join(".kilo").join("kilo.jsonc");
        if dot_kilo.exists() {
            dot_kilo
        } else {
            root.join("kilo.jsonc")
        }
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::kilo_config_file()?,
            Scope::Local(p) => Self::local_mcp_path(p),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".kilo").join("skills"),
            Scope::Local(p) => p.join(".kilo").join("skills"),
        })
    }
}

impl Integration for KiloCodeAgent {
    fn id(&self) -> &'static str {
        "kilocode"
    }

    fn display_name(&self) -> &'static str {
        "Kilo Code"
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
        let root = self.require_local(scope)?;
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "kilocode",
                field: "rules",
            })?;
        rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.require_local(scope)?;
        rules_dir::uninstall(root, RULES_DIR, tag)
    }
}

impl McpSurface for KiloCodeAgent {
    fn id(&self) -> &'static str {
        "kilocode"
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
        let presence =
            mcp_json_map::config_presence(&cfg, &["mcp"], name, mcp_json_map::ConfigFormat::Jsonc)?;
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
        agent_planning::mcp_json_map_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::mcp_path(scope),
            &["mcp"],
            mcp_json_map::command_array_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::mcp_json_map_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
            &["mcp"],
            mcp_json_map::ConfigFormat::Jsonc,
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
        mcp_json_map::install(
            &cfg,
            &ledger,
            spec,
            &["mcp"],
            mcp_json_map::command_array_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
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
        mcp_json_map::uninstall(
            &cfg,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            &["mcp"],
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }
}

impl SkillSurface for KiloCodeAgent {
    fn id(&self) -> &'static str {
        "kilocode"
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
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl InstructionSurface for KiloCodeAgent {
    fn id(&self) -> &'static str {
        "kilocode"
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
        InstructionSpec::validate_name(name)?;
        let root = self.require_local(scope)?;
        let config_dir = root.join(".kilocode");
        let instruction_dir = root.join(".kilocode/rules");
        let (instr_path, led) =
            instructions_dir::paths_for_status(&config_dir, &instruction_dir, name);
        let presence = if instr_path.exists() {
            ConfigPresence::Single
        } else {
            ConfigPresence::Absent
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
        let root = self.require_local(scope)?;
        let config_dir = root.join(".kilocode");
        let instruction_dir = root.join(".kilocode/rules");
        let changes =
            instructions_dir::plan_install(&config_dir, spec, None, Some(&instruction_dir), None)?;
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
        let root = self.require_local(scope)?;
        let config_dir = root.join(".kilocode");
        let instruction_dir = root.join(".kilocode/rules");
        let changes = instructions_dir::plan_uninstall(
            &config_dir,
            name,
            owner_tag,
            None,
            Some(&instruction_dir),
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let root = self.require_local(scope)?;
        let config_dir = root.join(".kilocode");
        let instruction_dir = root.join(".kilocode/rules");
        let instr_path = instruction_dir.join(format!("{}.md", spec.name));
        scope.ensure_contained(&instr_path)?;
        instructions_dir::install(&config_dir, spec, None, Some(&instruction_dir), None)
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = self.require_local(scope)?;
        let config_dir = root.join(".kilocode");
        let instruction_dir = root.join(".kilocode/rules");
        scope.ensure_contained(&instruction_dir.join(format!("{name}.md")))?;
        instructions_dir::uninstall(&config_dir, name, owner_tag, None, Some(&instruction_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
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
            .env("FOO", "bar")
            .build()
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_rules_writes_existing_kilocode_rules_path() {
        let dir = tempdir().unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".kilocode/rules/alpha.md").exists());
    }

    #[test]
    fn install_mcp_writes_project_kilo_jsonc() {
        let dir = tempdir().unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join("kilo.jsonc");
        let v = read_json(&cfg);
        assert_eq!(v["mcp"]["github"]["type"], json!("local"));
        assert_eq!(
            v["mcp"]["github"]["command"],
            json!(["npx", "-y", "@example/server"])
        );
        assert_eq!(v["mcp"]["github"]["environment"]["FOO"], json!("bar"));
    }

    #[test]
    fn install_mcp_uses_existing_dot_kilo_config() {
        let dir = tempdir().unwrap();
        let dot = dir.path().join(".kilo/kilo.jsonc");
        std::fs::create_dir_all(dot.parent().unwrap()).unwrap();
        std::fs::write(&dot, "{\n  // existing\n}\n").unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        assert!(dot.exists());
        assert!(!dir.path().join("kilo.jsonc").exists());
    }

    #[test]
    fn install_mcp_reads_jsonc_with_comments_and_trailing_commas() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("kilo.jsonc");
        std::fs::write(
            &cfg,
            r#"{
  // user server
  "mcp": {
    "user": {
      "type": "remote",
      "url": "https://example.com/mcp",
    },
  },
}
"#,
        )
        .unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&cfg);
        assert_eq!(v["mcp"]["user"]["url"], json!("https://example.com/mcp"));
        assert_eq!(v["mcp"]["github"]["type"], json!("local"));
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    fn instruction_spec(name: &str, owner: &str, body: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::StandaloneFile)
            .body(body)
            .build()
    }

    #[test]
    fn instruction_writes_to_rules_dir() {
        let dir = tempdir().unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("RTK", "myapp", "# Use RTK\n"))
            .unwrap();
        let instr = dir.path().join(".kilocode/rules/RTK.md");
        assert!(instr.exists());
        assert!(std::fs::read_to_string(&instr)
            .unwrap()
            .contains("# Use RTK"));
    }

    #[test]
    fn instruction_uninstall_removes_file() {
        let dir = tempdir().unwrap();
        let agent = KiloCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("RTK", "myapp", "# Use RTK\n"))
            .unwrap();
        agent.uninstall_instruction(&scope, "RTK", "myapp").unwrap();
        assert!(!dir.path().join(".kilocode/rules/RTK.md").exists());
    }

    #[test]
    fn instruction_rejects_global_scope() {
        let agent = KiloCodeAgent::new();
        let err = agent
            .install_instruction(&Scope::Global, &instruction_spec("RTK", "myapp", "body\n"))
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::UnsupportedScope { .. }));
    }
}
