//! Gemini CLI integration.
//!
//! Hook surface: `<scope>/.gemini/settings.json` using event name
//! `BeforeTool` (Gemini's spelling — Claude/Codex use `PreToolUse`).
//!
//! ```json
//! {
//!   "hooks": {
//!     "BeforeTool": [
//!       {
//!         "matcher": "write_file|replace",
//!         "hooks": [
//!           { "type": "command", "command": "...", "timeout": 5000 }
//!         ],
//!         "_ai_hooker_tag": "myapp"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Optional prompt surface: `<scope>/GEMINI.md` (or `~/.gemini/GEMINI.md`)
//! with a tagged HTML-comment fence.

use std::path::PathBuf;

use serde_json::json;

use crate::agents::planning as agent_planning;
use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, json_patch, mcp_json_object, md_block, ownership, planning, skills_dir,
};

/// Gemini CLI (Google's official Gemini code agent).
pub struct GeminiAgent;

impl GeminiAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn settings_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("settings.json"),
            Scope::Local(p) => p.join(".gemini").join("settings.json"),
        })
    }

    fn memory_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("GEMINI.md"),
            Scope::Local(p) => p.join("GEMINI.md"),
        })
    }

    /// MCP servers live under the `mcpServers` key inside the same
    /// `settings.json` that holds hooks.
    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Self::settings_path(scope)
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("skills"),
            Scope::Local(p) => p.join(".gemini").join("skills"),
        })
    }
}

