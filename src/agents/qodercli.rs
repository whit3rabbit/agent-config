//! Qoder CLI integration.
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: `AGENTS.md` (`~/.qoder/AGENTS.md` or `<root>/AGENTS.md`).
//! 2. **MCP servers**: `mcpServers` JSON map. The user-scope file is
//!    `~/.qoder.json` (a flat file in `$HOME`, not under `~/.qoder/`); the
//!    project-scope file is `<root>/.mcp.json`.
//!
//! Skills and hooks are not part of Qoder's documented file-config surface.

use std::path::{Path, PathBuf};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec};
use crate::status::StatusReport;
use crate::util::{file_lock, fs_atomic, mcp_json_object, md_block, ownership, safe_fs};

/// Qoder CLI installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct QoderCliAgent {
    _private: (),
}

impl QoderCliAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn qoder_home_from_home(home: &Path) -> PathBuf {
        home.join(".qoder")
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => Self::qoder_home_from_home(&paths::home_dir()?).join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".qoder.json"),
            Scope::Local(p) => p.join(".mcp.json"),
        })
    }
}

impl Integration for QoderCliAgent {
    fn id(&self) -> &'static str {
        "qodercli"
    }

    fn display_name(&self) -> &'static str {
        "Qoder CLI"
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
                id: "qodercli",
                field: "rules",
            })?;
        let path = Self::rules_path(scope)?;
        let mut report = InstallReport::default();
        scope.ensure_contained(&path)?;
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
        let mut report = UninstallReport::default();
        scope.ensure_contained(&path)?;
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

impl McpSurface for QoderCliAgent {
    fn id(&self) -> &'static str {
        "qodercli"
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

#[cfg(test)]
mod tests {
    use super::*;
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
            .build()
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_writes_agents_md_block() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use Qoder."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(body.contains("Use Qoder."));
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "x")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn install_mcp_writes_local_dot_mcp_json() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&dir.path().join(".mcp.json"));
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_mcp_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn plan_install_does_not_write() {
        let dir = tempdir().unwrap();
        let agent = QoderCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let _plan = agent
            .plan_install(&scope, &rules_spec("alpha", "rules"))
            .unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
    }
}
