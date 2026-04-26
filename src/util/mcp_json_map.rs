//! Shared MCP installer for harnesses that key servers by name inside a JSON
//! object.
//!
//! Different agents use different top-level keys and server-entry shapes:
//! `mcpServers` for Claude/Cursor/Gemini/Cline/Roo/Windsurf/Antigravity,
//! `servers` for VS Code Copilot, and object-based `mcp` for OpenCode/Kilo.
//! This module centralizes ownership, JSON/JSONC parsing, and uninstall
//! behavior so each agent only supplies its file path and serializer.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::{has_refusal, PlannedChange, RefusalReason};
use crate::spec::{McpSpec, McpTransport};
use crate::status::ConfigPresence;
use crate::util::{file_lock, fs_atomic, json5_patch, json_patch, ownership, planning};

/// The on-disk syntax to accept when reading the config.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConfigFormat {
    /// Strict JSON.
    Json,
    /// JSON with comments and trailing commas accepted.
    Jsonc,
    /// JSON5 with comments, trailing commas, unquoted keys, and single quotes.
    Json5,
}

/// Builder for one server entry under the chosen object key.
pub(crate) type ServerBuilder = fn(&McpSpec) -> Value;

/// Returns true if `name` exists in the MCP ownership ledger.
pub(crate) fn is_installed(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(ledger_path, name)
}

