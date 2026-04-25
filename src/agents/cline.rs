//! Cline integration.
//!
//! Two surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.clinerules/<tag>.md`.
//!    Same model as Roo / Kilo (one file per consumer, owned outright).
//!
//! 2. **Hooks (v3.36+)** — executable scripts at
//!    `.clinerules/hooks/<event>` (Local) or `~/Documents/Cline/Hooks/<event>`
//!    (Global, macOS/Linux only). Cline reads JSON event payloads on stdin
//!    and inspects the script's exit code / JSON stdout. Filenames are event
//!    names, so concurrent consumers wanting the same event must coordinate;
//!    we record ownership in a sibling `.ai-hooker-hooks.json` ledger and
//!    refuse to overwrite a hook owned by a different consumer.
//!
//! Cline does not yet expose MCP-server registration or skill installation,
//! so this agent does not implement [`McpSurface`](crate::McpSurface) or
//! [`SkillSurface`](crate::SkillSurface).

use std::path::PathBuf;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, UninstallReport};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, ScriptTemplate};
use crate::util::{fs_atomic, ownership, rules_dir};

const RULES_DIR: &str = ".clinerules";
const HOOKS_SUBDIR: &str = "hooks";
const KIND: &str = "cline hook";

/// Cline integration.
pub struct ClineAgent;

impl ClineAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn project_root<'a>(&self, scope: &'a Scope) -> Result<&'a std::path::Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "cline",
                scope: ScopeKind::Global,
            }),
        }
    }

    /// `.clinerules/hooks/` (Local). Global is unsupported (Cline's
    /// `~/Documents/Cline/Hooks/` is macOS/Linux-only and the path
    /// convention is unstable enough that we leave it out of v0.1).
    fn hooks_dir(&self, scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(self.project_root(scope)?.join(RULES_DIR).join(HOOKS_SUBDIR))
    }

    fn hook_path(&self, scope: &Scope, event: &Event) -> Result<PathBuf, HookerError> {
        Ok(self.hooks_dir(scope)?.join(event_to_filename(event)))
    }

    fn ledger_path(&self, scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(self.hooks_dir(scope)?.join(".ai-hooker-hooks.json"))
    }
}

