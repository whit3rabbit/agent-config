//! Legacy MCP installer for harnesses that store servers as a JSON array.
//!
//! No registered agent currently uses this shape, but the helper is retained
//! for future array-backed configs where entries carry a `name` field.
//!
//! Mirrors [`super::mcp_json_object`] but keys entries by the `name` field
//! within array members instead of by object key.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::spec::{McpSpec, McpTransport};
use crate::util::{file_lock, fs_atomic, json_patch, ownership};

/// Default array key for array-backed MCP configs.
pub(crate) const ARRAY_KEY: &str = "mcp";

/// Field within each array entry that uniquely identifies the server.
pub(crate) const NAME_FIELD: &str = "name";

/// Returns true if `name` is present in the ledger.
pub(crate) fn is_installed(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(ledger_path, name)
}

/// Install or update the server. The array entry includes the `name` field
/// so the harness can load it.
pub(crate) fn install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
) -> Result<InstallReport, HookerError> {
    file_lock::with_lock(config_path, || {
        let mut report = InstallReport::default();
        let mut root = json_patch::read_or_empty(config_path)?;
        let in_config =
            json_patch::contains_in_named_array(&root, &[ARRAY_KEY], NAME_FIELD, &spec.name);
        ownership::require_owner(
            ledger_path,
            &spec.name,
            &spec.owner_tag,
            "mcp server",
            in_config,
        )?;

        let value = build_array_entry(spec);
        let changed = json_patch::upsert_named_array_entry(
            &mut root,
            &[ARRAY_KEY],
            NAME_FIELD,
            &spec.name,
            value,
        )?;

        let prior_owner = ownership::owner_of(ledger_path, &spec.name)?;
        let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());

        if changed {
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

/// Uninstall the server. Refuses on owner mismatch / hand-installed entries.
pub(crate) fn uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
) -> Result<UninstallReport, HookerError> {
    let root = json_patch::read_or_empty(config_path)?;
    let in_config = json_patch::contains_in_named_array(&root, &[ARRAY_KEY], NAME_FIELD, name);
    let in_ledger = ownership::contains(ledger_path, name)?;
    if !in_config && !in_ledger {
        return Ok(UninstallReport {
            not_installed: true,
            ..UninstallReport::default()
        });
    }

    file_lock::with_lock(config_path, || {
        let mut report = UninstallReport::default();
        let mut root = json_patch::read_or_empty(config_path)?;

        let in_config = json_patch::contains_in_named_array(&root, &[ARRAY_KEY], NAME_FIELD, name);
        let in_ledger = ownership::contains(ledger_path, name)?;

        if !in_config && !in_ledger {
            report.not_installed = true;
            return Ok(report);
        }

        ownership::require_owner(ledger_path, name, owner_tag, kind, in_config)?;

        if in_config {
            let removed =
                json_patch::remove_named_array_entry(&mut root, &[ARRAY_KEY], NAME_FIELD, name)?;
            debug_assert!(removed);

            let now_empty = root.as_object().map(Map::is_empty).unwrap_or(true);
            if now_empty && fs_atomic::restore_backup(config_path)? {
                report.restored.push(config_path.to_path_buf());
            } else if now_empty {
                fs_atomic::remove_if_exists(config_path)?;
                report.removed.push(config_path.to_path_buf());
            } else {
                let bytes = json_patch::to_pretty(&root);
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

fn build_array_entry(spec: &McpSpec) -> Value {
    let mut obj = Map::new();
    obj.insert(NAME_FIELD.into(), Value::String(spec.name.clone()));
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            obj.insert("type".into(), Value::String("local".into()));
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            if !env.is_empty() {
                obj.insert("env".into(), env_value(env));
            }
        }
        McpTransport::Http { url, headers } => {
            obj.insert("type".into(), Value::String("remote".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                obj.insert("headers".into(), env_value(headers));
            }
        }
        McpTransport::Sse { url, headers } => {
            obj.insert("type".into(), Value::String("sse".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                obj.insert("headers".into(), env_value(headers));
            }
        }
    }
    Value::Object(obj)
}

fn env_value(map: &BTreeMap<String, String>) -> Value {
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
        (dir.join("config.json"), dir.join(".ai-hooker-mcp.json"))
    }

    fn stdio_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    #[test]
    fn install_adds_array_entry_with_name() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        let arr = v["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], json!("github"));
        assert_eq!(arr[0]["command"], json!("npx"));
        assert_eq!(arr[0]["type"], json!("local"));
    }

    #[test]
    fn install_idempotent_with_same_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        let s = stdio_spec("github", "myapp");
        install(&cfg, &led, &s).unwrap();
        let r = install(&cfg, &led, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_coexists_with_user_servers() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcp": [ { "name": "user", "command": "user-cmd" } ] }"#,
        )
        .unwrap();
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        let arr = v["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(dir.path().join("config.json.bak").exists());
    }

    #[test]
    fn install_refuses_other_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "appA")).unwrap();
        let err = install(&cfg, &led, &stdio_spec("github", "appB")).unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_refuses_user_installed_same_name() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcp": [ { "name": "github", "command": "user-cmd" } ] }"#,
        )
        .unwrap();
        let err = install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn uninstall_refuses_other_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "appA")).unwrap();
        let err = uninstall(&cfg, &led, "github", "appB", "mcp server").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_refuses_user_installed() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcp": [ { "name": "user", "command": "user-cmd" } ] }"#,
        )
        .unwrap();
        let err = uninstall(&cfg, &led, "user", "myapp", "mcp server").unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn uninstall_keeps_siblings() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("alpha", "myapp")).unwrap();
        install(&cfg, &led, &stdio_spec("beta", "myapp")).unwrap();
        uninstall(&cfg, &led, "alpha", "myapp", "mcp server").unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        let arr = v["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], json!("beta"));
    }
}
