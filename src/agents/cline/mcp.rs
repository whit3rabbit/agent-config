//! Cline MCP surface. Global VS Code extension config at
//! `Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`,
//! keyed by server name under `mcpServers`.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, McpSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec};
use crate::status::StatusReport;
use crate::util::{fs_atomic, mcp_json_object, ownership, safe_fs};

use super::ClineAgent;

impl ClineAgent {
    pub(super) fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        match scope {
            Scope::Global => paths::cline_mcp_global_file(),
            Scope::Local(_) => Err(AgentConfigError::UnsupportedScope {
                id: "cline",
                scope: ScopeKind::Local,
            }),
        }
    }
}

impl McpSurface for ClineAgent {
    fn id(&self) -> &'static str {
        "cline"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global]
    }

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::mcp_path(scope)?;
        let presence = mcp_json_object::config_presence(&cfg, name)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let recorded = ownership::owner_of(&ledger, name)?;

        let primary_status = StatusReport::for_mcp(
            name,
            cfg.clone(),
            ledger,
            presence,
            expected_owner,
            recorded,
        );

        if matches!(
            primary_status.status,
            crate::status::InstallStatus::InstalledOwned { .. }
                | crate::status::InstallStatus::InstalledOtherOwner { .. }
        ) {
            Ok(primary_status)
        } else {
            if let Ok(legacy_cfg) = paths::legacy_cline_mcp_global_file() {
                if legacy_cfg.exists() {
                    let legacy_presence = mcp_json_object::config_presence(&legacy_cfg, name)?;
                    let legacy_ledger = ownership::mcp_ledger_for(&legacy_cfg);
                    let legacy_recorded = ownership::owner_of(&legacy_ledger, name)?;
                    let legacy_status = StatusReport::for_mcp(
                        name,
                        legacy_cfg,
                        legacy_ledger,
                        legacy_presence,
                        expected_owner,
                        legacy_recorded,
                    );
                    if matches!(
                        legacy_status.status,
                        crate::status::InstallStatus::InstalledOwned { .. }
                            | crate::status::InstallStatus::InstalledOtherOwner { .. }
                    ) {
                        return Ok(legacy_status);
                    }
                }
            }
            Ok(primary_status)
        }
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
        let mut plan = agent_planning::mcp_json_object_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
        )?;
        if let Ok(legacy_cfg) = paths::legacy_cline_mcp_global_file() {
            if legacy_cfg.exists() {
                let legacy_plan = agent_planning::mcp_json_object_uninstall(
                    McpSurface::id(self),
                    scope,
                    name,
                    owner_tag,
                    Ok(legacy_cfg),
                )?;
                plan.changes.extend(legacy_plan.changes);
            }
        }
        Ok(plan)
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

        // Support legacy global settings migration
        if !cfg.exists() {
            if let Ok(legacy_cfg) = paths::legacy_cline_mcp_global_file() {
                if legacy_cfg.exists() {
                    let content = fs_atomic::read_capped_or_empty(&legacy_cfg)?;
                    safe_fs::write(scope, &cfg, &content, true)?;
                    let legacy_ledger = ownership::mcp_ledger_for(&legacy_cfg);
                    if legacy_ledger.exists() {
                        let new_ledger = ownership::mcp_ledger_for(&cfg);
                        let ledger_content = fs_atomic::read_capped_or_empty(&legacy_ledger)?;
                        safe_fs::write(scope, &new_ledger, &ledger_content, true)?;
                    }
                    let _ = safe_fs::remove_file(scope, &legacy_cfg);
                    let legacy_ledger = ownership::mcp_ledger_for(&legacy_cfg);
                    let _ = safe_fs::remove_file(scope, &legacy_ledger);
                }
            }
        }

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
        let mut report = mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")?;

        if let Ok(legacy_cfg) = paths::legacy_cline_mcp_global_file() {
            if legacy_cfg.exists() {
                scope.ensure_contained(&legacy_cfg)?;
                let legacy_ledger = ownership::mcp_ledger_for(&legacy_cfg);
                let legacy_report = mcp_json_object::uninstall(
                    &legacy_cfg,
                    &legacy_ledger,
                    name,
                    owner_tag,
                    "mcp server",
                )?;
                report.merge(legacy_report);
            }
        }
        Ok(report)
    }
}
