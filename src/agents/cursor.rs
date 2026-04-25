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
//!       { "command": "...", "matcher": "Shell", "_ai_hooker_tag": "myapp" }
//!     ]
//!   }
//! }
//! ```

use std::path::PathBuf;

use serde_json::{json, Value};

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::paths;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec};
use crate::util::{fs_atomic, json_patch, mcp_json_object, ownership};

/// Cursor (the AI editor and CLI).
pub struct CursorAgent;

impl CursorAgent {
    /// Construct an instance. The struct is stateless.
    pub const fn new() -> Self {
        Self
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::cursor_home()?.join("hooks.json"),
            Scope::Local(p) => p.join(".cursor").join("hooks.json"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::cursor_mcp_user_file()?,
            Scope::Local(p) => p.join(".cursor").join("mcp.json"),
        })
    }
}

impl Default for CursorAgent {
    fn default() -> Self {
        Self::new()
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

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let p = Self::hooks_path(scope)?;
        let root = json_patch::read_or_empty(&p)?;
        Ok(json_patch::contains_tagged(
            &root,
            &["hooks", "preToolUse"],
            tag,
        ))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_path(scope)?;
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
            "command": spec.command,
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
            let outcome = fs_atomic::write_atomic(&p, &bytes, true)?;
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

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::hooks_path(scope)?;
        if !p.exists() {
            report.not_installed = true;
            return Ok(report);
        }

        let mut root = json_patch::read_or_empty(&p)?;
        let mut changed = false;
        for event_key in ["preToolUse", "postToolUse"] {
            if json_patch::remove_tagged_array_entry(
                &mut root,
                &["hooks", event_key],
                tag,
            )? {
                changed = true;
            }
        }

        if !changed {
            report.not_installed = true;
            return Ok(report);
        }

        if is_effectively_empty(&root) {
            if fs_atomic::restore_backup(&p)? {
                report.restored.push(p.clone());
            } else {
                fs_atomic::remove_if_exists(&p)?;
                report.removed.push(p.clone());
            }
        } else {
            let bytes = json_patch::to_pretty(&root);
            fs_atomic::write_atomic(&p, &bytes, false)?;
            report.patched.push(p.clone());
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

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&Self::mcp_path(scope)?);
        mcp_json_object::is_installed(&ledger, name)
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::install(&cfg, &ledger, spec)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
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
            .command("myapp hook")
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
            v["hooks"]["preToolUse"][0]["_ai_hooker_tag"],
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
        assert!(!p.exists(), "we authored the file; should be removed on uninstall");
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
            .command("noop")
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
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
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
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
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
        agent.install_mcp(&scope, &local_mcp_spec("github", "appA")).unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_mcp_round_trip() {
        let dir = tempdir().unwrap();
        let agent = CursorAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        assert!(!dir.path().join(".cursor/mcp.json").exists());
    }
}
