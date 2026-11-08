//! Cline skills surface. Directory-scoped skills at the standard
//! `.agents/skills/<name>/` (Local) and `~/.agents/skills/<name>/` (Global).
//! Cline natively loads these locations (cline/cline#9074); legacy
//! `.cline/skills/` installs are still detected and removed.

use std::path::PathBuf;

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{InstallReport, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{ownership, skills_dir};

use super::ClineAgent;

impl ClineAgent {
    /// Skills root: `<root>/.agents/skills/` (Local) or `~/.agents/skills/`
    /// (Global).
    ///
    /// Cline moved its native default to the standard Agent Skills locations
    /// (`.agents/skills` / `~/.agents/skills`); `.cline/skills/` still loads
    /// but is legacy. The shared `~/.agents/skills` root is also read by
    /// Codex, Cursor, Pi, Copilot, and others — the per-root ownership
    /// ledger keeps cross-consumer installs safe.
    pub(super) fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::home_dir()?.join(".agents").join("skills"),
            Scope::Local(p) => p.join(".agents").join("skills"),
        })
    }

    /// Legacy skill root (`.cline/skills/`), still probed for status and
    /// uninstall so previously installed skills drain.
    fn legacy_skills_root(scope: &Scope) -> Option<PathBuf> {
        Some(match scope {
            Scope::Global => paths::home_dir().ok()?.join(".cline").join("skills"),
            Scope::Local(p) => p.join(".cline").join("skills"),
        })
    }

    /// Returns the root actually holding `name`: the standard root when the
    /// skill directory or its ledger entry exists there, else the legacy
    /// `.cline/skills` root, else the standard root for new installs.
    fn existing_skills_root(scope: &Scope, name: &str) -> Result<PathBuf, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        let (dir, _, ledger) = skills_dir::paths_for_status(&root, name);
        if dir.exists() || ownership::owner_of(&ledger, name)?.is_some() {
            return Ok(root);
        }

        if let Some(legacy) = Self::legacy_skills_root(scope) {
            let (dir, _, ledger) = skills_dir::paths_for_status(&legacy, name);
            if dir.exists() || ownership::owner_of(&ledger, name)?.is_some() {
                return Ok(legacy);
            }
        }

        Ok(root)
    }
}

impl SkillSurface for ClineAgent {
    fn id(&self) -> &'static str {
        "cline"
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
        if !dir.exists() && ownership::owner_of(&ledger, name)?.is_none() {
            if let Some(legacy) = Self::legacy_skills_root(scope) {
                let (legacy_dir, legacy_manifest, legacy_ledger) =
                    skills_dir::paths_for_status(&legacy, name);
                if legacy_dir.exists() || ownership::owner_of(&legacy_ledger, name)?.is_some() {
                    let recorded = ownership::owner_of(&legacy_ledger, name)?;
                    return Ok(StatusReport::for_skill(
                        name,
                        legacy_dir,
                        legacy_manifest,
                        legacy_ledger,
                        expected_owner,
                        recorded,
                    ));
                }
            }
        }
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
            Self::existing_skills_root(scope, name),
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
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = Self::existing_skills_root(scope, name)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}
