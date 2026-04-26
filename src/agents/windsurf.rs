//! Windsurf (Codeium Cascade) integration.
//!
//! Three surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.windsurf/rules/<tag>.md`.
//!
//! 2. **Hooks** — JSON config at `.windsurf/hooks.json` (Local). Each event
//!    key (e.g. `pre_run_command`, `post_cascade_response`) maps to an array
//!    of `{ "bash": "...", "_agent_config_tag": "..." }` entries; multiple
//!    consumers coexist via the standard tagged-array helper.
//!
//! 3. **MCP servers** — JSON config at `.windsurf/mcp_config.json` (Local) or
//!    `~/.codeium/windsurf/mcp_config.json` (Global), keyed by server name
//!    under `mcpServers`. Same shape as Claude/Cursor; reuses
//!    `util::mcp_json_object`.
//!
//! 4. **Skills** — directory-scoped skills at `.windsurf/skills/<name>/`
//!    (Local) or `~/.codeium/windsurf/skills/<name>/` (Global).

use std::path::PathBuf;

use serde_json::json;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{
    has_refusal, InstallPlan, PlanTarget as DryPlanTarget, RefusalReason, UninstallPlan,
};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, McpSpec, SkillSpec};
use crate::status::{ConfigPresence, InstallStatus, PathStatus, PlanTarget, StatusReport};
use crate::util::{
    file_lock, instructions_dir, json_patch, mcp_json_object, ownership, planning, rules_dir,
    safe_fs, skills_dir,
};

const RULES_DIR: &str = ".windsurf/rules";

/// Windsurf integration.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindsurfAgent {
    _private: (),
}

impl WindsurfAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "windsurf",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn hooks_path(&self, scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(self.project_root(scope)?.join(".windsurf/hooks.json"))
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::windsurf_mcp_global_file()?,
            Scope::Local(p) => p.join(".windsurf/mcp_config.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?
                .join(".codeium")
                .join("windsurf")
                .join("skills"),
            Scope::Local(p) => p.join(".windsurf").join("skills"),
        })
    }
}

