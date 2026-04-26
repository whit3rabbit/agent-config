//! Qwen Code integration.
//!
//! Qwen Code is Alibaba's terminal coding agent. It is a Gemini-CLI fork, so
//! the file layout mirrors Gemini's: a `settings.json` that holds the
//! `mcpServers` map, a `skills/` directory with `SKILL.md` folders, and a
//! `QWEN.md` memory file.
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: fenced HTML-comment block in `QWEN.md`.
//! 2. **MCP servers**: `mcpServers` JSON map in `settings.json`.
//! 3. **Skills**: directory-scoped `SKILL.md` folders.
//!
//! Hooks are not part of Qwen's documented surface, so [`Integration`] only
//! wires the prompt-rules path and refuses installs that lack
//! [`HookSpec::rules`].

use std::path::{Path, PathBuf};

use crate::agents::planning as agent_planning;
use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{file_lock, fs_atomic, mcp_json_object, md_block, ownership, skills_dir};

/// Qwen Code installer.
pub struct QwenAgent;

impl QwenAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn qwen_home_from_home(home: &Path) -> PathBuf {
        home.join(".qwen")
    }

    fn settings_path_from_home(home: &Path) -> PathBuf {
        Self::qwen_home_from_home(home).join("settings.json")
    }

    fn memory_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => Self::qwen_home_from_home(&paths::home_dir()?).join("QWEN.md"),
            Scope::Local(p) => p.join("QWEN.md"),
        })
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => Self::settings_path_from_home(&paths::home_dir()?),
            Scope::Local(p) => p.join(".qwen").join("settings.json"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => Self::qwen_home_from_home(&paths::home_dir()?).join("skills"),
            Scope::Local(p) => p.join(".qwen").join("skills"),
        })
    }
}

impl Default for QwenAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for QwenAgent {
    fn id(&self) -> &'static str {
        "qwen"
    }

    fn display_name(&self) -> &'static str {
        "Qwen Code"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::memory_path(scope)?;
        StatusReport::for_markdown_block_hook(tag, path)
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
        agent_planning::markdown_install(
            Integration::id(self),
            scope,
            spec,
            Self::memory_path(scope),
            true,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
        agent_planning::markdown_uninstall(
            Integration::id(self),
            scope,
            tag,
            Self::memory_path(scope),
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let rules = spec.rules.as_ref().ok_or(HookerError::MissingSpecField {
            id: "qwen",
            field: "rules",
        })?;
        let path = Self::memory_path(scope)?;
        let mut report = InstallReport::default();
        scope.ensure_contained(&path)?;
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
            let outcome = fs_atomic::write_atomic(&path, new_host.as_bytes(), true)?;
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
            Ok::<(), HookerError>(())
        })?;
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::memory_path(scope)?;
        let mut report = UninstallReport::default();
        scope.ensure_contained(&path)?;
        file_lock::with_lock(&path, || {
            let host = fs_atomic::read_to_string_or_empty(&path)?;
            let (stripped, removed) = md_block::remove(&host, tag);

            if !removed {
                report.not_installed = true;
                return Ok(());
            }

            if stripped.trim().is_empty() {
                if fs_atomic::restore_backup_if_matches(&path, stripped.as_bytes())? {
                    report.restored.push(path.clone());
                } else {
                    fs_atomic::remove_if_exists(&path)?;
                    report.removed.push(path.clone());
                }
            } else {
                fs_atomic::write_atomic(&path, stripped.as_bytes(), false)?;
                report.patched.push(path.clone());
            }
            Ok::<(), HookerError>(())
        })?;
        Ok(report)
    }
}

impl McpSurface for QwenAgent {
    fn id(&self) -> &'static str {
        "qwen"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

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
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::mcp_json_object_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
        )
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
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
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_object::uninstall(&cfg, &ledger, name, owner_tag, "mcp server")
    }
}

impl SkillSurface for QwenAgent {
    fn id(&self) -> &'static str {
        "qwen"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

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
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(body).build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    fn skill(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Test Qwen skill.")
            .body("## Goal\nDo it.\n")
            .build()
    }

    fn read_json(p: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn install_writes_qwen_md_block() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use Qwen rules."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("QWEN.md")).unwrap();
        assert!(body.contains("Use Qwen rules."));
        assert!(body.contains("AI-HOOKER:alpha"));
    }

    #[test]
    fn rules_install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "rules");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "rules"))
            .unwrap();
        assert!(agent.is_installed(&scope, "alpha").unwrap());
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!agent.is_installed(&scope, "alpha").unwrap());
    }

    #[test]
    fn install_requires_rules() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha").command("noop").build();
        let err = agent.install(&scope, &spec).unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { .. }));
    }

    #[test]
    fn install_mcp_writes_settings_json() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_json(&dir.path().join(".qwen/settings.json"));
        assert_eq!(v["mcpServers"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn install_mcp_idempotent() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = mcp_spec("github", "myapp");
        agent.install_mcp(&scope, &s).unwrap();
        let r = agent.install_mcp(&scope, &s).unwrap();
        assert!(r.already_installed);
    }

    #[test]
    fn install_skill_writes_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha-skill", "myapp"))
            .unwrap();
        assert!(dir
            .path()
            .join(".qwen/skills/alpha-skill/SKILL.md")
            .exists());
    }

    #[test]
    fn skill_owner_mismatch_refused() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha-skill", "app-a"))
            .unwrap();
        let err = agent
            .install_skill(&scope, &skill("alpha-skill", "app-b"))
            .unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn plan_install_does_not_write() {
        let dir = tempdir().unwrap();
        let agent = QwenAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "rules");
        let _plan = agent.plan_install(&scope, &spec).unwrap();
        assert!(!dir.path().join("QWEN.md").exists());
    }
}
