//! Claude Code integration.
//!
//! Hook surface: `<scope>/.claude/settings.json` with the JSON envelope:
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "Bash",
//!         "hooks": [{ "type": "command", "command": "..." }],
//!         "_agent_config_tag": "myapp"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Optional prompt surface: `~/.claude/CLAUDE.md` (Global) or
//! `<project>/CLAUDE.md` (Local), with a tagged HTML-comment fence.

use std::path::PathBuf;

use serde_json::json;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, json_patch, mcp_json_object, md_block, ownership, planning, safe_fs,
    skills_dir,
};

/// Claude Code (Anthropic's official CLI).
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeAgent {
    _private: (),
}

impl ClaudeAgent {
    /// Construct an instance. The struct is stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn settings_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::claude_home()?.join("settings.json"),
            Scope::Local(p) => p.join(".claude").join("settings.json"),
        })
    }

    fn memory_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::claude_home()?.join("CLAUDE.md"),
            Scope::Local(p) => p.join("CLAUDE.md"),
        })
    }

    /// Path to the MCP config file for the given scope.
    ///
    /// Global → `~/.claude.json`.
    ///
    /// Local → `<root>/.mcp.json` (the canonical project-shared MCP file
    /// Anthropic's own CLI writes; *not* under `.claude/`).
    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::claude_mcp_user_file()?,
            Scope::Local(p) => p.join(".mcp.json"),
        })
    }

    /// `~/.claude/skills/` (Global) or `<root>/.claude/skills/` (Local).
    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::claude_home()?.join("skills"),
            Scope::Local(p) => p.join(".claude").join("skills"),
        })
    }

    /// Instruction layout for the given scope.
    ///
    /// Returns `(config_dir, host_file, instruction_dir, reference_line)`:
    /// - `config_dir`: where the ledger lives
    /// - `host_file`: the file that gets the include reference (`CLAUDE.md`)
    /// - `instruction_dir`: where instruction files are written
    /// - `reference_line`: the include reference string (e.g., `@RTK.md`)
    fn instruction_layout(
        scope: &Scope,
        name: &str,
    ) -> Result<(PathBuf, PathBuf, PathBuf, String), AgentConfigError> {
        Ok(match scope {
            Scope::Global => {
                let claude_home = paths::claude_home()?;
                let host = claude_home.join("CLAUDE.md");
                let ref_line = format!("@{name}.md");
                (claude_home.clone(), host, claude_home, ref_line)
            }
            Scope::Local(p) => {
                let config_dir = p.join(".claude");
                let host = p.join("CLAUDE.md");
                let instr_dir = config_dir.join("instructions");
                let ref_line = format!("@.claude/instructions/{name}.md");
                (config_dir, host, instr_dir, ref_line)
            }
        })
    }
}

impl Integration for ClaudeAgent {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
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
        let matcher_str = matcher_to_claude(&spec.matcher);
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
        {
            let _settings_lock = file_lock::FileLock::acquire(&settings)?;
            let mut root = json_patch::read_or_empty(&settings)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_claude(&spec.matcher);

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
        }