impl Default for GeminiAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for GeminiAgent {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::settings_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&p, &["hooks"], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, p, presence))
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let p = Self::settings_path(scope)?;
        let mut changes = Vec::new();

        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_gemini(&spec.matcher);
        let entry = json!({
            "matcher": matcher_str,
            "hooks": [
                { "type": "command", "command": spec.command.render_shell() }
            ],
        });
        planning::plan_tagged_json_upsert(
            &mut changes,
            &p,
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

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let mut changes = Vec::new();
        let p = Self::settings_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
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

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::settings_path(scope)?;
        scope.ensure_contained(&p)?;
        file_lock::with_lock(&p, || {
            let mut root = json_patch::read_or_empty(&p)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_gemini(&spec.matcher);

            let entry = json!({
                "matcher": matcher_str,
                "hooks": [
                    { "type": "command", "command": spec.command.render_shell() }
                ],
            });

            let changed = json_patch::upsert_tagged_array_entry(
                &mut root,
                &["hooks", &event_key],
                &spec.tag,
                entry,
            )?;

            if changed {
                let bytes = json_patch::to_pretty(&root);
                let outcome = fs_atomic::write_atomic(&p, &bytes, true)?;
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
            Ok::<(), HookerError>(())
        })?;

        if let Some(rules) = &spec.rules {
            let memory = Self::memory_path(scope)?;
            scope.ensure_contained(&memory)?;
            file_lock::with_lock(&memory, || {
                let host = fs_atomic::read_to_string_or_empty(&memory)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = fs_atomic::write_atomic(&memory, new_host.as_bytes(), true)?;
                if outcome.existed && !outcome.no_change {
                    report.patched.push(outcome.path.clone());
                    report.already_installed = false;
                } else if !outcome.existed {
                    report.created.push(outcome.path.clone());
                    report.already_installed = false;
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                Ok::<(), HookerError>(())
            })?;
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::settings_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut root = json_patch::read_or_empty(&p)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
                if changed {
                    let empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if empty && fs_atomic::restore_backup_if_matches(&p, &bytes)? {
                        report.restored.push(p.clone());
                    } else if empty {
                        fs_atomic::remove_if_exists(&p)?;
                        report.removed.push(p.clone());
                    } else {
                        fs_atomic::write_atomic(&p, &bytes, false)?;
                        report.patched.push(p.clone());
                    }
                }
                Ok::<(), HookerError>(())
            })?;
        }

        let memory = Self::memory_path(scope)?;
        scope.ensure_contained(&memory)?;
        file_lock::with_lock(&memory, || {
            let host = fs_atomic::read_to_string_or_empty(&memory)?;
            let (stripped, removed) = md_block::remove(&host, tag);
            if removed {
                if stripped.trim().is_empty() {
                    if fs_atomic::restore_backup_if_matches(&memory, stripped.as_bytes())? {
                        report.restored.push(memory.clone());
                    } else {
                        fs_atomic::remove_if_exists(&memory)?;
                        report.removed.push(memory.clone());
                    }
                } else {
                    fs_atomic::write_atomic(&memory, stripped.as_bytes(), false)?;
                    report.patched.push(memory.clone());
                }
            }
            Ok::<(), HookerError>(())
        })?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for GeminiAgent {
    fn id(&self) -> &'static str {
        "gemini"
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
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

impl SkillSurface for GeminiAgent {
    fn id(&self) -> &'static str {
        "gemini"
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
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

fn matcher_to_gemini(m: &Matcher) -> String {
    // Gemini matchers are regex for tool events. Tool names use snake_case
    // (e.g., `run_shell_command`, `write_file`, `replace`).
    match m {
        Matcher::All => "*".to_string(),
        // Conventional shell-tool name in Gemini's tool registry.
        Matcher::Bash => "run_shell_command".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "BeforeTool".into(),
        Event::PostToolUse => "AfterTool".into(),
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
    fn writes_before_tool_event_with_run_shell_command_matcher() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".gemini/settings.json"));
        assert_eq!(
            v["hooks"]["BeforeTool"][0]["matcher"],
            json!("run_shell_command")
        );
        assert_eq!(
            v["hooks"]["BeforeTool"][0]["hooks"][0]["command"],
            json!("myapp hook")
        );
        assert_eq!(
            v["hooks"]["BeforeTool"][0]["_ai_hooker_tag"],
            json!("alpha")
        );
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".gemini/settings.json").exists());
    }

    #[test]
    fn rules_block_upserts_into_gemini_md() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .matcher(Matcher::Bash)
            .rules("Always prefix shell calls.")
            .build();
        agent.install(&scope, &spec).unwrap();
        let md = std::fs::read_to_string(dir.path().join("GEMINI.md")).unwrap();
        assert!(md.contains("Always prefix shell calls."));
        assert!(md.contains("AI-HOOKER:alpha"));
    }

    #[test]
    fn matcher_mapping() {
        assert_eq!(matcher_to_gemini(&Matcher::All), "*");
        assert_eq!(matcher_to_gemini(&Matcher::Bash), "run_shell_command");
        assert_eq!(
            matcher_to_gemini(&Matcher::Exact("write_file".into())),
            "write_file"
        );
        assert_eq!(
            matcher_to_gemini(&Matcher::AnyOf(vec![
                "read_file".into(),
                "write_file".into()
            ])),
            "read_file|write_file"
        );
    }

    #[test]
    fn post_tool_use_maps_to_after_tool() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::PostToolUse)
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_json(&dir.path().join(".gemini/settings.json"));
        assert!(v["hooks"]["AfterTool"].is_array());
    }

    #[test]
    fn install_preserves_existing_settings() {
        let dir = tempdir().unwrap();
        let gemini_dir = dir.path().join(".gemini");
        std::fs::create_dir_all(&gemini_dir).unwrap();
        let settings = gemini_dir.join("settings.json");
        let existing = json!({
            "hooks": {
                "BeforeTool": [{
                    "matcher": "write_file",
                    "hooks": [{ "type": "command", "command": "user-cmd" }],
                    "_ai_hooker_tag": "user-tool"
                }]
            }
        });
        std::fs::write(&settings, serde_json::to_string_pretty(&existing).unwrap()).unwrap();

        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&settings);
        let entries = v["hooks"]["BeforeTool"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["_ai_hooker_tag"], json!("user-tool"));
        assert_eq!(entries[1]["_ai_hooker_tag"], json!("alpha"));
    }

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    #[test]
    fn install_mcp_writes_into_settings_json_alongside_hooks() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        // Install a hook first.
        agent.install(&scope, &local_spec("alpha")).unwrap();
        // Then install MCP — must coexist in same file.
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();

        let settings = dir.path().join(".gemini/settings.json");
        let v = read_json(&settings);
        // Hook is preserved.
        assert!(v["hooks"]["BeforeTool"].is_array());
        // MCP server is added under top-level mcpServers.
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn uninstall_mcp_keeps_hooks_in_settings_json() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();

        let settings = dir.path().join(".gemini/settings.json");
        let v = read_json(&settings);
        assert!(
            v["hooks"]["BeforeTool"].is_array(),
            "hooks must survive MCP uninstall"
        );
        assert!(v.get("mcpServers").is_none(), "mcpServers should be pruned");
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_mcp_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = GeminiAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }
}
