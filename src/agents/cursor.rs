//! Cursor integration.
//!
//! Hook surface: `<scope>/.cursor/hooks.json`. Cursor uses lowerCamelCase
//! event names and requires a top-level `"version": 1`.
//!
//! ```json
//! {
//!   "version": 1,
//!   "hooks": {
//!     "preToolUse": [
//!       { "command": "...", "matcher": "Shell", "_agent_config_tag": "myapp" }
//!     ]
//!   }
//! }
//! ```

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, json_patch, mcp_json_object, ownership, planning, safe_fs, skills_dir,
};

/// Cursor (the AI editor and CLI).
#[derive(Debug, Clone, Copy, Default)]
pub struct CursorAgent {
    _private: (),
}

impl CursorAgent {
    /// Construct an instance. The struct is stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::cursor_home()?.join("hooks.json"),
            Scope::Local(p) => p.join(".cursor").join("hooks.json"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::cursor_mcp_user_file()?,
            Scope::Local(p) => p.join(".cursor").join("mcp.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::cursor_home()?.join("skills"),
            Scope::Local(p) => p.join(".cursor").join("skills"),
        })
    }
}

impl Integration for CursorAgent {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::hooks_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&p, &["hooks"], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, p, presence))
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
        let p = Self::hooks_path(scope)?;
        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_cursor(&spec.matcher);
        let entry = json!({
            "command": spec.command.render_shell(),
            "matcher": matcher_str,
        });
        let mut changes = Vec::new();
        planning::plan_tagged_json_upsert(
            &mut changes,
            &p,
            &["hooks", event_key.as_str()],
            &spec.tag,
            entry,
            |root| {
                if root.get("version").is_none() {
                    if let Some(obj) = root.as_object_mut() {
                        obj.insert("version".into(), json!(1));
                    }
                }
            },
        )?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let p = Self::hooks_path(scope)?;
        let mut changes = Vec::new();
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
            &["hooks"],
            tag,
            is_effectively_empty,
            true,
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_path(scope)?;
        scope.ensure_contained(&p)?;
        file_lock::with_lock(&p, || {
            let mut root = json_patch::read_or_empty(&p)?;

            // Cursor requires top-level version: 1.
            if root.get("version").is_none() {
                if let Some(obj) = root.as_object_mut() {
                    obj.insert("version".into(), json!(1));
                }
            }

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_cursor(&spec.matcher);

            let entry = json!({
                "command": spec.command.render_shell(),
                "matcher": matcher_str,
            });

            let changed = json_patch::upsert_tagged_array_entry(
                &mut root,
                &["hooks", &event_key],
                &spec.tag,
                entry,
            )?;

            if changed {
                let bytes = json_patch::to_pretty(&root);
                let outcome = safe_fs::write(scope, &p, &bytes, true)?;
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

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::hooks_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut root = json_patch::read_or_empty(&p)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;

                if !changed {
                    report.not_installed = true;
                    return Ok(());
                }

                if is_effectively_empty(&root) {
                    let bytes = json_patch::to_pretty(&root);
                    if safe_fs::restore_backup_if_matches(scope, &p, &bytes)? {
                        report.restored.push(p.clone());
                    } else {
                        safe_fs::remove_file(scope, &p)?;
                        report.removed.push(p.clone());
                    }
                } else {
                    let bytes = json_patch::to_pretty(&root);
                    safe_fs::write(scope, &p, &bytes, false)?;
                    report.patched.push(p.clone());
                }
                Ok::<(), AgentConfigError>(())
            })?;
        } else {
            report.not_installed = true;
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }

        Ok(report)
    }
}

