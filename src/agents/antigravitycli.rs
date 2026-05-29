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
//!
//! Native CLI skills are currently flat markdown files under
//! `~/.gemini/antigravity-cli/skills/` or `.agents/skills/`, while this
//! crate's `SkillSurface` is a directory-scoped `SKILL.md` package surface.
//! Do not register Antigravity CLI as skill-capable until that layout is
//! represented explicitly.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, McpSpec, McpTransport};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, mcp_json_map, md_block, ownership, safe_fs,
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

    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::antigravity_cli_mcp_global_file()?,
            Scope::Local(p) => p.join(".agents").join("mcp_config.json"),
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
        StatusReport::for_markdown_block_hook(tag, path)
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::markdown_install(
            Integration::id(self),
            scope,
            spec,
            Self::rules_path(scope),
            true,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::markdown_uninstall(
            Integration::id(self),
            scope,
            tag,
            Self::rules_path(scope),
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let rules = spec
            .rules
            .as_ref()
            .ok_or(AgentConfigError::MissingSpecField {
                id: "antigravitycli",
                field: "rules",
            })?;
        let path = Self::rules_path(scope)?;
        scope.ensure_contained(&path)?;
        let mut report = InstallReport::default();
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = safe_fs::write(scope, &path, new_host.as_bytes(), true)?;
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
            Ok::<(), AgentConfigError>(())
        })?;
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::rules_path(scope)?;
        scope.ensure_contained(&path)?;
        let mut report = UninstallReport::default();
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let (stripped, removed) = md_block::remove(&host, tag);

            if !removed {
                report.not_installed = true;
                return Ok(());
            }

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
            Ok::<(), AgentConfigError>(())
        })?;
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
}
