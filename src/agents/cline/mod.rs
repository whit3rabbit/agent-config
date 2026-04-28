//! Cline integration.
//!
//! Surfaces:
//!
//! 1. **Rules** — project-local markdown files at `.clinerules/<tag>.md`.
//!    Same model as Roo / Kilo (one file per consumer, owned outright).
//!
//! 2. **Hooks (v3.36+)** — executable scripts at
//!    `.clinerules/hooks/<event>` (Local) or `~/Documents/Cline/Hooks/<event>`
//!    (Global, macOS/Linux only). Cline reads JSON event payloads on stdin
//!    and inspects the script's exit code / JSON stdout. Filenames are event
//!    names, so concurrent consumers wanting the same event must coordinate;
//!    we record ownership in a sibling `.agent-config-hooks.json` ledger and
//!    refuse to overwrite a hook owned by a different consumer.
//!
//! 3. **MCP servers** — global VS Code extension config at
//!    `Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`,
//!    keyed by server name under `mcpServers`. Lives in `mcp.rs`.
//!
//! 4. **Skills** — directory-scoped skills at `.cline/skills/<name>/`
//!    (Local) or `~/.cline/skills/<name>/` (Global). Lives in `skills.rs`.
//!
//! 5. **Instructions** — standalone files in `.clinerules/`. Lives in
//!    `instructions.rs`.

use std::path::PathBuf;

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, Integration, UninstallReport};
use crate::plan::{
    has_refusal, InstallPlan, PlanTarget as DryPlanTarget, PlannedChange, RefusalReason,
    UninstallPlan,
};
use crate::scope::{Scope, ScopeKind};
use crate::spec::HookSpec;
#[cfg(not(windows))]
use crate::spec::{Event, ScriptTemplate};
use crate::status::{InstallStatus, PathStatus, PlanTarget, StatusReport, StatusWarning};
#[cfg(not(windows))]
use crate::util::fs_atomic;
use crate::util::{file_lock, ownership, planning, rules_dir, safe_fs};

mod instructions;
mod mcp;
mod skills;

pub(super) const RULES_DIR: &str = ".clinerules";
const HOOKS_SUBDIR: &str = "hooks";
#[cfg(not(windows))]
const KIND: &str = "cline hook";

/// Cline integration.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClineAgent {
    _private: (),
}