impl Integration for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn display_name(&self) -> &'static str {
        "Windsurf"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let rules_file = rules_dir::target_path(root, RULES_DIR, tag);
        let rules_exists = rules_file.exists();

        let hooks_file = self.hooks_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&hooks_file, &[], tag)?;

        let hook_present = matches!(
            presence,
            ConfigPresence::Single | ConfigPresence::Duplicate { .. }
        );

        let status = if rules_exists || hook_present {
            InstallStatus::InstalledOwned {
                owner: tag.to_string(),
            }
        } else if let ConfigPresence::Invalid { reason } = &presence {
            InstallStatus::Drifted {
                issues: vec![crate::status::DriftIssue::InvalidConfig {
                    path: hooks_file.clone(),
                    reason: reason.clone(),
                }],
            }
        } else {
            InstallStatus::Absent
        };

        let mut files = vec![if rules_exists {
            PathStatus::Exists {
                path: rules_file.clone(),
            }
        } else {
            PathStatus::Missing {
                path: rules_file.clone(),
            }
        }];
        files.push(if hooks_file.exists() {
            PathStatus::Exists {
                path: hooks_file.clone(),
            }
        } else {
            PathStatus::Missing {
                path: hooks_file.clone(),
            }
        });

        Ok(StatusReport {
            target: PlanTarget::Hook {
                tag: tag.to_string(),
            },
            status,
            config_path: Some(hooks_file),
            ledger_path: None,
            files,
            warnings: Vec::new(),
        })
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(AgentConfigError::UnsupportedScope { .. }) => {
                return Ok(InstallPlan::refused(
                    target,
                    None,
                    RefusalReason::UnsupportedScope,
                ));
            }
            Err(e) => return Err(e),
        };
        let mut changes = Vec::new();
        if let Some(rules) = &spec.rules {
            changes.extend(rules_dir::plan_install(
                root,
                RULES_DIR,
                &spec.tag,
                &rules.content,
            )?);
        }
        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        if spec.script.is_some() || spec.rules.is_none() {
            let event_key = event_to_windsurf(&spec.event);
            let p = self.hooks_path(scope)?;
            let entry = json!({
                "bash": spec.command.render_shell(),
            });
            planning::plan_tagged_json_upsert(
                &mut changes,
                &p,
                &[event_key.as_str()],
                &spec.tag,
                entry,
                |_| {},
            )?;
        }

        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(AgentConfigError::UnsupportedScope { .. }) => {
                return Ok(UninstallPlan::refused(
                    target,
                    None,
                    RefusalReason::UnsupportedScope,
                ));
            }
            Err(e) => return Err(e),
        };
        let mut changes = rules_dir::plan_uninstall(root, RULES_DIR, tag)?;
        let p = self.hooks_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
            &[],
            tag,
            planning::json_object_empty,
            false,
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let mut report = InstallReport::default();

        if let Some(rules) = &spec.rules {
            scope.ensure_contained(&rules_dir::target_path(root, RULES_DIR, &spec.tag))?;
            let r = rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)?;
            report.merge(r);
        }

        // Hook entry — written when caller didn't ask for rules-only by
        // explicitly supplying nothing else, OR when they supplied a script.
        if spec.script.is_some() || spec.rules.is_none() {
            let event_key = event_to_windsurf(&spec.event);
            let p = self.hooks_path(scope)?;
            scope.ensure_contained(&p)?;
            file_lock::with_lock(&p, || {
                let mut root_doc = json_patch::read_or_empty(&p)?;
                let entry = json!({
                    "bash": spec.command.render_shell(),
                });
                let changed = json_patch::upsert_tagged_array_entry(
                    &mut root_doc,
                    &[event_key.as_str()],
                    &spec.tag,
                    entry,
                )?;
                if changed {
                    let bytes = json_patch::to_pretty(&root_doc);
                    let outcome = safe_fs::write(scope, &p, &bytes, true)?;
                    if outcome.existed {
                        report.patched.push(outcome.path.clone());
                    } else {
                        report.created.push(outcome.path.clone());
                    }
                    if let Some(b) = outcome.backup {
                        report.backed_up.push(b);
                    }
                    report.already_installed = false;
                } else if report.created.is_empty() && report.patched.is_empty() {
                    report.already_installed = true;
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let mut report = UninstallReport::default();

        scope.ensure_contained(&rules_dir::target_path(root, RULES_DIR, tag))?;
        let r = rules_dir::uninstall(root, RULES_DIR, tag)?;
        report.merge(r);

        let p = self.hooks_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut doc = json_patch::read_or_empty(&p)?;
                let changed = json_patch::remove_tagged_array_entries_under(&mut doc, &[], tag)?;
                if changed {
                    let now_empty = doc.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    if now_empty {
                        // The file holds only tagged entries; once they're all
                        // gone the `.bak` snapshots an intermediate multi-event
                        // install of our own, not pre-install user content.
                        safe_fs::remove_file(scope, &p)?;
                        safe_fs::remove_backup_if_exists(scope, &p)?;
                        report.removed.push(p.clone());
                    } else {
                        let bytes = json_patch::to_pretty(&doc);
                        safe_fs::write(scope, &p, &bytes, false)?;
                        report.patched.push(p.clone());
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
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

impl SkillSurface for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
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

impl InstructionSurface for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
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
        let root = self.project_root(scope)?;
        let config_dir = root.join(".windsurf");
        let instruction_dir = root.join(".windsurf/rules");
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
        let target = DryPlanTarget::Instruction {
            integration_id: InstructionSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let root = self.project_root(scope)?;
        let config_dir = root.join(".windsurf");
        let instruction_dir = root.join(".windsurf/rules");
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
        let target = DryPlanTarget::Instruction {
            integration_id: InstructionSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let root = self.project_root(scope)?;
        let config_dir = root.join(".windsurf");
        let instruction_dir = root.join(".windsurf/rules");
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
        let root = self.project_root(scope)?;
        let config_dir = root.join(".windsurf");
        let instruction_dir = root.join(".windsurf/rules");
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
        let root = self.project_root(scope)?;
        let config_dir = root.join(".windsurf");
        let instruction_dir = root.join(".windsurf/rules");
        scope.ensure_contained(&instruction_dir.join(format!("{name}.md")))?;
        instructions_dir::uninstall(&config_dir, name, owner_tag, None, Some(&instruction_dir))
    }
}

/// Map [`Event`] to Windsurf's hook key. Windsurf uses snake_case event
/// names (`pre_run_command`, `post_cascade_response`, etc.); the
/// PreToolUse/PostToolUse defaults map to the closest equivalents. Use
/// [`Event::Custom`] for anything else.
fn event_to_windsurf(event: &Event) -> String {
    match event {
        Event::PreToolUse => "pre_run_command".into(),
        Event::PostToolUse => "post_cascade_response".into(),
        Event::Custom(s) => s.clone(),
    }
}

/// Event keys documented by Windsurf. Kept for tests and documentation; hook
/// detection/removal scans all top-level arrays so custom events are covered.
#[cfg(test)]
fn known_event_keys() -> &'static [&'static str] {
    &[
        "pre_user_prompt",
        "pre_read_code",
        "pre_write_code",
        "pre_run_command",
        "pre_mcp_tool_use",
        "post_cascade_response",
        "post_user_prompt",
        "post_read_code",
        "post_write_code",
        "post_run_command",
        "post_mcp_tool_use",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::InstructionPlacement;
    use serde_json::Value;
    use std::fs;
    use tempfile::tempdir;

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
    }

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn hook_spec(tag: &str, event: Event, command: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_shell_unchecked(command)
            .event(event)
            .build()
    }

    #[test]
    fn install_rules_writes_dot_windsurf_rules_file() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".windsurf/rules/alpha.md").exists());
    }

    #[test]
    fn install_default_event_writes_pre_run_command_entry() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "myapp hook"))
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["bash"], serde_json::json!("myapp hook"));
        assert_eq!(arr[0]["_agent_config_tag"], serde_json::json!("alpha"));
    }

    #[test]
    fn install_custom_event_passes_through() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(
                &scope,
                &hook_spec("alpha", Event::Custom("pre_write_code".into()), "x"),
            )
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        assert!(v["pre_write_code"].is_array());
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = hook_spec("alpha", Event::PreToolUse, "x");
        agent.install(&scope, &s).unwrap();
        let r2 = agent.install(&scope, &s).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_coexists_with_other_consumer() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("appB", Event::PreToolUse, "b"))
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn install_mcp_writes_mcp_config_separate_from_hooks() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["@example/server"])
            .build();
        agent.install_mcp(&scope, &spec).unwrap();
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "x"))
            .unwrap();
        assert!(dir.path().join(".windsurf/mcp_config.json").exists());
        assert!(dir.path().join(".windsurf/hooks.json").exists());
    }

    #[test]
    fn mcp_supports_global_and_local_scopes() {
        let agent = WindsurfAgent::new();
        let scopes = agent.supported_mcp_scopes();
        assert!(scopes.contains(&ScopeKind::Global));
        assert!(scopes.contains(&ScopeKind::Local));
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = McpSpec::builder("github")
            .owner("appA")
            .stdio("npx", ["@example/server"])
            .build();
        agent.install_mcp(&scope, &spec).unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_strips_tagged_entry_from_all_event_arrays() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("alpha", Event::PostToolUse, "b"))
            .unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        // After uninstall, the tag must not appear in any event array.
        let p = dir.path().join(".windsurf/hooks.json");
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
        if p.exists() {
            let v = read_json(&p);
            for key in known_event_keys() {
                let Some(arr) = v.get(key).and_then(|x| x.as_array()) else {
                    continue;
                };
                assert!(
                    !arr.iter().any(|e| e["_agent_config_tag"] == "alpha"),
                    "tag should be stripped from {key}"
                );
            }
        }
    }

    #[test]
    fn uninstall_keeps_other_consumer_entries() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("appB", Event::PreToolUse, "b"))
            .unwrap();
        agent.uninstall(&scope, "appA").unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["_agent_config_tag"], serde_json::json!("appB"));
    }

    #[test]
    fn rejects_global_scope() {
        let agent = WindsurfAgent::new();
        let err = agent.is_installed(&Scope::Global, "x").unwrap_err();
        assert!(matches!(err, AgentConfigError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_rules_and_hook_independent() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "rules body"))
            .unwrap();
        // No hook file produced when only rules are present.
        assert!(!dir.path().join(".windsurf/hooks.json").exists());
        assert!(dir.path().join(".windsurf/rules/alpha.md").exists());
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
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("RTK", "myapp", "# Use RTK\n"))
            .unwrap();
        let instr = dir.path().join(".windsurf/rules/RTK.md");
        assert!(instr.exists());
        assert!(fs::read_to_string(&instr).unwrap().contains("# Use RTK"));
    }

    #[test]
    fn instruction_uninstall_removes_file() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction_spec("RTK", "myapp", "# Use RTK\n"))
            .unwrap();
        agent.uninstall_instruction(&scope, "RTK", "myapp").unwrap();
        assert!(!dir.path().join(".windsurf/rules/RTK.md").exists());
    }

    #[test]
    fn instruction_rejects_global_scope() {
        let agent = WindsurfAgent::new();
        let err = agent
            .install_instruction(&Scope::Global, &instruction_spec("RTK", "myapp", "body\n"))
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::UnsupportedScope { .. }));
    }
}
