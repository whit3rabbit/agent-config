//! Codex MCP surface. MCP servers live in `<codex-home>/config.toml` (Global)
//! or `<root>/.codex/config.toml` (Local) as `[mcp_servers.<name>]` tables.
//! Uses `toml_edit` to preserve user comments and ordering.

use std::path::PathBuf;

use toml_edit::{value, Array, InlineTable, Table};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, McpSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, PlannedChange, RefusalReason, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, McpTransport};
use crate::status::StatusReport;
use crate::util::{file_lock, ownership, planning, safe_fs, toml_patch};

use super::CodexAgent;

impl CodexAgent {
    /// `<codex-home>/config.toml` (Global) or `<root>/.codex/config.toml`
    /// (Local). MCP servers live here as `[mcp_servers.<name>]` tables.
    pub(super) fn config_toml_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("config.toml"),
            Scope::Local(p) => p.join(".codex").join("config.toml"),
        })
    }
}

impl McpSurface for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
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
        let cfg = Self::config_toml_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence = toml_patch::config_presence(&cfg, &["mcp_servers"], name)?;
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
        let cfg = Self::config_toml_path(scope)?;
        if let Some(plan) = agent_planning::mcp_local_inline_secret_refusal(
            target.clone(),
            scope,
            spec,
            Some(cfg.clone()),
        ) {
            return Ok(plan);
        }
        let ledger = ownership::mcp_ledger_for(&cfg);
        let mut changes = Vec::new();
        let mut doc = match toml_patch::read_or_empty(&cfg) {
            Ok(doc) => doc,
            Err(AgentConfigError::TomlInvalid { .. }) => {
                changes.push(PlannedChange::Refuse {
                    path: Some(cfg),
                    reason: RefusalReason::InvalidConfig,
                });
                return Ok(InstallPlan::from_changes(target, changes));
            }
            Err(e) => return Err(e),
        };
        let in_config = toml_patch::contains_named_table(&doc, &["mcp_servers"], &spec.name);
        let prior_owner = ownership::owner_of(&ledger, &spec.name)?;
        let adopting = spec.adopt_unowned && in_config && prior_owner.is_none();
        match (prior_owner.as_deref(), in_config) {
            (Some(owner), _) if owner != spec.owner_tag => {
                changes.push(PlannedChange::Refuse {
                    path: Some(ledger),
                    reason: RefusalReason::OwnerMismatch,
                });
                return Ok(InstallPlan::from_changes(target, changes));
            }
            (None, true) if !spec.adopt_unowned => {
                changes.push(PlannedChange::Refuse {
                    path: Some(cfg),
                    reason: RefusalReason::UserInstalledEntry,
                });
                return Ok(InstallPlan::from_changes(target, changes));
            }
            _ => {}
        }

        let table = build_mcp_table(spec);
        let changed =
            toml_patch::upsert_named_table(&mut doc, &["mcp_servers"], &spec.name, table)?;
        let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());
        if changed {
            let bytes = toml_patch::to_string(&doc);
            planning::plan_write_file(&mut changes, &cfg, &bytes, true)?;
        }
        if !has_refusal(&changes) && (changed || owner_changed || adopting) {
            planning::plan_write_ledger(&mut changes, &ledger, &spec.name, &spec.owner_tag);
        }
        if changes.is_empty() {
            changes.push(PlannedChange::NoOp {
                path: cfg.clone(),
                reason: "MCP server is already up to date".into(),
            });
        }
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
        let cfg = Self::config_toml_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let mut changes = Vec::new();
        let mut doc = match toml_patch::read_or_empty(&cfg) {
            Ok(doc) => doc,
            Err(AgentConfigError::TomlInvalid { .. }) => {
                changes.push(PlannedChange::Refuse {
                    path: Some(cfg),
                    reason: RefusalReason::InvalidConfig,
                });
                return Ok(UninstallPlan::from_changes(target, changes));
            }
            Err(e) => return Err(e),
        };
        let in_config = toml_patch::contains_named_table(&doc, &["mcp_servers"], name);
        let actual_owner = ownership::owner_of(&ledger, name)?;
        if !in_config && actual_owner.is_none() {
            changes.push(PlannedChange::NoOp {
                path: cfg,
                reason: "mcp server is already absent".into(),
            });
            return Ok(UninstallPlan::from_changes(target, changes));
        }
        match (actual_owner.as_deref(), in_config) {
            (Some(owner), _) if owner != owner_tag => {
                changes.push(PlannedChange::Refuse {
                    path: Some(ledger),
                    reason: RefusalReason::OwnerMismatch,
                });
                return Ok(UninstallPlan::from_changes(target, changes));
            }
            (None, true) => {
                changes.push(PlannedChange::Refuse {
                    path: Some(cfg),
                    reason: RefusalReason::UserInstalledEntry,
                });
                return Ok(UninstallPlan::from_changes(target, changes));
            }
            _ => {}
        }

        if in_config {
            let removed = toml_patch::remove_named_table(&mut doc, &["mcp_servers"], name)?;
            debug_assert!(removed);
            if doc.as_table().is_empty() {
                let bytes = toml_patch::to_string(&doc);
                planning::plan_restore_backup_or_remove(&mut changes, &cfg, &bytes)?;
            } else {
                let bytes = toml_patch::to_string(&doc);
                planning::plan_write_file(&mut changes, &cfg, &bytes, false)?;
            }
        }
        if actual_owner.is_some() {
            planning::plan_remove_ledger_entry(&mut changes, &ledger, name);
        }
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let mut report = InstallReport::default();
        let cfg = Self::config_toml_path(scope)?;
        spec.validate_local_secret_policy(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);

        file_lock::with_lock(&cfg, || {
            let mut doc = toml_patch::read_or_empty(&cfg)?;
            let in_config = toml_patch::contains_named_table(&doc, &["mcp_servers"], &spec.name);
            let prior_owner = ownership::owner_of(&ledger, &spec.name)?;
            let adopting = spec.adopt_unowned && in_config && prior_owner.is_none();
            ownership::require_owner_with_policy(
                &ledger,
                &spec.name,
                &spec.owner_tag,
                "mcp server",
                in_config,
                spec.adopt_unowned,
            )?;

            let table = build_mcp_table(spec);
            let changed =
                toml_patch::upsert_named_table(&mut doc, &["mcp_servers"], &spec.name, table)?;

            let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());

            let written_bytes: Option<Vec<u8>> = if changed {
                let bytes = toml_patch::to_string(&doc);
                let outcome = safe_fs::write(scope, &cfg, &bytes, true)?;
                if outcome.existed {
                    report.patched.push(outcome.path.clone());
                } else {
                    report.created.push(outcome.path.clone());
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                Some(bytes)
            } else {
                None
            };

            if changed || owner_changed || adopting {
                let hash = match written_bytes.as_deref() {
                    Some(b) => Some(ownership::content_hash(b)),
                    None => ownership::file_content_hash(&cfg)?,
                };
                ownership::record_install(&ledger, &spec.name, &spec.owner_tag, hash.as_deref())?;
            }
            if !changed && !owner_changed && !adopting {
                report.already_installed = true;
            }
            Ok::<(), AgentConfigError>(())
        })?;
        Ok(report)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let mut report = UninstallReport::default();

        let cfg = Self::config_toml_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);

        if !cfg.exists() && !ledger.exists() {
            report.not_installed = true;
            return Ok(report);
        }

        file_lock::with_lock(&cfg, || {
            let mut doc = toml_patch::read_or_empty(&cfg)?;
            let in_config = toml_patch::contains_named_table(&doc, &["mcp_servers"], name);
            let in_ledger = ownership::contains(&ledger, name)?;

            if !in_config && !in_ledger {
                report.not_installed = true;
                return Ok(());
            }

            ownership::require_owner(&ledger, name, owner_tag, "mcp server", in_config)?;

            if in_config {
                let removed = toml_patch::remove_named_table(&mut doc, &["mcp_servers"], name)?;
                debug_assert!(removed);

                let now_empty = doc.as_table().is_empty();
                let bytes = toml_patch::to_string(&doc);
                if now_empty && safe_fs::restore_backup_if_matches(scope, &cfg, &bytes)? {
                    report.restored.push(cfg.clone());
                } else if now_empty {
                    safe_fs::remove_file(scope, &cfg)?;
                    report.removed.push(cfg.clone());
                } else {
                    safe_fs::write(scope, &cfg, &bytes, false)?;
                    report.patched.push(cfg.clone());
                }
            }

            ownership::record_uninstall(&ledger, name)?;
            Ok::<(), AgentConfigError>(())
        })?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

