//! Shared MCP installer for YAML configs that key servers by name.
//!
//! Hermes stores outbound MCP servers under `mcp_servers` in
//! `~/.hermes/config.yaml`. This helper mirrors `mcp_json_map` but parses and
//! writes YAML while keeping the same sidecar ownership behavior.

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::{has_refusal, PlannedChange, RefusalReason};
use crate::spec::McpSpec;
use crate::status::ConfigPresence;
use crate::util::{file_lock, fs_atomic, json_patch, ownership, planning};

/// Builder for one YAML server entry under the chosen object key.
pub(crate) type ServerBuilder = fn(&McpSpec) -> Value;

/// Returns true if `name` exists in the MCP ownership ledger.
pub(crate) fn is_installed(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(ledger_path, name)
}

/// Probe whether `name` is present in the named YAML object at `config_path`.
/// Parse failures map to [`ConfigPresence::Invalid`] for drift reporting.
pub(crate) fn config_presence(
    config_path: &Path,
    servers_path: &[&str],
    name: &str,
) -> Result<ConfigPresence, HookerError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let root = match read_or_empty(config_path) {
        Ok(v) => v,
        Err(HookerError::Other(e)) => {
            return Ok(ConfigPresence::Invalid {
                reason: e.to_string(),
            });
        }
        Err(e) => return Err(e),
    };
    Ok(if json_patch::contains_named(&root, servers_path, name) {
        ConfigPresence::Single
    } else {
        ConfigPresence::Absent
    })
}

/// Install or update an MCP server in a named YAML object.
pub(crate) fn install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
    servers_path: &[&str],
    build_server: ServerBuilder,
) -> Result<InstallReport, HookerError> {
    file_lock::with_lock(config_path, || {
        let mut report = InstallReport::default();

        let mut root = read_or_empty(config_path)?;
        let in_config = json_patch::contains_named(&root, servers_path, &spec.name);
        ownership::require_owner(
            ledger_path,
            &spec.name,
            &spec.owner_tag,
            "mcp server",
            in_config,
        )?;

        let value = build_server(spec);
        let changed =
            json_patch::upsert_named_object_entry(&mut root, servers_path, &spec.name, value)?;

        let prior_owner = ownership::owner_of(ledger_path, &spec.name)?;
        let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());

        if changed {
            let bytes = to_yaml_bytes(&root)?;
            let outcome = fs_atomic::write_atomic(config_path, &bytes, true)?;
            if outcome.existed {
                report.patched.push(outcome.path.clone());
            } else {
                report.created.push(outcome.path.clone());
            }
            if let Some(b) = outcome.backup {
                report.backed_up.push(b);
            }
        }

        if changed || owner_changed {
            ownership::record_install(ledger_path, &spec.name, &spec.owner_tag)?;
        }

        if !changed && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    })
}

/// Plan installing or updating a named YAML MCP entry.
pub(crate) fn plan_install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
    servers_path: &[&str],
    build_server: ServerBuilder,
) -> Result<Vec<PlannedChange>, HookerError> {
    let mut changes = Vec::new();

    let mut root = match read_or_empty(config_path) {
        Ok(root) => root,
        Err(HookerError::Other(_)) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(changes);
        }
        Err(e) => return Err(e),
    };
    let in_config = json_patch::contains_named(&root, servers_path, &spec.name);
    let prior_owner = ownership::owner_of(ledger_path, &spec.name)?;

    match (prior_owner.as_deref(), in_config) {
        (Some(owner), _) if owner != spec.owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(ledger_path.to_path_buf()),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    let value = build_server(spec);
    let changed =
        json_patch::upsert_named_object_entry(&mut root, servers_path, &spec.name, value)?;
    let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());

    if changed {
        let bytes = to_yaml_bytes(&root)?;
        planning::plan_write_file(&mut changes, config_path, &bytes, true)?;
    }

    if !has_refusal(&changes) && (changed || owner_changed) {
        planning::plan_write_ledger(&mut changes, ledger_path, &spec.name, &spec.owner_tag);
    }

    if changes.is_empty() {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: "MCP server is already up to date".into(),
        });
    }

    Ok(changes)
}

