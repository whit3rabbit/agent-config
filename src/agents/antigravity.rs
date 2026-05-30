//! Google Antigravity integration.
//!
//! Four surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.agents/rules/<tag>.md`.
//!    Legacy `.agent/rules/<tag>.md` installs are still detected and removed.
//!
//! 2. **Skills** — directory-scoped skills at `.agents/skills/<name>/` (Local)
//!    or `~/.gemini/antigravity/skills/<name>/` (Global). Each skill is a
//!    folder with `SKILL.md` plus optional `scripts/`/`references/`/`assets/`.
//!    Legacy `.agent/skills/<name>/` installs are still detected and removed.
//!
//! 3. **MCP servers** — JSON config at `.agents/mcp_config.json` (Local) or
//!    `~/.gemini/config/mcp_config.json` (Global), keyed by server name
//!    under `mcpServers`.
//!
//! 4. **Hooks** — event hooks inside `hooks.json` at `.agents/hooks.json` (Local)
//!    or `~/.gemini/config/hooks.json` (Global).

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{hooks_json, instructions_dir, mcp_json_object, ownership, rules_dir, skills_dir};

const RULES_DIR: &str = ".agents/rules";
const LEGACY_RULES_DIR: &str = ".agent/rules";

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

    /// Skills root: `<root>/.agents/skills/` (Local) or
    /// `~/.gemini/antigravity/skills/` (Global). Both scopes are supported
    /// for skills.
    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("antigravity").join("skills"),
            Scope::Local(p) => p.join(".agents").join("skills"),
        })
    }

    fn legacy_skills_root(scope: &Scope) -> Option<PathBuf> {
        match scope {
            Scope::Global => None,
            Scope::Local(p) => Some(p.join(".agent").join("skills")),
        }
    }

    fn existing_skills_root(scope: &Scope, name: &str) -> Result<PathBuf, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        let (dir, _, ledger) = skills_dir::paths_for_status(&root, name);
        if dir.exists() || ownership::owner_of(&ledger, name)?.is_some() {
            return Ok(root);
        }

        if let Some(legacy) = Self::legacy_skills_root(scope) {
            let (dir, _, ledger) = skills_dir::paths_for_status(&legacy, name);
            if dir.exists() || ownership::owner_of(&ledger, name)?.is_some() {
                return Ok(legacy);
            }
        }

        Ok(root)
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("config").join("hooks.json"),
            Scope::Local(p) => p.join(".agents").join("hooks.json"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_mcp_global_file()?,
            Scope::Local(p) => p.join(".agents").join("mcp_config.json"),
        })
    }

    fn existing_mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        let primary = Self::mcp_path(scope)?;
        if primary.exists() {
            return Ok(primary);
        }
        if let Scope::Local(p) = scope {
            let legacy = p.join(".agent").join("mcp_config.json");
            if legacy.exists() {
                return Ok(legacy);
            }
        }
        Ok(primary)
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
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;

        // 1. Check rules files (Local scope only)
        if let Scope::Local(p) = scope {
            let path = rules_dir::target_path(p, RULES_DIR, tag);
            if path.exists() {
                return Ok(StatusReport::for_file_hook(tag, path));
            }
            let legacy = rules_dir::target_path(p, LEGACY_RULES_DIR, tag);
            if legacy.exists() {
                return Ok(StatusReport::for_file_hook(tag, legacy));
            }
        }

        // 2. Check hooks.json
        let hooks_path = Self::hooks_path(scope)?;
        let presence = hooks_json::config_presence(&hooks_path, tag)?;
        if let crate::status::ConfigPresence::Absent = presence {
            if let Scope::Local(p) = scope {
                let path = rules_dir::target_path(p, RULES_DIR, tag);
                Ok(StatusReport::for_file_hook(tag, path))
            } else {
                Ok(StatusReport::for_tagged_hook(tag, hooks_path, presence))
            }
        } else {
            Ok(StatusReport::for_tagged_hook(tag, hooks_path, presence))
        }
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let mut changes = Vec::new();

        // 1. Plan hook command installation in hooks.json
        let hooks_path = Self::hooks_path(scope)?;
        hooks_json::plan_install(&mut changes, &hooks_path, spec, build_hook_value)?;

        // 2. Plan rules installation if spec.rules is Some
        if let Some(rules) = &spec.rules {
            let root = self.project_root(scope);
            let root = match root {
                Ok(root) => root,
                Err(AgentConfigError::UnsupportedScope { .. }) => {
                    return Ok(InstallPlan::refused(
                        target,
                        None,
                        crate::plan::RefusalReason::UnsupportedScope,
                    ));
                }
                Err(e) => return Err(e),
            };
            let rule_changes = rules_dir::plan_install(root, RULES_DIR, &spec.tag, &rules.content)?;
            changes.extend(rule_changes);
        }

        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let mut changes = Vec::new();

        // 1. Plan hook command removal
        let hooks_path = Self::hooks_path(scope)?;
        hooks_json::plan_uninstall(&mut changes, &hooks_path, tag)?;

        // 2. Plan rules removal if in Local scope
        if let Scope::Local(p) = scope {
            let current = rules_dir::target_path(p, RULES_DIR, tag);
            let legacy = rules_dir::target_path(p, LEGACY_RULES_DIR, tag);
            let rules_dir_name = if !current.exists() && legacy.exists() {
                LEGACY_RULES_DIR
            } else {
                RULES_DIR
            };
            let rule_changes = rules_dir::plan_uninstall(p, rules_dir_name, tag)?;
            changes.extend(rule_changes);
        }

        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        // 1. Install hook command
        let hooks_path = Self::hooks_path(scope)?;
        let hook_report = hooks_json::install(scope, &hooks_path, spec, build_hook_value)?;
        report.created.extend(hook_report.created);
        report.patched.extend(hook_report.patched);
        report.backed_up.extend(hook_report.backed_up);
        report.already_installed = hook_report.already_installed;

        // 2. Install rules if present
        if let Some(rules) = &spec.rules {
            let _ = self.project_root(scope)?;
            let rules_report = rules_dir::install(scope, RULES_DIR, &spec.tag, &rules.content)?;
            report.created.extend(rules_report.created);
            report.patched.extend(rules_report.patched);
            report.backed_up.extend(rules_report.backed_up);
            if !rules_report.already_installed {
                report.already_installed = false;
            }
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        // 1. Uninstall hook command
        let hooks_path = Self::hooks_path(scope)?;
        let hook_report = hooks_json::uninstall(scope, &hooks_path, tag)?;
        report.removed.extend(hook_report.removed);
        report.patched.extend(hook_report.patched);
        report.restored.extend(hook_report.restored);
        report.not_installed = hook_report.not_installed;

        // 2. Uninstall rules if in Local scope
        if let Scope::Local(p) = scope {
            let current = rules_dir::target_path(p, RULES_DIR, tag);
            let legacy = rules_dir::target_path(p, LEGACY_RULES_DIR, tag);
            let rules_dir_name = if !current.exists() && legacy.exists() {
                LEGACY_RULES_DIR
            } else {
                RULES_DIR
            };
            let rules_report = rules_dir::uninstall(scope, rules_dir_name, tag)?;
            report.removed.extend(rules_report.removed);
            report.patched.extend(rules_report.patched);
            report.restored.extend(rules_report.restored);
            if !rules_report.not_installed {
                report.not_installed = false;
            }
        }

        Ok(report)
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
        let cfg = Self::existing_mcp_path(scope)?;
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
            Self::existing_mcp_path(scope),
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
            Self::existing_mcp_path(scope),
        )
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::existing_mcp_path(scope)?;
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
        let cfg = Self::existing_mcp_path(scope)?;
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
        if !dir.exists() && ownership::owner_of(&ledger, name)?.is_none() {
            if let Some(legacy) = Self::legacy_skills_root(scope) {
                let (legacy_dir, legacy_manifest, legacy_ledger) =
                    skills_dir::paths_for_status(&legacy, name);
                if legacy_dir.exists() || ownership::owner_of(&legacy_ledger, name)?.is_some() {
                    let recorded = ownership::owner_of(&legacy_ledger, name)?;
                    return Ok(StatusReport::for_skill(
                        name,
                        legacy_dir,
                        legacy_manifest,
                        legacy_ledger,
                        expected_owner,
                        recorded,
                    ));
                }
            }
        }
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
            Self::existing_skills_root(scope, name),
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
        let root = Self::existing_skills_root(scope, name)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl AntigravityAgent {
    fn standalone_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        let root = self.project_root(scope)?;
        Ok(instructions_dir::StandaloneLayout {
            config_dir: root.join(".agents"),
            instruction_dir: root.join(RULES_DIR),
        })
    }

    fn legacy_standalone_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        let root = self.project_root(scope)?;
        Ok(instructions_dir::StandaloneLayout {
            config_dir: root.join(".agent"),
            instruction_dir: root.join(LEGACY_RULES_DIR),
        })
    }

    fn existing_standalone_layout(
        &self,
        scope: &Scope,
        name: &str,
    ) -> Result<instructions_dir::StandaloneLayout, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        let primary = self.standalone_layout(scope)?;
        let primary_file = primary.instruction_dir.join(format!("{name}.md"));
        let primary_ledger = instructions_dir::ledger_path(&primary.config_dir);
        if primary_file.exists() || ownership::owner_of(&primary_ledger, name)?.is_some() {
            return Ok(primary);
        }

        let legacy = self.legacy_standalone_layout(scope)?;
        let legacy_file = legacy.instruction_dir.join(format!("{name}.md"));
        let legacy_ledger = instructions_dir::ledger_path(&legacy.config_dir);
        if legacy_file.exists() || ownership::owner_of(&legacy_ledger, name)?.is_some() {
            return Ok(legacy);
        }

        Ok(primary)
    }
}

