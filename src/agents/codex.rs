//! Codex CLI integration (OpenAI's official Codex CLI).
//!
//! Hook surface: `<scope>/.codex/hooks.json` using PascalCase event names
//! (`PreToolUse`/`PostToolUse`), JSON shape mirrors Gemini's:
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "pattern",
//!         "hooks": [{ "type": "command", "command": "...", "timeout": 600 }],
//!         "_ai_hooker_tag": "myapp"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Optional prompt surface: `$CODEX_HOME/AGENTS.md` (Global, default
//! `~/.codex/AGENTS.md`) or `<project>/AGENTS.md` (Local). Codex does not yet
//! support `@import`, so we inject content directly via fenced block.

use std::path::PathBuf;

use serde_json::json;
use toml_edit::{value, Array, InlineTable, Table};

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::paths;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec, McpTransport};
use crate::util::{fs_atomic, json_patch, md_block, ownership, toml_patch};

/// Codex CLI.
pub struct CodexAgent;

impl CodexAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("hooks.json"),
            Scope::Local(p) => p.join(".codex").join("hooks.json"),
        })
    }

    fn agents_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    /// `<codex-home>/config.toml` (Global) or `<root>/.codex/config.toml`
    /// (Local). MCP servers live here as `[mcp_servers.<name>]` tables.
    fn config_toml_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("config.toml"),
            Scope::Local(p) => p.join(".codex").join("config.toml"),
        })
    }
}

impl Default for CodexAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex CLI"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let p = Self::hooks_path(scope)?;
        let root = json_patch::read_or_empty(&p)?;
        Ok(json_patch::contains_tagged(
            &root,
            &["hooks", "PreToolUse"],
            tag,
        ))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_path(scope)?;
        let mut root = json_patch::read_or_empty(&p)?;

        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_codex(&spec.matcher);

        let entry = json!({
            "matcher": matcher_str,
            "hooks": [{ "type": "command", "command": spec.command }],
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

        if let Some(rules) = &spec.rules {
            let agents = Self::agents_path(scope)?;
            let host = fs_atomic::read_to_string_or_empty(&agents)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = fs_atomic::write_atomic(&agents, new_host.as_bytes(), true)?;
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
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::hooks_path(scope)?;
        if p.exists() {
            let mut root = json_patch::read_or_empty(&p)?;
            let mut changed = false;
            for event_key in ["PreToolUse", "PostToolUse"] {
                if json_patch::remove_tagged_array_entry(&mut root, &["hooks", event_key], tag)? {
                    changed = true;
                }
            }
            if changed {
                let empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                if empty && fs_atomic::restore_backup(&p)? {
                    report.restored.push(p.clone());
                } else if empty {
                    fs_atomic::remove_if_exists(&p)?;
                    report.removed.push(p.clone());
                } else {
                    let bytes = json_patch::to_pretty(&root);
                    fs_atomic::write_atomic(&p, &bytes, false)?;
                    report.patched.push(p.clone());
                }
            }
        }

        let agents = Self::agents_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&agents)?;
        let (stripped, removed) = md_block::remove(&host, tag);
        if removed {
            if stripped.trim().is_empty() {
                if fs_atomic::restore_backup(&agents)? {
                    report.restored.push(agents.clone());
                } else {
                    fs_atomic::remove_if_exists(&agents)?;
                    report.removed.push(agents.clone());
                }
            } else {
                fs_atomic::write_atomic(&agents, stripped.as_bytes(), false)?;
                report.patched.push(agents.clone());
            }
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&Self::config_toml_path(scope)?);
        ownership::contains(&ledger, name)
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let mut report = InstallReport::default();
        let cfg = Self::config_toml_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);

        let mut doc = toml_patch::read_or_empty(&cfg)?;
        let table = build_mcp_table(spec);
        let changed =
            toml_patch::upsert_named_table(&mut doc, &["mcp_servers"], &spec.name, table)?;

        let prior_owner = ownership::owner_of(&ledger, &spec.name)?;
        let owner_changed = prior_owner.as_deref() != Some(spec.owner_tag.as_str());

        if changed {
            let bytes = toml_patch::to_string(&doc);
            let outcome = fs_atomic::write_atomic(&cfg, &bytes, true)?;
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
            ownership::record_install(&ledger, &spec.name, &spec.owner_tag)?;
        }
        if !changed && !owner_changed {
            report.already_installed = true;
        }
        Ok(report)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let mut report = UninstallReport::default();

        let cfg = Self::config_toml_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);

        let mut doc = toml_patch::read_or_empty(&cfg)?;
        let in_config = toml_patch::contains_named_table(&doc, &["mcp_servers"], name);
        let in_ledger = ownership::contains(&ledger, name)?;

        if !in_config && !in_ledger {
            report.not_installed = true;
            return Ok(report);
        }

        ownership::require_owner(&ledger, name, owner_tag, "mcp server", in_config)?;

        if in_config {
            let removed = toml_patch::remove_named_table(&mut doc, &["mcp_servers"], name)?;
            debug_assert!(removed);

            let now_empty = doc.as_table().is_empty();
            if now_empty && fs_atomic::restore_backup(&cfg)? {
                report.restored.push(cfg.clone());
            } else if now_empty {
                fs_atomic::remove_if_exists(&cfg)?;
                report.removed.push(cfg.clone());
            } else {
                let bytes = toml_patch::to_string(&doc);
                fs_atomic::write_atomic(&cfg, &bytes, false)?;
                report.patched.push(cfg.clone());
            }
        }

        ownership::record_uninstall(&ledger, name)?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

