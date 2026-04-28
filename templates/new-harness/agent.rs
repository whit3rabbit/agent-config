//! <Display Name> integration.
//!
//! Replace `Myagent`/`myagent`/`MyAgent` throughout this file before adding it
//! to `src/agents/`. See `templates/new-harness/README.md` for the full guide.
//!
//! Surfaces this template implements (delete the blocks you do not need):
//!
//! - Hooks (always required): `<scope>/.myagent/settings.json`
//! - Prompt/rules (optional): `<scope>/.myagent/RULES.md` or `<scope>/RULES.md`
//! - MCP (optional, JSON shape): `<scope>/.myagent/mcp.json`
//! - Skills (optional): `<scope>/.myagent/skills/<name>/`
//! - Instructions (optional): standalone `<NAME>.md` files installed via
//!   `InstructionSurface` — InlineBlock by default for single-memory-file
//!   harnesses, StandaloneFile for per-tag rules-dir harnesses, or
//!   ReferencedFile when the harness documents an `@import` syntax.

use std::path::PathBuf;

use serde_json::json;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, json_patch, mcp_json_object, md_block, ownership,
    planning, safe_fs, skills_dir,
};

/// <Display Name> harness.
#[derive(Debug, Clone, Copy, Default)]
pub struct MyagentAgent {
    _private: (),
}

impl MyagentAgent {
    /// Construct an instance. The struct is stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Hooks config file.
    fn hooks_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("settings.json"),
            Scope::Local(p) => p.join(".myagent").join("settings.json"),
        })
    }

    /// Rules/memory markdown file. Delete this helper if your harness has no
    /// prompt-instructions surface.
    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("RULES.md"),
            Scope::Local(p) => p.join("RULES.md"),
        })
    }

    /// MCP config file. Delete if your harness has no file-backed MCP contract.
    fn mcp_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("mcp.json"),
            Scope::Local(p) => p.join(".myagent").join("mcp.json"),
        })
    }

    /// Skills directory. Delete if your harness has no skills concept.
    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("skills"),
            Scope::Local(p) => p.join(".myagent").join("skills"),
        })
    }

    /// Directory holding the instruction ownership ledger
    /// (`<dir>/.agent-config-instructions.json`). For `InlineBlock` placement
    /// this should sit next to (or inside a sibling of) the host file; for
    /// `StandaloneFile` it can be the same as the rules directory.
    /// Delete if your harness has no prompt or rules-dir surface.
    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent"),
            Scope::Local(p) => p.join(".myagent"),
        })
    }
}

// === REQUIRED: Integration (hooks + optional rules markdown) ===

