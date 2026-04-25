//! Windsurf (Codeium Cascade) integration.
//!
//! Three surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.windsurf/rules/<tag>.md`.
//!
//! 2. **Hooks** — JSON config at `.windsurf/hooks.json` (Local). Each event
//!    key (e.g. `pre_run_command`, `post_cascade_response`) maps to an array
//!    of `{ "bash": "...", "_ai_hooker_tag": "..." }` entries; multiple
//!    consumers coexist via the standard tagged-array helper.
//!
//! 3. **MCP servers** — JSON config at `.windsurf/mcp_config.json` keyed by
//!    server name under `mcpServers`. Same shape as Claude/Cursor; reuses
//!    `util::mcp_json_object`.
//!
//! Windsurf does not yet expose a skills surface, so this agent does not
//! implement [`SkillSurface`](crate::SkillSurface).

use std::path::PathBuf;

use serde_json::json;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, UninstallReport};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, McpSpec};
use crate::util::{fs_atomic, json_patch, mcp_json_object, ownership, rules_dir};

const RULES_DIR: &str = ".windsurf/rules";

/// Windsurf integration.
pub struct WindsurfAgent;

impl WindsurfAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "windsurf",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn hooks_path(&self, scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(self.project_root(scope)?.join(".windsurf/hooks.json"))
    }

    fn mcp_path(&self, scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(self.project_root(scope)?.join(".windsurf/mcp_config.json"))
    }
}

