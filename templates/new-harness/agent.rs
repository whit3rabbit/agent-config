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

use std::path::PathBuf;

use serde_json::json;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, json_patch, mcp_json_object, md_block, ownership, planning, skills_dir,
};

/// <Display Name> harness.
pub struct MyagentAgent;

impl MyagentAgent {
    /// Construct an instance. The struct is stateless.
    pub const fn new() -> Self {
        Self
    }

    /// Hooks config file.
    fn hooks_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("settings.json"),
            Scope::Local(p) => p.join(".myagent").join("settings.json"),
        })
    }

    /// Rules/memory markdown file. Delete this helper if your harness has no
    /// prompt-instructions surface.
    fn rules_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("RULES.md"),
            Scope::Local(p) => p.join("RULES.md"),
        })
    }

    /// MCP config file. Delete if your harness has no file-backed MCP contract.
    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("mcp.json"),
            Scope::Local(p) => p.join(".myagent").join("mcp.json"),
        })
    }

    /// Skills directory. Delete if your harness has no skills concept.
    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".myagent").join("skills"),
            Scope::Local(p) => p.join(".myagent").join("skills"),
        })
    }
}

impl Default for MyagentAgent {
    fn default() -> Self {
        Self::new()
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

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::hooks_path(scope)?;
        let presence = json_patch::tagged_hook_presence(&p, &["hooks"], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, p, presence))
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
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
            "hooks": [{ "type": "command", "command": spec.command }],
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

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
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

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::hooks_path(scope)?;
        {
            // Cross-process file lock prevents concurrent installs from
            // racing on the same file. Drop the guard before touching any
            // other file you also want to lock independently.
            let _hooks_lock = file_lock::FileLock::acquire(&p)?;
            let mut root = json_patch::read_or_empty(&p)?;

            let event_key = event_to_string(&spec.event);
            let matcher_str = matcher_to_myagent(&spec.matcher);

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
        }

        // Optional rules-markdown injection. Delete this `if let` block (and
        // the `rules_path` helper above) if your harness has no rules file.
        if let Some(rules) = &spec.rules {
            let rules_file = Self::rules_path(scope)?;
            let _rules_lock = file_lock::FileLock::acquire(&rules_file)?;
            let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = fs_atomic::write_atomic(&rules_file, new_host.as_bytes(), true)?;
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
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::hooks_path(scope)?;
        {
            let _hooks_lock = file_lock::FileLock::acquire(&p)?;
            if p.exists() {
                let mut root = json_patch::read_or_empty(&p)?;
                let changed =
                    json_patch::remove_tagged_array_entries_under(&mut root, &["hooks"], tag)?;
                if changed {
                    let empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if empty && fs_atomic::restore_backup_if_matches(&p, &bytes)? {
                        report.restored.push(p.clone());
                    } else if empty {
                        fs_atomic::remove_if_exists(&p)?;
                        report.removed.push(p.clone());
                    } else {
                        fs_atomic::write_atomic(&p, &bytes, false)?;
                        report.patched.push(p.clone());
                    }
                }
            }
        }

        // Delete this rules-cleanup block if your harness has no rules file.
        let rules_file = Self::rules_path(scope)?;
        let _rules_lock = file_lock::FileLock::acquire(&rules_file)?;
        let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
        let (stripped, removed) = md_block::remove(&host, tag);
        if removed {
            if stripped.trim().is_empty() {
                if fs_atomic::restore_backup_if_matches(&rules_file, stripped.as_bytes())? {
                    report.restored.push(rules_file.clone());
                } else {
                    fs_atomic::remove_if_exists(&rules_file)?;
                    report.removed.push(rules_file.clone());
                }
            } else {
                fs_atomic::write_atomic(&rules_file, stripped.as_bytes(), false)?;
                report.patched.push(rules_file.clone());
            }
        }

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
    ) -> Result<StatusReport, HookerError> {
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

    fn plan_install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallPlan, HookerError> {
        spec.validate()?;
        let target = PlanTarget::Mcp {
            integration_id: McpSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let changes = mcp_json_object::plan_install(&cfg, &ledger, spec)?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let target = PlanTarget::Mcp {
            integration_id: McpSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let changes =
            mcp_json_object::plan_uninstall(&cfg, &ledger, name, owner_tag, "mcp server")?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
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
        let cfg = Self::mcp_path(scope)?;
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
    ) -> Result<StatusReport, HookerError> {
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
    ) -> Result<InstallPlan, HookerError> {
        spec.validate()?;
        let target = PlanTarget::Skill {
            integration_id: SkillSurface::id(self),
            scope: scope.clone(),
            name: spec.name.clone(),
            owner: spec.owner_tag.clone(),
        };
        let root = Self::skills_root(scope)?;
        let changes = skills_dir::plan_install(&root, spec)?;
        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, HookerError> {
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let target = PlanTarget::Skill {
            integration_id: SkillSurface::id(self),
            scope: scope.clone(),
            name: name.to_string(),
            owner: owner_tag.to_string(),
        };
        let root = Self::skills_root(scope)?;
        let changes = skills_dir::plan_uninstall(&root, name, owner_tag)?;
        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let root = Self::skills_root(scope)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = Self::skills_root(scope)?;
        skills_dir::uninstall(&root, name, owner_tag)
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
            .command("myapp hook")
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
            v["hooks"]["PreToolUse"][0]["_ai_hooker_tag"],
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
