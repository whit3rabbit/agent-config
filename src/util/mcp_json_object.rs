//! Shared MCP installer for harnesses that key servers by name in a JSON
//! object (Claude, Cursor, Gemini, Windsurf).
//!
//! Each harness has its own config-file path, but the on-disk shape is
//! identical:
//!
//! ```json
//! { "mcpServers": { "<name>": { "command": "...", "args": [...], "env": {} } } }
//! ```
//!
//! For HTTP/SSE transports the shape is `{ "url": "...", "headers": {...} }`
//! (with no `command`/`args`).
//!
//! Ownership is tracked in a sidecar `.ai-hooker-mcp.json` next to the config
//! file so the harness config never carries unknown keys.

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::spec::{McpSpec, McpTransport};
use crate::util::{fs_atomic, json_patch, ownership};

/// Top-level key under which named server entries live. Every supported
/// harness uses `mcpServers`.
const SERVERS_KEY: &str = "mcpServers";
const SERVERS_PATH: &[&str] = &[SERVERS_KEY];

/// Returns true if `name` exists in the ledger sidecar (single source of truth
/// for "is this currently installed by some consumer").
pub(crate) fn is_installed(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(ledger_path, name)
}

/// Install or update an MCP server in the harness config. Records ownership
/// in the sidecar ledger.
pub(crate) fn install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
) -> Result<InstallReport, HookerError> {
    let mut report = InstallReport::default();

    let mut root = json_patch::read_or_empty(config_path)?;
    let value = build_server_value(spec);
    let changed =
        json_patch::upsert_named_object_entry(&mut root, SERVERS_PATH, &spec.name, value)?;

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
}

/// Uninstall the server identified by `name`. Refuses if the recorded owner
/// differs from `owner_tag`, or if `name` is present in the harness config but
/// missing from the ledger (i.e. user installed by hand).
pub(crate) fn uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
) -> Result<UninstallReport, HookerError> {
    let mut report = UninstallReport::default();

    let mut root = json_patch::read_or_empty(config_path)?;
    let in_config = json_patch::contains_named(&root, SERVERS_PATH, name);
    let in_ledger = ownership::contains(ledger_path, name)?;

    if !in_config && !in_ledger {
        report.not_installed = true;
        return Ok(report);
    }

    ownership::require_owner(ledger_path, name, owner_tag, kind, in_config)?;

    if in_config {
        let removed = json_patch::remove_named_object_entry(&mut root, SERVERS_PATH, name)?;
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
}

/// Translate an [`McpSpec`] into the harness's JSON server-entry shape.
fn build_server_value(spec: &McpSpec) -> Value {
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            let mut obj = Map::new();
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            if !env.is_empty() {
                let mut env_obj = Map::new();
                for (k, v) in env {
                    env_obj.insert(k.clone(), Value::String(v.clone()));
                }
                obj.insert("env".into(), Value::Object(env_obj));
            }
            Value::Object(obj)
        }
        McpTransport::Http { url, headers } => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::String("http".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                obj.insert("headers".into(), headers_value(headers));
            }
            Value::Object(obj)
        }
        McpTransport::Sse { url, headers } => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::String("sse".into()));
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                obj.insert("headers".into(), headers_value(headers));
            }
            Value::Object(obj)
        }
    }
}

fn headers_value(headers: &std::collections::BTreeMap<String, String>) -> Value {
    let mut h = Map::new();
    for (k, v) in headers {
        h.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn paths(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        (dir.join("mcp.json"), dir.join(".ai-hooker-mcp.json"))
    }

    fn stdio_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    fn http_spec(name: &str, owner: &str) -> McpSpec {
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".into(), "Bearer xyz".into());
        McpSpec {
            name: name.into(),
            owner_tag: owner.into(),
            transport: McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers,
            },
            friendly_name: None,
        }
    }

    #[test]
    fn install_creates_config_and_ledger() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        let report = install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        assert!(report.created.contains(&cfg));
        assert!(led.exists(), "ledger created");
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
        assert_eq!(v["mcpServers"]["github"]["env"]["FOO"], json!("bar"));
    }

    #[test]
    fn install_idempotent_with_same_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        let spec = stdio_spec("github", "myapp");
        install(&cfg, &led, &spec).unwrap();
        let r2 = install(&cfg, &led, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_replaces_owner_on_change() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "appA")).unwrap();
        let r2 = install(&cfg, &led, &stdio_spec("github", "appB")).unwrap();
        // owner changed, content same: server payload didn't change but ledger
        // was updated, so we report it as not-already-installed.
        assert!(!r2.already_installed);
        assert_eq!(
            ownership::owner_of(&led, "github").unwrap().as_deref(),
            Some("appB")
        );
    }

    #[test]
    fn install_http_writes_url_and_headers() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &http_spec("remote", "myapp")).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["remote"]["url"],
            json!("https://example.com/mcp")
        );
        assert_eq!(v["mcpServers"]["remote"]["type"], json!("http"));
        assert_eq!(
            v["mcpServers"]["remote"]["headers"]["Authorization"],
            json!("Bearer xyz")
        );
    }

    #[test]
    fn install_coexists_with_existing_user_servers() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcpServers": { "user-thing": { "command": "user-cmd" } } }"#,
        )
        .unwrap();
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["user-thing"]["command"], json!("user-cmd"));
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
        // backup made for the modified pre-existing file
        assert!(dir.path().join("mcp.json.bak").exists());
    }

    #[test]
    fn uninstall_removes_owned_entry_only() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        let r = uninstall(&cfg, &led, "github", "myapp", "mcp server").unwrap();
        assert!(r.removed.contains(&cfg) || r.restored.contains(&cfg));
        assert!(!led.exists(), "empty ledger removed");
        assert!(ownership::owner_of(&led, "github").unwrap().is_none());
    }

    #[test]
    fn uninstall_refuses_other_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "appA")).unwrap();
        let err = uninstall(&cfg, &led, "github", "appB", "mcp server").unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller {
                actual: Some(_),
                ..
            }
        ));
        // Config and ledger untouched.
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert!(v["mcpServers"]["github"].is_object());
        assert!(led.exists());
    }

    #[test]
    fn uninstall_refuses_user_installed_entry() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcpServers": { "user-thing": { "command": "user-cmd" } } }"#,
        )
        .unwrap();
        let err = uninstall(&cfg, &led, "user-thing", "myapp", "mcp server").unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
    }

    #[test]
    fn uninstall_unknown_entry_is_noop() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        let r = uninstall(&cfg, &led, "ghost", "myapp", "mcp server").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn uninstall_keeps_other_servers_intact() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("alpha", "myapp")).unwrap();
        install(&cfg, &led, &stdio_spec("beta", "myapp")).unwrap();
        uninstall(&cfg, &led, "alpha", "myapp", "mcp server").unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert!(v["mcpServers"]["beta"].is_object());
        assert!(v["mcpServers"]["alpha"].is_null());
    }

    #[test]
    fn is_installed_uses_ledger() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        assert!(!is_installed(&led, "github").unwrap());
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        assert!(is_installed(&led, "github").unwrap());
    }
}
