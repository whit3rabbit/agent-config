//! The contract every AI-harness integration implements.
//!
//! There are several distinct **surfaces** an integration may expose:
//!
//! - [`Integration`]: the hooks surface.
//! - [`McpSurface`]: MCP server registration.
//! - [`SkillSurface`]: skill installation.
//! - [`InstructionSurface`]: standalone instruction files.
//!
//! Each surface is its own trait so callers cannot accidentally call
//! `install_mcp` on a harness that does not support MCP, the type system
//! forbids it. Use [`crate::registry::mcp_capable`] to enumerate the agents
//! that implement `McpSurface`.

use std::path::PathBuf;

use crate::error::AgentConfigError;
use crate::plan::{InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, InstructionSpec, McpSpec, SkillSpec};
use crate::status::{InstallStatus, StatusReport};
use crate::validation::ValidationReport;

/// One AI harness's hook installer.
///
/// The trait is intentionally narrow so adding a new harness is a small,
/// mechanical exercise: implement `Integration`, then register it in
/// [`crate::registry::all`].
///
/// All operations must be idempotent: calling `install` twice with the same
/// spec, or `uninstall` twice with the same tag, must produce the same end
/// state as calling once.
pub trait Integration: Send + Sync {
    /// Stable, kebab-case identifier (e.g., `"claude"`, `"cursor"`).
    fn id(&self) -> &'static str;

    /// Human-readable name (e.g., `"Claude Code"`).
    fn display_name(&self) -> &'static str;

    /// Which scopes this integration accepts.
    fn supported_scopes(&self) -> &'static [ScopeKind];

    /// Returns true if a hook with this `tag` is currently installed in this
    /// scope. Used by CLI consumers to render install/uninstall state.
    ///
    /// Default impl matches on the richer [`status`](Integration::status)
    /// result and treats [`InstallStatus::InstalledOwned`] and
    /// [`InstallStatus::InstalledOtherOwner`] as installed; agents that have
    /// already implemented `status` get this for free.
    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, AgentConfigError> {
        Ok(matches!(
            self.status(scope, tag)?.status,
            InstallStatus::InstalledOwned { .. } | InstallStatus::InstalledOtherOwner { .. }
        ))
    }

    /// Detailed installation state for the hook identified by `tag`.
    ///
    /// Distinguishes installed-by-us from installed-by-someone-else, surfaces
    /// drift (parse failures, duplicate entries), and reports any pending
    /// `.bak` files. See [`StatusReport`] for the full shape.
    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError>;

    /// Validate hook state without mutating user files.
    ///
    /// Unlike [`status`](Integration::status), this reports whether the
    /// discovered state is internally consistent and safe to repair.
    fn validate(&self, scope: &Scope, tag: &str) -> Result<ValidationReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: self.id(),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let status = match self.status(scope, tag) {
            Ok(status) => status,
            Err(AgentConfigError::JsonInvalid { path, source }) => {
                return Ok(crate::validation::malformed_ledger_report(
                    target,
                    path,
                    source.to_string(),
                ));
            }
            Err(e) => return Err(e),
        };
        Ok(crate::validation::hook_report_from_status(target, status))
    }

    /// Plan a hook install without mutating user files.
    fn plan_install(&self, scope: &Scope, spec: &HookSpec)
        -> Result<InstallPlan, AgentConfigError>;

    /// Plan a hook uninstall without mutating user files.
    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError>;

    /// Install the hook. Repeated calls with the same `spec.tag` are a no-op
    /// after the first (the on-disk state is reached, then preserved).
    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError>;

    /// Uninstall the hook identified by `tag`. Restores `.bak` files when
    /// removing our content leaves the target file empty or pristine.
    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError>;

    /// Migrate any prior layout produced by an earlier version of the consumer
    /// (e.g., remove a legacy shell-script wrapper that has since been
    /// superseded by a native binary). Default impl is a no-op.
    fn migrate(&self, _scope: &Scope, _tag: &str) -> Result<MigrationReport, AgentConfigError> {
        Ok(MigrationReport::NoOp)
    }
}

/// One AI harness's MCP-server installer.
///
/// Implemented by harnesses that load MCP server configs from a known file.
/// Harnesses without a confirmed file-backed MCP contract do not implement
/// this trait, so callers discover that at compile time or through
/// [`crate::registry::mcp_capable`].
///
/// All operations must be idempotent: installing the same [`McpSpec`] twice
/// reaches and preserves a single on-disk state. Uninstalls refuse to remove
/// entries owned by another consumer (recorded in a sidecar ledger).
pub trait McpSurface: Send + Sync {
    /// Stable, kebab-case identifier matching [`Integration::id`] for the same
    /// agent (e.g., `"claude"`).
    fn id(&self) -> &'static str;

