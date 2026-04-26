//! Side-effect-free drift validation reports.
//!
//! Status answers what is present. Validation answers whether that state is
//! internally consistent enough for a caller to repair or mutate safely.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AgentConfigError;
use crate::plan::PlanTarget;
use crate::status::{DriftIssue, InstallStatus, StatusReport, StatusWarning};
use crate::util::{fs_atomic, md_block, ownership};

/// Validation result for one hook, MCP server, or skill target.
#[must_use]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ValidationReport {
    /// What install target was validated.
    pub target: PlanTarget,
    /// True when validation found no drift issues.
    pub ok: bool,
    /// Concrete drift issues in deterministic order.
    pub issues: Vec<DriftIssue>,
    /// Suggested safe next actions derived from the issues.
    pub suggested_actions: Vec<SuggestedAction>,
}

/// Suggested follow-up for a validation report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SuggestedAction {
    /// Reinstall the missing config, directory, or file entry.
    Reinstall,
    /// Uninstall using the owner currently recorded in the ledger.
    UninstallWithOwner,
    /// Remove a stale ownership ledger entry.
    RemoveLedgerEntry,
    /// Remove an unowned config, directory, or file entry.
    RemoveConfigEntry,
    /// Restore a backup before retrying mutation.
    RestoreBackup,
    /// Stop and inspect the files manually.
    ManualReview,
    /// No follow-up is needed.
    NoAction,
}

