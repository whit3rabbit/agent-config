//! Trae agent integration (ByteDance).
//!
//! Surfaces:
//!
//! 1. **Prompt rules**: project-local fenced block in `.trae/project_rules.md`.
//!    Trae also reads `.trae/user_rules.md`; this crate writes the
//!    project-scoped file.
//! 2. **Skills**: directory-scoped `SKILL.md` folders at `.trae/skills/<name>/`.
//!
//! MCP and hooks are not part of Trae's documented file-config surface.

use std::path::{Path, PathBuf};

use crate::agents::planning as agent_planning;
use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{file_lock, fs_atomic, md_block, ownership, skills_dir};

/// Trae agent installer.
pub struct TraeAgent;

impl TraeAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn require_local(scope: &Scope) -> Result<&Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "trae",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(Self::require_local(scope)?
            .join(".trae")
            .join("project_rules.md"))
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".trae").join("skills"),
            Scope::Local(p) => p.join(".trae").join("skills"),
        })
    }
}

impl Default for TraeAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for TraeAgent {
    fn id(&self) -> &'static str {
        "trae"
    }

    fn display_name(&self) -> &'static str {
        "Trae"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::rules_path(scope)?;
        StatusReport::for_markdown_block_hook(tag, path)
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
        agent_planning::markdown_install(
            Integration::id(self),
            scope,
            spec,
            Self::rules_path(scope),
            true,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
        agent_planning::markdown_uninstall(
            Integration::id(self),
            scope,
            tag,
            Self::rules_path(scope),
        )
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let rules = spec.rules.as_ref().ok_or(HookerError::MissingSpecField {
            id: "trae",
            field: "rules",
        })?;
        let path = Self::rules_path(scope)?;
        scope.ensure_contained(&path)?;
        let mut report = InstallReport::default();
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

impl SkillSurface for TraeAgent {
    fn id(&self) -> &'static str {
        "trae"
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
    use tempfile::tempdir;

    fn rules_spec(tag: &str, body: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(body).build()
    }

    fn skill(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("Test Trae skill.")
            .body("## Goal\nDo it.\n")
            .build()
    }

    #[test]
    fn install_writes_project_rules_md() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_spec("alpha", "Use Trae."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join(".trae/project_rules.md")).unwrap();
        assert!(body.contains("Use Trae."));
    }

    #[test]
    fn global_prompt_scope_rejected() {
        let agent = TraeAgent::new();
        let err = agent
            .install(&Scope::Global, &rules_spec("alpha", "x"))
            .unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &rules_spec("alpha", "x")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join(".trae/project_rules.md").exists());
    }

    #[test]
    fn install_skill_writes_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = TraeAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_skill(&scope, &skill("alpha-skill", "myapp"))
            .unwrap();
        assert!(dir
            .path()
            .join(".trae/skills/alpha-skill/SKILL.md")
            .exists());
    }
}