    /// Which scopes this MCP installer accepts.
    fn supported_mcp_scopes(&self) -> &'static [ScopeKind];

    /// Returns true if a server with `name` is currently recorded under any
    /// owner in this scope's ownership ledger.
    ///
    /// Default impl folds the richer
    /// [`mcp_status`](McpSurface::mcp_status) into the historical boolean
    /// ("under any owner"); concretely, both
    /// [`InstallStatus::InstalledOwned`] and
    /// [`InstallStatus::InstalledOtherOwner`] count as installed.
    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, AgentConfigError> {
        // Pass the agent's id as the expected owner so any real consumer
        // owner (e.g. "myapp") routes through `InstalledOtherOwner`. The
        // boolean fold collapses both arms anyway.
        Ok(matches!(
            self.mcp_status(scope, name, self.id())?.status,
            InstallStatus::InstalledOwned { .. } | InstallStatus::InstalledOtherOwner { .. }
        ))
    }

    /// Detailed installation state for the MCP server identified by `name`,
    /// scored against `expected_owner`.
    ///
    /// `expected_owner` is the consumer tag the caller wants to compare
    /// against — when the ledger records this owner, the report returns
    /// [`InstallStatus::InstalledOwned`]; anything else recorded becomes
    /// [`InstallStatus::InstalledOtherOwner`].
    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError>;

    /// Validate MCP state without mutating user files.
    fn validate_mcp(
        &self,
        scope: &Scope,
        name: &str,
    ) -> Result<ValidationReport, AgentConfigError> {
        self.validate_mcp_for_owner(scope, name, None)
    }

    /// Validate MCP state against a caller-supplied expected owner.
    fn validate_mcp_for_owner(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: Option<&str>,
    ) -> Result<ValidationReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        if let Some(owner) = expected_owner {
            HookSpec::validate_tag(owner)?;
        }
        let status = match self.mcp_status(scope, name, expected_owner.unwrap_or("")) {
            Ok(status) => status,
            Err(AgentConfigError::JsonInvalid { path, source }) => {
                let target = PlanTarget::Mcp {
                    integration_id: self.id(),
                    scope: scope.clone(),
                    name: name.to_string(),
                    owner: expected_owner.unwrap_or_default().to_string(),
                };
                return Ok(crate::validation::malformed_ledger_report(
                    target,
                    path,
                    source.to_string(),
                ));
            }
            Err(e) => return Err(e),
        };
        let target = PlanTarget::Mcp {
            integration_id: self.id(),
            scope: scope.clone(),
            name: name.to_string(),
            owner: expected_owner
                .map(str::to_owned)
                .or_else(|| owner_from_status(&status))
                .unwrap_or_default(),
        };
        crate::validation::ledger_backed_report_from_status(target, name, expected_owner, status)
    }

    /// Plan an MCP server install without mutating user files.
    fn plan_install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallPlan, AgentConfigError>;

    /// Plan an MCP server uninstall without mutating user files.
    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError>;

    /// Install (or update) the MCP server. Repeated calls with the same
    /// `spec.name` and same content are a no-op after the first.
    fn install_mcp(&self, scope: &Scope, spec: &McpSpec)
        -> Result<InstallReport, AgentConfigError>;

    /// Uninstall the MCP server identified by `name`, owned by `owner_tag`.
    ///
    /// Returns [`AgentConfigError::NotOwnedByCaller`] if the entry is recorded
    /// under a different owner, or if it exists in the harness config but is
    /// missing from the ledger (i.e., user-installed by hand).
    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError>;
}

/// One AI harness's skill installer.
///
/// Skills are directory-scoped: each one is a folder under the harness's
/// `skills/` root containing a `SKILL.md` plus optional `scripts/`,
/// `references/`, and `assets/` subdirectories. Implemented by harnesses with
/// upstream Agent Skills support.
///
/// Like [`McpSurface`], ownership is tracked via a sidecar ledger so multiple
/// consumers can coexist and uninstall is refused on owner mismatch.
pub trait SkillSurface: Send + Sync {
    /// Stable, kebab-case identifier matching [`Integration::id`] for the
    /// same agent.
    fn id(&self) -> &'static str;

    /// Which scopes this skill installer accepts.
    fn supported_skill_scopes(&self) -> &'static [ScopeKind];

    /// Returns true if a skill named `name` is currently recorded in the
    /// ownership ledger for this scope.
    ///
    /// Default impl mirrors [`McpSurface::is_mcp_installed`]: both
    /// [`InstallStatus::InstalledOwned`] and
    /// [`InstallStatus::InstalledOtherOwner`] count as installed.
    fn is_skill_installed(&self, scope: &Scope, name: &str) -> Result<bool, AgentConfigError> {
        Ok(matches!(
            self.skill_status(scope, name, self.id())?.status,
            InstallStatus::InstalledOwned { .. } | InstallStatus::InstalledOtherOwner { .. }
        ))
    }