impl Integration for MyagentAgent {
    fn id(&self) -> &'static str {
        "myagent"
    }

    fn display_name(&self) -> &'static str {
        "MyAgent"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    // The default `is_installed` impl folds `status` into a bool and is
    // sufficient for any agent that implements `status` correctly. Override
    // only if you want to skip the richer probe (rare).

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::hooks_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&p, &["hooks"], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, p, presence))
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let p = Self::hooks_path(scope)?;
        let mut changes = Vec::new();

        let event_key = event_to_string(&spec.event);
        let matcher_str = matcher_to_myagent(&spec.matcher);
        let entry = json!({
            "matcher": matcher_str,
            "hooks": [{ "type": "command", "command": spec.command.render_shell() }],
        });
        planning::plan_tagged_json_upsert(
            &mut changes,
            &p,
            &["hooks", event_key.as_str()],
            &spec.tag,
            entry,
            |_| {},
        )?;
        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        // Optional rules markdown: delete this `if let` block if your harness
        // has no rules/memory file.
        if let Some(rules) = &spec.rules {
            let rules_file = Self::rules_path(scope)?;
            planning::plan_markdown_upsert(&mut changes, &rules_file, &spec.tag, &rules.content)?;
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
        let p = Self::hooks_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
            &["hooks"],
            tag,
            planning::json_object_empty,
            true,
        )?;
        if has_refusal(&changes) {
            return Ok(UninstallPlan::from_changes(target, changes));
        }

        // Delete this rules cleanup if your harness has no rules file.
        let rules_file = Self::rules_path(scope)?;
        planning::plan_markdown_remove(&mut changes, &rules_file, tag)?;

        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_path(scope)?;
        // Symlink/path-traversal defense: for `Scope::Local`, refuse to mutate
        // any path whose parent canonicalizes outside the project root. No-op
        // for `Scope::Global`. Always call this BEFORE acquiring a lock or
        // touching the file.
        scope.ensure_contained(&p)?;
        // `with_lock` acquires a cross-process file lock for the closure body
        // and drops it on exit (success or error). Drop the guard before
        // locking a different file independently.
        file_lock::with_lock(&p, || {
            let mut root = json_patch::read_or_empty(&p)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_myagent(&spec.matcher);

            let entry = json!({
                "matcher": matcher_str,
                "hooks": [{ "type": "command", "command": spec.command.render_shell() }],
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

        // Optional rules-markdown injection. Delete this `if let` block (and
        // the `rules_path` helper above) if your harness has no rules file.
        if let Some(rules) = &spec.rules {
            let rules_file = Self::rules_path(scope)?;
            scope.ensure_contained(&rules_file)?;
            file_lock::with_lock(&rules_file, || {
                let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = safe_fs::write(scope, &rules_file, new_host.as_bytes(), true)?;
                if !outcome.no_change {
                    if outcome.existed {
                        report.patched.push(outcome.path.clone());
                    } else {
                        report.created.push(outcome.path.clone());
                    }
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
                    // `restore_backup_if_matches` only restores the `.bak` if
                    // the desired post-uninstall content matches what the
                    // backup holds. Stale backups (touched after install) are
                    // left in place rather than overwriting user changes.
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

        // Delete this rules-cleanup block if your harness has no rules file.
        let rules_file = Self::rules_path(scope)?;
        scope.ensure_contained(&rules_file)?;
        file_lock::with_lock(&rules_file, || {
            let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
            let (stripped, removed) = md_block::remove(&host, tag);
            if removed {
                if stripped.trim().is_empty() {
                    if safe_fs::restore_backup_if_matches(scope, &rules_file, stripped.as_bytes())? {
                        report.restored.push(rules_file.clone());
                    } else {
                        safe_fs::remove_file(scope, &rules_file)?;
                        report.removed.push(rules_file.clone());
                    }
                } else {
                    safe_fs::write(scope, &rules_file, stripped.as_bytes(), false)?;
                    report.patched.push(rules_file.clone());
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

// === OPTIONAL: McpSurface (JSON `{"mcpServers": {...}}` shape) ===
//
// Delete this whole `impl` block (and the `mcp_path` helper plus the
// `mcp_json_object` import) if your harness has no file-backed MCP contract.
//
// For TOML-shaped MCP (e.g., Codex's `[mcp_servers.<name>]`), see
// `src/agents/codex.rs`. For object-map shapes under arbitrary keys, see
// `src/agents/opencode.rs` and `src/agents/copilot.rs`. For YAML maps see
// `src/agents/hermes.rs`.
//
// The `mcp_json_object` helpers internally compute and record a SHA-256
// content hash in the sidecar ledger (schema v2). Custom MCP impls (TOML,
// JSONC, etc.) need to compute the hash explicitly via
// `ownership::content_hash(...)` and pass it to `ownership::record_install`.

impl McpSurface for MyagentAgent {
    fn id(&self) -> &'static str {
        "myagent"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    // Default `is_mcp_installed` folds `mcp_status` into a bool. Override
    // only if you want to bypass the status probe.

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence = mcp_json_object::config_presence(&cfg, name)?;
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

    fn plan_install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::mcp_json_object_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::mcp_path(scope),
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::mcp_json_object_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
        )
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::install(&cfg, &ledger, spec)
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
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

// === OPTIONAL: SkillSurface ===
//
// Delete this whole `impl` block (and the `skills_root` helper plus the
// `skills_dir` import) if your harness has no skills concept. The thin
// implementation below works for any harness whose skills are directory-
// scoped under a `skills/` root with `SKILL.md` plus optional
// `scripts/`, `references/`, `assets/` subdirs.

impl SkillSurface for MyagentAgent {
    fn id(&self) -> &'static str {
        "myagent"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    // Default `is_skill_installed` folds `skill_status` into a bool.

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

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
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
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

// === OPTIONAL: InstructionSurface (standalone instruction files) ===
//
// Delete this whole block + the `instruction_config_dir` helper above + the
// imports of `InstructionSurface`, `InstructionSpec`, and `instructions_dir`
// if your harness has no documented prompt or rules-dir surface.
//
// This template shows the **InlineBlock** shape via the `inline_*` shim
// helpers in `instructions_dir`. The agent only supplies an `InlineLayout`
// (the per-scope paths); the shim handles validation, scope containment,
// status detection, and `UnsupportedScope -> RefusalReason::UnsupportedScope`
// conversion in plan methods.
//
// For other placements:
//
// - **StandaloneFile** (per-tag rules directory like `.clinerules/`):
//   provide a `StandaloneLayout { config_dir, instruction_dir }` and call the
//   `standalone_*` shims. See `src/agents/cline.rs` or `src/agents/roo.rs`.
// - **ReferencedFile** (Claude only — writes `<NAME>.md` separately and
//   injects `@<NAME>.md` into a host file): no shim exists; call
//   `instructions_dir::{install, uninstall, plan_install, plan_uninstall}`
//   directly. See `src/agents/claude.rs`.

impl MyagentAgent {
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

impl InstructionSurface for MyagentAgent {
    fn id(&self) -> &'static str {
        "myagent"
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

/// Map our generic [`Matcher`] to the harness's matcher syntax. Codex maps
/// `Bash` → `"shell"`; Claude maps `Bash` → `"Bash"`. Pick whichever string
/// your harness expects.
fn matcher_to_myagent(m: &Matcher) -> String {
    match m {
        Matcher::All => "*".to_string(),
        Matcher::Bash => "Bash".to_string(),
        Matcher::Exact(s) => s.clone(),
        Matcher::AnyOf(names) => names.join("|"),
        Matcher::Regex(s) => s.clone(),
    }
}

/// Map our generic [`Event`] to the harness's event-name syntax.
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
    use serde_json::Value;
    use tempfile::tempdir;

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("myapp", ["hook"])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    fn read_json(p: &std::path::Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_writes_settings_with_expected_shape() {
        let dir = tempdir().unwrap();
        let agent = MyagentAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_json(&dir.path().join(".myagent/settings.json"));
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("Bash"));
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["_agent_config_tag"],
            json!("alpha")
        );
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempdir().unwrap();
        let agent = MyagentAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = MyagentAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        assert!(agent.is_installed(&scope, "alpha").unwrap());
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
    }

    #[test]
    fn plan_install_then_install_matches() {
        // Plans must be side-effect-free: the planner should not create the
        // config file. Then the actual install should succeed and the planned
        // change set should describe what happened.
        let dir = tempdir().unwrap();
        let agent = MyagentAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");

        let plan = agent.plan_install(&scope, &spec).unwrap();
        assert!(!dir.path().join(".myagent/settings.json").exists());
        assert!(!plan.changes.is_empty());

        agent.install(&scope, &spec).unwrap();
        assert!(dir.path().join(".myagent/settings.json").exists());
    }
}