impl InstructionSurface for AntigravityAgent {
    fn id(&self) -> &'static str {
        "antigravity"
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
        instructions_dir::standalone_status(
            self.existing_standalone_layout(scope, name)?,
            name,
            expected_owner,
        )
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
            self.existing_standalone_layout(scope, name),
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
            self.existing_standalone_layout(scope, name)?,
            name,
            owner_tag,
        )
    }
}

fn matcher_to_antigravity(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "run_command".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn build_hook_value(spec: &HookSpec) -> serde_json::Value {
    let matcher_str = matcher_to_antigravity(&spec.matcher);
    let command_str = spec.command.render_shell();
    serde_json::json!([
        {
            "matcher": matcher_str,
            "hooks": [
                {
                    "type": "command",
                    "command": command_str,
                    "timeout": 10
                }
            ]
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
    use std::fs;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn hook_only_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("myapp", ["hook"])
            .matcher(Matcher::Bash)
            .event(crate::spec::Event::PreToolUse)
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

    fn instruction(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::StandaloneFile)
            .body("Use Antigravity instructions.\n")
            .build()
    }

    #[test]
    fn install_rules_uses_plural_dot_agents() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".agents/rules/alpha.md").exists());
        assert!(!dir.path().join(".agent/rules/alpha.md").exists());
    }

    #[test]
    fn legacy_dot_agent_rules_status_and_uninstall_still_work() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        fs::create_dir_all(dir.path().join(".agent/rules")).unwrap();
        fs::write(dir.path().join(".agent/rules/alpha.md"), "legacy\n").unwrap();

        assert!(agent.is_installed(&scope, "alpha").unwrap());
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".agent/rules/alpha.md").exists());
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
    fn install_skill_writes_under_dot_agents_skills() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha", "myapp"))
            .unwrap();
        assert!(dir.path().join(".agents/skills/alpha/SKILL.md").exists());
        assert!(!dir.path().join(".agent/skills/alpha/SKILL.md").exists());
        let s = fs::read_to_string(dir.path().join(".agents/skills/alpha/SKILL.md")).unwrap();
        assert!(s.contains("name: alpha"));
        assert!(s.contains("description: Format Git commits."));
    }

    #[test]
    fn legacy_dot_agent_skill_status_and_uninstall_still_work() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let legacy_root = dir.path().join(".agent/skills");
        skills_dir::install(&legacy_root, &skill("alpha", "myapp")).unwrap();

        assert!(agent
            .is_skill_installed(&scope, "alpha")
            .expect("legacy skill status"));
        agent.uninstall_skill(&scope, "alpha", "myapp").unwrap();
        assert!(!dir.path().join(".agent/skills/alpha").exists());
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
        assert!(!dir.path().join(".agents/skills/alpha").exists());
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
    fn install_instruction_writes_under_dot_agents_rules() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction("alpha", "myapp"))
            .unwrap();
        assert!(dir.path().join(".agents/rules/alpha.md").exists());
        assert!(!dir.path().join(".agent/rules/alpha.md").exists());
    }

    #[test]
    fn legacy_dot_agent_instruction_status_and_uninstall_still_work() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let legacy = instructions_dir::StandaloneLayout {
            config_dir: dir.path().join(".agent"),
            instruction_dir: dir.path().join(".agent/rules"),
        };
        instructions_dir::standalone_install(&scope, legacy, &instruction("alpha", "myapp"))
            .unwrap();

        assert!(agent
            .is_instruction_installed(&scope, "alpha")
            .expect("legacy instruction status"));
        agent
            .uninstall_instruction(&scope, "alpha", "myapp")
            .unwrap();
        assert!(!dir.path().join(".agent/rules/alpha.md").exists());
    }

    #[test]
    fn install_mcp_writes_dot_agents_mcp_config() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".agents/mcp_config.json");
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

    #[test]
    fn install_hook_writes_hooks_json() {
        let dir = tempdir().unwrap();
        let agent = AntigravityAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &hook_only_spec("alpha")).unwrap();
        let cfg = dir.path().join(".agents/hooks.json");
        let v: serde_json::Value = serde_json::from_slice(&fs::read(cfg).unwrap()).unwrap();
        assert_eq!(
            v["alpha"]["PreToolUse"][0]["matcher"],
            serde_json::json!("run_command")
        );
    }
}
