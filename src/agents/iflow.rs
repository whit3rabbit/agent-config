//! iFlow CLI integration.
//!
//! iFlow combines hooks and MCP servers in a single `settings.json`:
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
//!   },
//!   "mcpServers": {
//!     "github": { "command": "npx", "args": ["..."] }
//!   }
//! }
//! ```
//!
//! Surfaces:
//!
//! 1. **Hooks**: Claude-shape JSON envelope.
//! 2. **MCP servers**: `mcpServers` JSON map in the same `settings.json`.
//!
//! Skills and a dedicated prompt-rules file are not part of iFlow's
//! documented surface.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec};
use crate::status::StatusReport;
use crate::util::{file_lock, json_patch, mcp_json_object, ownership, planning, safe_fs};

use crate::agents::planning as agent_planning;

/// iFlow CLI installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct IFlowAgent {
    _private: (),
}

impl IFlowAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn iflow_home_from_home(home: &Path) -> PathBuf {
        home.join(".iflow")
    }

    fn settings_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => Self::iflow_home_from_home(&paths::home_dir()?).join("settings.json"),
            Scope::Local(p) => p.join(".iflow").join("settings.json"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Self::settings_path(scope)
    }
}

impl Integration for IFlowAgent {
    fn id(&self) -> &'static str {
        "iflow"
    }

    fn display_name(&self) -> &'static str {
        "iFlow CLI"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::settings_path(scope)?;
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
        let p = Self::settings_path(scope)?;
        let mut changes = Vec::new();

        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_iflow(&spec.matcher);
        let entry = json!({
            "matcher": matcher_str,
            "hooks": [{ "type": "command", "command": spec.command.render_shell() }],
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
        let p = Self::settings_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
            &["hooks"],
            tag,
            planning::json_object_empty,
            true,
        )?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::settings_path(scope)?;
        scope.ensure_contained(&p)?;
        file_lock::with_lock(&p, || {
            let mut root = json_patch::read_or_empty(&p)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_iflow(&spec.matcher);

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

        let p = Self::settings_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut root = json_patch::read_or_empty(&p)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
                if changed {
                    let is_now_empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if is_now_empty && safe_fs::restore_backup_if_matches(scope, &p, &bytes)? {
                        report.restored.push(p.clone());
                    } else if is_now_empty {
                        safe_fs::remove_file(scope, &p)?;
                        report.removed.push(p.clone());
                    } else {
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

impl McpSurface for IFlowAgent {
    fn id(&self) -> &'static str {
        "iflow"
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

fn matcher_to_iflow(m: &Matcher) -> String {
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
    fn install_writes_settings_with_claude_shape() {
        let dir = tempdir().unwrap();
        let agent = IFlowAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".iflow/settings.json"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("Bash"));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = IFlowAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn hook_and_mcp_share_settings_json() {
        let dir = tempdir().unwrap();
        let agent = IFlowAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&dir.path().join(".iflow/settings.json"));
        assert!(v["hooks"]["PreToolUse"].is_array());
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = IFlowAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".iflow/settings.json").exists());
    }

    #[test]
    fn uninstall_mcp_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = IFlowAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }
}
