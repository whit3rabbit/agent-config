//! Utility for installing, uninstalling, planning, and checking status of
//! hooks defined in a standalone `hooks.json` file.
//!
//! The JSON format keys hook configurations under top-level consumer tags:
//!
//! ```json
//! {
//!   "my-app": {
//!     "PreToolUse": [
//!       {
//!         "matcher": "run_command",
//!         "hooks": [
//!           { "type": "command", "command": "...", "timeout": 10 }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::AgentConfigError;
use crate::integration::{InstallReport, UninstallReport};
use crate::plan::{PlannedChange, RefusalReason};
use crate::spec::HookSpec;
use crate::status::ConfigPresence;
use crate::util::{file_lock, fs_atomic, json_patch, planning, safe_fs};

/// Check if tag is present in hooks.json.
pub(crate) fn config_presence(
    config_path: &Path,
    tag: &str,
) -> Result<ConfigPresence, AgentConfigError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let root = match json_patch::read_or_empty(config_path) {
        Ok(v) => v,
        Err(AgentConfigError::JsonInvalid { source, .. }) => {
            return Ok(ConfigPresence::Invalid {
                reason: source.to_string(),
            });
        }
        Err(e) => return Err(e),
    };
    if root.get(tag).is_some() {
        Ok(ConfigPresence::Single)
    } else {
        Ok(ConfigPresence::Absent)
    }
}

pub(crate) fn plan_install(
    changes: &mut Vec<PlannedChange>,
    config_path: &Path,
    spec: &HookSpec,
    build_hook_value: impl Fn(&HookSpec) -> Value,
) -> Result<(), AgentConfigError> {
    let mut root = match json_patch::read_or_empty(config_path) {
        Ok(v) => v,
        Err(AgentConfigError::JsonInvalid { .. }) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let mut tag_obj = match root.get(&spec.tag) {
        Some(Value::Object(obj)) => obj.clone(),
        _ => Map::new(),
    };

    let event_key = match &spec.event {
        crate::spec::Event::PreToolUse => "PreToolUse".to_string(),
        crate::spec::Event::PostToolUse => "PostToolUse".to_string(),
        crate::spec::Event::Custom(s) => s.clone(),
        other => other.as_str().to_string(),
    };

    let hook_val = build_hook_value(spec);
    tag_obj.insert(event_key, hook_val);

    let changed =
        json_patch::upsert_named_object_entry(&mut root, &[], &spec.tag, Value::Object(tag_obj))?;

    if changed {
        let bytes = json_patch::to_pretty(&root);
        planning::plan_write_file(changes, config_path, &bytes, true)?;
    } else {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: "hook is already up to date".into(),
        });
    }
    Ok(())
}

pub(crate) fn plan_uninstall(
    changes: &mut Vec<PlannedChange>,
    config_path: &Path,
    tag: &str,
) -> Result<(), AgentConfigError> {
    if !config_path.exists() {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: "config file is already absent".into(),
        });
        return Ok(());
    }

    let mut root = match json_patch::read_or_empty(config_path) {
        Ok(v) => v,
        Err(AgentConfigError::JsonInvalid { .. }) => {
            changes.push(PlannedChange::Refuse {
                path: Some(config_path.to_path_buf()),
                reason: RefusalReason::InvalidConfig,
            });
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let changed = json_patch::remove_named_object_entry(&mut root, &[], tag)?;
    if !changed {
        changes.push(PlannedChange::NoOp {
            path: config_path.to_path_buf(),
            reason: "hook is already absent".into(),
        });
        return Ok(());
    }

    if planning::json_object_empty(&root) {
        changes.push(PlannedChange::RemoveFile {
            path: config_path.to_path_buf(),
        });
        let backup = fs_atomic::backup_path(config_path);
        if backup.exists() {
            changes.push(PlannedChange::RemoveFile { path: backup });
        }
    } else {
        let bytes = json_patch::to_pretty(&root);
        planning::plan_write_file(changes, config_path, &bytes, false)?;
    }
    Ok(())
}

pub(crate) fn install(
    scope: &crate::scope::Scope,
    config_path: &Path,
    spec: &HookSpec,
    build_hook_value: impl Fn(&HookSpec) -> Value,
) -> Result<InstallReport, AgentConfigError> {
    let mut report = InstallReport::default();
    scope.ensure_contained(config_path)?;

    file_lock::with_lock(config_path, || {
        let mut root = json_patch::read_or_empty(config_path)?;

        let mut tag_obj = match root.get(&spec.tag) {
            Some(Value::Object(obj)) => obj.clone(),
            _ => Map::new(),
        };

        let event_key = match &spec.event {
            crate::spec::Event::PreToolUse => "PreToolUse".to_string(),
            crate::spec::Event::PostToolUse => "PostToolUse".to_string(),
            crate::spec::Event::Custom(s) => s.clone(),
            other => other.as_str().to_string(),
        };

        let hook_val = build_hook_value(spec);
        tag_obj.insert(event_key, hook_val);

        let changed = json_patch::upsert_named_object_entry(
            &mut root,
            &[],
            &spec.tag,
            Value::Object(tag_obj),
        )?;

        if changed {
            let bytes = json_patch::to_pretty(&root);
            let outcome = safe_fs::write(scope, config_path, &bytes, true)?;
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

    Ok(report)
}

pub(crate) fn uninstall(
    scope: &crate::scope::Scope,
    config_path: &Path,
    tag: &str,
) -> Result<UninstallReport, AgentConfigError> {
    let mut report = UninstallReport::default();
    scope.ensure_contained(config_path)?;

    if config_path.exists() {
        file_lock::with_lock(config_path, || {
            let mut root = json_patch::read_or_empty(config_path)?;
            let changed = json_patch::remove_named_object_entry(&mut root, &[], tag)?;
            if changed {
                let empty = planning::json_object_empty(&root);
                let bytes = json_patch::to_pretty(&root);
                if empty && safe_fs::restore_backup_if_matches(scope, config_path, &bytes)? {
                    report.restored.push(config_path.to_path_buf());
                } else if empty {
                    safe_fs::remove_file(scope, config_path)?;
                    report.removed.push(config_path.to_path_buf());
                } else {
                    safe_fs::write(scope, config_path, &bytes, false)?;
                    report.patched.push(config_path.to_path_buf());
                }
            } else {
                report.not_installed = true;
            }
            Ok::<(), AgentConfigError>(())
        })?;
    } else {
        report.not_installed = true;
    }

    Ok(report)
}
