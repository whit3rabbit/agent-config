//! OpenCode integration (sst/opencode).
//!
//! OpenCode loads plugins from `~/.config/opencode/plugins/*.{ts,js}` (Global)
//! or `<project>/.opencode/plugins/*.{ts,js}` (Local). We write a single TS
//! file per consumer (`<tag>.ts`) whose body is supplied by the caller via
//! [`ScriptTemplate::TypeScript`].
//!
//! If the caller does not supply a script, this integration falls back to a
//! generic plugin that intercepts `tool.execute.before` for the `bash` tool
//! and exec's `spec.command`, passing the call's args via stdin (JSON).

use std::path::PathBuf;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::paths;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, ScriptTemplate};
use crate::util::{fs_atomic, mcp_json_array, ownership};

/// OpenCode plugin installer.
pub struct OpenCodeAgent;

impl OpenCodeAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn plugin_path(scope: &Scope, tag: &str) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::opencode_plugins_dir()?.join(format!("{tag}.ts")),
            Scope::Local(p) => p
                .join(".opencode")
                .join("plugins")
                .join(format!("{tag}.ts")),
        })
    }

    /// `~/.config/opencode/opencode.json` (Global) or
    /// `<root>/.opencode/opencode.json` (Local). MCP servers live in the
    /// `mcp` array.
    fn config_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::opencode_config_file()?,
            Scope::Local(p) => p.join(".opencode").join("opencode.json"),
        })
    }
}

impl Default for OpenCodeAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for OpenCodeAgent {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        Ok(Self::plugin_path(scope, tag)?.exists())
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let p = Self::plugin_path(scope, &spec.tag)?;

        let body = match &spec.script {
            Some(ScriptTemplate::TypeScript(s)) => s.clone(),
            Some(ScriptTemplate::Shell(_)) => {
                return Err(HookerError::MissingSpecField {
                    id: "opencode",
                    field: "script (TypeScript)",
                });
            }
            None => default_plugin_body(&spec.command),
        };
        let body = ensure_trailing_newline(&body);

        let outcome = fs_atomic::write_atomic(&p, body.as_bytes(), true)?;
        let mut report = InstallReport::default();
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
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();
        let p = Self::plugin_path(scope, tag)?;
        if !p.exists() {
            report.not_installed = true;
            return Ok(report);
        }
        fs_atomic::remove_if_exists(&p)?;
        report.removed.push(p.clone());

        // Tidy: prune empty plugins dir.
        if let Some(parent) = p.parent() {
            if std::fs::read_dir(parent)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(parent);
            }
        }
        Ok(report)
    }
}

impl McpSurface for OpenCodeAgent {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&Self::config_path(scope)?);
        mcp_json_array::is_installed(&ledger, name)
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::config_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_array::install(&cfg, &ledger, spec)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::config_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_array::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// A minimal TS plugin body that runs `command` before every `bash`-tool call,
/// piping the call's args (JSON) on stdin.
///
/// Callers who need richer behavior should pass their own [`ScriptTemplate::TypeScript`].
fn default_plugin_body(command: &str) -> String {
    let escaped = escape_js_double_string(command);
    format!(
        r#"// Generated by ai-hooker. Edit at your own risk.
// Re-running install will overwrite this file.

import type {{ Plugin }} from "@opencode-ai/plugin";

export const Hook: Plugin = async ({{ $ }}) => ({{
  "tool.execute.before": async ({{ tool }}, {{ args }}) => {{
    if (tool !== "bash") return;
    const payload = JSON.stringify({{ tool, args }});
    await $`echo ${{payload}} | {escaped}`;
  }},
}});
"#
    )
}

fn escape_js_double_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('`', "\\`").replace('$', "\\$")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Event, Matcher};
    use tempfile::tempdir;

    fn spec_with_script(tag: &str, ts: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command("noop")
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .script(ScriptTemplate::TypeScript(ts.into()))
            .build()
    }

    #[test]
    fn install_writes_typescript_plugin_file() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let custom = "export const X = 1;";
        agent
            .install(&scope, &spec_with_script("alpha", custom))
            .unwrap();
        let p = dir.path().join(".opencode/plugins/alpha.ts");
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("export const X = 1;"));
    }

    #[test]
    fn install_without_script_uses_default_template() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command("myapp hook opencode")
            .build();
        agent.install(&scope, &s).unwrap();
        let body =
            std::fs::read_to_string(dir.path().join(".opencode/plugins/alpha.ts")).unwrap();
        assert!(body.contains("myapp hook opencode"));
        assert!(body.contains("tool.execute.before"));
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &spec_with_script("alpha", "// x"))
            .unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".opencode/plugins/alpha.ts").exists());
        // Empty plugins dir was pruned.
        assert!(!dir.path().join(".opencode/plugins").exists());
    }

    #[test]
    fn install_with_shell_script_returns_typed_error() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command("noop")
            .script(ScriptTemplate::Shell("#!/bin/sh\nexit 0".into()))
            .build();
        let err = agent.install(&scope, &s).unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { .. }));
    }

    fn read_json(p: &std::path::Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    #[test]
    fn install_mcp_appends_to_mcp_array() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
        let cfg = dir.path().join(".opencode/opencode.json");
        assert!(cfg.exists());
        let v = read_json(&cfg);
        let arr = v["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], serde_json::json!("github"));
    }

    #[test]
    fn install_mcp_coexists_with_user_mcp_entries() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".opencode/opencode.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            r#"{ "mcp": [ { "name": "user", "command": "user-cmd" } ] }"#,
        )
        .unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
        let v = read_json(&cfg);
        let arr = v["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_mcp_does_not_collide_with_plugin_install() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let plugin_spec = HookSpec::builder("alpha").command("noop").build();
        agent.install(&scope, &plugin_spec).unwrap();
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
        // Plugin file and MCP config are separate.
        assert!(dir.path().join(".opencode/plugins/alpha.ts").exists());
        assert!(dir.path().join(".opencode/opencode.json").exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install_mcp(&scope, &local_mcp_spec("github", "appA")).unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_mcp_round_trip() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install_mcp(&scope, &local_mcp_spec("github", "myapp")).unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        // Empty config gets removed.
        assert!(!dir.path().join(".opencode/opencode.json").exists());
    }
}