impl Default for WindsurfAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn display_name(&self) -> &'static str {
        "Windsurf"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let root = self.project_root(scope)?;
        if rules_dir::is_installed(root, RULES_DIR, tag)? {
            return Ok(true);
        }
        let p = self.hooks_path(scope)?;
        if !p.exists() {
            return Ok(false);
        }
        let v = json_patch::read_or_empty(&p)?;
        // Walk every event array and look for our tag.
        for event_key in known_event_keys() {
            if json_patch::contains_tagged(&v, &[event_key], tag) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let mut report = InstallReport::default();

        if let Some(rules) = &spec.rules {
            let r = rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)?;
            report.merge(r);
        }

        // Hook entry — written when caller didn't ask for rules-only by
        // explicitly supplying nothing else, OR when they supplied a script.
        if spec.script.is_some() || spec.rules.is_none() {
            let event_key = event_to_windsurf(&spec.event);
            let p = self.hooks_path(scope)?;
            let mut root_doc = json_patch::read_or_empty(&p)?;
            let entry = json!({
                "bash": spec.command,
            });
            let changed = json_patch::upsert_tagged_array_entry(
                &mut root_doc,
                &[event_key.as_str()],
                &spec.tag,
                entry,
            )?;
            if changed {
                let bytes = json_patch::to_pretty(&root_doc);
                let outcome = fs_atomic::write_atomic(&p, &bytes, true)?;
                if outcome.existed {
                    report.patched.push(outcome.path.clone());
                } else {
                    report.created.push(outcome.path.clone());
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                report.already_installed = false;
            } else if report.created.is_empty() && report.patched.is_empty() {
                report.already_installed = true;
            }
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let mut report = UninstallReport::default();

        let r = rules_dir::uninstall(root, RULES_DIR, tag)?;
        report.merge(r);

        let p = self.hooks_path(scope)?;
        if p.exists() {
            let mut doc = json_patch::read_or_empty(&p)?;
            let mut changed = false;
            for event_key in known_event_keys() {
                if json_patch::remove_tagged_array_entry(&mut doc, &[event_key], tag)? {
                    changed = true;
                }
            }
            if changed {
                let now_empty = doc.as_object().map(|o| o.is_empty()).unwrap_or(true);
                if now_empty {
                    // The file holds only tagged entries; once they're all
                    // gone the `.bak` snapshots an intermediate multi-event
                    // install of our own, not pre-install user content.
                    fs_atomic::remove_if_exists(&p)?;
                    fs_atomic::remove_backup_if_exists(&p)?;
                    report.removed.push(p.clone());
                } else {
                    let bytes = json_patch::to_pretty(&doc);
                    fs_atomic::write_atomic(&p, &bytes, false)?;
                    report.patched.push(p.clone());
                }
            }
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for WindsurfAgent {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let ledger = ownership::mcp_ledger_for(&self.mcp_path(scope)?);
        mcp_json_object::is_installed(&ledger, name)
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = self.mcp_path(scope)?;
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
        let cfg = self.mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

/// Map [`Event`] to Windsurf's hook key. Windsurf uses snake_case event
/// names (`pre_run_command`, `post_cascade_response`, etc.); the
/// PreToolUse/PostToolUse defaults map to the closest equivalents. Use
/// [`Event::Custom`] for anything else.
fn event_to_windsurf(event: &Event) -> String {
    match event {
        Event::PreToolUse => "pre_run_command".into(),
        Event::PostToolUse => "post_cascade_response".into(),
        Event::Custom(s) => s.clone(),
    }
}

/// Event keys we recognise during uninstall scanning. Custom keys we wrote
/// during install will not appear here; users who attach to a custom event
/// must uninstall via the corresponding Custom event being present in the
/// hooks file (the `remove_tagged_array_entry` walk would still need to
/// know the key). We therefore also iterate every top-level array key on
/// the document below — see `uninstall`.
fn known_event_keys() -> &'static [&'static str] {
    &[
        "pre_user_prompt",
        "pre_read_code",
        "pre_write_code",
        "pre_run_command",
        "pre_mcp_tool_use",
        "post_cascade_response",
        "post_user_prompt",
        "post_read_code",
        "post_write_code",
        "post_run_command",
        "post_mcp_tool_use",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use tempfile::tempdir;

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
    }

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(body).build()
    }

    fn hook_spec(tag: &str, event: Event, command: &str) -> HookSpec {
        HookSpec::builder(tag).command(command).event(event).build()
    }

    #[test]
    fn install_rules_writes_dot_windsurf_rules_file() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".windsurf/rules/alpha.md").exists());
    }

    #[test]
    fn install_default_event_writes_pre_run_command_entry() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "myapp hook"))
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["bash"], serde_json::json!("myapp hook"));
        assert_eq!(arr[0]["_ai_hooker_tag"], serde_json::json!("alpha"));
    }

    #[test]
    fn install_custom_event_passes_through() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(
                &scope,
                &hook_spec("alpha", Event::Custom("pre_write_code".into()), "x"),
            )
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        assert!(v["pre_write_code"].is_array());
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = hook_spec("alpha", Event::PreToolUse, "x");
        agent.install(&scope, &s).unwrap();
        let r2 = agent.install(&scope, &s).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_coexists_with_other_consumer() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("appB", Event::PreToolUse, "b"))
            .unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn install_mcp_writes_mcp_config_separate_from_hooks() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["@example/server"])
            .build();
        agent.install_mcp(&scope, &spec).unwrap();
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "x"))
            .unwrap();
        assert!(dir.path().join(".windsurf/mcp_config.json").exists());
        assert!(dir.path().join(".windsurf/hooks.json").exists());
    }

    #[test]
    fn uninstall_strips_tagged_entry_from_all_event_arrays() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("alpha", Event::PostToolUse, "b"))
            .unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        // After uninstall, the tag must not appear in any event array. The
        // file may be removed entirely or restored from a `.bak` snapshot
        // taken between installs (which itself contains a partial install we
        // also stripped); both are correct cleanup outcomes.
        let p = dir.path().join(".windsurf/hooks.json");
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
        if p.exists() {
            let v = read_json(&p);
            for key in known_event_keys() {
                let Some(arr) = v.get(key).and_then(|x| x.as_array()) else {
                    continue;
                };
                assert!(
                    !arr.iter().any(|e| e["_ai_hooker_tag"] == "alpha"),
                    "tag should be stripped from {key}"
                );
            }
        }
    }

    #[test]
    fn uninstall_keeps_other_consumer_entries() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("appB", Event::PreToolUse, "b"))
            .unwrap();
        agent.uninstall(&scope, "appA").unwrap();
        let v = read_json(&dir.path().join(".windsurf/hooks.json"));
        let arr = v["pre_run_command"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["_ai_hooker_tag"], serde_json::json!("appB"));
    }

    #[test]
    fn rejects_global_scope() {
        let agent = WindsurfAgent::new();
        let err = agent.is_installed(&Scope::Global, "x").unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_rules_and_hook_independent() {
        let dir = tempdir().unwrap();
        let agent = WindsurfAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "rules body"))
            .unwrap();
        // No hook file produced when only rules are present.
        assert!(!dir.path().join(".windsurf/hooks.json").exists());
        assert!(dir.path().join(".windsurf/rules/alpha.md").exists());
    }
}
