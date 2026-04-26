//! Side-effect-free install and uninstall planning.
//!
//! Plans describe what an install or uninstall would do before any user-owned
//! files are touched. They are intended for downstream CLIs that want to show a
//! precise preview or refuse unsafe operations before calling the mutating API.

use std::path::PathBuf;

use crate::scope::Scope;

/// Side-effect-free preview of an install operation.
#[must_use]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InstallPlan {
    /// What install target this plan describes.
    pub target: PlanTarget,
    /// File, directory, permission, ledger, no-op, or refusal changes.
    pub changes: Vec<PlannedChange>,
    /// High-level outcome of the plan.
    pub status: PlanStatus,
    /// Advisory information that does not alter the status.
    pub warnings: Vec<PlanWarning>,
}

impl InstallPlan {
    /// Construct a plan and derive its status from `changes`.
    pub(crate) fn from_changes(target: PlanTarget, changes: Vec<PlannedChange>) -> Self {
        Self {
            target,
            status: status_for_changes(&changes),
            changes,
            warnings: Vec::new(),
        }
    }

    /// Construct a refused install plan.
    pub(crate) fn refused(
        target: PlanTarget,
        path: Option<PathBuf>,
        reason: RefusalReason,
    ) -> Self {
        Self::from_changes(target, vec![PlannedChange::Refuse { path, reason }])
    }
}

/// Side-effect-free preview of an uninstall operation.
#[must_use]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UninstallPlan {
    /// What uninstall target this plan describes.
    pub target: PlanTarget,
    /// File, directory, permission, ledger, no-op, or refusal changes.
    pub changes: Vec<PlannedChange>,
    /// High-level outcome of the plan.
    pub status: PlanStatus,
    /// Advisory information that does not alter the status.
    pub warnings: Vec<PlanWarning>,
}

impl UninstallPlan {
    /// Construct a plan and derive its status from `changes`.
    pub(crate) fn from_changes(target: PlanTarget, changes: Vec<PlannedChange>) -> Self {
        Self {
            target,
            status: status_for_changes(&changes),
            changes,
            warnings: Vec::new(),
        }
    }

    /// Construct a refused uninstall plan.
    pub(crate) fn refused(
        target: PlanTarget,
        path: Option<PathBuf>,
        reason: RefusalReason,
    ) -> Self {
        Self::from_changes(target, vec![PlannedChange::Refuse { path, reason }])
    }
}

/// The operation target described by an install/uninstall plan.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PlanTarget {
    /// Hook target for one integration, scope, and consumer tag.
    Hook {
        /// Stable integration id.
        integration_id: &'static str,
        /// Target scope.
        scope: Scope,
        /// Consumer tag.
        tag: String,
    },
    /// MCP target for one integration, scope, server name, and owner tag.
    Mcp {
        /// Stable integration id.
        integration_id: &'static str,
        /// Target scope.
        scope: Scope,
        /// MCP server name.
        name: String,
        /// Expected owner tag.
        owner: String,
    },
    /// Skill target for one integration, scope, skill name, and owner tag.
    Skill {
        /// Stable integration id.
        integration_id: &'static str,
        /// Target scope.
        scope: Scope,
        /// Skill name.
        name: String,
        /// Expected owner tag.
        owner: String,
    },
    /// Instruction target for one integration, scope, instruction name, and owner.
    Instruction {
        /// Stable integration id.
        integration_id: &'static str,
        /// Target scope.
        scope: Scope,
        /// Instruction name.
        name: String,
        /// Expected owner tag.
        owner: String,
    },
}

/// High-level status for a dry-run install or uninstall plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanStatus {
    /// The operation would change filesystem or ledger state.
    WillChange,
    /// The operation would not change anything.
    NoOp,
    /// The operation is predictable but refused for safety.
    Refused,
}

/// One concrete change in a dry-run plan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlannedChange {
    /// Create a file.
    CreateFile {
        /// File path.
        path: PathBuf,
    },
    /// Patch an existing file in place.
    PatchFile {
        /// File path.
        path: PathBuf,
    },
    /// Remove a file.
    RemoveFile {
        /// File path.
        path: PathBuf,
    },
    /// Restore a backup file over its original target.
    RestoreBackup {
        /// Backup path.
        backup: PathBuf,
        /// Restore target path.
        target: PathBuf,
    },
    /// Create a backup file before patching a target.
    CreateBackup {
        /// Backup path.
        backup: PathBuf,
        /// Original target path.
        target: PathBuf,
    },
    /// Create a directory.
    CreateDir {
        /// Directory path.
        path: PathBuf,
    },
    /// Remove a directory.
    RemoveDir {
        /// Directory path.
        path: PathBuf,
    },
    /// Write or update an ownership ledger entry.
    WriteLedger {
        /// Ledger file path.
        path: PathBuf,
        /// Ledger key.
        key: String,
        /// Owner tag.
        owner: String,
    },
    /// Remove an ownership ledger entry.
    RemoveLedgerEntry {
        /// Ledger file path.
        path: PathBuf,
        /// Ledger key.
        key: String,
    },
    /// Set file permissions.
    SetPermissions {
        /// File path.
        path: PathBuf,
        /// Unix mode, no-op on non-Unix platforms.
        mode: u32,
    },
    /// No filesystem or ledger change is needed for this path.
    NoOp {
        /// Path checked by the planner.
        path: PathBuf,
        /// Human-readable no-op reason.
        reason: String,
    },
    /// Refuse the operation before mutation.
    Refuse {
        /// Path that caused the refusal, when one exists.
        path: Option<PathBuf>,
        /// Refusal reason.
        reason: RefusalReason,
    },
}

/// A predictable dry-run refusal reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefusalReason {
    /// A sidecar ledger records a different owner.
    OwnerMismatch,
    /// The config entry exists without an agent-config ledger entry.
    UserInstalledEntry,
    /// Existing config could not be parsed or had an unsupported shape.
    InvalidConfig,
    /// A required first-touch backup already exists.
    ///
    /// Retained for compatibility; current planners preserve existing backups
    /// and patch without creating another one.
    BackupAlreadyExists,
    /// The integration does not support the requested scope.
    UnsupportedScope,
    /// The supplied spec is missing a field required by this integration.
    MissingRequiredSpecField,
    /// Local-scope MCP install would write likely secret material inline.
    InlineSecretInLocalScope,
}

/// Advisory warning attached to a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlanWarning {
    /// Related path, when any.
    pub path: Option<PathBuf>,
    /// Human-readable warning.
    pub message: String,
}

fn status_for_changes(changes: &[PlannedChange]) -> PlanStatus {
    if has_refusal(changes) {
        return PlanStatus::Refused;
    }
    if changes.is_empty()
        || changes
            .iter()
            .all(|c| matches!(c, PlannedChange::NoOp { .. }))
    {
        return PlanStatus::NoOp;
    }
    PlanStatus::WillChange
}

/// Returns true when any planned change refuses the operation.
pub(crate) fn has_refusal(changes: &[PlannedChange]) -> bool {
    changes
        .iter()
        .any(|c| matches!(c, PlannedChange::Refuse { .. }))
}
