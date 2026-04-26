//! OpenCode integration (sst/opencode).
//!
//! OpenCode loads plugins from `~/.config/opencode/plugins/*.{ts,js}` (Global)
//! or `<project>/.opencode/plugins/*.{ts,js}` (Local). We write a single TS
//! file per consumer (`<tag>.ts`) whose body is supplied by the caller via
//! [`ScriptTemplate::TypeScript`].
//!
//! If the caller does not supply a script, this integration falls back to a
//! generic plugin that intercepts `tool.execute.before` for the `bash` tool
//! and execs the rendered hook command, passing the call's args via stdin
//! (JSON). Safe program commands are shell-quoted before rendering.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, RefusalReason, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, ScriptTemplate, SkillSpec};
use crate::status::StatusReport;
use crate::util::{fs_atomic, mcp_json_map, ownership, planning, safe_fs, skills_dir};

/// OpenCode plugin installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenCodeAgent {
    _private: (),
}

impl OpenCodeAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn plugin_path(scope: &Scope, tag: &str) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::opencode_plugins_dir()?.join(format!("{tag}.ts")),
            Scope::Local(p) => p
                .join(".opencode")
                .join("plugins")
                .join(format!("{tag}.ts")),
        })
    }

    /// `~/.config/opencode/opencode.json` (Global) or
    /// `<root>/opencode.json` (Local). MCP servers live in the object-based
    /// `mcp` key.
    fn config_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::opencode_config_file()?,
            Scope::Local(p) => p.join("opencode.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?
                .join(".config")
                .join("opencode")
                .join("skills"),
            Scope::Local(p) => p.join(".opencode").join("skills"),
        })
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

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::plugin_path(scope, tag)?;
        Ok(StatusReport::for_file_hook(tag, p))
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let p = Self::plugin_path(scope, &spec.tag)?;
        let body = match &spec.script {
            Some(ScriptTemplate::TypeScript(s)) => s.clone(),
            Some(ScriptTemplate::Shell(_)) => {
                return Ok(InstallPlan::refused(
                    target,
                    None,
                    RefusalReason::MissingRequiredSpecField,
                ));
            }
            None => default_plugin_body(&spec.command.render_shell()),
        };
        let body = fs_atomic::ensure_trailing_newline(&body);
        let mut changes = Vec::new();
        planning::plan_write_file(&mut changes, &p, body.as_bytes(), true)?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let p = Self::plugin_path(scope, tag)?;
        let mut changes = Vec::new();
        planning::plan_remove_file(&mut changes, &p);
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let p = Self::plugin_path(scope, &spec.tag)?;

        let body = match &spec.script {
            Some(ScriptTemplate::TypeScript(s)) => s.clone(),
            Some(ScriptTemplate::Shell(_)) => {
                return Err(AgentConfigError::MissingSpecField {
                    id: "opencode",
                    field: "script (TypeScript)",
                });
            }
            None => default_plugin_body(&spec.command.render_shell()),
        };
        let body = fs_atomic::ensure_trailing_newline(&body);

        scope.ensure_contained(&p)?;
        let outcome = safe_fs::write(scope, &p, body.as_bytes(), true)?;
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

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();
        let p = Self::plugin_path(scope, tag)?;
        scope.ensure_contained(&p)?;
        if !p.exists() {
            report.not_installed = true;
            return Ok(report);
        }
        safe_fs::remove_file(scope, &p)?;
        report.removed.push(p.clone());

        // Tidy: prune empty plugins dir.
        if let Some(parent) = p.parent() {
            if std::fs::read_dir(parent)
                .map(|mut it| it.next().is_none())
                .unwrap_or(false)
            {
                let _ = safe_fs::remove_empty_dir(scope, parent);
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

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::config_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence =
            mcp_json_map::config_presence(&cfg, &["mcp"], name, mcp_json_map::ConfigFormat::Jsonc)?;
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
        agent_planning::mcp_json_map_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::config_path(scope),
            &["mcp"],
            mcp_json_map::command_array_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::mcp_json_map_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::config_path(scope),
            &["mcp"],
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::config_path(scope)?;
        spec.validate_local_secret_policy(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::install(
            &cfg,
            &ledger,
            spec,
            &["mcp"],
            mcp_json_map::command_array_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::config_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::uninstall(
            &cfg,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            &["mcp"],
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }
}

impl SkillSurface for OpenCodeAgent {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn skill_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        let (dir, manifest, ledger) = skills_dir::paths_for_status(&root, name);
        let recorded = ownership::owner_of(&ledger, name)?;
        Ok(StatusReport::for_skill(
            name,
            dir,
            manifest,
            ledger,
            expected_owner,
            recorded,
        ))
    }

    fn plan_install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::skill_install(
            SkillSurface::id(self),
            scope,
            spec,
            Self::skills_root(scope),
        )
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

/// A minimal TS plugin body that runs `command` before every `bash`-tool call,
/// piping the call's args (JSON) on stdin.
///
/// Callers who need richer behavior should pass their own [`ScriptTemplate::TypeScript`].
fn default_plugin_body(command: &str) -> String {
    let escaped = escape_js_template_literal(command);
    format!(
        r#"// Generated by agent-config. Edit at your own risk.
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

fn escape_js_template_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{Event, Matcher};
    use tempfile::tempdir;

    fn spec_with_script(tag: &str, ts: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
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
            .command_program("myapp", ["hook", "opencode"])
            .build();
        agent.install(&scope, &s).unwrap();
        let body = std::fs::read_to_string(dir.path().join(".opencode/plugins/alpha.ts")).unwrap();
        assert!(body.contains("myapp hook opencode"));
        assert!(body.contains("tool.execute.before"));
    }

    #[test]
    fn install_without_script_quotes_program_arguments() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program(
                "my hook",
                ["repo path", "semi;$(not run)", "`tick`", "quote's"],
            )
            .build();

        agent.install(&scope, &s).unwrap();

        let body = std::fs::read_to_string(dir.path().join(".opencode/plugins/alpha.ts")).unwrap();
        assert!(body.contains("'my hook' 'repo path' 'semi;$(not run)'"));
        assert!(body.contains("'\\`tick\\`'"));
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
            .command_program("noop", [] as [&str; 0])
            .script(ScriptTemplate::Shell("#!/bin/sh\nexit 0".into()))
            .build();
        let err = agent.install(&scope, &s).unwrap_err();
        assert!(matches!(err, AgentConfigError::MissingSpecField { .. }));
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
    fn install_mcp_writes_object_based_mcp() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join("opencode.json");
        assert!(cfg.exists());
        let v = read_json(&cfg);
        assert_eq!(v["mcp"]["github"]["type"], serde_json::json!("local"));
        assert_eq!(
            v["mcp"]["github"]["command"],
            serde_json::json!(["npx", "-y", "@example/server"])
        );
    }

    #[test]
    fn install_mcp_coexists_with_user_mcp_entries() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("opencode.json");
        std::fs::write(
            &cfg,
            r#"{ "mcp": { "user": { "type": "local", "command": ["user-cmd"] } } }"#,
        )
        .unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&cfg);
        assert_eq!(v["mcp"]["user"]["command"], serde_json::json!(["user-cmd"]));
        assert_eq!(v["mcp"]["github"]["type"], serde_json::json!("local"));
    }

    #[test]
    fn install_mcp_reads_jsonc_with_comments_and_trailing_commas() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("opencode.json");
        std::fs::write(
            &cfg,
            r#"{
  // existing OpenCode config
  "mcp": {
    "user": {
      "type": "remote",
      "url": "https://example.com/mcp",
    },
  },
}
"#,
        )
        .unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&cfg);
        assert_eq!(
            v["mcp"]["user"]["url"],
            serde_json::json!("https://example.com/mcp")
        );
        assert_eq!(v["mcp"]["github"]["type"], serde_json::json!("local"));
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
        let plugin_spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .build();
        agent.install(&scope, &plugin_spec).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        // Plugin file and MCP config are separate.
        assert!(dir.path().join(".opencode/plugins/alpha.ts").exists());
        assert!(dir.path().join("opencode.json").exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
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
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        // Empty config gets removed.
        assert!(!dir.path().join("opencode.json").exists());
    }
}
