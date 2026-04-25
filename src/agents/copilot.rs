//! GitHub Copilot integration (CLI + cloud agent + VS Code agent).
//!
//! Copilot loads hook configs from any `.json` file under
//! `<project>/.github/hooks/`. We write one file per consumer
//! (`<tag>-rewrite.json`) so multiple CLIs coexist cleanly without sharing a
//! single mutable JSON document.
//!
//! Copilot uses lowerCamelCase events (`preToolUse`) and a flat entry shape
//! with `bash` (or `powershell`) as the command field, not `command`:
//!
//! ```json
//! {
//!   "version": 1,
//!   "hooks": {
//!     "preToolUse": [
//!       { "type": "command", "bash": "...", "comment": "..." }
//!     ]
//!   }
//! }
//! ```
//!
//! Optional prompt surface: `<project>/.github/copilot-instructions.md` with
//! a tagged HTML-comment fence.

use std::path::PathBuf;

use serde_json::json;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec, SkillSpec};
use crate::util::{fs_atomic, mcp_json_map, md_block, ownership, skills_dir};

/// GitHub Copilot.
pub struct CopilotAgent;

impl CopilotAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn hooks_file(scope: &Scope, tag: &str) -> Result<PathBuf, HookerError> {
        let root = match scope {
            Scope::Local(p) => p,
            Scope::Global => {
                return Err(HookerError::UnsupportedScope {
                    id: "copilot",
                    scope: ScopeKind::Global,
                });
            }
        };
        Ok(root
            .join(".github")
            .join("hooks")
            .join(format!("{tag}-rewrite.json")))
    }

    fn instructions_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        let Scope::Local(root) = scope else {
            return Err(HookerError::UnsupportedScope {
                id: "copilot",
                scope: ScopeKind::Global,
            });
        };
        Ok(root.join(".github").join("copilot-instructions.md"))
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".copilot").join("mcp-config.json"),
            Scope::Local(root) => root.join(".mcp.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".copilot").join("skills"),
            Scope::Local(root) => root.join(".github").join("skills"),
        })
    }
}

impl Default for CopilotAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for CopilotAgent {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn display_name(&self) -> &'static str {
        "GitHub Copilot"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        Ok(Self::hooks_file(scope, tag)?.exists())
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_file(scope, &spec.tag)?;
        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_copilot(&spec.matcher);

        // Each per-consumer file owns its whole contents. No tag dedupe inside
        // the file because the filename itself carries the tag.
        let entry = json!({
            "type": "command",
            "bash": spec.command,
            "matcher": matcher_str,
        });
        let doc = json!({
            "version": 1,
            "hooks": { event_key: [entry] },
        });
        let bytes = {
            let mut b = serde_json::to_vec_pretty(&doc).expect("serialize");
            b.push(b'\n');
            b
        };
        let outcome = fs_atomic::write_atomic(&p, &bytes, true)?;
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
            let instr = Self::instructions_path(scope)?;
            let host = fs_atomic::read_to_string_or_empty(&instr)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = fs_atomic::write_atomic(&instr, new_host.as_bytes(), true)?;
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

        let p = Self::hooks_file(scope, tag)?;
        if p.exists() {
            fs_atomic::remove_if_exists(&p)?;
            report.removed.push(p.clone());

            // Tidy: remove .github/hooks/ if empty.
            if let Some(parent) = p.parent() {
                if std::fs::read_dir(parent)
                    .map(|mut it| it.next().is_none())
                    .unwrap_or(false)
                {
                    let _ = std::fs::remove_dir(parent);
                }
            }
        }

        let instr = Self::instructions_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&instr)?;
        let (stripped, removed) = md_block::remove(&host, tag);
        if removed {
            if stripped.trim().is_empty() {
                if fs_atomic::restore_backup(&instr)? {
                    report.restored.push(instr.clone());
                } else {
                    fs_atomic::remove_if_exists(&instr)?;
                    report.removed.push(instr.clone());
                }
            } else {
                fs_atomic::write_atomic(&instr, stripped.as_bytes(), false)?;
                report.patched.push(instr.clone());
            }
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for CopilotAgent {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&Self::mcp_path(scope)?);
        mcp_json_map::is_installed(&ledger, name)
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::install(
            &cfg,
            &ledger,
            spec,
            &["mcpServers"],
            mcp_json_map::mcp_servers_value,
            mcp_json_map::ConfigFormat::Json,
        )
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
        mcp_json_map::uninstall(
            &cfg,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            &["mcpServers"],
            mcp_json_map::ConfigFormat::Json,
        )
    }
}

impl SkillSurface for CopilotAgent {
    fn id(&self) -> &'static str {
        "copilot"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn is_skill_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::is_installed(&root, name)
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

fn matcher_to_copilot(m: &Matcher) -> String {
    // Same family as Cursor: lowerCamelCase events, PascalCase tool names.
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
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command("myapp hook")
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_writes_per_tag_file_with_bash_field() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let p = dir.path().join(".github/hooks/alpha-rewrite.json");
        let v = read_json(&p);
        assert_eq!(v["version"], json!(1));
        assert_eq!(v["hooks"]["preToolUse"][0]["bash"], json!("myapp hook"));
        assert_eq!(v["hooks"]["preToolUse"][0]["matcher"], json!("Shell"));
    }

    #[test]
    fn distinct_tags_get_distinct_files() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.install(&scope, &local_spec("beta")).unwrap();
        assert!(dir.path().join(".github/hooks/alpha-rewrite.json").exists());
        assert!(dir.path().join(".github/hooks/beta-rewrite.json").exists());
    }

    #[test]
    fn uninstall_removes_only_our_file() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.install(&scope, &local_spec("beta")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();

        assert!(!dir.path().join(".github/hooks/alpha-rewrite.json").exists());
        assert!(dir.path().join(".github/hooks/beta-rewrite.json").exists());
    }

    #[test]
    fn rejects_global_scope() {
        let agent = CopilotAgent::new();
        let err = agent.is_installed(&Scope::Global, "alpha").unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_mcp_writes_cli_workspace_file() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("memory", "myapp"))
            .unwrap();

        let p = dir.path().join(".mcp.json");
        let v = read_json(&p);
        assert_eq!(v["mcpServers"]["memory"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = mcp_spec("memory", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = CopilotAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("memory", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "memory", "appB").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }
}