/// Probe whether `name` is present in the named-object MCP config at
/// `config_path`. Parse failures are converted to
/// [`ConfigPresence::Invalid`] so callers can surface them as drift instead
/// of propagating an error.
pub(crate) fn config_presence(
    config_path: &Path,
    servers_path: &[&str],
    name: &str,
    format: ConfigFormat,
) -> Result<ConfigPresence, HookerError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let root = match read_or_empty(config_path, format) {
        Ok(v) => v,
        Err(HookerError::JsonInvalid { source, .. }) => {
            return Ok(ConfigPresence::Invalid {
                reason: source.to_string(),
            });
        }
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

/// Install or update an MCP server in a named object.
pub(crate) fn install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
    servers_path: &[&str],
    build_server: ServerBuilder,
    format: ConfigFormat,
) -> Result<InstallReport, HookerError> {
    file_lock::with_lock(config_path, || {
        let mut report = InstallReport::default();

        let mut root = read_or_empty(config_path, format)?;
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

        let written_bytes: Option<Vec<u8>> = if changed {
            let bytes = json_patch::to_pretty(&root);
            let outcome = fs_atomic::write_atomic(config_path, &bytes, true)?;
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

        if changed || owner_changed {
            let hash = match written_bytes.as_deref() {
                Some(b) => Some(ownership::content_hash(b)),
                None => ownership::file_content_hash(config_path)?,
            };
            ownership::record_install(ledger_path, &spec.name, &spec.owner_tag, hash.as_deref())?;
        }

        if !changed && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    })
}

/// Plan installing or updating an MCP server in a named object.
pub(crate) fn plan_install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
    servers_path: &[&str],
    build_server: ServerBuilder,
    format: ConfigFormat,
) -> Result<Vec<PlannedChange>, HookerError> {
    let mut changes = Vec::new();

    let mut root = match read_or_empty(config_path, format) {
        Ok(root) => root,
        Err(HookerError::JsonInvalid { .. }) | Err(HookerError::Other(_)) => {
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
        let bytes = match format {
            ConfigFormat::Json | ConfigFormat::Jsonc | ConfigFormat::Json5 => {
                json_patch::to_pretty(&root)
            }
        };
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

/// Uninstall the server identified by `name`. Refuses on owner mismatch or
/// hand-installed entries.
pub(crate) fn uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
    servers_path: &[&str],
    format: ConfigFormat,
) -> Result<UninstallReport, HookerError> {
    if !config_path.exists() && !ledger_path.exists() {
        return Ok(UninstallReport {
            not_installed: true,
            ..UninstallReport::default()
        });
    }

    file_lock::with_lock(config_path, || {
        let mut report = UninstallReport::default();

        let mut root = read_or_empty(config_path, format)?;
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
            let bytes = json_patch::to_pretty(&root);
            if now_empty && fs_atomic::restore_backup_if_matches(config_path, &bytes)? {
                report.restored.push(config_path.to_path_buf());
            } else if now_empty {
                fs_atomic::remove_if_exists(config_path)?;
                report.removed.push(config_path.to_path_buf());
            } else {
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

/// Plan uninstalling an MCP server from a named object.
pub(crate) fn plan_uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
    servers_path: &[&str],
    format: ConfigFormat,
) -> Result<Vec<PlannedChange>, HookerError> {
    let mut changes = Vec::new();
    let mut root = match read_or_empty(config_path, format) {
        Ok(root) => root,
        Err(HookerError::JsonInvalid { .. }) | Err(HookerError::Other(_)) => {
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
            let bytes = match format {
                ConfigFormat::Json | ConfigFormat::Jsonc | ConfigFormat::Json5 => {
                    json_patch::to_pretty(&root)
                }
            };
            planning::plan_restore_backup_or_remove(&mut changes, config_path, &bytes)?;
        } else {
            let bytes = match format {
                ConfigFormat::Json | ConfigFormat::Jsonc | ConfigFormat::Json5 => {
                    json_patch::to_pretty(&root)
                }
            };
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

/// Standard `mcpServers.<name>` entry shape (Claude/Cursor/Gemini/etc.).
pub(crate) fn mcp_servers_value(spec: &McpSpec) -> Value {
    named_object_value(spec, false)
}

/// VS Code MCP `servers.<name>` entry shape — same as `mcp_servers_value`
/// but with an explicit `"type": "stdio"` discriminant on stdio entries.
#[allow(dead_code)]
pub(crate) fn vscode_servers_value(spec: &McpSpec) -> Value {
    named_object_value(spec, true)
}

fn named_object_value(spec: &McpSpec, include_stdio_type: bool) -> Value {
    let mut obj = Map::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            if include_stdio_type {
                obj.insert("type".into(), Value::String("stdio".into()));
            }
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            if !env.is_empty() {
                obj.insert("env".into(), string_map_value(env));
            }
        }
        McpTransport::Http { url, headers } => insert_remote(&mut obj, "http", url, headers),
        McpTransport::Sse { url, headers } => insert_remote(&mut obj, "sse", url, headers),
    }
    Value::Object(obj)
}

fn insert_remote(
    obj: &mut Map<String, Value>,
    type_tag: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
) {
    obj.insert("type".into(), Value::String(type_tag.into()));
    obj.insert("url".into(), Value::String(url.into()));
    if !headers.is_empty() {
        obj.insert("headers".into(), string_map_value(headers));
    }
}

/// OpenCode/Kilo object-based `mcp.<name>` entry shape.
pub(crate) fn command_array_value(spec: &McpSpec) -> Value {
    let mut obj = Map::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            obj.insert("type".into(), Value::String("local".into()));
            let command_array = std::iter::once(command.clone())
                .chain(args.iter().cloned())
                .map(Value::String)
                .collect();
            obj.insert("command".into(), Value::Array(command_array));
            if !env.is_empty() {
                obj.insert("environment".into(), string_map_value(env));
            }
        }
        McpTransport::Http { url, headers } | McpTransport::Sse { url, headers } => {
            insert_remote(&mut obj, "remote", url, headers);
        }
    }
    Value::Object(obj)
}

fn read_or_empty(path: &Path, format: ConfigFormat) -> Result<Value, HookerError> {
    match format {
        ConfigFormat::Json => json_patch::read_or_empty(path),
        ConfigFormat::Jsonc => read_jsonc_or_empty(path),
        ConfigFormat::Json5 => json5_patch::read_or_empty(path),
    }
}

fn read_jsonc_or_empty(path: &Path) -> Result<Value, HookerError> {
    let text = fs_atomic::read_to_string_or_empty(path)?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    jsonc_parser::parse_to_serde_value::<Value>(&text, &Default::default()).map_err(|e| {
        HookerError::Other(anyhow::anyhow!(
            "invalid JSONC in {}: {}",
            path.display(),
            e
        ))
    })
}

fn string_map_value(map: &BTreeMap<String, String>) -> Value {
    let mut obj = Map::new();
    for (k, v) in map {
        obj.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn paths(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        (dir.join("config.jsonc"), dir.join(".ai-hooker-mcp.json"))
    }

    fn stdio_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    #[test]
    fn install_mcp_servers_object() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(
            &cfg,
            &led,
            &stdio_spec("github", "myapp"),
            &["mcpServers"],
            mcp_servers_value,
            ConfigFormat::Json,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
        assert_eq!(v["mcpServers"]["github"]["env"]["FOO"], json!("bar"));
    }

    #[test]
    fn install_vscode_servers_object() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(
            &cfg,
            &led,
            &stdio_spec("memory", "myapp"),
            &["servers"],
            vscode_servers_value,
            ConfigFormat::Json,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["servers"]["memory"]["type"], json!("stdio"));
        assert_eq!(v["servers"]["memory"]["command"], json!("npx"));
    }

    #[test]
    fn install_command_array_object_from_jsonc_with_comments() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{
  // user comment
  "mcp": {
    "user": {
      "type": "local",
      "command": ["uvx", "user-server"],
    },
  },
}
"#,
        )
        .unwrap();
        install(
            &cfg,
            &led,
            &stdio_spec("github", "myapp"),
            &["mcp"],
            command_array_value,
            ConfigFormat::Jsonc,
        )
        .unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcp"]["github"]["type"], json!("local"));
        assert_eq!(
            v["mcp"]["github"]["command"],
            json!(["npx", "-y", "@example/server"])
        );
        assert_eq!(v["mcp"]["github"]["environment"]["FOO"], json!("bar"));
        assert_eq!(v["mcp"]["user"]["command"][0], json!("uvx"));
    }

    #[test]
    fn install_refuses_other_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(
            &cfg,
            &led,
            &stdio_spec("github", "app-a"),
            &["mcpServers"],
            mcp_servers_value,
            ConfigFormat::Json,
        )
        .unwrap();
        let err = install(
            &cfg,
            &led,
            &stdio_spec("github", "app-b"),
            &["mcpServers"],
            mcp_servers_value,
            ConfigFormat::Json,
        )
        .unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_refuses_hand_installed_same_name() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcpServers": { "github": { "command": "user-cmd" } } }"#,
        )
        .unwrap();
        let err = install(
            &cfg,
            &led,
            &stdio_spec("github", "myapp"),
            &["mcpServers"],
            mcp_servers_value,
            ConfigFormat::Json,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn uninstall_refuses_hand_installed_entry() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcp": { "user": { "type": "remote", "url": "x" } } }"#,
        )
        .unwrap();
        let err = uninstall(
            &cfg,
            &led,
            "user",
            "myapp",
            "mcp server",
            &["mcp"],
            ConfigFormat::Json,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }
}
