//! Windsurf (Codeium Cascade) integration.
//!
//! Three surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.windsurf/rules/<tag>.md`.
//!
//! 2. **Hooks** — JSON config at `.windsurf/hooks.json` (Local). Each event
//!    key (e.g. `pre_run_command`, `post_cascade_response`) maps to an array
//!    of `{ "bash": "...", "_ai_hooker_tag": "..." }` entries; multiple
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
use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{
    has_refusal, InstallPlan, PlanTarget as DryPlanTarget, RefusalReason, UninstallPlan,
};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, McpSpec, SkillSpec};
use crate::status::{ConfigPresence, InstallStatus, PathStatus, PlanTarget, StatusReport};
use crate::util::{
    fs_atomic, json_patch, mcp_json_object, ownership, planning, rules_dir, skills_dir,
};

const RULES_DIR: &str = ".windsurf/rules";

/// Windsurf integration.
pub struct WindsurfAgent;

impl WindsurfAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "windsurf",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn hooks_path(&self, scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(self.project_root(scope)?.join(".windsurf/hooks.json"))
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::windsurf_mcp_global_file()?,
            Scope::Local(p) => p.join(".windsurf/mcp_config.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?
                .join(".codeium")
                .join("windsurf")
                .join("skills"),
            Scope::Local(p) => p.join(".windsurf").join("skills"),
        })
    }
}

impl Default for WindsurfAgent {
    fn default() -> Self {
        Self::new()
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

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
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

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(HookerError::UnsupportedScope { .. }) => {
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
                "bash": spec.command,
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

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
        HookSpec::validate_tag(tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(HookerError::UnsupportedScope { .. }) => {
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

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let mut report = InstallReport::default();

        if let Some(rules) = &spec.rules {
            let r = rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)?;
            report.merge(r);
        }

        // Hook entry — written when caller didn't ask for rules-only by
        // explicitly supplying nothing else, OR when they supplied a script.
        if spec.script.is_some() || spec.rules.is_none() {
            let event_key = event_to_windsurf(&spec.event);
            let p = self.hooks_path(scope)?;
            let mut root_doc = json_patch::read_or_empty(&p)?;
            let entry = json!({
                "bash": spec.command,
            });
            let changed = json_patch::upsert_tagged_array_entry(
                &mut root_doc,
                &[event_key.as_str()],
                &spec.tag,
                entry,
            )?;
            if changed {
                let bytes = json_patch::to_pretty(&root_doc);
                let outcome = fs_atomic::write_atomic(&p, &bytes, true)?;
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
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let mut report = UninstallReport::default();

        let r = rules_dir::uninstall(root, RULES_DIR, tag)?;
        report.merge(r);

        let p = self.hooks_path(scope)?;
        if p.exists() {
            let mut doc = json_patch::read_or_empty(&p)?;
            let changed = json_patch::remove_tagged_array_entries_under(&mut doc, &[], tag)?;
            if changed {
                let now_empty = doc.as_object().map(|o| o.is_empty()).unwrap_or(true);
                if now_empty {
                    // The file holds only tagged entries; once they're all
                    // gone the `.bak` snapshots an intermediate multi-event
                    // install of our own, not pre-install user content.
                    fs_atomic::remove_if_exists(&p)?;
                    fs_atomic::remove_backup_if_exists(&p)?;
                    report.removed.push(p.clone());
                } else {
                    let bytes = json_patch::to_pretty(&doc);
                    fs_atomic::write_atomic(&p, &bytes, false)?;
                    report.patched.push(p.clone());
                }
            }
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
    ) -> Result<StatusReport, HookerError> {
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

    fn plan_install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallPlan, HookerError> {
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
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::mcp_json_object_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
        )
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
    ) -> Result<StatusReport, HookerError> {
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
    ) -> Result<InstallPlan, HookerError> {
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
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::uninstall(&root, name, owner_tag)
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
    use serde_json::Value;
    use std::fs;
    use tempfile::tempdir;

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
    }

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(body).build()
    }

    fn hook_spec(tag: &str, event: Event, command: &str) -> HookSpec {
        HookSpec::builder(tag).command(command).event(event).build()
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
        assert_eq!(arr[0]["_ai_hooker_tag"], serde_json::json!("alpha"));
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
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
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
        // After uninstall, the tag must not appear in any event array. The
        // file may be removed entirely or restored from a `.bak` snapshot
        // taken between installs (which itself contains a partial install we
        // also stripped); both are correct cleanup outcomes.
        let p = dir.path().join(".windsurf/hooks.json");
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
        if p.exists() {
            let v = read_json(&p);
            for key in known_event_keys() {
                let Some(arr) = v.get(key).and_then(|x| x.as_array()) else {
                    continue;
                };
                assert!(
                    !arr.iter().any(|e| e["_ai_hooker_tag"] == "alpha"),
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
        assert_eq!(arr[0]["_ai_hooker_tag"], serde_json::json!("appB"));
    }

    #[test]
    fn rejects_global_scope() {
        let agent = WindsurfAgent::new();
        let err = agent.is_installed(&Scope::Global, "x").unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
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
}