/// Translate an [`McpSpec`] into a TOML `[mcp_servers.<name>]` sub-table.
fn build_mcp_table(spec: &McpSpec) -> Table {
    let mut t = Table::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            t["command"] = value(command.clone());
            let mut arr = Array::new();
            for a in args {
                arr.push(a.clone());
            }
            t["args"] = value(arr);
            if !env.is_empty() {
                let mut env_t = InlineTable::new();
                for (k, v) in env {
                    env_t.insert(k, v.clone().into());
                }
                t["env"] = value(env_t);
            }
        }
        McpTransport::Http { url, headers } => {
            t["type"] = value("http");
            t["url"] = value(url.clone());
            if !headers.is_empty() {
                let mut h = InlineTable::new();
                for (k, v) in headers {
                    h.insert(k, v.clone().into());
                }
                t["headers"] = value(h);
            }
        }
        McpTransport::Sse { url, headers } => {
            t["type"] = value("sse");
            t["url"] = value(url.clone());
            if !headers.is_empty() {
                let mut h = InlineTable::new();
                for (k, v) in headers {
                    h.insert(k, v.clone().into());
                }
                t["headers"] = value(h);
            }
        }
    }
    t
}

fn matcher_to_codex(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "shell".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "PreToolUse".into(),
        Event::PostToolUse => "PostToolUse".into(),
        Event::Custom(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command("myapp hook")
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    #[test]
    fn writes_pre_tool_use_pascalcase() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".codex/hooks.json"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("shell"));
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!("myapp hook")
        );
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".codex/hooks.json").exists());
    }

    #[test]
    fn matcher_mapping() {
        assert_eq!(matcher_to_codex(&Matcher::All), "*");
        assert_eq!(matcher_to_codex(&Matcher::Bash), "shell");
        assert_eq!(matcher_to_codex(&Matcher::Exact("Edit".into())), "Edit");
        assert_eq!(
            matcher_to_codex(&Matcher::AnyOf(vec!["Read".into(), "Write".into()])),
            "Read|Write"
        );
        assert_eq!(
            matcher_to_codex(&Matcher::Regex("Bash|Edit".into())),
            "Bash|Edit"
        );
    }

    #[test]
    fn post_tool_use_pascal_case() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command("noop")
            .event(Event::PostToolUse)
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_json(&dir.path().join(".codex/hooks.json"));
        assert!(v["hooks"]["PostToolUse"].is_array());
    }

    #[test]
    fn rules_injects_into_agents_md() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command("noop")
            .rules("Use strict mode.")
            .build();
        agent.install(&scope, &spec).unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(md.contains("Use strict mode."));
        assert!(md.contains("AI-HOOKER:alpha"));
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        let r2 = agent.install(&scope, &local_spec("alpha")).unwrap();
        assert!(r2.already_installed);
    }

    fn local_mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    fn read_toml(p: &std::path::Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn install_mcp_writes_named_table_in_config_toml() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        assert!(cfg.exists());
        let s = read_toml(&cfg);
        assert!(s.contains("[mcp_servers.github]"), "got:\n{s}");
        assert!(s.contains(r#"command = "npx""#), "got:\n{s}");
        assert!(s.contains(r#"FOO = "bar""#), "got:\n{s}");
    }

    #[test]
    fn install_mcp_preserves_user_comments_and_other_sections() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original =
            "# Codex configuration\n# Hand-authored.\n\n[some.section]\nkey = \"value\"\n";
        std::fs::write(&cfg, original).unwrap();

        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();

        let s = read_toml(&cfg);
        assert!(
            s.contains("# Codex configuration"),
            "comment lost. got:\n{s}"
        );
        assert!(s.contains("[some.section]"), "user section lost");
        assert!(s.contains("[mcp_servers.github]"));
        // .bak made when we modified an existing file.
        assert!(dir.path().join(".codex/config.toml.bak").exists());
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = local_mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_mcp_does_not_collide_with_hook_install() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        // Hooks use a separate file; both must exist.
        assert!(dir.path().join(".codex/hooks.json").exists());
        assert!(dir.path().join(".codex/config.toml").exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn uninstall_mcp_round_trip() {
        let dir = tempdir().unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        // Empty doc: the file is removed entirely.
        assert!(!cfg.exists());
    }

    #[test]
    fn uninstall_mcp_keeps_user_sections() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original = "[other]\nfoo = \"bar\"\n";
        std::fs::write(&cfg, original).unwrap();
        let agent = CodexAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &local_mcp_spec("github", "myapp"))
            .unwrap();
        agent.uninstall_mcp(&scope, "github", "myapp").unwrap();
        let s = read_toml(&cfg);
        assert!(s.contains("[other]"), "got:\n{s}");
        assert!(!s.contains("[mcp_servers"), "mcp_servers should be pruned");
    }
}