/// Translate an [`McpSpec`] into a TOML `[mcp_servers.<name>]` sub-table.
fn build_mcp_table(spec: &McpSpec) -> Table {
    let mut t = Table::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            t["command"] = value(command.clone());
            let mut arr = Array::new();
            for a in args {
                arr.push(a.clone());
            }
            t["args"] = value(arr);
            if !env.is_empty() {
                let mut env_t = InlineTable::new();
                for (k, v) in env {
                    env_t.insert(k, v.clone().into());
                }
                t["env"] = value(env_t);
            }
        }
        McpTransport::Http { url, headers } => {
            t["type"] = value("http");
            t["url"] = value(url.clone());
            if !headers.is_empty() {
                let mut h = InlineTable::new();
                for (k, v) in headers {
                    h.insert(k, v.clone().into());
                }
                t["headers"] = value(h);
            }
        }
        McpTransport::Sse { url, headers } => {
            t["type"] = value("sse");
            t["url"] = value(url.clone());
            if !headers.is_empty() {
                let mut h = InlineTable::new();
                for (k, v) in headers {
                    h.insert(k, v.clone().into());
                }
                t["headers"] = value(h);
            }
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::Integration;
    use tempfile::tempdir;

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    fn read_toml(p: &std::path::Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn install_mcp_writes_named_table_in_config_toml() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        assert!(cfg.exists());
        let s = read_toml(&cfg);
        assert!(s.contains("[mcp_servers.github]"), "got:\n{s}");
        assert!(s.contains(r#"command = "npx""#), "got:\n{s}");
        assert!(s.contains(r#"FOO = "bar""#), "got:\n{s}");
    }

    #[test]
    fn install_mcp_preserves_user_comments_and_other_sections() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original =
            "# Codex configuration\n# Hand-authored.\n\n[some.section]\nkey = \"value\"\n";
        std::fs::write(&cfg, original).unwrap();

        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();

        let s = read_toml(&cfg);
        assert!(
            s.contains("# Codex configuration"),
            "comment lost. got:\n{s}"
        );
        assert!(s.contains("[some.section]"), "user section lost");
        assert!(s.contains("[mcp_servers.github]"));
        // .bak made when we modified an existing file.
        assert!(dir.path().join(".codex/config.toml.bak").exists());
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "appA"))
            .unwrap();
        let err = agent
            .install_mcp(&scope, &local_mcp_spec("github", "appB"))
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_mcp_refuses_hand_installed_same_name() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "[mcp_servers.github]\ncommand = \"user-cmd\"\n").unwrap();

        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let err = agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::NotOwnedByCaller { actual: None, .. }
        ));
        let s = read_toml(&cfg);
        assert!(s.contains("user-cmd"));
    }

    #[test]
    fn install_mcp_does_not_collide_with_hook_install() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let hook_spec = HookSpec::builder("alpha")
            .command_program("myapp", ["hook"])
            .build();
        agent.install(&scope, &hook_spec).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        // Hooks use a separate file; both must exist.
        assert!(dir.path().join(".codex/hooks.json").exists());
        assert!(dir.path().join(".codex/config.toml").exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
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
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        // Empty doc: the file is removed entirely.
        assert!(!cfg.exists());
    }

    #[test]
    fn uninstall_mcp_keeps_user_sections() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original = "[other]\nfoo = \"bar\"\n";
        std::fs::write(&cfg, original).unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        let s = read_toml(&cfg);
        assert!(s.contains("[other]"), "got:\n{s}");
        assert!(!s.contains("[mcp_servers"), "mcp_servers should be pruned");
    }
}