impl McpSurface for CursorAgent {
    fn id(&self) -> &'static str {
        "cursor"
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

impl SkillSurface for CursorAgent {
    fn id(&self) -> &'static str {
        "cursor"
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

/// True if the document has nothing meaningful left (only `{"version": ...}`
/// or fully empty).
fn is_effectively_empty(v: &Value) -> bool {
    let Some(obj) = v.as_object() else {
        return true;
    };
    obj.iter().all(|(k, _)| k == "version")
}

/// Map our [`Matcher`] enum to Cursor's matcher syntax.
///
/// For `preToolUse`/`postToolUse`, matcher is a tool-type literal:
/// `Shell`, `Read`, `Write`, `Edit`, `Grep`, `Delete`, `Task`,
/// or `MCP:<tool_name>`. For shell execution Cursor uses `Shell` (Claude's
/// equivalent is `Bash`).
fn matcher_to_cursor(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "Shell".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "preToolUse".into(),
        Event::PostToolUse => "postToolUse".into(),
        Event::Custom(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
    fn writes_lowercamel_event_and_shell_matcher() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".cursor/hooks.json"));
        assert_eq!(v["version"], json!(1));
        assert_eq!(v["hooks"]["preToolUse"][0]["matcher"], json!("Shell"));
        assert_eq!(v["hooks"]["preToolUse"][0]["command"], json!("myapp hook"));
        assert_eq!(
            v["hooks"]["preToolUse"][0]["_agent_config_tag"],
            json!("alpha")
        );
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let r1 = agent.install(&scope, &local_spec("alpha")).unwrap();
        let r2 = agent.install(&scope, &local_spec("alpha")).unwrap();
        assert!(!r1.already_installed && r2.already_installed);
    }

    #[test]
    fn install_preserves_user_hooks_and_other_settings() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".cursor/hooks.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"{
  "version": 1,
  "hooks": { "preToolUse": [
    { "command": "user-script", "matcher": "Edit" }
  ]},
  "beforeShellExecution": [
    { "command": "user-net-check", "matcher": "curl" }
  ]
}"#,
        )
        .unwrap();

        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&p);
        assert_eq!(v["hooks"]["preToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(
            v["beforeShellExecution"][0]["command"],
            json!("user-net-check")
        );
    }

    #[test]
    fn uninstall_removes_only_our_entry_and_keeps_user_data() {
        let dir = tempdir().unwrap();
        let p = dir.path().join(".cursor/hooks.json");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            r#"{
  "version": 1,
  "hooks": { "preToolUse": [
    { "command": "user", "matcher": "Edit" }
  ]}
}"#,
        )
        .unwrap();

        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();

        let v = read_json(&p);
        let arr = v["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["matcher"], json!("Edit"));
    }

    #[test]
    fn uninstall_only_us_restores_backup_or_removes() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        let p = dir.path().join(".cursor/hooks.json");
        assert!(p.exists());

        agent.uninstall(&scope, "alpha").unwrap();
        assert!(
            !p.exists(),
            "we authored the file; should be removed on uninstall"
        );
    }

    #[test]
    fn matcher_bash_maps_to_shell_not_bash() {
        // This is the most common cross-tool footgun; pin the behavior.
        assert_eq!(matcher_to_cursor(&Matcher::Bash), "Shell");
    }

    #[test]
    fn post_tool_use_lowercamel() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::PostToolUse)
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_json(&dir.path().join(".cursor/hooks.json"));
        assert!(v["hooks"]["postToolUse"].is_array());
    }

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    #[test]
    fn install_mcp_writes_dot_cursor_mcp_json() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".cursor/mcp.json");
        assert!(cfg.exists());
        let v = read_json(&cfg);
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_separate_from_hooks_file() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        assert!(dir.path().join(".cursor/hooks.json").exists());
        assert!(dir.path().join(".cursor/mcp.json").exists());
        // Hooks file does not contain the MCP server.
        let hooks = read_json(&dir.path().join(".cursor/hooks.json"));
        assert!(hooks.get("mcpServers").is_none());
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &spec).unwrap();
        let r2 = agent.install_mcp(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
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
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        assert!(!dir.path().join(".cursor/mcp.json").exists());
    }
}