/// Uninstall a YAML MCP entry. Refuses on owner mismatch or hand edits.
pub(crate) fn uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
    servers_path: &[&str],
) -> Result<UninstallReport, HookerError> {
    if !config_path.exists() && !ledger_path.exists() {
        return Ok(UninstallReport {
            not_installed: true,
            ..UninstallReport::default()
        });
    }

    file_lock::with_lock(config_path, || {
        let mut report = UninstallReport::default();

        let mut root = read_or_empty(config_path)?;
        let in_config = json_patch::contains_named(&root, servers_path, name);
        let in_ledger = ownership::contains(ledger_path, name)?;

        if !in_config && !in_ledger {
            report.not_installed = true;
            return Ok(report);
        }

        ownership::require_owner(ledger_path, name, owner_tag, kind, in_config)?;

        if in_config {
            let removed = json_patch::remove_named_object_entry(&mut root, servers_path, name)?;
            debug_assert!(removed);

            let now_empty = root.as_object().map(Map::is_empty).unwrap_or(true);
            if now_empty && fs_atomic::restore_backup(config_path)? {
                report.restored.push(config_path.to_path_buf());
            } else if now_empty {
                fs_atomic::remove_if_exists(config_path)?;
                report.removed.push(config_path.to_path_buf());
            } else {
                let bytes = to_yaml_bytes(&root)?;
                fs_atomic::write_atomic(config_path, &bytes, false)?;
                report.patched.push(config_path.to_path_buf());
            }
        }

        ownership::record_uninstall(ledger_path, name)?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    })
}

/// Plan uninstalling a YAML MCP entry.
pub(crate) fn plan_uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
    servers_path: &[&str],
) -> Result<Vec<PlannedChange>, HookerError> {
    let mut changes = Vec::new();

    let mut root = match read_or_empty(config_path) {
        Ok(root) => root,
        Err(HookerError::Other(_)) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(changes);
        }
        Err(e) => return Err(e),
    };
    let in_config = json_patch::contains_named(&root, servers_path, name);
    let actual_owner = ownership::owner_of(ledger_path, name)?;

    if !in_config && actual_owner.is_none() {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: format!("{kind} is already absent"),
        });
        return Ok(changes);
    }

    match (actual_owner.as_deref(), in_config) {
        (Some(owner), _) if owner != owner_tag => {
            changes.push(PlannedChange::Refuse {
                path: Some(ledger_path.to_path_buf()),
                reason: RefusalReason::OwnerMismatch,
            });
            return Ok(changes);
        }
        (None, true) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::UserInstalledEntry,
            });
            return Ok(changes);
        }
        _ => {}
    }

    if in_config {
        let removed = json_patch::remove_named_object_entry(&mut root, servers_path, name)?;
        debug_assert!(removed);

        let now_empty = root.as_object().map(Map::is_empty).unwrap_or(true);
        if now_empty {
            planning::plan_restore_backup_or_remove(&mut changes, config_path);
        } else {
            let bytes = to_yaml_bytes(&root)?;
            planning::plan_write_file(&mut changes, config_path, &bytes, false)?;
        }
    }

    if actual_owner.is_some() {
        planning::plan_remove_ledger_entry(&mut changes, ledger_path, name);
    }

    if changes.is_empty() {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: format!("{kind} is already absent"),
        });
    }

    Ok(changes)
}

fn read_or_empty(path: &Path) -> Result<Value, HookerError> {
    let text = fs_atomic::read_to_string_or_empty(path)?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    yaml_serde::from_str::<Value>(&text).map_err(|e| {
        HookerError::Other(anyhow::anyhow!("invalid YAML in {}: {}", path.display(), e))
    })
}

fn to_yaml_bytes(root: &Value) -> Result<Vec<u8>, HookerError> {
    let mut text = yaml_serde::to_string(root)
        .map_err(|e| HookerError::Other(anyhow::anyhow!("could not serialize YAML: {e}")))?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{McpSpec, McpTransport};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn stdio_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    fn value(spec: &McpSpec) -> Value {
        let mut obj = Map::new();
        if let McpTransport::Stdio { command, args, env } = &spec.transport {
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            obj.insert(
                "env".into(),
                Value::Object(
                    env.iter()
                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                        .collect(),
                ),
            );
        }
        Value::Object(obj)
    }

    #[test]
    fn install_preserves_unrelated_yaml_keys() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");
        let led = dir.path().join(".ai-hooker-mcp.json");
        std::fs::write(&cfg, "model: anthropic/claude\nother:\n  enabled: true\n").unwrap();

        install(
            &cfg,
            &led,
            &stdio_spec("github", "myapp"),
            &["mcp_servers"],
            value,
        )
        .unwrap();

        let parsed: Value = yaml_serde::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(parsed["model"], json!("anthropic/claude"));
        assert_eq!(parsed["other"]["enabled"], json!(true));
        assert_eq!(parsed["mcp_servers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn uninstall_removes_empty_config() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");
        let led = dir.path().join(".ai-hooker-mcp.json");
        let spec = stdio_spec("github", "myapp");

        install(&cfg, &led, &spec, &["mcp_servers"], value).unwrap();
        uninstall(
            &cfg,
            &led,
            "github",
            "myapp",
            "mcp server",
            &["mcp_servers"],
        )
        .unwrap();
        assert!(!cfg.exists());
        assert!(!led.exists());
    }
}