        if let Some(rules) = &spec.rules {
            let memory = Self::memory_path(scope)?;
            scope.ensure_contained(&memory)?;
            let _memory_lock = file_lock::FileLock::acquire(&memory)?;
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
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let settings = Self::settings_path(scope)?;
        scope.ensure_contained(&settings)?;
        if settings.exists() {
            let _settings_lock = file_lock::FileLock::acquire(&settings)?;
            let mut root = json_patch::read_or_empty(&settings)?;
            let changed =
                json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
            if changed {
                let is_now_empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                let bytes = json_patch::to_pretty(&root);
                if is_now_empty && safe_fs::restore_backup_if_matches(scope, &settings, &bytes)? {
                    report.restored.push(settings.clone());
                } else if is_now_empty {
                    safe_fs::remove_file(scope, &settings)?;
                    report.removed.push(settings.clone());
                } else {
                    safe_fs::write(scope, &settings, &bytes, false)?;
                    report.patched.push(settings.clone());
                }
            }
        }

        let memory = Self::memory_path(scope)?;
        scope.ensure_contained(&memory)?;
        let _memory_lock = file_lock::FileLock::acquire(&memory)?;
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

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for ClaudeAgent {
    fn id(&self) -> &'static str {
        "claude"
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
        spec.validate()?;
        let target = PlanTarget::Mcp {
            integration_id: McpSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let cfg = Self::mcp_path(scope)?;
        if let Some(plan) = agent_planning::mcp_local_inline_secret_refusal(
            target.clone(),
            scope,
            spec,
            Some(cfg.clone()),
        ) {
            return Ok(plan);
        }
        let ledger = ownership::mcp_ledger_for(&cfg);
        let changes = mcp_json_object::plan_install(&cfg, &ledger, spec)?;
        Ok(agent_planning::mcp_install_plan_from_changes(
            target,
            changes,
            scope,
            spec,
            Some(cfg),
        ))
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let target = PlanTarget::Mcp {
            integration_id: McpSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let changes =
            mcp_json_object::plan_uninstall(&cfg, &ledger, name, owner_tag, "mcp server")?;
        Ok(UninstallPlan::from_changes(target, changes))
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

impl SkillSurface for ClaudeAgent {
    fn id(&self) -> &'static str {
        "claude"
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
        spec.validate()?;
        let target = PlanTarget::Skill {
            integration_id: SkillSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let root = Self::skills_root(scope)?;
        let changes = skills_dir::plan_install(&root, spec)?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let target = PlanTarget::Skill {
            integration_id: SkillSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let root = Self::skills_root(scope)?;
        let changes = skills_dir::plan_uninstall(&root, name, owner_tag)?;
        Ok(UninstallPlan::from_changes(target, changes))
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

impl crate::integration::InstructionSurface for ClaudeAgent {
    fn id(&self) -> &'static str {
        "claude"
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
        InstructionSpec::validate_name(name)?;
        let (config_dir, host_file, instr_dir, _prefix) = Self::instruction_layout(scope, name)?;
        let led = crate::util::instructions_dir::ledger_path(&config_dir);
        let instr_path = instr_dir.join(format!("{name}.md"));

        let instr_exists = instr_path.exists();
        let block_in_host = if host_file.exists() {
            let host = fs_atomic::read_to_string_or_empty(&host_file)?;
            md_block::contains(&host, name)
        } else {
            false
        };

        let presence = if instr_exists || block_in_host {
            crate::status::ConfigPresence::Single
        } else {
            crate::status::ConfigPresence::Absent
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
            integration_id: crate::integration::InstructionSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let (config_dir, host_file, instr_dir, ref_line) =
            Self::instruction_layout(scope, &spec.name)?;
        let changes = crate::util::instructions_dir::plan_install(
            &config_dir,
            spec,
            Some(&host_file),
            Some(&instr_dir),
            Some(&ref_line),
        )?;
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
            integration_id: crate::integration::InstructionSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let (config_dir, host_file, instr_dir, _) = Self::instruction_layout(scope, name)?;
        let changes = crate::util::instructions_dir::plan_uninstall(
            &config_dir,
            name,
            owner_tag,
            Some(&host_file),
            Some(&instr_dir),
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        scope.ensure_contained(&Self::memory_path(scope)?)?;
        let (config_dir, host_file, instr_dir, ref_line) =
            Self::instruction_layout(scope, &spec.name)?;
        scope.ensure_contained(&host_file)?;
        scope.ensure_contained(&instr_dir.join(&spec.name))?;
        crate::util::instructions_dir::install(
            &config_dir,
            spec,
            Some(&host_file),
            Some(&instr_dir),
            Some(&ref_line),
        )
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let (config_dir, host_file, instr_dir, _) = Self::instruction_layout(scope, name)?;
        crate::util::instructions_dir::uninstall(
            &config_dir,
            name,
            owner_tag,
            Some(&host_file),
            Some(&instr_dir),
        )
    }
}
///
/// Claude treats matchers as exact tool-name match when the string contains
/// only `[A-Za-z0-9_|]`; anything else makes it a JS regex. We pass `Regex`
/// through verbatim and let the user own that.
fn matcher_to_claude(m: &Matcher) -> String {
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
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("myapp", ["hook"])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn local_install_writes_settings_json_with_correct_shape() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let p = dir.path().join(".claude/settings.json");
        let v = read_json(&p);
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("Bash"));
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("myapp hook")
        );
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["type"],
            json!("command")
        );
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["_agent_config_tag"],
            json!("alpha")
        );
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");

        let r1 = agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(!r1.already_installed);
        assert!(r2.already_installed);
    }

    #[test]
    fn install_preserves_user_authored_hooks() {
        let dir = tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{
  "hooks": {
    "PreToolUse": [
      { "matcher": "Edit", "hooks": [{ "type": "command", "command": "user-thing" }] }
    ]
  },
  "permissions": { "allow": ["Read"] }
}
"#,
        )
        .unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&settings);
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(v["permissions"]["allow"], json!(["Read"]));
        // Backup was made.
        assert!(dir.path().join(".claude/settings.json.bak").exists());
    }

    #[test]
    fn install_with_rules_writes_claude_md_block() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .matcher(Matcher::Bash)
            .rules("Use myapp prefix.")
            .build();
        agent.install(&scope, &spec).unwrap();

        let md = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(md.contains("<!-- BEGIN AGENT-CONFIG:alpha -->"));
        assert!(md.contains("Use myapp prefix."));
        assert!(md.contains("<!-- END AGENT-CONFIG:alpha -->"));
    }

    #[test]
    fn uninstall_removes_hook_and_restores_backup_if_we_were_only_content() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let settings = dir.path().join(".claude/settings.json");
        assert!(settings.exists());

        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!settings.exists(), "empty settings.json removed");
    }

    #[test]
    fn uninstall_preserves_user_hooks_after_removing_ours() {
        let dir = tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(
            &settings,
            r#"{ "hooks": { "PreToolUse": [
              { "matcher": "Edit", "hooks": [{ "type": "command", "command": "user-thing" }] }
            ]}}"#,
        )
        .unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();

        let v = read_json(&settings);
        let arr = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["matcher"], json!("Edit"));
    }

    #[test]
    fn uninstall_unknown_tag_is_noop() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let r = agent.uninstall(&scope, "ghost").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn matcher_any_of_pipes_join() {
        assert_eq!(
            matcher_to_claude(&Matcher::AnyOf(vec!["Edit".into(), "Write".into()])),
            "Edit|Write"
        );
    }

    #[test]
    fn malformed_settings_json_aborts_with_typed_error() {
        let dir = tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{ this is not json").unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let err = agent.install(&scope, &local_spec("alpha")).unwrap_err();
        assert!(matches!(err, AgentConfigError::JsonInvalid { .. }));
    }

    #[test]
    fn custom_event_passes_through() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::Custom("myCustomEvent".into()))
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_json(&dir.path().join(".claude/settings.json"));
        assert!(v["hooks"]["myCustomEvent"].is_array());
    }

    #[test]
    fn install_report_paths_under_project_dir() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let r = agent.install(&scope, &local_spec("alpha")).unwrap();
        assert!(!r.created.is_empty());
        let path = &r.created[0];
        assert!(path.starts_with(dir.path()));
        assert!(path.ends_with(PathBuf::from(".claude").join("settings.json")));
    }

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
            .env_from_host("GITHUB_TOKEN")
            .build()
    }

    #[test]
    fn local_install_mcp_writes_dot_mcp_json_at_project_root() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".mcp.json");
        assert!(cfg.exists(), "expected {} to exist", cfg.display());
        let v = read_json(&cfg);
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_does_not_touch_settings_or_dotclaude() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        assert!(!dir.path().join(".claude/settings.json").exists());
        assert!(!dir.path().join(".claude.json").exists());
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &spec).unwrap();
        let r2 = agent.install_mcp(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_mcp_coexists_with_hook_install() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        // Hooks live in .claude/settings.json; MCP lives in .mcp.json — separate files.
        assert!(dir.path().join(".claude/settings.json").exists());
        assert!(dir.path().join(".mcp.json").exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_mcp_round_trip() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        assert!(agent.is_mcp_installed(&scope, "github").unwrap());
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        assert!(!agent.is_mcp_installed(&scope, "github").unwrap());
        assert!(!dir.path().join(".mcp.json").exists());
    }

    #[test]
    fn install_mcp_invalid_name_rejected() {
        let dir = tempdir().unwrap();
        let agent = ClaudeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        // Build a spec by skipping the validating builder.
        let spec = McpSpec {
            name: "bad name".into(),
            owner_tag: "myapp".into(),
            transport: crate::spec::McpTransport::Stdio {
                command: "x".into(),
                args: vec![],
                env: Default::default(),
            },
            friendly_name: None,
            secret_policy: crate::spec::SecretPolicy::RefuseInlineSecretsInLocalScope,
        };
        let err = agent.install_mcp(&scope, &spec).unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }
}

