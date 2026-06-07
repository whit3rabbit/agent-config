//! OpenCode integration (sst/opencode).
//!
//! OpenCode loads plugins from `~/.config/opencode/plugins/*.{ts,js}` (Global)
//! or `<project>/.opencode/plugins/*.{ts,js}` (Local). We write a single TS
//! file per consumer (`<tag>.ts`) whose body is supplied by the caller via
//! [`ScriptTemplate::TypeScript`].
//!
//! Optional prompt surface: `~/.config/opencode/AGENTS.md` (Global) or
//! `<project>/AGENTS.md` (Local). If the caller does not supply a script,
//! this integration falls back to a generic plugin that intercepts
//! `tool.execute.before` for the `bash` tool and execs the rendered hook
//! command, passing the call's args via stdin (JSON). Safe program commands
//! are shell-quoted before rendering.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, RefusalReason, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, Matcher, McpSpec, ScriptTemplate, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, mcp_json_map, md_block, ownership, planning, safe_fs,
    skills_dir,
};

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

    fn agents_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?
                .join(".config")
                .join("opencode")
                .join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
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

    fn resolve_skills_root(scope: &Scope, name: &str) -> Result<PathBuf, AgentConfigError> {
        let roots = match scope {
            Scope::Global => vec![
                paths::home_dir()?
                    .join(".config")
                    .join("opencode")
                    .join("skills"),
                paths::home_dir()?.join(".claude").join("skills"),
                paths::home_dir()?.join(".agents").join("skills"),
            ],
            Scope::Local(p) => vec![
                p.join(".opencode").join("skills"),
                p.join(".claude").join("skills"),
                p.join(".agents").join("skills"),
            ],
        };
        for root in &roots {
            let (dir, _, ledger) = skills_dir::paths_for_status(root, name);
            if ownership::contains(&ledger, name).unwrap_or(false) || dir.exists() {
                return Ok(root.clone());
            }
        }
        Ok(roots[0].clone())
    }

    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".config").join("opencode"),
            Scope::Local(p) => p.join(".opencode"),
        })
    }

    fn inline_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::InlineLayout, AgentConfigError> {
        Ok(instructions_dir::InlineLayout {
            config_dir: Self::instruction_config_dir(scope)?,
            host_file: Self::agents_path(scope)?,
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
            None => generate_plugin_body(spec),
        };
        let body = fs_atomic::ensure_trailing_newline(&body);
        let mut changes = Vec::new();
        planning::plan_write_file(&mut changes, &p, body.as_bytes(), true)?;
        if let Some(rules) = &spec.rules {
            planning::plan_markdown_upsert(
                &mut changes,
                &Self::agents_path(scope)?,
                &spec.tag,
                &rules.content,
            )?;
        }
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
        planning::plan_markdown_remove(&mut changes, &Self::agents_path(scope)?, tag)?;
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
            None => generate_plugin_body(spec),
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
        if let Some(rules) = &spec.rules {
            let agents = Self::agents_path(scope)?;
            scope.ensure_contained(&agents)?;
            file_lock::with_lock(&agents, || {
                let host = fs_atomic::read_to_string_or_empty(&agents)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = safe_fs::write(scope, &agents, new_host.as_bytes(), true)?;
                if outcome.existed && !outcome.no_change {
                    report.patched.push(outcome.path.clone());
                    report.already_installed = false;
                } else if !outcome.existed {
                    report.created.push(outcome.path.clone());
                    report.already_installed = false;
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();
        let p = Self::plugin_path(scope, tag)?;
        scope.ensure_contained(&p)?;
        let mut removed_any = false;
        if p.exists() {
            safe_fs::remove_file(scope, &p)?;
            report.removed.push(p.clone());
            removed_any = true;

            // Tidy: prune empty plugins dir.
            if let Some(parent) = p.parent() {
                if std::fs::read_dir(parent)
                    .map(|mut it| it.next().is_none())
                    .unwrap_or(false)
                {
                    let _ = safe_fs::remove_empty_dir(scope, parent);
                }
            }
        }

        let agents = Self::agents_path(scope)?;
        scope.ensure_contained(&agents)?;
        file_lock::with_lock(&agents, || {
            let host = fs_atomic::read_to_string_or_empty(&agents)?;
            let (stripped, removed) = md_block::remove(&host, tag);
            if removed {
                if stripped.trim().is_empty() {
                    if safe_fs::restore_backup_if_matches(scope, &agents, stripped.as_bytes())? {
                        report.restored.push(agents.clone());
                        removed_any = true;
                    } else {
                        safe_fs::remove_file(scope, &agents)?;
                        report.removed.push(agents.clone());
                        removed_any = true;
                    }
                } else {
                    safe_fs::write(scope, &agents, stripped.as_bytes(), false)?;
                    report.patched.push(agents.clone());
                    removed_any = true;
                }
            }
            Ok::<(), AgentConfigError>(())
        })?;
        report.not_installed = !removed_any;
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
        let root = Self::resolve_skills_root(scope, name)?;
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
        let root = Self::resolve_skills_root(scope, &spec.name)?;
        agent_planning::skill_install(SkillSurface::id(self), scope, spec, Ok(root))
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        let root = Self::resolve_skills_root(scope, name)?;
        agent_planning::skill_uninstall(SkillSurface::id(self), scope, name, owner_tag, Ok(root))
    }

    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        let root = Self::resolve_skills_root(scope, &spec.name)?;
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        let root = Self::resolve_skills_root(scope, name)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl InstructionSurface for OpenCodeAgent {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn supported_instruction_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn instruction_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        instructions_dir::inline_status(self.inline_layout(scope)?, name, expected_owner)
    }

    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_install(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            spec,
        )
    }

    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_uninstall(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            name,
            owner_tag,
        )
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        instructions_dir::inline_install(scope, self.inline_layout(scope)?, spec)
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        instructions_dir::inline_uninstall(scope, self.inline_layout(scope)?, name, owner_tag)
    }
}

/// A dynamically generated TS plugin body based on the event and matcher of HookSpec.
fn generate_plugin_body(spec: &HookSpec) -> String {
    let command = spec.command.render_shell();
    let escaped = escape_js_template_literal(&command);

    let hook_name = match &spec.event {
        Event::PreToolUse => "tool.execute.before",
        Event::PostToolUse => "tool.execute.after",
        Event::Custom(name) => name.as_str(),
        other => other.as_str(),
    };

    let is_tool_event = hook_name == "tool.execute.before" || hook_name == "tool.execute.after";

    let guard = if is_tool_event {
        match &spec.matcher {
            Matcher::All => "".to_string(),
            Matcher::Bash => "    if (input.tool !== \"bash\") return;\n".to_string(),
            Matcher::Exact(tool) => format!("    if (input.tool !== {:?}) return;\n", tool),
            Matcher::AnyOf(tools) => {
                let list = tools
                    .iter()
                    .map(|t| format!("{:?}", t))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("    if (![{}].includes(input.tool)) return;\n", list)
            }
            Matcher::Regex(pattern) => {
                let escaped_pat = pattern.replace('\\', "\\\\").replace('"', "\\\"");
                format!(
                    "    if (!new RegExp(\"{}\").test(input.tool)) return;\n",
                    escaped_pat
                )
            }
        }
    } else {
        "".to_string()
    };

    let payload_js = if is_tool_event {
        "    const payload = JSON.stringify({ tool: input.tool, args: output.args });"
    } else {
        "    const payload = JSON.stringify({ event: input });"
    };

    format!(
        r#"// Generated by agent-config. Edit at your own risk.
// Re-running install will overwrite this file.

import type {{ Plugin }} from "@opencode-ai/plugin";

export const Hook: Plugin = async ({{ $ }}) => ({{
  {:?}: async (input, output) => {{
{}{}
    await $`echo ${{payload}} | {escaped}`;
  }},
}});
"#,
        hook_name, guard, payload_js
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
    fn generate_plugin_body_with_various_matchers_and_events() {
        // Test PostToolUse with Matcher::All
        let s1 = HookSpec::builder("all_post")
            .command_program("test", [] as [&str; 0])
            .matcher(Matcher::All)
            .event(Event::PostToolUse)
            .build();
        let body1 = generate_plugin_body(&s1);
        assert!(body1.contains("\"tool.execute.after\""));
        assert!(!body1.contains("if (input.tool"));
        assert!(body1
            .contains("const payload = JSON.stringify({ tool: input.tool, args: output.args });"));

        // Test Custom event with Matcher::Exact
        let s2 = HookSpec::builder("custom")
            .command_program("test", [] as [&str; 0])
            .matcher(Matcher::Exact("git".into()))
            .event(Event::Custom("session.idle".into()))
            .build();
        let body2 = generate_plugin_body(&s2);
        assert!(body2.contains("\"session.idle\""));
        // Custom non-tool event should not generate matcher guard since input.tool might not exist
        assert!(!body2.contains("if (input.tool !== \"git\")"));
        assert!(body2.contains("const payload = JSON.stringify({ event: input });"));

        // Test PreToolUse with Matcher::AnyOf
        let s3 = HookSpec::builder("any_of")
            .command_program("test", [] as [&str; 0])
            .matcher(Matcher::AnyOf(vec!["git".into(), "bash".into()]))
            .event(Event::PreToolUse)
            .build();
        let body3 = generate_plugin_body(&s3);
        assert!(body3.contains("if (![\"git\", \"bash\"].includes(input.tool))"));

        // Test PreToolUse with Matcher::Regex
        let s4 = HookSpec::builder("regex")
            .command_program("test", [] as [&str; 0])
            .matcher(Matcher::Regex("g.t".into()))
            .event(Event::PreToolUse)
            .build();
        let body4 = generate_plugin_body(&s4);
        assert!(body4.contains("if (!new RegExp(\"g.t\").test(input.tool))"));
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
        assert!(body.contains("async (input, output)"));
        assert!(body.contains("input.tool"));
        assert!(body.contains("output.args"));
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
    fn install_with_rules_writes_agents_md() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program("myapp", ["hook"])
            .rules("Use OpenCode project rules.")
            .build();

        agent.install(&scope, &s).unwrap();

        let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("BEGIN AGENT-CONFIG:alpha"));
        assert!(agents.contains("Use OpenCode project rules."));
    }

    #[test]
    fn uninstall_removes_rules_even_when_plugin_file_missing() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program("myapp", ["hook"])
            .rules("Use OpenCode project rules.")
            .build();

        agent.install(&scope, &s).unwrap();
        std::fs::remove_file(dir.path().join(".opencode/plugins/alpha.ts")).unwrap();
        let report = agent.uninstall(&scope, "alpha").unwrap();

        assert!(!report.not_installed);
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn instruction_surface_round_trip_uses_agents_md() {
        let dir = tempdir().unwrap();
        let agent = OpenCodeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = InstructionSpec::builder("guide")
            .owner("myapp")
            .placement(crate::spec::InstructionPlacement::InlineBlock)
            .body("# Guide\n\nUse OpenCode instructions.\n")
            .try_build()
            .unwrap();

        agent.install_instruction(&scope, &spec).unwrap();
        let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(agents.contains("BEGIN AGENT-CONFIG-INSTR:guide"));
        assert!(agent.is_instruction_installed(&scope, "guide").unwrap());

        agent
            .uninstall_instruction(&scope, "guide", "myapp")
            .unwrap();
        assert!(!agent.is_instruction_installed(&scope, "guide").unwrap());
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

    #[test]
    fn skills_resolve_multiple_roots() {
        let dir = tempdir().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());

        // 1. Initially, should resolve to .opencode/skills (primary)
        let root = OpenCodeAgent::resolve_skills_root(&scope, "my-skill").unwrap();
        assert_eq!(root, dir.path().join(".opencode").join("skills"));

        // 2. Install manually in .claude/skills and check if resolved root updates
        let claude_skills = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&claude_skills).unwrap();
        std::fs::create_dir(claude_skills.join("my-skill")).unwrap();
        let root = OpenCodeAgent::resolve_skills_root(&scope, "my-skill").unwrap();
        assert_eq!(root, claude_skills);

        // 3. Let's install to .agents/skills/my-skill manually and check
        std::fs::remove_dir(claude_skills.join("my-skill")).unwrap();
        let agents_skills = dir.path().join(".agents").join("skills");
        std::fs::create_dir_all(&agents_skills).unwrap();
        std::fs::create_dir(agents_skills.join("my-skill")).unwrap();
        let root = OpenCodeAgent::resolve_skills_root(&scope, "my-skill").unwrap();
        assert_eq!(root, agents_skills);
    }
}