    /// Detailed installation state for the skill identified by `name`,
    /// scored against `expected_owner`. See [`McpSurface::mcp_status`] for
    /// the owner-comparison semantics.
    fn skill_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError>;

    /// Validate skill state without mutating user files.
    fn validate_skill(
        &self,
        scope: &Scope,
        name: &str,
    ) -> Result<ValidationReport, AgentConfigError> {
        self.validate_skill_for_owner(scope, name, None)
    }

    /// Validate skill state against a caller-supplied expected owner.
    fn validate_skill_for_owner(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: Option<&str>,
    ) -> Result<ValidationReport, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        if let Some(owner) = expected_owner {
            HookSpec::validate_tag(owner)?;
        }
        let status = match self.skill_status(scope, name, expected_owner.unwrap_or("")) {
            Ok(status) => status,
            Err(AgentConfigError::JsonInvalid { path, source }) => {
                let target = PlanTarget::Skill {
                    integration_id: self.id(),
                    scope: scope.clone(),
                    name: name.to_string(),
                    owner: expected_owner.unwrap_or_default().to_string(),
                };
                return Ok(crate::validation::malformed_ledger_report(
                    target,
                    path,
                    source.to_string(),
                ));
            }
            Err(e) => return Err(e),
        };
        let target = PlanTarget::Skill {
            integration_id: self.id(),
            scope: scope.clone(),
            name: name.to_string(),
            owner: expected_owner
                .map(str::to_owned)
                .or_else(|| owner_from_status(&status))
                .unwrap_or_default(),
        };
        crate::validation::skill_report_from_status(target, name, expected_owner, status)
    }

    /// Plan a skill install without mutating user files.
    fn plan_install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallPlan, AgentConfigError>;

    /// Plan a skill uninstall without mutating user files.
    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError>;

    /// Install (or update) the skill directory and record ownership.
    /// Repeated calls with byte-identical contents are a no-op after the
    /// first.
    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, AgentConfigError>;

    /// Uninstall the skill identified by `name`, owned by `owner_tag`.
    /// Returns [`AgentConfigError::NotOwnedByCaller`] on owner mismatch or when
    /// the skill exists on disk but is missing from the ledger.
    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError>;
}

/// One AI harness's standalone-instruction installer.
///
/// Instructions are named markdown files that provide persistent context to
/// the agent. Unlike hook rules (which are tied to a single hook spec),
/// instructions are independent documents that are loaded by the agent on
/// every session start.
///
/// Implemented by harnesses that support standalone instruction files or
/// managed include references in their memory/rules files. Use
/// [`crate::registry::instruction_capable`] to enumerate agents that
/// implement this trait.
///
/// All operations must be idempotent: installing the same [`InstructionSpec`]
/// twice reaches and preserves a single on-disk state. Uninstalls refuse to
/// remove entries owned by another consumer (recorded in a sidecar ledger).
pub trait InstructionSurface: Send + Sync {
    /// Stable, kebab-case identifier matching [`Integration::id`] for the
    /// same agent.
    fn id(&self) -> &'static str;

    /// Which scopes this instruction installer accepts.
    fn supported_instruction_scopes(&self) -> &'static [ScopeKind];

    /// Returns true if an instruction named `name` is currently recorded
    /// under any owner in this scope's ownership ledger.
    fn is_instruction_installed(
        &self,
        scope: &Scope,
        name: &str,
    ) -> Result<bool, AgentConfigError> {
        Ok(matches!(
            self.instruction_status(scope, name, self.id())?.status,
            InstallStatus::InstalledOwned { .. } | InstallStatus::InstalledOtherOwner { .. }
        ))
    }

    /// Detailed installation state for the instruction identified by `name`,
    /// scored against `expected_owner`.
    fn instruction_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError>;

    /// Validate instruction state without mutating user files.
    fn validate_instruction(
        &self,
        scope: &Scope,
        name: &str,
    ) -> Result<ValidationReport, AgentConfigError> {
        self.validate_instruction_for_owner(scope, name, None)
    }

