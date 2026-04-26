//! Tencent CodeBuddy CLI integration.
//!
//! CodeBuddy mirrors the Claude Code envelope: a `settings.json` with the
//! same `hooks.<event>` array structure, plus a `CLAUDE.md` memory file.
//!
//! Surfaces:
//!
//! 1. **Hooks**: `settings.json` JSON envelope (Claude shape). CodeBuddy
//!    documents nine events (`PreToolUse`, `PostToolUse`, `Notification`,
//!    `UserPromptSubmit`, `Stop`, `SubagentStop`, `PreCompact`,
//!    `SessionStart`, `SessionEnd`).
//! 2. **Prompt rules**: fenced HTML-comment block in `CLAUDE.md`.
//! 3. **Skills**: directory-scoped `SKILL.md` folders.
//!
//! MCP is not part of CodeBuddy's documented file-config surface.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, json_patch, md_block, ownership, planning, safe_fs, skills_dir,
};

use crate::agents::planning as agent_planning;

/// Tencent CodeBuddy CLI installer.
pub struct CodeBuddyAgent;

impl CodeBuddyAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn codebuddy_home_from_home(home: &Path) -> PathBuf {
        home.join(".codebuddy")
    }

    fn settings_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => {
                Self::codebuddy_home_from_home(&paths::home_dir()?).join("settings.json")
            }
            Scope::Local(p) => p.join(".codebuddy").join("settings.json"),
        })
    }

    fn memory_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => Self::codebuddy_home_from_home(&paths::home_dir()?).join("CLAUDE.md"),
            Scope::Local(p) => p.join("CLAUDE.md"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => Self::codebuddy_home_from_home(&paths::home_dir()?).join("skills"),
            Scope::Local(p) => p.join(".codebuddy").join("skills"),
        })
    }
}

impl Default for CodeBuddyAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for CodeBuddyAgent {
    fn id(&self) -> &'static str {
        "codebuddy"
    }

    fn display_name(&self) -> &'static str {
        "CodeBuddy CLI"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let settings = Self::settings_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&settings, &["hooks"], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, settings, presence))
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
        let settings = Self::settings_path(scope)?;
        let mut changes = Vec::new();

        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_codebuddy(&spec.matcher);
        let entry = json!({
            "matcher": matcher_str,
            "hooks": [{ "type": "command", "command": spec.command.render_shell() }],
        });
        planning::plan_tagged_json_upsert(
            &mut changes,
            &settings,
            &["hooks", event_key.as_str()],
            &spec.tag,
            entry,
            |_| {},
        )?;
        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        if let Some(rules) = &spec.rules {
            let memory = Self::memory_path(scope)?;
            planning::plan_markdown_upsert(&mut changes, &memory, &spec.tag, &rules.content)?;
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
        let settings = Self::settings_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &settings,
            &["hooks"],
            tag,
            planning::json_object_empty,
            true,
        )?;
        if has_refusal(&changes) {
            return Ok(UninstallPlan::from_changes(target, changes));
        }

        let memory = Self::memory_path(scope)?;
        planning::plan_markdown_remove(&mut changes, &memory, tag)?;

        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let settings = Self::settings_path(scope)?;
        scope.ensure_contained(&settings)?;
        file_lock::with_lock(&settings, || {
            let mut root = json_patch::read_or_empty(&settings)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_codebuddy(&spec.matcher);

            let entry = json!({
                "matcher": matcher_str,
                "hooks": [{ "type": "command", "command": spec.command.render_shell() }],
            });

            let changed = json_patch::upsert_tagged_array_entry(
                &mut root,
                &["hooks", &event_key],
                &spec.tag,
                entry,
            )?;

            if changed {
                let bytes = json_patch::to_pretty(&root);
                let outcome = safe_fs::write(scope, &settings, &bytes, true)?;
                if outcome.existed {
                    report.patched.push(outcome.path.clone());
                } else {
                    report.created.push(outcome.path.clone());
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
            } else {
                report.already_installed = true;
            }
            Ok::<(), AgentConfigError>(())
        })?;

        if let Some(rules) = &spec.rules {
            let memory = Self::memory_path(scope)?;
            scope.ensure_contained(&memory)?;
            file_lock::with_lock(&memory, || {
                let host = fs_atomic::read_to_string_or_empty(&memory)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = safe_fs::write(scope, &memory, new_host.as_bytes(), true)?;
                if !outcome.no_change {
                    if outcome.existed {
                        report.patched.push(outcome.path.clone());
                    } else {
                        report.created.push(outcome.path.clone());
                    }
                    report.already_installed = false;
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let settings = Self::settings_path(scope)?;
        scope.ensure_contained(&settings)?;
        if settings.exists() {
            file_lock::with_lock(&settings, || {
                let mut root = json_patch::read_or_empty(&settings)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
                if changed {
                    let is_now_empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if is_now_empty && safe_fs::restore_backup_if_matches(scope, &settings, &bytes)?
                    {
                        report.restored.push(settings.clone());
                    } else if is_now_empty {
                        safe_fs::remove_file(scope, &settings)?;
                        report.removed.push(settings.clone());
                    } else {
                        safe_fs::write(scope, &settings, &bytes, false)?;
                        report.patched.push(settings.clone());
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        let memory = Self::memory_path(scope)?;
        scope.ensure_contained(&memory)?;
        file_lock::with_lock(&memory, || {
            let host = fs_atomic::read_to_string_or_empty(&memory)?;
            let (stripped, removed) = md_block::remove(&host, tag);
            if removed {
                if stripped.trim().is_empty() {
                    if safe_fs::restore_backup_if_matches(scope, &memory, stripped.as_bytes())? {
                        report.restored.push(memory.clone());
                    } else {
                        safe_fs::remove_file(scope, &memory)?;
                        report.removed.push(memory.clone());
                    }
                } else {
                    safe_fs::write(scope, &memory, stripped.as_bytes(), false)?;
                    report.patched.push(memory.clone());
                }
            }
            Ok::<(), AgentConfigError>(())
        })?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl SkillSurface for CodeBuddyAgent {
    fn id(&self) -> &'static str {
        "codebuddy"
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

fn matcher_to_codebuddy(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "Bash".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "PreToolUse".into(),
        Event::PostToolUse => "PostToolUse".into(),
        Event::Custom(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::tempdir;

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("myapp", ["hook"])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    fn skill(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Test CodeBuddy skill.")
            .body("## Goal\nDo it.\n")
            .build()
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_writes_settings_with_claude_shape() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".codebuddy/settings.json"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("Bash"));
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["_agent_config_tag"],
            json!("alpha")
        );
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".codebuddy/settings.json").exists());
    }

    #[test]
    fn rules_block_writes_to_claude_md() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .rules("Use CodeBuddy rules.")
            .build();
        agent.install(&scope, &spec).unwrap();
        let md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(md.contains("Use CodeBuddy rules."));
    }

    #[test]
    fn install_skill_writes_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha-skill", "myapp"))
            .unwrap();
        assert!(dir
            .path()
            .join(".codebuddy/skills/alpha-skill/SKILL.md")
            .exists());
    }

    #[test]
    fn custom_event_round_trip() {
        let dir = tempdir().unwrap();
        let agent = CodeBuddyAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::Custom("SessionStart".into()))
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_json(&dir.path().join(".codebuddy/settings.json"));
        assert!(v["hooks"]["SessionStart"].is_array());
    }
}