impl ValidationReport {
    pub(crate) fn from_issues(target: PlanTarget, mut issues: Vec<DriftIssue>) -> Self {
        sort_issues(&mut issues);
        let ok = issues.is_empty();
        let suggested_actions = suggested_actions_for(&issues);
        Self {
            target,
            ok,
            issues,
            suggested_actions,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presence {
    Present,
    Absent,
    Malformed,
    Unknown,
}

pub(crate) fn hook_report_from_status(
    target: PlanTarget,
    status: StatusReport,
) -> ValidationReport {
    let mut issues = Vec::new();
    match &status.status {
        InstallStatus::Absent | InstallStatus::InstalledOwned { .. } => {}
        InstallStatus::InstalledOtherOwner { owner } => {
            let expected = match &target {
                PlanTarget::Hook { tag, .. } => tag.clone(),
                _ => String::new(),
            };
            push_issue(
                &mut issues,
                DriftIssue::OwnerMismatch {
                    expected,
                    actual: Some(owner.clone()),
                    path: status.ledger_path.clone(),
                },
            );
        }
        InstallStatus::PresentUnowned => {
            push_issue(
                &mut issues,
                DriftIssue::ConfigOnly {
                    path: primary_path(&status),
                },
            );
        }
        InstallStatus::LedgerOnly { owner } => {
            push_issue(
                &mut issues,
                DriftIssue::LedgerOnly {
                    path: status
                        .ledger_path
                        .clone()
                        .unwrap_or_else(|| primary_path(&status)),
                    owner: Some(owner.clone()),
                },
            );
        }
        InstallStatus::Drifted { issues: drift } => {
            push_mapped_issues(&mut issues, drift);
        }
        InstallStatus::Unknown => {
            push_issue(
                &mut issues,
                DriftIssue::UnexpectedDirectoryShape {
                    path: primary_path(&status),
                    reason: "status probe returned unknown".into(),
                },
            );
        }
    }
    add_hook_ledger_issues(&mut issues, &target, &status);
    add_markdown_fence_issues(&mut issues, &target, &status);
    add_backup_issues(&mut issues, &status);
    ValidationReport::from_issues(target, issues)
}

pub(crate) fn ledger_backed_report_from_status(
    target: PlanTarget,
    name: &str,
    expected_owner: Option<&str>,
    status: StatusReport,
) -> Result<ValidationReport, AgentConfigError> {
    let mut issues = ledger_backed_issues(name, expected_owner, &status)?;
    add_backup_issues(&mut issues, &status);
    Ok(ValidationReport::from_issues(target, issues))
}

pub(crate) fn skill_report_from_status(
    target: PlanTarget,
    name: &str,
    expected_owner: Option<&str>,
    status: StatusReport,
) -> Result<ValidationReport, AgentConfigError> {
    let mut issues = ledger_backed_issues(name, expected_owner, &status)?;
    add_skill_shape_issues(&mut issues, &status)?;
    add_backup_issues(&mut issues, &status);
    Ok(ValidationReport::from_issues(target, issues))
}

pub(crate) fn malformed_ledger_report(
    target: PlanTarget,
    path: PathBuf,
    reason: String,
) -> ValidationReport {
    ValidationReport::from_issues(target, vec![DriftIssue::MalformedLedger { path, reason }])
}

fn ledger_backed_issues(
    name: &str,
    expected_owner: Option<&str>,
    status: &StatusReport,
) -> Result<Vec<DriftIssue>, AgentConfigError> {
    let mut issues = Vec::new();
    if let InstallStatus::Drifted { issues: drift } = &status.status {
        push_mapped_issues(&mut issues, drift);
    }

    let presence = presence_from_status(status);
    let mut owner = None;
    let mut ledger_malformed = false;

    if let Some(ledger_path) = status.ledger_path.as_ref() {
        match ownership::read_strict(ledger_path)? {
            ownership::StrictLedgerRead::Missing => {}
            ownership::StrictLedgerRead::Valid { entries } => {
                owner = entries.get(name).cloned();
            }
            ownership::StrictLedgerRead::Malformed { reason } => {
                ledger_malformed = true;
                push_issue(
                    &mut issues,
                    DriftIssue::MalformedLedger {
                        path: ledger_path.clone(),
                        reason,
                    },
                );
            }
        }
    }

    if presence == Presence::Malformed || ledger_malformed {
        return Ok(issues);
    }

    match (presence, owner.as_ref()) {
        (Presence::Present, None) => {
            push_issue(
                &mut issues,
                DriftIssue::ConfigOnly {
                    path: primary_path(status),
                },
            );
        }
        (Presence::Absent, Some(owner)) => {
            push_issue(
                &mut issues,
                DriftIssue::LedgerOnly {
                    path: status
                        .ledger_path
                        .clone()
                        .unwrap_or_else(|| primary_path(status)),
                    owner: Some(owner.owner.clone()),
                },
            );
        }
        _ => {}
    }

    if let (Some(expected), Some(actual)) =
        (expected_owner, owner.as_ref().map(|e| e.owner.as_str()))
    {
        if expected != actual {
            push_issue(
                &mut issues,
                DriftIssue::OwnerMismatch {
                    expected: expected.to_string(),
                    actual: Some(actual.to_string()),
                    path: status.ledger_path.clone(),
                },
            );
        }
    }

    Ok(issues)
}

fn presence_from_status(status: &StatusReport) -> Presence {
    match &status.status {
        InstallStatus::InstalledOwned { .. }
        | InstallStatus::InstalledOtherOwner { .. }
        | InstallStatus::PresentUnowned => Presence::Present,
        InstallStatus::Absent | InstallStatus::LedgerOnly { .. } => Presence::Absent,
        InstallStatus::Drifted { issues } => {
            if issues.iter().any(|issue| {
                matches!(
                    issue,
                    DriftIssue::InvalidConfig { .. } | DriftIssue::MalformedConfig { .. }
                )
            }) {
                Presence::Malformed
            } else if issues.iter().any(|issue| {
                matches!(
                    issue,
                    DriftIssue::MultipleEntries { .. }
                        | DriftIssue::SkillIncomplete { .. }
                        | DriftIssue::SkillMissingSkillMd { .. }
                        | DriftIssue::UnexpectedDirectoryShape { .. }
                        | DriftIssue::SkillAssetEscapesRoot { .. }
                )
            }) {
                Presence::Present
            } else {
                Presence::Unknown
            }
        }
        InstallStatus::Unknown => Presence::Unknown,
    }
}

fn push_mapped_issues(out: &mut Vec<DriftIssue>, drift: &[DriftIssue]) {
    for issue in drift {
        match issue {
            DriftIssue::InvalidConfig { path, reason } => {
                push_issue(
                    out,
                    DriftIssue::MalformedConfig {
                        path: path.clone(),
                        reason: reason.clone(),
                    },
                );
            }
            DriftIssue::SkillIncomplete { dir, missing } => {
                push_issue(
                    out,
                    DriftIssue::SkillMissingSkillMd {
                        dir: dir.clone(),
                        missing: missing.clone(),
                    },
                );
            }
            other => push_issue(out, other.clone()),
        }
    }
}

fn add_hook_ledger_issues(
    issues: &mut Vec<DriftIssue>,
    target: &PlanTarget,
    status: &StatusReport,
) {
    let (PlanTarget::Hook { tag, .. }, Some(ledger_path)) = (target, status.ledger_path.as_ref())
    else {
        return;
    };
    match ownership::read_strict(ledger_path) {
        Ok(ownership::StrictLedgerRead::Missing) => {}
        Ok(ownership::StrictLedgerRead::Malformed { reason }) => {
            push_issue(
                issues,
                DriftIssue::MalformedLedger {
                    path: ledger_path.clone(),
                    reason,
                },
            );
        }
        Ok(ownership::StrictLedgerRead::Valid { entries }) => {
            let Some(hooks_dir) = ledger_path.parent() else {
                return;
            };
            for (filename, entry) in entries {
                if entry.owner != *tag {
                    continue;
                }
                let script = hooks_dir.join(&filename);
                if !script.exists() {
                    push_issue(
                        issues,
                        DriftIssue::LedgerOnly {
                            path: ledger_path.clone(),
                            owner: Some(entry.owner.clone()),
                        },
                    );
                    continue;
                }
                add_unix_executable_issue(issues, &script);
            }
        }
        Err(e) => {
            push_issue(
                issues,
                DriftIssue::MalformedLedger {
                    path: ledger_path.clone(),
                    reason: e.to_string(),
                },
            );
        }
    }
}

fn add_markdown_fence_issues(
    issues: &mut Vec<DriftIssue>,
    target: &PlanTarget,
    status: &StatusReport,
) {
    let PlanTarget::Hook { tag, .. } = target else {
        return;
    };
    let path = primary_path(status);
    let text = match fs_atomic::read_to_string_or_empty(&path) {
        Ok(text) => text,
        Err(e) => {
            push_issue(
                issues,
                DriftIssue::MalformedConfig {
                    path,
                    reason: format!("could not read hook markdown: {e}"),
                },
            );
            return;
        }
    };
    if md_block::malformed(&text, tag) {
        push_issue(
            issues,
            DriftIssue::MalformedConfig {
                path,
                reason: "malformed agent-config markdown fence".into(),
            },
        );
    }
}

#[cfg(unix)]
fn add_unix_executable_issue(issues: &mut Vec<DriftIssue>, path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    match fs::metadata(path) {
        Ok(metadata) if metadata.permissions().mode() & 0o111 == 0 => {
            push_issue(
                issues,
                DriftIssue::UnexpectedDirectoryShape {
                    path: path.to_path_buf(),
                    reason: "hook script is not executable".into(),
                },
            );
        }
        Ok(_) => {}
        Err(e) => {
            push_issue(
                issues,
                DriftIssue::UnexpectedDirectoryShape {
                    path: path.to_path_buf(),
                    reason: format!("could not inspect hook script permissions: {e}"),
                },
            );
        }
    }
}

#[cfg(not(unix))]
fn add_unix_executable_issue(_issues: &mut Vec<DriftIssue>, _path: &Path) {}

fn add_skill_shape_issues(
    issues: &mut Vec<DriftIssue>,
    status: &StatusReport,
) -> Result<(), AgentConfigError> {
    let dir = primary_path(status);
    if !dir.exists() {
        return Ok(());
    }

    let metadata = fs::symlink_metadata(&dir).map_err(|e| AgentConfigError::io(&dir, e))?;
    if !metadata.is_dir() {
        push_issue(
            issues,
            DriftIssue::UnexpectedDirectoryShape {
                path: dir,
                reason: "skill path exists but is not a directory".into(),
            },
        );
        return Ok(());
    }

    let manifest = dir.join("SKILL.md");
    if !manifest.is_file() {
        push_issue(
            issues,
            DriftIssue::SkillMissingSkillMd {
                dir: dir.clone(),
                missing: manifest,
            },
        );
    }

    let canonical_root = match fs::canonicalize(&dir) {
        Ok(path) => path,
        Err(e) => {
            push_issue(
                issues,
                DriftIssue::UnexpectedDirectoryShape {
                    path: dir,
                    reason: format!("could not canonicalize skill directory: {e}"),
                },
            );
            return Ok(());
        }
    };
    walk_skill_dir(&canonical_root, &canonical_root, issues)?;
    Ok(())
}

fn walk_skill_dir(
    dir: &Path,
    canonical_root: &Path,
    issues: &mut Vec<DriftIssue>,
) -> Result<(), AgentConfigError> {
    // `dir` is descended from `canonical_root` through real (non-symlink)
    // directories, so non-symlink entries cannot escape — only canonicalize
    // when an entry is a symlink.
    for entry in fs::read_dir(dir).map_err(|e| AgentConfigError::io(dir, e))? {
        let entry = entry.map_err(|e| AgentConfigError::io(dir, e))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| AgentConfigError::io(&path, e))?;

        if metadata.file_type().is_symlink() {
            match fs::canonicalize(&path) {
                Ok(canonical) if !canonical.starts_with(canonical_root) => {
                    push_issue(
                        issues,
                        DriftIssue::SkillAssetEscapesRoot {
                            path: path.clone(),
                            root: canonical_root.to_path_buf(),
                        },
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    push_issue(
                        issues,
                        DriftIssue::UnexpectedDirectoryShape {
                            path: path.clone(),
                            reason: format!("could not canonicalize skill entry: {e}"),
                        },
                    );
                }
            }
        }

        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            walk_skill_dir(&path, canonical_root, issues)?;
        }
    }
    Ok(())
}

fn add_backup_issues(issues: &mut Vec<DriftIssue>, status: &StatusReport) {
    if let Some(config_path) = status.config_path.as_ref() {
        let backup = fs_atomic::backup_path(config_path);
        if backup.exists() {
            let issue = if config_path.exists() {
                DriftIssue::BackupCollision { path: backup }
            } else {
                DriftIssue::StaleBackup { path: backup }
            };
            push_issue(issues, issue);
        }
    }

    for warning in &status.warnings {
        let StatusWarning::BackupExists { path } = warning;
        let issue = if status.config_path.as_ref().is_some_and(|p| p.exists()) {
            DriftIssue::BackupCollision { path: path.clone() }
        } else {
            DriftIssue::StaleBackup { path: path.clone() }
        };
        push_issue(issues, issue);
    }
}

fn primary_path(status: &StatusReport) -> PathBuf {
    status
        .config_path
        .clone()
        .or_else(|| status.files.first().map(path_from_status))
        .unwrap_or_default()
}

fn path_from_status(path_status: &crate::status::PathStatus) -> PathBuf {
    match path_status {
        crate::status::PathStatus::Missing { path }
        | crate::status::PathStatus::Exists { path }
        | crate::status::PathStatus::Invalid { path, .. } => path.clone(),
    }
}

fn push_issue(issues: &mut Vec<DriftIssue>, issue: DriftIssue) {
    if !issues.contains(&issue) {
        issues.push(issue);
    }
}

fn sort_issues(issues: &mut [DriftIssue]) {
    // sort_by_cached_key extracts each key once instead of twice per comparison,
    // and the stable sort preserves insertion order for fully-equal keys.
    issues.sort_by_cached_key(|issue| (issue_rank(issue), issue_path(issue)));
}

fn issue_rank(issue: &DriftIssue) -> u8 {
    match issue {
        DriftIssue::MalformedConfig { .. } | DriftIssue::InvalidConfig { .. } => 0,
        DriftIssue::MalformedLedger { .. } => 1,
        DriftIssue::LedgerOnly { .. } => 2,
        DriftIssue::ConfigOnly { .. } => 3,
        DriftIssue::OwnerMismatch { .. } => 4,
        DriftIssue::MultipleEntries { .. } => 5,
        DriftIssue::UnexpectedDirectoryShape { .. } => 6,
        DriftIssue::SkillMissingSkillMd { .. } | DriftIssue::SkillIncomplete { .. } => 7,
        DriftIssue::SkillAssetEscapesRoot { .. } => 8,
        DriftIssue::BackupCollision { .. } => 9,
        DriftIssue::MissingBackup { .. } => 10,
        DriftIssue::StaleBackup { .. } => 11,
        DriftIssue::UnsupportedButPresent { .. } => 12,
    }
}

fn issue_path(issue: &DriftIssue) -> String {
    let path = match issue {
        DriftIssue::LedgerOnly { path, .. }
        | DriftIssue::ConfigOnly { path }
        | DriftIssue::MalformedConfig { path, .. }
        | DriftIssue::MalformedLedger { path, .. }
        | DriftIssue::BackupCollision { path }
        | DriftIssue::MissingBackup { path }
        | DriftIssue::StaleBackup { path }
        | DriftIssue::UnexpectedDirectoryShape { path, .. }
        | DriftIssue::SkillAssetEscapesRoot { path, .. }
        | DriftIssue::UnsupportedButPresent { path }
        | DriftIssue::InvalidConfig { path, .. } => path.display().to_string(),
        DriftIssue::OwnerMismatch { path, .. } => path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        DriftIssue::SkillMissingSkillMd { missing, .. }
        | DriftIssue::SkillIncomplete { missing, .. } => missing.display().to_string(),
        DriftIssue::MultipleEntries { name, .. } => name.clone(),
    };
    path
}

fn suggested_actions_for(issues: &[DriftIssue]) -> Vec<SuggestedAction> {
    let mut actions = Vec::new();
    for issue in issues {
        match issue {
            DriftIssue::LedgerOnly { .. } => {
                push_action(&mut actions, SuggestedAction::Reinstall);
                push_action(&mut actions, SuggestedAction::RemoveLedgerEntry);
            }
            DriftIssue::ConfigOnly { .. } => {
                push_action(&mut actions, SuggestedAction::RemoveConfigEntry);
            }
            DriftIssue::OwnerMismatch { .. } => {
                push_action(&mut actions, SuggestedAction::UninstallWithOwner);
            }
            DriftIssue::BackupCollision { .. }
            | DriftIssue::MissingBackup { .. }
            | DriftIssue::StaleBackup { .. } => {
                push_action(&mut actions, SuggestedAction::RestoreBackup);
            }
            DriftIssue::MalformedConfig { .. }
            | DriftIssue::MalformedLedger { .. }
            | DriftIssue::UnexpectedDirectoryShape { .. }
            | DriftIssue::SkillMissingSkillMd { .. }
            | DriftIssue::SkillAssetEscapesRoot { .. }
            | DriftIssue::UnsupportedButPresent { .. }
            | DriftIssue::InvalidConfig { .. }
            | DriftIssue::SkillIncomplete { .. }
            | DriftIssue::MultipleEntries { .. } => {
                push_action(&mut actions, SuggestedAction::ManualReview);
            }
        }
    }
    if actions.is_empty() {
        actions.push(SuggestedAction::NoAction);
    }
    actions
}

fn push_action(actions: &mut Vec<SuggestedAction>, action: SuggestedAction) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}
