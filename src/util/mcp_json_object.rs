//! Shared MCP installer for harnesses that key servers by name in a JSON
//! object (Claude, Cursor, Gemini, Cline, Roo, Windsurf, Antigravity).
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

use crate::error::HookerError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::PlannedChange;
use crate::spec::McpSpec;
use crate::status::ConfigPresence;
use crate::util::{mcp_json_map, ownership};

/// Top-level key under which named server entries live for this helper.
const SERVERS_KEY: &str = "mcpServers";
const SERVERS_PATH: &[&str] = &[SERVERS_KEY];

/// Returns true if `name` exists in the ledger sidecar (single source of truth
/// for "is this currently installed by some consumer").
#[allow(dead_code)]
pub(crate) fn is_installed(ledger_path: &Path, name: &str) -> Result<bool, HookerError> {
    ownership::contains(ledger_path, name)
}

/// Probe whether `name` is present in the standard `mcpServers` object.
pub(crate) fn config_presence(
    config_path: &Path,
    name: &str,
) -> Result<ConfigPresence, HookerError> {
    mcp_json_map::config_presence(
        config_path,
        SERVERS_PATH,
        name,
        mcp_json_map::ConfigFormat::Json,
    )
}

/// Install or update an MCP server in the harness config. Records ownership
/// in the sidecar ledger.
pub(crate) fn install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
) -> Result<InstallReport, HookerError> {
    mcp_json_map::install(
        config_path,
        ledger_path,
        spec,
        SERVERS_PATH,
        mcp_json_map::mcp_servers_value,
        mcp_json_map::ConfigFormat::Json,
    )
}

/// Plan installing or updating an MCP server in the harness config.
pub(crate) fn plan_install(
    config_path: &Path,
    ledger_path: &Path,
    spec: &McpSpec,
) -> Result<Vec<PlannedChange>, HookerError> {
    mcp_json_map::plan_install(
        config_path,
        ledger_path,
        spec,
        SERVERS_PATH,
        mcp_json_map::mcp_servers_value,
        mcp_json_map::ConfigFormat::Json,
    )
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
    mcp_json_map::uninstall(
        config_path,
        ledger_path,
        name,
        owner_tag,
        kind,
        SERVERS_PATH,
        mcp_json_map::ConfigFormat::Json,
    )
}

/// Plan uninstalling the server identified by `name`.
pub(crate) fn plan_uninstall(
    config_path: &Path,
    ledger_path: &Path,
    name: &str,
    owner_tag: &str,
    kind: &'static str,
) -> Result<Vec<PlannedChange>, HookerError> {
    mcp_json_map::plan_uninstall(
        config_path,
        ledger_path,
        name,
        owner_tag,
        kind,
        SERVERS_PATH,
        mcp_json_map::ConfigFormat::Json,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::McpTransport;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::tempdir;

    fn run_two<A, B, FA, FB>(a: FA, b: FB) -> (A, B)
    where
        A: Send + 'static,
        B: Send + 'static,
        FA: FnOnce() -> A + Send + 'static,
        FB: FnOnce() -> B + Send + 'static,
    {
        let barrier = Arc::new(Barrier::new(3));
        let a_barrier = Arc::clone(&barrier);
        let b_barrier = Arc::clone(&barrier);
        let a_thread = thread::spawn(move || {
            a_barrier.wait();
            a()
        });
        let b_thread = thread::spawn(move || {
            b_barrier.wait();
            b()
        });
        barrier.wait();
        (
            a_thread.join().expect("first MCP writer panicked"),
            b_thread.join().expect("second MCP writer panicked"),
        )
    }

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
    fn install_refuses_other_owner() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        install(&cfg, &led, &stdio_spec("github", "appA")).unwrap();
        let err = install(&cfg, &led, &stdio_spec("github", "appB")).unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
        assert_eq!(
            ownership::owner_of(&led, "github").unwrap().as_deref(),
            Some("appA")
        );
    }

    #[test]
    fn install_refuses_user_installed_same_name() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcpServers": { "github": { "command": "user-cmd" } } }"#,
        )
        .unwrap();
        let err = install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap_err();
        assert!(matches!(
            err,
            HookerError::NotOwnedByCaller { actual: None, .. }
        ));
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["github"]["command"], json!("user-cmd"));
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
    fn uninstall_final_entry_does_not_restore_stale_backup() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        std::fs::write(
            &cfg,
            r#"{ "mcpServers": { "user-thing": { "command": "user-cmd" } } }"#,
        )
        .unwrap();

        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();

        let mut current: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        current["mcpServers"]
            .as_object_mut()
            .unwrap()
            .remove("user-thing");
        std::fs::write(&cfg, crate::util::json_patch::to_pretty(&current)).unwrap();

        let report = uninstall(&cfg, &led, "github", "myapp", "mcp server").unwrap();
        assert!(report.removed.contains(&cfg));
        assert!(!cfg.exists(), "stale backup should not be restored");
        assert!(
            dir.path().join("mcp.json.bak").exists(),
            "stale backup is left for manual recovery"
        );
    }

    #[test]
    fn is_installed_uses_ledger() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        assert!(!is_installed(&led, "github").unwrap());
        install(&cfg, &led, &stdio_spec("github", "myapp")).unwrap();
        assert!(is_installed(&led, "github").unwrap());
    }

    #[test]
    fn concurrent_install_different_names_keeps_both_config_and_ledger_entries() {
        let dir = tempdir().unwrap();
        let (cfg, led) = paths(dir.path());
        let cfg_a = cfg.clone();
        let led_a = led.clone();
        let cfg_b = cfg.clone();
        let led_b = led.clone();
        let spec_a = stdio_spec("alpha", "appA");
        let spec_b = stdio_spec("beta", "appB");

        let (ra, rb) = run_two(
            move || install(&cfg_a, &led_a, &spec_a),
            move || install(&cfg_b, &led_b, &spec_b),
        );

        ra.unwrap();
        rb.unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&cfg).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["alpha"]["command"], json!("npx"));
        assert_eq!(v["mcpServers"]["beta"]["command"], json!("npx"));
        assert_eq!(
            ownership::owner_of(&led, "alpha").unwrap().as_deref(),
            Some("appA")
        );
        assert_eq!(
            ownership::owner_of(&led, "beta").unwrap().as_deref(),
            Some("appB")
        );
    }
}
