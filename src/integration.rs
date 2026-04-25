//! The contract every AI-harness integration implements.
//!
//! There are several distinct **surfaces** an integration may expose:
//!
//! - [`Integration`] — the hooks surface (every registered agent implements
//!   this).
//! - [`McpSurface`] — MCP server registration (Phase 2; only some agents).
//! - `SkillSurface` — skill installation (Phase 3; only some agents).
//!
//! Each surface is its own trait so callers cannot accidentally call
//! `install_mcp` on a harness that does not support MCP — the type system
//! forbids it. Use [`crate::registry::mcp_capable`] to enumerate the agents
//! that implement `McpSurface`.

use std::path::PathBuf;

use crate::error::HookerError;
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, SkillSpec};

/// One AI harness's hook installer.
///
/// Implementations live in [`crate::agents`]. The trait is intentionally narrow
/// so adding a new harness is a small, mechanical exercise: create a new file
/// in `agents/`, implement `Integration`, and register it in
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
    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError>;

    /// Install the hook. Repeated calls with the same `spec.tag` are a no-op
    /// after the first (the on-disk state is reached, then preserved).
    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError>;

    /// Uninstall the hook identified by `tag`. Restores `.bak` files when
    /// removing our content leaves the target file empty or pristine.
    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError>;

    /// Migrate any prior layout produced by an earlier version of the consumer
    /// (e.g., remove a legacy shell-script wrapper that has since been
    /// superseded by a native binary). Default impl is a no-op.
    fn migrate(&self, _scope: &Scope, _tag: &str) -> Result<MigrationReport, HookerError> {
        Ok(MigrationReport::NoOp)
    }
}

/// One AI harness's MCP-server installer.
///
/// Implemented by harnesses that load MCP server configs from a known file
/// (Claude, Cursor, Gemini, Codex, OpenCode, Windsurf). Harnesses without
/// upstream MCP support (Cline, Roo, Kilo, Antigravity, Copilot) do not
/// implement this trait — the resulting compile error tells callers up-front.
///
/// All operations must be idempotent: installing the same [`McpSpec`] twice
/// reaches and preserves a single on-disk state. Uninstalls refuse to remove
/// entries owned by another consumer (recorded in a sidecar ledger).
pub trait McpSurface: Send + Sync {
    /// Stable, kebab-case identifier matching [`Integration::id`] for the same
    /// agent (e.g., `"claude"`).
    fn id(&self) -> &'static str;

    /// Which scopes this MCP installer accepts. Most agents support both
    /// Global and Local; OpenCode and Codex may differ.
    fn supported_mcp_scopes(&self) -> &'static [ScopeKind];

    /// Returns true if a server with `name` is currently recorded under any
    /// owner in this scope's ownership ledger.
    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError>;

    /// Install (or update) the MCP server. Repeated calls with the same
    /// `spec.name` and same content are a no-op after the first.
    fn install_mcp(&self, scope: &Scope, spec: &McpSpec)
        -> Result<InstallReport, HookerError>;

    /// Uninstall the MCP server identified by `name`, owned by `owner_tag`.
    ///
    /// Returns [`HookerError::NotOwnedByCaller`] if the entry is recorded
    /// under a different owner, or if it exists in the harness config but is
    /// missing from the ledger (i.e., user-installed by hand).
    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError>;
}

/// One AI harness's skill installer.
///
/// Skills are directory-scoped: each one is a folder under the harness's
/// `skills/` root containing a `SKILL.md` plus optional `scripts/`,
/// `references/`, and `assets/` subdirectories. Implemented by harnesses with
/// upstream skills support (Claude Code, Google Antigravity).
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
    fn is_skill_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError>;

    /// Install (or update) the skill directory and record ownership.
    /// Repeated calls with byte-identical contents are a no-op after the
    /// first.
    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, HookerError>;

    /// Uninstall the skill identified by `name`, owned by `owner_tag`.
    /// Returns [`HookerError::NotOwnedByCaller`] on owner mismatch or when
    /// the skill exists on disk but is missing from the ledger.
    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError>;
}

/// Outcome of a successful [`Integration::install`].
#[derive(Debug, Default, Clone)]
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

/// Outcome of a successful [`Integration::uninstall`].
#[derive(Debug, Default, Clone)]
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

/// Outcome of a successful [`Integration::migrate`].
#[derive(Debug, Clone)]
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
