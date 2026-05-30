//! Google Antigravity CLI integration.
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: fenced HTML-comment blocks in `AGENTS.md` locally and
//!    `~/.gemini/GEMINI.md` globally. The CLI reads the same workspace context
//!    files as Gemini CLI plus global Gemini constraints.
//! 2. **MCP servers**: standard `mcpServers` JSON map at
//!    `~/.gemini/antigravity-cli/mcp_config.json` (Global) or
//!    `<root>/.agents/mcp_config.json` (Local). Remote entries use
//!    `serverUrl`, not Gemini CLI's legacy `url` field.
//! 3. **Instructions**: `InlineBlock` placement inside the same prompt files.
//! 4. **Hooks**: event hooks inside `hooks.json` at `<root>/.agents/hooks.json` (Local)
//!    or `~/.gemini/antigravity-cli/hooks.json` (Global).
//! 5. **Skills**: directory-scoped skills at `.agents/skills/<name>/` (Local)
//!    or `~/.gemini/antigravity-cli/skills/<name>/` (Global).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, Matcher, McpSpec, McpTransport, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, hooks_json, instructions_dir, mcp_json_map, md_block, ownership,
    planning, safe_fs, skills_dir,
};

const MCP_SERVERS_PATH: &[&str] = &["mcpServers"];

/// Google Antigravity CLI installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct AntigravityCliAgent {
    _private: (),
}

impl AntigravityCliAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?.join("GEMINI.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_cli_home()?.join("hooks.json"),
            Scope::Local(p) => p.join(".agents").join("hooks.json"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_cli_mcp_global_file()?,
            Scope::Local(p) => p.join(".agents").join("mcp_config.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_cli_home()?.join("skills"),
            Scope::Local(p) => p.join(".agents").join("skills"),
        })
    }

    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::gemini_home()?,
            Scope::Local(p) => p.join(".agents"),
        })
    }

    fn inline_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::InlineLayout, AgentConfigError> {
        Ok(instructions_dir::InlineLayout {
            config_dir: Self::instruction_config_dir(scope)?,
            host_file: Self::rules_path(scope)?,
        })
    }
}