impl ClineAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    pub(super) fn project_root<'a>(
        &self,
        scope: &'a Scope,
    ) -> Result<&'a std::path::Path, AgentConfigError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(AgentConfigError::UnsupportedScope {
                id: "cline",
                scope: ScopeKind::Global,
            }),
        }
    }

    /// `.clinerules/hooks/` (Local). Global is unsupported (Cline's
    /// `~/Documents/Cline/Hooks/` is macOS/Linux-only and the path
    /// convention is unstable enough that we leave it out of v0.1).
    fn hooks_dir(&self, scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(self.project_root(scope)?.join(RULES_DIR).join(HOOKS_SUBDIR))
    }

    fn ledger_path(&self, scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(self.hooks_dir(scope)?.join(".agent-config-hooks.json"))
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
    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let rules_file = rules_dir::target_path(root, RULES_DIR, tag);
        let rules_exists = rules_file.exists();

        let ledger = self.ledger_path(scope)?;
        let owned_hook_count = if ledger.exists() {
            let v = crate::util::json_patch::read_or_empty(&ledger)?;
            v.get("entries")
                .and_then(|e| e.as_object())
                .map(|m| {
                    m.values()
                        .filter(|entry| entry.get("owner").and_then(|o| o.as_str()) == Some(tag))
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };

        let mut files = vec![if rules_exists {
            PathStatus::Exists {
                path: rules_file.clone(),
            }
        } else {
            PathStatus::Missing {
                path: rules_file.clone(),
            }
        }];
        if ledger.exists() {
            files.push(PathStatus::Exists {
                path: ledger.clone(),
            });
        }

        let mut warnings = Vec::new();
        let status = if rules_exists || owned_hook_count > 0 {
            InstallStatus::InstalledOwned {
                owner: tag.to_string(),
            }
        } else {
            // Surface a backup file if it exists for the rules markdown.
            let mut bak = rules_file.clone();
            if let Some(name) = bak.file_name().map(|n| n.to_os_string()) {
                if let Ok(mut s) = name.into_string() {
                    s.push_str(".bak");
                    bak.set_file_name(s);
                    if bak.exists() {
                        warnings.push(StatusWarning::BackupExists { path: bak });
                    }
                }
            }
            InstallStatus::Absent
        };

        Ok(StatusReport {
            target: PlanTarget::Hook {
                tag: tag.to_string(),
            },
            status,
            config_path: Some(rules_file),
            ledger_path: Some(ledger),
            files,
            warnings,
        })
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(AgentConfigError::UnsupportedScope { .. }) => {
                return Ok(InstallPlan::refused(
                    target,
                    None,
                    RefusalReason::UnsupportedScope,
                ));
            }
            Err(e) => return Err(e),
        };
        let mut changes = Vec::new();
        if let Some(rules) = &spec.rules {
            changes.extend(rules_dir::plan_install(
                root,
                RULES_DIR,
                &spec.tag,
                &rules.content,
            )?);
        }
        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        if spec.script.is_some() || spec.rules.is_none() {
            // Cline's hook contract is a `bash`-shebanged script chmod'd
            // executable. On native Windows the chmod is a no-op and the
            // shebang is meaningless, so the install would silently
            // produce a non-runnable hook. Refuse before any mutation.
            #[cfg(windows)]
            {
                return Ok(InstallPlan::refused(
                    target,
                    None,
                    RefusalReason::UnsupportedPlatform,
                ));
            }
            #[cfg(not(windows))]
            {
                let body = match &spec.script {
                    Some(ScriptTemplate::Shell(s)) => {
                        fs_atomic::ensure_trailing_newline(&prefix_shebang(s))
                    }
                    Some(ScriptTemplate::TypeScript(_)) => {
                        return Ok(InstallPlan::refused(
                            target,
                            None,
                            RefusalReason::MissingRequiredSpecField,
                        ));
                    }
                    None => default_hook_body(&spec.command.render_shell()),
                };
                let event_filename = event_to_filename(&spec.event)?;
                let path = self.hooks_dir(scope)?.join(&event_filename);
                let ledger = self.ledger_path(scope)?;
                let actual_owner = ownership::owner_of(&ledger, &event_filename)?;
                match (actual_owner.as_deref(), path.exists()) {
                    (Some(owner), _) if owner != spec.tag => {
                        changes.push(PlannedChange::Refuse {
                            path: Some(ledger),
                            reason: RefusalReason::OwnerMismatch,
                        });
                        return Ok(InstallPlan::from_changes(target, changes));
                    }
                    (None, true) => {
                        changes.push(PlannedChange::Refuse {
                            path: Some(path),
                            reason: RefusalReason::UserInstalledEntry,
                        });
                        return Ok(InstallPlan::from_changes(target, changes));
                    }
                    _ => {}
                }
                planning::plan_write_file(&mut changes, &path, body.as_bytes(), false)?;
                planning::plan_set_permissions(&mut changes, &path, 0o755);
                let owner_changed = actual_owner.as_deref() != Some(spec.tag.as_str());
                let file_changed = changes.iter().any(|change| {
                    matches!(
                        change,
                        PlannedChange::CreateFile { .. } | PlannedChange::PatchFile { .. }
                    )
                });
                if owner_changed || file_changed {
                    planning::plan_write_ledger(&mut changes, &ledger, &event_filename, &spec.tag);
                }
            }
        }

        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = DryPlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let root = match self.project_root(scope) {
            Ok(root) => root,
            Err(AgentConfigError::UnsupportedScope { .. }) => {
                return Ok(UninstallPlan::refused(
                    target,
                    None,
                    RefusalReason::UnsupportedScope,
                ));
            }
            Err(e) => return Err(e),
        };
        let mut changes = rules_dir::plan_uninstall(root, RULES_DIR, tag)?;
        let ledger = self.ledger_path(scope)?;
        if ledger.exists() {
            let v = match crate::util::json_patch::read_or_empty(&ledger) {
                Ok(v) => v,
                Err(AgentConfigError::JsonInvalid { .. }) => {
                    changes.push(PlannedChange::Refuse {
                        path: Some(ledger),
                        reason: RefusalReason::InvalidConfig,
                    });
                    return Ok(UninstallPlan::from_changes(target, changes));
                }
                Err(e) => return Err(e),
            };
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
                if validate_custom_event_filename(&filename).is_err() {
                    changes.push(PlannedChange::Refuse {
                        path: Some(ledger),
                        reason: RefusalReason::InvalidConfig,
                    });
                    return Ok(UninstallPlan::from_changes(target, changes));
                }
                let path = self.hooks_dir(scope)?.join(&filename);
                if path.exists() {
                    changes.push(PlannedChange::RemoveFile { path });
                }
                planning::plan_remove_ledger_entry(&mut changes, &ledger, &filename);
            }
        }
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.project_root(scope)?;
        let mut report = InstallReport::default();

        if let Some(rules) = &spec.rules {
            scope.ensure_contained(&rules_dir::target_path(root, RULES_DIR, &spec.tag))?;
            let r = rules_dir::install(scope, RULES_DIR, &spec.tag, &rules.content)?;
            report.merge(r);
        }

        if spec.script.is_some() || spec.rules.is_none() {
            // Cline's hook script body is bash-shebanged and chmod'd 0o755;
            // both are no-ops on native Windows. Refuse before any
            // mutation so the install does not silently produce a
            // non-runnable hook. See `plan_install` for the matching
            // refusal in dry-run mode.
            #[cfg(windows)]
            {
                return Err(AgentConfigError::UnsupportedPlatform {
                    id: "cline",
                    reason: "Cline hooks require a POSIX shell environment; native Windows is not supported",
                });
            }
            #[cfg(not(windows))]
            let hooks_dir = self.hooks_dir(scope)?;
            #[cfg(not(windows))]
            scope.ensure_contained(&hooks_dir)?;
            #[cfg(not(windows))]
            file_lock::with_lock(&hooks_dir, || {
                let body = match &spec.script {
                    Some(ScriptTemplate::Shell(s)) => {
                        fs_atomic::ensure_trailing_newline(&prefix_shebang(s))
                    }
                    Some(ScriptTemplate::TypeScript(_)) => {
                        return Err(AgentConfigError::MissingSpecField {
                            id: "cline",
                            field: "script (Shell — TypeScript not supported)",
                        });
                    }
                    None => default_hook_body(&spec.command.render_shell()),
                };

                let event_filename = event_to_filename(&spec.event)?;
                let path = hooks_dir.join(&event_filename);
                let ledger = hooks_dir.join(".agent-config-hooks.json");

                // Refuse to overwrite a hook owned by a different consumer.
                ownership::require_owner(&ledger, &event_filename, &spec.tag, KIND, path.exists())?;

                let outcome = safe_fs::write(scope, &path, body.as_bytes(), false)?;
                #[cfg(unix)]
                safe_fs::chmod(scope, &path, 0o755)?;
                if !outcome.no_change {
                    if outcome.existed {
                        report.patched.push(outcome.path.clone());
                    } else {
                        report.created.push(outcome.path.clone());
                    }
                    let hash = ownership::content_hash(body.as_bytes());
                    ownership::record_install(&ledger, &event_filename, &spec.tag, Some(&hash))?;
                    report.already_installed = false;
                } else {
                    let prior = ownership::owner_of(&ledger, &event_filename)?;
                    if prior.as_deref() != Some(spec.tag.as_str()) {
                        let hash = ownership::content_hash(body.as_bytes());
                        ownership::record_install(
                            &ledger,
                            &event_filename,
                            &spec.tag,
                            Some(&hash),
                        )?;
                        report.already_installed = false;
                    } else if report.created.is_empty() && report.patched.is_empty() {
                        report.already_installed = true;
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let root = self.project_root(scope)?;
        let mut report = UninstallReport::default();

        scope.ensure_contained(&rules_dir::target_path(root, RULES_DIR, tag))?;
        let r = rules_dir::uninstall(scope, RULES_DIR, tag)?;
        report.merge(r);

        // Any hook scripts owned by this tag.
        let hooks_dir = self.hooks_dir(scope)?;
        scope.ensure_contained(&hooks_dir)?;
        file_lock::with_lock(&hooks_dir, || {
            let ledger = hooks_dir.join(".agent-config-hooks.json");
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
                    validate_custom_event_filename(&filename)?;
                    let path = hooks_dir.join(&filename);
                    if path.exists() {
                        safe_fs::remove_file(scope, &path)?;
                        report.removed.push(path);
                    }
                    ownership::record_uninstall(&ledger, &filename)?;
                }
            }
            Ok::<(), AgentConfigError>(())
        })?;

        // Tidy: prune empty hooks/ then .clinerules/ in case the rules path
        // already pruned them.
        for empty_dir in [self.hooks_dir(scope)?, root.join(RULES_DIR)] {
            if let Ok(mut entries) = std::fs::read_dir(&empty_dir) {
                if entries.next().is_none() {
                    let _ = safe_fs::remove_empty_dir(scope, &empty_dir);
                }
            }
        }

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

/// Map [`Event`] to Cline's filename convention. Custom names become
/// file names, so they must be path-safe single components.
#[cfg(not(windows))]
fn event_to_filename(event: &Event) -> Result<String, AgentConfigError> {
    match event {
        Event::PreToolUse => Ok("PreToolUse".into()),
        Event::PostToolUse => Ok("PostToolUse".into()),
        Event::Custom(s) => validate_custom_event_filename(s).map(|()| s.clone()),
    }
}

fn validate_custom_event_filename(name: &str) -> Result<(), AgentConfigError> {
    if name.is_empty() {
        return Err(AgentConfigError::InvalidTag {
            tag: name.into(),
            reason: "Cline custom event must not be empty",
        });
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AgentConfigError::InvalidTag {
            tag: name.into(),
            reason: "Cline custom event may only contain ASCII letters, digits, '_' and '-'",
        });
    }
    Ok(())
}

#[cfg(not(windows))]
fn prefix_shebang(s: &str) -> String {
    if s.starts_with("#!") {
        s.to_string()
    } else {
        format!("#!/usr/bin/env bash\n{s}")
    }
}

/// Minimal default hook body: pipe stdin to the rendered command and forward
/// exit code. Safe program commands are shell-quoted before they reach here;
/// unchecked shell commands intentionally pass through as shell syntax.
#[cfg(not(windows))]
fn default_hook_body(command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n# Generated by agent-config.\n# Forwards Cline's JSON event payload to the consumer command.\n{command}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::McpSurface;
    use crate::spec::McpSpec;
    // `Event` is imported in the parent module behind `cfg(not(windows))` so
    // the lib build on Windows stays warning-clean. The test mod still needs
    // it because `#[cfg(windows)]` tests in this file construct `HookSpec`s
    // with `Event::PreToolUse`. Re-importing here keeps the symbol in scope
    // on every platform without producing a duplicate-import warning.
    use crate::spec::Event;
    use std::fs;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("noop", [] as [&str; 0])
            .rules(body)
            .build()
    }

    fn hook_spec(tag: &str, event: Event, command: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_shell_unchecked(command)
            .event(event)
            .build()
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

    #[cfg(not(windows))]
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

    #[cfg(windows)]
    #[test]
    fn plan_install_hook_refuses_on_windows() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = hook_spec("alpha", Event::PreToolUse, "noop");
        let plan = agent.plan_install(&scope, &spec).unwrap();
        assert_eq!(plan.status, crate::plan::PlanStatus::Refused);
        let refusal = plan
            .changes
            .iter()
            .find_map(|c| match c {
                PlannedChange::Refuse { reason, .. } => Some(*reason),
                _ => None,
            })
            .expect("plan should contain a refusal");
        assert!(matches!(refusal, RefusalReason::UnsupportedPlatform));
    }

    #[cfg(windows)]
    #[test]
    fn install_hook_returns_unsupported_platform_on_windows() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = hook_spec("alpha", Event::PreToolUse, "noop");
        let err = agent.install(&scope, &spec).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::UnsupportedPlatform { id: "cline", .. }
        ));
        assert!(!dir.path().join(".clinerules/hooks/PreToolUse").exists());
    }

    #[cfg(windows)]
    #[test]
    fn install_rules_only_still_works_on_windows() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        // Rules-only spec does not enter the script-writing branch, so it
        // must still install on Windows.
        agent
            .install(&scope, &rules_spec("alpha", "rule body"))
            .unwrap();
        assert!(dir.path().join(".clinerules/alpha.md").exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn install_hook_default_quotes_program_arguments() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program(
                "my hook",
                ["repo path", "semi;$(not run)", "`tick`", "quote's"],
            )
            .build();

        agent.install(&scope, &spec).unwrap();

        let body = fs::read_to_string(dir.path().join(".clinerules/hooks/PreToolUse")).unwrap();
        assert!(body.contains("\n'my hook' 'repo path' 'semi;$(not run)' '`tick`' 'quote'\\''s'\n"));
    }

    #[cfg(not(windows))]
    #[test]
    fn install_hook_with_custom_script_body() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::Custom("TaskStart".into()))
            .script(ScriptTemplate::Shell("echo started".into()))
            .build();
        agent.install(&scope, &s).unwrap();
        let p = dir.path().join(".clinerules/hooks/TaskStart");
        assert!(p.exists());
        let body = fs::read_to_string(&p).unwrap();
        assert!(body.contains("echo started"));
    }

    #[cfg(not(windows))]
    #[test]
    fn install_hook_rejects_unsafe_custom_event_filename() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());

        for bad in [
            "../TaskStart",
            "/tmp/TaskStart",
            "C:\\TaskStart",
            "Task.Start",
        ] {
            let spec = HookSpec::builder("alpha")
                .command_program("noop", [] as [&str; 0])
                .event(Event::Custom(bad.into()))
                .build();
            let err = agent.install(&scope, &spec).unwrap_err();
            assert!(
                matches!(err, AgentConfigError::InvalidTag { .. }),
                "expected invalid custom event for {bad:?}"
            );
        }

        assert!(!dir.path().join(".clinerules/hooks").exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn plan_hook_rejects_unsafe_custom_event_filename() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::Custom("../TaskStart".into()))
            .build();

        let err = agent.plan_install(&scope, &spec).unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[cfg(not(windows))]
    #[test]
    fn install_hook_records_ownership() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &hook_spec("myapp", Event::PreToolUse, "noop"))
            .unwrap();
        let ledger = dir
            .path()
            .join(".clinerules/hooks/.agent-config-hooks.json");
        assert!(ledger.exists());
        let v: serde_json::Value = serde_json::from_slice(&fs::read(&ledger).unwrap()).unwrap();
        assert_eq!(
            v["entries"]["PreToolUse"]["owner"],
            serde_json::json!("myapp")
        );
    }

    #[cfg(not(windows))]
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
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
        // appA's hook untouched.
        let body = fs::read_to_string(dir.path().join(".clinerules/hooks/PreToolUse")).unwrap();
        assert!(body.contains("a\n"));
    }

    #[cfg(not(windows))]
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

    #[cfg(not(windows))]
    #[test]
    fn install_typescript_script_rejected() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .script(ScriptTemplate::TypeScript("export {}".into()))
            .build();
        let err = agent.install(&scope, &s).unwrap_err();
        assert!(matches!(err, AgentConfigError::MissingSpecField { .. }));
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

    #[cfg(not(windows))]
    #[test]
    fn install_with_rules_and_script_creates_both() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = HookSpec::builder("alpha")
            .command_program("noop", [] as [&str; 0])
            .event(Event::PreToolUse)
            .rules("rules body")
            .script(ScriptTemplate::Shell("echo hi".into()))
            .build();
        agent.install(&scope, &s).unwrap();
        assert!(dir.path().join(".clinerules/alpha.md").exists());
        assert!(dir.path().join(".clinerules/hooks/PreToolUse").exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn uninstall_removes_rules_and_owned_hooks() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(
                &scope,
                &HookSpec::builder("alpha")
                    .command_program("noop", [] as [&str; 0])
                    .event(Event::PreToolUse)
                    .rules("body")
                    .build(),
            )
            .unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".clinerules").exists());
    }

    #[cfg(not(windows))]
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
    fn uninstall_rejects_unsafe_ledger_filename() {
        let dir = tempdir().unwrap();
        let agent = ClineAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let hooks_dir = dir.path().join(".clinerules/hooks");
        fs::create_dir_all(&hooks_dir).unwrap();
        fs::write(
            hooks_dir.join(".agent-config-hooks.json"),
            r#"{"entries":{"../escape":{"owner":"alpha"}}}"#,
        )
        .unwrap();
        let escaped = dir.path().join(".clinerules/escape");
        fs::write(&escaped, "do not remove").unwrap();

        let err = agent.uninstall(&scope, "alpha").unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
        assert!(escaped.exists());
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
        assert!(matches!(err, AgentConfigError::UnsupportedScope { .. }));
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

    #[test]
    fn mcp_supports_global_only() {
        let agent = ClineAgent::new();
        assert_eq!(agent.supported_mcp_scopes(), &[ScopeKind::Global]);

        let dir = tempdir().unwrap();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["@example/server"])
            .build();
        let err = agent.install_mcp(&scope, &spec).unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::UnsupportedScope {
                scope: ScopeKind::Local,
                ..
            }
        ));
    }
}