    /// Validate instruction state against a caller-supplied expected owner.
    fn validate_instruction_for_owner(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: Option<&str>,
    ) -> Result<ValidationReport, AgentConfigError> {
        InstructionSpec::validate_name(name)?;
        if let Some(owner) = expected_owner {
            HookSpec::validate_tag(owner)?;
        }
        let status = match self.instruction_status(scope, name, expected_owner.unwrap_or("")) {
            Ok(status) => status,
            Err(AgentConfigError::JsonInvalid { path, source }) => {
                let target = PlanTarget::Instruction {
                    integration_id: self.id(),
                    scope: scope.clone(),
                    name: name.to_string(),
                    owner: expected_owner.unwrap_or_default().to_string(),
                };
                return Ok(crate::validation::malformed_ledger_report(
                    target,
                    path,
                    source.to_string(),
                ));
            }
            Err(e) => return Err(e),
        };
        let target = PlanTarget::Instruction {
            integration_id: self.id(),
            scope: scope.clone(),
            name: name.to_string(),
            owner: expected_owner
                .map(str::to_owned)
                .or_else(|| owner_from_status(&status))
                .unwrap_or_default(),
        };
        crate::validation::ledger_backed_report_from_status(target, name, expected_owner, status)
    }

    /// Plan an instruction install without mutating user files.
    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError>;

    /// Plan an instruction uninstall without mutating user files.
    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError>;

    /// Install (or update) the instruction. Repeated calls with the same
    /// name and identical content are a no-op after the first.
    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError>;

    /// Uninstall the instruction identified by `name`, owned by `owner_tag`.
    /// Returns [`AgentConfigError::NotOwnedByCaller`] on owner mismatch.
    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError>;
}

/// Outcome of a successful [`Integration::install`].
#[must_use]
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct InstallReport {
    /// Files this call created (did not previously exist).
    pub created: Vec<PathBuf>,
    /// Existing files this call modified.
    pub patched: Vec<PathBuf>,
    /// Sibling `.bak` files written. Each entry is the backup path; the
    /// original lives at the same path without `.bak`.
    pub backed_up: Vec<PathBuf>,
    /// True if every target was already in the desired state; nothing changed.
    pub already_installed: bool,
}

impl InstallReport {
    /// Fold another report's contents into this one.
    ///
    /// `already_installed` stays true only if both reports were already
    /// installed *and* this report has not produced any created/patched
    /// entries from earlier merges.
    pub(crate) fn merge(&mut self, from: InstallReport) {
        if !from.already_installed {
            self.already_installed = false;
        } else if self.created.is_empty() && self.patched.is_empty() {
            self.already_installed = true;
        }
        self.created.extend(from.created);
        self.patched.extend(from.patched);
        self.backed_up.extend(from.backed_up);
    }
}

/// Outcome of a successful [`Integration::uninstall`].
#[must_use]
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct UninstallReport {
    /// Files removed entirely.
    pub removed: Vec<PathBuf>,
    /// Files modified (tagged content stripped, file kept).
    pub patched: Vec<PathBuf>,
    /// Backups restored to their original location.
    pub restored: Vec<PathBuf>,
    /// True if the integration was not installed; nothing changed.
    pub not_installed: bool,
}

impl UninstallReport {
    /// Fold another report's contents into this one. `not_installed` survives
    /// only when neither report removed/patched/restored anything.
    pub(crate) fn merge(&mut self, from: UninstallReport) {
        self.not_installed = from.not_installed
            && self.removed.is_empty()
            && self.patched.is_empty()
            && self.restored.is_empty();
        self.removed.extend(from.removed);
        self.patched.extend(from.patched);
        self.restored.extend(from.restored);
    }
}

/// Outcome of a successful [`Integration::migrate`].
#[must_use]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum MigrationReport {
    /// Nothing to migrate.
    NoOp,
    /// Migrated files. Includes paths removed and paths rewritten.
    Migrated {
        /// Paths removed during migration (e.g., legacy shell scripts).
        removed: Vec<PathBuf>,
        /// Paths rewritten in place.
        rewritten: Vec<PathBuf>,
    },
}

fn owner_from_status(status: &StatusReport) -> Option<String> {
    match &status.status {
        InstallStatus::InstalledOwned { owner }
        | InstallStatus::InstalledOtherOwner { owner }
        | InstallStatus::LedgerOnly { owner } => Some(owner.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_report_default() {
        let r = InstallReport::default();
        assert!(r.created.is_empty());
        assert!(r.patched.is_empty());
        assert!(r.backed_up.is_empty());
        assert!(!r.already_installed);
    }

    #[test]
    fn uninstall_report_default() {
        let r = UninstallReport::default();
        assert!(r.removed.is_empty());
        assert!(r.patched.is_empty());
        assert!(r.restored.is_empty());
        assert!(!r.not_installed);
    }

    #[test]
    fn migration_report_debug_clone() {
        let noop = MigrationReport::NoOp;
        let _ = format!("{noop:?}");

        let migrated = MigrationReport::Migrated {
            removed: vec![PathBuf::from("/a")],
            rewritten: vec![],
        };
        let cloned = migrated.clone();
        let _ = format!("{cloned:?}");
    }
}