impl Integration for AntigravityCliAgent {
    fn id(&self) -> &'static str {
        "antigravitycli"
    }

    fn display_name(&self) -> &'static str {
        "Antigravity CLI"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::rules_path(scope)?;
        if path.exists() {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            if md_block::contains(&host, tag) {
                return StatusReport::for_markdown_block_hook(tag, path);
            }
        }

        let hooks_path = Self::hooks_path(scope)?;
        let presence = hooks_json::config_presence(&hooks_path, tag)?;
        if let crate::status::ConfigPresence::Absent = presence {
            StatusReport::for_markdown_block_hook(tag, path)
        } else {
            Ok(StatusReport::for_tagged_hook(tag, hooks_path, presence))
        }
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
        let mut changes = Vec::new();

        // 1. Plan hook command installation in hooks.json
        let hooks_path = Self::hooks_path(scope)?;
        hooks_json::plan_install(&mut changes, &hooks_path, spec, build_hook_value)?;

        // 2. Plan rules installation if spec.rules is Some
        if let Some(rules) = &spec.rules {
            let path = Self::rules_path(scope)?;
            planning::plan_markdown_upsert(&mut changes, &path, &spec.tag, &rules.content)?;
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
        let mut changes = Vec::new();

        // 1. Plan hook command removal
        let hooks_path = Self::hooks_path(scope)?;
        hooks_json::plan_uninstall(&mut changes, &hooks_path, tag)?;

        // 2. Plan rules removal
        let path = Self::rules_path(scope)?;
        planning::plan_markdown_remove(&mut changes, &path, tag)?;

        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        // 1. Install hook command
        let hooks_path = Self::hooks_path(scope)?;
        let hook_report = hooks_json::install(scope, &hooks_path, spec, build_hook_value)?;
        report.created.extend(hook_report.created);
        report.patched.extend(hook_report.patched);
        report.backed_up.extend(hook_report.backed_up);
        report.already_installed = hook_report.already_installed;

        // 2. Install rules if present
        if let Some(rules) = &spec.rules {
            let path = Self::rules_path(scope)?;
            scope.ensure_contained(&path)?;
            file_lock::with_lock(&path, || {
                let host = fs_atomic::read_to_string_or_empty(&path)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = safe_fs::write(scope, &path, new_host.as_bytes(), true)?;
                if outcome.no_change {
                    // if hook_report.already_installed was true, keep it, otherwise set false
                } else if outcome.existed {
                    report.patched.push(outcome.path.clone());
                    report.already_installed = false;
                } else {
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

        // 1. Uninstall hook command
        let hooks_path = Self::hooks_path(scope)?;
        let hook_report = hooks_json::uninstall(scope, &hooks_path, tag)?;
        report.removed.extend(hook_report.removed);
        report.patched.extend(hook_report.patched);
        report.restored.extend(hook_report.restored);
        report.not_installed = hook_report.not_installed;

        // 2. Uninstall rules
        let path = Self::rules_path(scope)?;
        scope.ensure_contained(&path)?;
        if path.exists() {
            file_lock::with_lock(&path, || {
                let host = fs_atomic::read_to_string_or_empty(&path)?;
                let (stripped, removed) = md_block::remove(&host, tag);
                if removed {
                    report.not_installed = false;
                    if stripped.trim().is_empty() {
                        if safe_fs::restore_backup_if_matches(scope, &path, stripped.as_bytes())? {
                            report.restored.push(path.clone());
                        } else {
                            safe_fs::remove_file(scope, &path)?;
                            report.removed.push(path.clone());
                        }
                    } else {
                        safe_fs::write(scope, &path, stripped.as_bytes(), false)?;
                        report.patched.push(path.clone());
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        Ok(report)
    }
}

impl McpSurface for AntigravityCliAgent {
    fn id(&self) -> &'static str {
        "antigravitycli"
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
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence = mcp_json_map::config_presence(
            &cfg,
            MCP_SERVERS_PATH,
            name,
            mcp_json_map::ConfigFormat::Json,
        )?;
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
            Self::mcp_path(scope),
            MCP_SERVERS_PATH,
            antigravity_cli_mcp_value,
            mcp_json_map::ConfigFormat::Json,
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
            Self::mcp_path(scope),
            MCP_SERVERS_PATH,
            mcp_json_map::ConfigFormat::Json,
        )
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        spec.validate_local_secret_policy(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::install(
            &cfg,
            &ledger,
            spec,
            MCP_SERVERS_PATH,
            antigravity_cli_mcp_value,
            mcp_json_map::ConfigFormat::Json,
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
        let cfg = Self::mcp_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::uninstall(
            &cfg,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            MCP_SERVERS_PATH,
            mcp_json_map::ConfigFormat::Json,
        )
    }
}

impl SkillSurface for AntigravityCliAgent {
    fn id(&self) -> &'static str {
        "antigravitycli"
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

impl InstructionSurface for AntigravityCliAgent {
    fn id(&self) -> &'static str {
        "antigravitycli"
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

fn matcher_to_antigravity(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "run_command".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn build_hook_value(spec: &HookSpec) -> serde_json::Value {
    let matcher_str = matcher_to_antigravity(&spec.matcher);
    let command_str = spec.command.render_shell();
    serde_json::json!([
        {
            "matcher": matcher_str,
            "hooks": [
                {
                    "type": "command",
                    "command": command_str,
                    "timeout": 10
                }
            ]
        }
    ])
}

fn antigravity_cli_mcp_value(spec: &McpSpec) -> Value {
    let mut obj = Map::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            if !env.is_empty() {
                obj.insert("env".into(), string_map_value(env));
            }
        }
        McpTransport::Http { url, headers } => {
            insert_remote(&mut obj, url, headers);
        }
        McpTransport::Sse { url, headers } => {
            insert_remote(&mut obj, url, headers);
        }
    }
    Value::Object(obj)
}

fn insert_remote(obj: &mut Map<String, Value>, url: &str, headers: &BTreeMap<String, String>) {
    obj.insert("serverUrl".into(), Value::String(url.into()));
    if !headers.is_empty() {
        obj.insert("headers".into(), string_map_value(headers));
    }
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
    use crate::spec::{InstructionPlacement, SecretPolicy};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn hook_only_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("myapp", ["hook"])
            .matcher(Matcher::Bash)
            .event(crate::spec::Event::PreToolUse)
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    fn http_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec {
            name: name.into(),
            owner_tag: owner.into(),
            transport: McpTransport::Http {
                url: "https://example.com/mcp".into(),
                headers: BTreeMap::new(),
            },
            friendly_name: None,
            secret_policy: SecretPolicy::RefuseInlineSecretsInLocalScope,
            adopt_unowned: false,
        }
    }

    fn instruction(name: &str, owner: &str) -> InstructionSpec {
        InstructionSpec::builder(name)
            .owner(owner)
            .placement(InstructionPlacement::InlineBlock)
            .body("Use Antigravity CLI instructions.\n")
            .build()
    }

    fn read_json(path: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn local_rules_write_agents_md() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(md.contains("AGENT-CONFIG:alpha"));
        assert!(md.contains("body"));
    }

    #[test]
    fn rules_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "body");
        agent.install(&scope, &spec).unwrap();
        let again = agent.install(&scope, &spec).unwrap();
        assert!(again.already_installed);
    }

    #[test]
    fn uninstall_rules_round_trip() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn install_mcp_writes_agents_mcp_config() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".agents/mcp_config.json");
        let v = read_json(&cfg);
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn remote_mcp_uses_server_url_not_url() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &http_spec("remote", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".agents/mcp_config.json");
        let v = read_json(&cfg);
        assert_eq!(
            v["mcpServers"]["remote"]["serverUrl"],
            json!("https://example.com/mcp")
        );
        assert!(v["mcpServers"]["remote"].get("url").is_none());
        assert!(v["mcpServers"]["remote"].get("type").is_none());
    }

    #[test]
    fn mcp_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &spec).unwrap();
        let again = agent.install_mcp(&scope, &spec).unwrap();
        assert!(again.already_installed);
    }

    #[test]
    fn mcp_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        assert!(!dir.path().join(".agents/mcp_config.json").exists());
    }

    #[test]
    fn mcp_uninstall_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_instruction_writes_agents_md_and_agents_ledger() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_instruction(&scope, &instruction("alpha", "myapp"))
            .unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(md.contains("AGENT-CONFIG-INSTR:alpha"));
        assert!(dir
            .path()
            .join(".agents/.agent-config-instructions.json")
            .exists());
    }

    #[test]
    fn install_hook_writes_hooks_json() {
        let dir = tempdir().unwrap();
        let agent = AntigravityCliAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &hook_only_spec("alpha")).unwrap();
        let cfg = dir.path().join(".agents/hooks.json");
        let v = read_json(&cfg);
        assert_eq!(
            v["alpha"]["PreToolUse"][0]["matcher"],
            json!("run_command")
        );
    }
}