#[cfg(test)]
mod instruction_tests {
    use super::*;
    use crate::integration::InstructionSurface;
    use crate::spec::InstructionPlacement;
    use std::fs;
    use tempfile::tempdir;

    fn instruction_spec(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::ReferencedFile)
            .body("# RTK\n\nUse rtk for compact output.\n")
            .build()
    }

    #[test]
    fn instruction_global_creates_rtk_md_and_reference() {
        let dir = tempdir().unwrap();
        let claude_home = dir.path().join("claude-home");
        fs::create_dir_all(&claude_home).unwrap();

        // Temporarily override home by using Local scope with a path that
        // mimics the global layout. Global scope requires a real home dir,
        // so we test the layout via Local scope instead.
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("RTK", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();

        let instr = root.join(".claude/instructions/RTK.md");
        assert!(instr.exists());
        assert!(fs::read_to_string(&instr).unwrap().contains("# RTK"));

        let claude_md = root.join("CLAUDE.md");
        let content = fs::read_to_string(&claude_md).unwrap();
        assert!(content.contains("@.claude/instructions/RTK.md"));
        assert!(content.contains("BEGIN AGENT-CONFIG:RTK"));
    }

    #[test]
    fn instruction_idempotent() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("RTK", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();
        let report = agent.install_instruction(&scope, &spec).unwrap();
        assert!(report.already_installed);
    }

    #[test]
    fn instruction_uninstall_removes_both() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("RTK", "myapp");
        agent.install_instruction(&scope, &spec).unwrap();

        agent.uninstall_instruction(&scope, "RTK", "myapp").unwrap();

        assert!(!root.join(".claude/instructions/RTK.md").exists());
        let claude_md = fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(!claude_md.contains("@.claude/instructions/RTK.md"));
    }

    #[test]
    fn instruction_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("RTK", "appA");
        agent.install_instruction(&scope, &spec).unwrap();

        let err = agent
            .uninstall_instruction(&scope, "RTK", "appB")
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn instruction_plan_does_not_mutate() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();

        let agent = ClaudeAgent::new();
        let scope = Scope::Local(root.clone());
        let spec = instruction_spec("RTK", "myapp");
        let plan = agent.plan_install_instruction(&scope, &spec).unwrap();

        assert!(!root.join(".claude/instructions/RTK.md").exists());
        assert!(!root.join("CLAUDE.md").exists());
        assert!(!plan.changes.is_empty());
    }
}
