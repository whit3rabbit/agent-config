//! Codex CLI integration (OpenAI's official Codex CLI).
//!
//! Hook surface: `<scope>/.codex/hooks.json` using PascalCase event names
//! (`PreToolUse`/`PostToolUse`), with tool-name matchers such as `Bash`.
//! JSON shape mirrors Gemini's:
//!
//! ```json
//! {
//!   "hooks": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "pattern",
//!         "hooks": [{ "type": "command", "command": "...", "timeout": 600 }],
//!         "_agent_config_tag": "myapp"
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Optional prompt surface: `$CODEX_HOME/AGENTS.md` (Global, default
//! `~/.codex/AGENTS.md`) or `<project>/AGENTS.md` (Local). Codex does not yet
//! support `@import`, so we inject content directly via fenced block.
//!
//! MCP surface (TOML at `config.toml`) lives in `mcp.rs`.

use std::path::PathBuf;

use serde_json::json;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, PlannedChange, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, Matcher, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, json_patch, md_block, ownership, planning, safe_fs,
    skills_dir, toml_patch,
};

mod mcp;

/// Codex CLI.
#[derive(Debug, Clone, Copy, Default)]
pub struct CodexAgent {
    _private: (),
}

impl CodexAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    fn hooks_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("hooks.json"),
            Scope::Local(p) => p.join(".codex").join("hooks.json"),
        })
    }

    fn agents_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?.join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".agents").join("skills"),
            Scope::Local(p) => p.join(".agents").join("skills"),
        })
    }

    /// Directory holding the instruction ownership ledger.
    /// Global: `$CODEX_HOME` (defaults to `~/.codex`). Local: `<root>/.codex`.
    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::codex_home()?,
            Scope::Local(p) => p.join(".codex"),
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

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let hooks_path = Self::hooks_path(scope)?;
        let config_path = Self::config_toml_path(scope)?;

        let presence_json = json_patch::tagged_hook_presence(&hooks_path, &["hooks"], tag)?;

        let presence_toml = if config_path.exists() {
            let doc = toml_patch::read_or_empty(&config_path)?;
            if toml_patch::contains_named_table(&doc, &["hooks"], tag) {
                crate::status::ConfigPresence::Single
            } else {
                crate::status::ConfigPresence::Absent
            }
        } else {
            crate::status::ConfigPresence::Absent
        };

        let json_installed = matches!(
            presence_json,
            crate::status::ConfigPresence::Single | crate::status::ConfigPresence::Duplicate { .. }
        );
        let toml_installed = matches!(presence_toml, crate::status::ConfigPresence::Single);

        let mut report = if json_installed {
            StatusReport::for_tagged_hook(tag, hooks_path.clone(), presence_json)
        } else if toml_installed {
            let target = crate::status::PlanTarget::Hook {
                tag: tag.to_string(),
            };
            let files = vec![crate::status::PathStatus::Exists {
                path: config_path.clone(),
            }];
            crate::status::StatusReport {
                target,
                status: crate::status::InstallStatus::InstalledOwned {
                    owner: tag.to_string(),
                },
                config_path: Some(config_path.clone()),
                ledger_path: None,
                files,
                warnings: Vec::new(),
            }
        } else {
            StatusReport::for_tagged_hook(tag, hooks_path.clone(), presence_json)
        };

        if config_path.exists()
            && !report.files.iter().any(
                |f| matches!(f, crate::status::PathStatus::Exists { path } if path == &config_path),
            )
        {
            report.files.push(crate::status::PathStatus::Exists {
                path: config_path.clone(),
            });
        }
        if hooks_path.exists()
            && !report.files.iter().any(
                |f| matches!(f, crate::status::PathStatus::Exists { path } if path == &hooks_path),
            )
        {
            report.files.push(crate::status::PathStatus::Exists {
                path: hooks_path.clone(),
            });
        }

        if json_installed && toml_installed {
            report
                .warnings
                .push(crate::status::StatusWarning::DualHooksExist {
                    hooks_json: hooks_path,
                    config_toml: config_path,
                });
        }

        Ok(report)
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

        if validate_codex_event(&spec.event, &spec.matcher).is_err() {
            return Ok(InstallPlan::refused(
                target,
                None,
                crate::plan::RefusalReason::UnsupportedSpecField,
            ));
        }

        let hooks_path = Self::hooks_path(scope)?;
        let config_path = Self::config_toml_path(scope)?;
        let inline_toml = spec.options.codex_inline_toml.unwrap_or(false);

        let mut changes = Vec::new();

        if inline_toml {
            let mut doc = match toml_patch::read_or_empty(&config_path) {
                Ok(doc) => doc,
                Err(AgentConfigError::TomlInvalid { .. }) => {
                    changes.push(PlannedChange::Refuse {
                        path: Some(config_path.clone()),
                        reason: crate::plan::RefusalReason::InvalidConfig,
                    });
                    return Ok(InstallPlan::from_changes(target, changes));
                }
                Err(e) => return Err(e),
            };
            let table = build_toml_hook_table(spec);
            let changed = toml_patch::upsert_named_table(&mut doc, &["hooks"], &spec.tag, table)?;
            if changed {
                let bytes = toml_patch::to_string(&doc);
                planning::plan_write_file(&mut changes, &config_path, &bytes, true)?;
            }
        } else {
            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_codex(&spec.matcher);
            let command_str = if cfg!(windows) {
                spec.options
                    .windows_command
                    .as_ref()
                    .unwrap_or(&spec.command)
                    .render_shell()
            } else {
                spec.command.render_shell()
            };

            let mut hook_obj = serde_json::Map::new();
            hook_obj.insert("type".to_string(), json!("command"));
            hook_obj.insert("command".to_string(), json!(command_str));
            if let Some(t) = spec.options.timeout_seconds {
                hook_obj.insert("timeout".to_string(), json!(t));
            }
            if let Some(msg) = &spec.options.status_message {
                hook_obj.insert("statusMessage".to_string(), json!(msg));
            }

            let entry = json!({
                "matcher": matcher_str,
                "hooks": vec![serde_json::Value::Object(hook_obj)],
            });
            planning::plan_tagged_json_upsert(
                &mut changes,
                &hooks_path,
                &["hooks", event_key.as_str()],
                &spec.tag,
                entry,
                |_| {},
            )?;
        }

        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        if let Some(rules) = &spec.rules {
            let agents = Self::agents_path(scope)?;
            planning::plan_markdown_upsert(&mut changes, &agents, &spec.tag, &rules.content)?;
        }

        let mut plan = InstallPlan::from_changes(target, changes);

        let has_json_hook = if hooks_path.exists() {
            json_patch::tagged_hook_presence(&hooks_path, &["hooks"], &spec.tag)
                .map(|presence| {
                    matches!(
                        presence,
                        crate::status::ConfigPresence::Single
                            | crate::status::ConfigPresence::Duplicate { .. }
                    )
                })
                .unwrap_or(false)
        } else {
            false
        };
        let has_toml_hook = if config_path.exists() {
            if let Ok(doc) = toml_patch::read_or_empty(&config_path) {
                toml_patch::contains_named_table(&doc, &["hooks"], &spec.tag)
            } else {
                false
            }
        } else {
            false
        };

        if inline_toml && has_json_hook {
            plan.warnings.push(crate::plan::PlanWarning {
                path: Some(hooks_path),
                message: "Legacy hooks.json hook still exists, which may coexist or conflict with config.toml hooks".to_string(),
            });
        } else if !inline_toml && has_toml_hook {
            plan.warnings.push(crate::plan::PlanWarning {
                path: Some(config_path),
                message: "Inline config.toml hook still exists, which may coexist or conflict with hooks.json hooks".to_string(),
            });
        }

        Ok(plan)
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let mut changes = Vec::new();
        let hooks_path = Self::hooks_path(scope)?;
        let config_path = Self::config_toml_path(scope)?;

        planning::plan_tagged_json_remove_under(
            &mut changes,
            &hooks_path,
            &["hooks"],
            tag,
            planning::json_object_empty,
            true,
        )?;

        if config_path.exists() {
            let mut doc = match toml_patch::read_or_empty(&config_path) {
                Ok(doc) => doc,
                Err(AgentConfigError::TomlInvalid { .. }) => {
                    changes.push(PlannedChange::Refuse {
                        path: Some(config_path.clone()),
                        reason: crate::plan::RefusalReason::InvalidConfig,
                    });
                    return Ok(UninstallPlan::from_changes(target, changes));
                }
                Err(e) => return Err(e),
            };
            if toml_patch::contains_named_table(&doc, &["hooks"], tag) {
                let removed = toml_patch::remove_named_table(&mut doc, &["hooks"], tag)?;
                if removed {
                    let empty = doc.as_table().is_empty();
                    let bytes = toml_patch::to_string(&doc);
                    if empty {
                        planning::plan_restore_backup_or_remove(
                            &mut changes,
                            &config_path,
                            &bytes,
                        )?;
                    } else {
                        planning::plan_write_file(&mut changes, &config_path, &bytes, false)?;
                    }
                }
            }
        }

        if has_refusal(&changes) {
            return Ok(UninstallPlan::from_changes(target, changes));
        }

        let agents = Self::agents_path(scope)?;
        planning::plan_markdown_remove(&mut changes, &agents, tag)?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        validate_codex_event(&spec.event, &spec.matcher)?;
        let mut report = InstallReport::default();

        let inline_toml = spec.options.codex_inline_toml.unwrap_or(false);
        if inline_toml {
            let p = Self::config_toml_path(scope)?;
            scope.ensure_contained(&p)?;
            file_lock::with_lock(&p, || {
                let mut doc = toml_patch::read_or_empty(&p)?;
                let table = build_toml_hook_table(spec);
                let changed =
                    toml_patch::upsert_named_table(&mut doc, &["hooks"], &spec.tag, table)?;
                if changed {
                    let bytes = toml_patch::to_string(&doc);
                    let outcome = safe_fs::write(scope, &p, &bytes, true)?;
                    if outcome.existed && !outcome.no_change {
                        report.patched.push(outcome.path.clone());
                    } else if !outcome.existed {
                        report.created.push(outcome.path.clone());
                    }
                    if let Some(b) = outcome.backup {
                        report.backed_up.push(b);
                    }
                } else {
                    report.already_installed = true;
                }
                Ok::<(), AgentConfigError>(())
            })?;
        } else {
            let p = Self::hooks_path(scope)?;
            scope.ensure_contained(&p)?;
            file_lock::with_lock(&p, || {
                let mut root = json_patch::read_or_empty(&p)?;

                let event_key = event_to_string(&spec.event);
                let matcher_str = matcher_to_codex(&spec.matcher);
                let command_str = if cfg!(windows) {
                    spec.options
                        .windows_command
                        .as_ref()
                        .unwrap_or(&spec.command)
                        .render_shell()
                } else {
                    spec.command.render_shell()
                };

                let mut hook_obj = serde_json::Map::new();
                hook_obj.insert("type".to_string(), json!("command"));
                hook_obj.insert("command".to_string(), json!(command_str));
                if let Some(t) = spec.options.timeout_seconds {
                    hook_obj.insert("timeout".to_string(), json!(t));
                }
                if let Some(msg) = &spec.options.status_message {
                    hook_obj.insert("statusMessage".to_string(), json!(msg));
                }

                let entry = json!({
                    "matcher": matcher_str,
                    "hooks": vec![serde_json::Value::Object(hook_obj)],
                });

                let changed = json_patch::upsert_tagged_array_entry(
                    &mut root,
                    &["hooks", &event_key],
                    &spec.tag,
                    entry,
                )?;

                if changed {
                    let bytes = json_patch::to_pretty(&root);
                    let outcome = safe_fs::write(scope, &p, &bytes, true)?;
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
                Ok::<(), AgentConfigError>(())
            })?;
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

        let p = Self::hooks_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut root = json_patch::read_or_empty(&p)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
                if changed {
                    let empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if empty && safe_fs::restore_backup_if_matches(scope, &p, &bytes)? {
                        report.restored.push(p.clone());
                    } else if empty {
                        safe_fs::remove_file(scope, &p)?;
                        report.removed.push(p.clone());
                    } else {
                        safe_fs::write(scope, &p, &bytes, false)?;
                        report.patched.push(p.clone());
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        let config_path = Self::config_toml_path(scope)?;
        if config_path.exists() {
            scope.ensure_contained(&config_path)?;
            file_lock::with_lock(&config_path, || {
                let mut doc = toml_patch::read_or_empty(&config_path)?;
                if toml_patch::contains_named_table(&doc, &["hooks"], tag) {
                    let removed = toml_patch::remove_named_table(&mut doc, &["hooks"], tag)?;
                    if removed {
                        let empty = doc.as_table().is_empty();
                        let bytes = toml_patch::to_string(&doc);
                        if empty && safe_fs::restore_backup_if_matches(scope, &config_path, &bytes)?
                        {
                            report.restored.push(config_path.clone());
                        } else if empty {
                            safe_fs::remove_file(scope, &config_path)?;
                            report.removed.push(config_path.clone());
                        } else {
                            safe_fs::write(scope, &config_path, &bytes, false)?;
                            report.patched.push(config_path.clone());
                        }
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
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
                    } else {
                        safe_fs::remove_file(scope, &agents)?;
                        report.removed.push(agents.clone());
                    }
                } else {
                    safe_fs::write(scope, &agents, stripped.as_bytes(), false)?;
                    report.patched.push(agents.clone());
                }
            }
            Ok::<(), AgentConfigError>(())
        })?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl SkillSurface for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
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

impl InstructionSurface for CodexAgent {
    fn id(&self) -> &'static str {
        "codex"
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

fn matcher_to_codex(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "Bash".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "PreToolUse".into(),
        Event::PostToolUse => "PostToolUse".into(),
        Event::PermissionRequest => "PermissionRequest".into(),
        Event::PreCompact => "PreCompact".into(),
        Event::SessionStart => "SessionStart".into(),
        Event::SubagentStop => "SubagentStop".into(),
        Event::UserPromptSubmit => "UserPromptSubmit".into(),
        Event::Stop => "Stop".into(),
        Event::Custom(s) => s.clone(),
        other => other.as_str().into(),
    }
}

fn validate_codex_event(event: &Event, matcher: &Matcher) -> Result<(), AgentConfigError> {
    let s = match event {
        Event::PreToolUse => "PreToolUse",
        Event::PostToolUse => "PostToolUse",
        Event::PermissionRequest => "PermissionRequest",
        Event::PreCompact => "PreCompact",
        Event::SessionStart => "SessionStart",
        Event::SubagentStop => "SubagentStop",
        Event::UserPromptSubmit => "UserPromptSubmit",
        Event::Stop => "Stop",
        Event::Custom(c) => c.as_str(),
        other => other.as_str(),
    };
    let allowed = [
        "PreToolUse",
        "PostToolUse",
        "PermissionRequest",
        "PreCompact",
        "SessionStart",
        "SubagentStop",
        "UserPromptSubmit",
        "Stop",
    ];
    let is_valid = allowed.contains(&s) || matches!(event, Event::Custom(_));
    if !is_valid {
        return Err(AgentConfigError::UnsupportedSpecField {
            id: "codex",
            field: "event",
            value: format!("{:?}", event),
        });
    }

    if !matches!(matcher, Matcher::All)
        && s != "PreToolUse"
        && s != "PostToolUse"
        && s != "PermissionRequest"
    {
        return Err(AgentConfigError::UnsupportedSpecField {
            id: "codex",
            field: "matcher",
            value: format!("{:?}", matcher),
        });
    }
    Ok(())
}

fn build_toml_hook_table(spec: &HookSpec) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    let command_str = if cfg!(windows) {
        spec.options
            .windows_command
            .as_ref()
            .unwrap_or(&spec.command)
            .render_shell()
    } else {
        spec.command.render_shell()
    };
    table["event"] = toml_edit::value(event_to_string(&spec.event));
    table["matcher"] = toml_edit::value(matcher_to_codex(&spec.matcher));
    table["command"] = toml_edit::value(command_str);
    if let Some(t) = spec.options.timeout_seconds {
        table["timeout"] = toml_edit::value(t as i64);
    }
    if let Some(msg) = &spec.options.status_message {
        table["statusMessage"] = toml_edit::value(msg.clone());
    }
    table
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
            .command_program("myapp", ["hook"])
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
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("Bash"));
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
        assert_eq!(matcher_to_codex(&Matcher::Bash), "Bash");
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
            .command_program("noop", [] as [&str; 0])
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
            .command_program("noop", [] as [&str; 0])
            .rules("Use strict mode.")
            .build();
        agent.install(&scope, &spec).unwrap();
        let md = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(md.contains("Use strict mode."));
        assert!(md.contains("AGENT-CONFIG:alpha"));
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
}