impl Default for ClineAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for ClineAgent {
    fn id(&self) -> &'static str {
        "cline"
    }

    fn display_name(&self) -> &'static str {
        "Cline"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    /// Reports installed if either the rules file *or* a hook script for the
    /// caller exists. (Tag is the consumer ID; hooks are keyed by event name
    /// and recorded by tag in the ledger.)
    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let root = self.project_root(scope)?;
        if rules_dir::is_installed(root, RULES_DIR, tag)? {
            return Ok(true);
        }
        // Tag-owned hook entries in the ledger?
        let ledger = self.ledger_path(scope)?;
        if !ledger.exists() {
            return Ok(false);
        }
        // Iterate ledger entries; if any owner == tag, we count as installed.
        let v = crate::util::json_patch::read_or_empty(&ledger)?;
        let owned_count = v
            .get("entries")
            .and_then(|e| e.as_object())
            .map(|m| {
                m.values()
                    .filter(|entry| entry.get("owner").and_then(|o| o.as_str()) == Some(tag))
                    .count()
            })
            .unwrap_or(0);
        Ok(owned_count > 0)
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let mut report = InstallReport::default();

        if let Some(rules) = &spec.rules {
            let r = rules_dir::install(root, RULES_DIR, &spec.tag, &rules.content)?;
            report.merge(r);
        }

        if spec.script.is_some() || spec.rules.is_none() {
            let body = match &spec.script {
                Some(ScriptTemplate::Shell(s)) => {
                    fs_atomic::ensure_trailing_newline(&prefix_shebang(s))
                }
                Some(ScriptTemplate::TypeScript(_)) => {
                    return Err(HookerError::MissingSpecField {
                        id: "cline",
                        field: "script (Shell — TypeScript not supported)",
                    });
                }
                None => default_hook_body(&spec.command),
            };

            let event_filename = event_to_filename(&spec.event);
            let path = self.hook_path(scope, &spec.event)?;
            let ledger = self.ledger_path(scope)?;

            // Refuse to overwrite a hook owned by a different consumer.
            ownership::require_owner(&ledger, &event_filename, &spec.tag, KIND, path.exists())?;

            let outcome = fs_atomic::write_atomic(&path, body.as_bytes(), false)?;
            #[cfg(unix)]
            fs_atomic::chmod(&path, 0o755)?;
            if !outcome.no_change {
                if outcome.existed {
                    report.patched.push(outcome.path.clone());
                } else {
                    report.created.push(outcome.path.clone());
                }
                ownership::record_install(&ledger, &event_filename, &spec.tag)?;
                report.already_installed = false;
            } else {
                // Content unchanged but make sure ledger reflects current owner.
                let prior = ownership::owner_of(&ledger, &event_filename)?;
                if prior.as_deref() != Some(spec.tag.as_str()) {
                    ownership::record_install(&ledger, &event_filename, &spec.tag)?;
                    report.already_installed = false;
                } else if report.created.is_empty() && report.patched.is_empty() {
                    report.already_installed = true;
                }
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

        // Any hook scripts owned by this tag.
        let ledger = self.ledger_path(scope)?;
        if ledger.exists() {
            let v = crate::util::json_patch::read_or_empty(&ledger)?;
            let owned: Vec<String> = v
                .get("entries")
                .and_then(|e| e.as_object())
                .map(|m| {
                    m.iter()
                        .filter(|(_, entry)| {
                            entry.get("owner").and_then(|o| o.as_str()) == Some(tag)
                        })
                        .map(|(k, _)| k.clone())
                        .collect()
                })
                .unwrap_or_default();

            for filename in owned {
                let path = self.hooks_dir(scope)?.join(&filename);
                if path.exists() {
                    fs_atomic::remove_if_exists(&path)?;
                    report.removed.push(path);
                }
                ownership::record_uninstall(&ledger, &filename)?;
            }
        }

        // Tidy: prune empty hooks/ then .clinerules/ in case the rules path
        // already pruned them.
        for empty_dir in [self.hooks_dir(scope)?, root.join(RULES_DIR)] {
            if let Ok(mut entries) = std::fs::read_dir(&empty_dir) {
                if entries.next().is_none() {
                    let _ = std::fs::remove_dir(&empty_dir);
                }
            }
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

/// Map [`Event`] to Cline's filename convention. Cline v3.36+ recognizes
/// these eight event types; arbitrary `Event::Custom` values pass through.
fn event_to_filename(event: &Event) -> String {
    match event {
        Event::PreToolUse => "PreToolUse".into(),
        Event::PostToolUse => "PostToolUse".into(),
        Event::Custom(s) => s.clone(),
    }
}

fn prefix_shebang(s: &str) -> String {
    if s.starts_with("#!") {
        s.to_string()
    } else {
        format!("#!/usr/bin/env bash\n{s}")
    }
}

/// Minimal default hook body: pipe stdin to `command` and forward exit code.
fn default_hook_body(command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n# Generated by ai-hooker.\n# Forwards Cline's JSON event payload to the consumer command.\n{command}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(body).build()
    }

    fn hook_spec(tag: &str, event: Event, command: &str) -> HookSpec {
        HookSpec::builder(tag).command(command).event(event).build()
    }

    #[test]
    fn install_rules_writes_dot_clinerules_file() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "rule body"))
            .unwrap();
        let p = dir.path().join(".clinerules/alpha.md");
        assert!(p.exists());
        assert_eq!(fs::read_to_string(&p).unwrap(), "rule body\n");
    }

    #[test]
    fn install_hook_default_writes_executable_script() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(
                &scope,
                &hook_spec("alpha", Event::PreToolUse, "myapp hook cline"),
            )
            .unwrap();
        let p = dir.path().join(".clinerules/hooks/PreToolUse");
        assert!(p.exists());
        let body = fs::read_to_string(&p).unwrap();
        assert!(body.starts_with("#!/usr/bin/env bash"));
        assert!(body.contains("myapp hook cline"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }
    }

    #[test]
    fn install_hook_with_custom_script_body() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command("noop")
            .event(Event::Custom("TaskStart".into()))
            .script(ScriptTemplate::Shell("echo started".into()))
            .build();
        agent.install(&scope, &s).unwrap();
        let p = dir.path().join(".clinerules/hooks/TaskStart");
        assert!(p.exists());
        let body = fs::read_to_string(&p).unwrap();
        assert!(body.contains("echo started"));
    }

    #[test]
    fn install_hook_records_ownership() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("myapp", Event::PreToolUse, "noop"))
            .unwrap();
        let ledger = dir.path().join(".clinerules/hooks/.ai-hooker-hooks.json");
        assert!(ledger.exists());
        let v: serde_json::Value = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert_eq!(
            v["entries"]["PreToolUse"]["owner"],
            serde_json::json!("myapp")
        );
    }

    #[test]
    fn install_hook_collision_with_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        let err = agent
            .install(&scope, &hook_spec("appB", Event::PreToolUse, "b"))
            .unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
        // appA's hook untouched.
        let body = fs::read_to_string(dir.path().join(".clinerules/hooks/PreToolUse")).unwrap();
        assert!(body.contains("a\n"));
    }

    #[test]
    fn install_idempotent_for_hook() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = hook_spec("alpha", Event::PreToolUse, "noop");
        agent.install(&scope, &s).unwrap();
        let r2 = agent.install(&scope, &s).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_typescript_script_rejected() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command("noop")
            .script(ScriptTemplate::TypeScript("export {}".into()))
            .build();
        let err = agent.install(&scope, &s).unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { .. }));
    }

    #[test]
    fn install_with_only_rules_does_not_create_hook() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(dir.path().join(".clinerules/alpha.md").exists());
        assert!(!dir.path().join(".clinerules/hooks").exists());
    }

    #[test]
    fn install_with_rules_and_script_creates_both() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command("noop")
            .event(Event::PreToolUse)
            .rules("rules body")
            .script(ScriptTemplate::Shell("echo hi".into()))
            .build();
        agent.install(&scope, &s).unwrap();
        assert!(dir.path().join(".clinerules/alpha.md").exists());
        assert!(dir.path().join(".clinerules/hooks/PreToolUse").exists());
    }

    #[test]
    fn uninstall_removes_rules_and_owned_hooks() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(
                &scope,
                &HookSpec::builder("alpha")
                    .command("noop")
                    .event(Event::PreToolUse)
                    .rules("body")
                    .build(),
            )
            .unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".clinerules").exists());
    }

    #[test]
    fn uninstall_keeps_other_consumers_hooks() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("appA", Event::PreToolUse, "a"))
            .unwrap();
        agent
            .install(&scope, &hook_spec("appB", Event::PostToolUse, "b"))
            .unwrap();
        agent.uninstall(&scope, "appA").unwrap();
        assert!(!dir.path().join(".clinerules/hooks/PreToolUse").exists());
        assert!(dir.path().join(".clinerules/hooks/PostToolUse").exists());
    }

    #[test]
    fn uninstall_unknown_tag_is_noop() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let r = agent.uninstall(&scope, "ghost").unwrap();
        assert!(r.not_installed);
    }

    #[test]
    fn rejects_global_scope() {
        let agent = ClineAgent::new();
        let err = agent.is_installed(&Scope::Global, "x").unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
    }

    #[test]
    fn is_installed_detects_either_surface() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
        agent.install(&scope, &rules_spec("alpha", "body")).unwrap();
        assert!(agent.is_installed(&scope, "alpha").unwrap());
        agent.uninstall(&scope, "alpha").unwrap();

        agent
            .install(&scope, &hook_spec("alpha", Event::PreToolUse, "x"))
            .unwrap();
        assert!(agent.is_installed(&scope, "alpha").unwrap());
    }
}
