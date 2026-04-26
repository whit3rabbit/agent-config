//! Internal adapters from agent path resolution to dry-run plan helpers.

use std::path::{Path, PathBuf};

use crate::error::HookerError;
use crate::plan::{InstallPlan, PlanTarget, PlanWarning, RefusalReason, UninstallPlan};
use crate::scope::Scope;
use crate::spec::{HookSpec, McpSpec, SkillSpec};
use crate::util::{
    mcp_json_map, mcp_json_object, ownership, planning, rules_dir, skills_dir, yaml_mcp_map,
};

pub(crate) fn rules_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &HookSpec,
    root: Result<&Path, HookerError>,
    rules_dir_name: &str,
) -> Result<InstallPlan, HookerError> {
    HookSpec::validate_tag(&spec.tag)?;
    let target = PlanTarget::Hook {
        integration_id,
        scope: scope.clone(),
        tag: spec.tag.clone(),
    };
    let root = match root {
        Ok(root) => root,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let Some(rules) = spec.rules.as_ref() else {
        return Ok(InstallPlan::refused(
            target,
            None,
            RefusalReason::MissingRequiredSpecField,
        ));
    };
    let changes = rules_dir::plan_install(root, rules_dir_name, &spec.tag, &rules.content)?;
    Ok(InstallPlan::from_changes(target, changes))
}

pub(crate) fn rules_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    tag: &str,
    root: Result<&Path, HookerError>,
    rules_dir_name: &str,
) -> Result<UninstallPlan, HookerError> {
    HookSpec::validate_tag(tag)?;
    let target = PlanTarget::Hook {
        integration_id,
        scope: scope.clone(),
        tag: tag.to_string(),
    };
    let root = match root {
        Ok(root) => root,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = rules_dir::plan_uninstall(root, rules_dir_name, tag)?;
    Ok(UninstallPlan::from_changes(target, changes))
}

pub(crate) fn markdown_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &HookSpec,
    path: Result<PathBuf, HookerError>,
    required_rules: bool,
) -> Result<InstallPlan, HookerError> {
    HookSpec::validate_tag(&spec.tag)?;
    let target = PlanTarget::Hook {
        integration_id,
        scope: scope.clone(),
        tag: spec.tag.clone(),
    };
    let path = match path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let Some(rules) = spec.rules.as_ref() else {
        if required_rules {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::MissingRequiredSpecField,
            ));
        }
        return Ok(InstallPlan::from_changes(
            target,
            vec![crate::plan::PlannedChange::NoOp {
                path,
                reason: "no markdown rules supplied".into(),
            }],
        ));
    };
    let mut changes = Vec::new();
    planning::plan_markdown_upsert(&mut changes, &path, &spec.tag, &rules.content)?;
    Ok(InstallPlan::from_changes(target, changes))
}

pub(crate) fn markdown_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    tag: &str,
    path: Result<PathBuf, HookerError>,
) -> Result<UninstallPlan, HookerError> {
    HookSpec::validate_tag(tag)?;
    let target = PlanTarget::Hook {
        integration_id,
        scope: scope.clone(),
        tag: tag.to_string(),
    };
    let path = match path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let mut changes = Vec::new();
    planning::plan_markdown_remove(&mut changes, &path, tag)?;
    Ok(UninstallPlan::from_changes(target, changes))
}

pub(crate) fn mcp_json_object_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &McpSpec,
    config_path: Result<PathBuf, HookerError>,
) -> Result<InstallPlan, HookerError> {
    spec.validate()?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    if let Some(plan) =
        mcp_local_inline_secret_refusal(target.clone(), scope, spec, Some(config_path.clone()))
    {
        return Ok(plan);
    }
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes = mcp_json_object::plan_install(&config_path, &ledger, spec)?;
    Ok(mcp_install_plan_from_changes(
        target,
        changes,
        scope,
        spec,
        Some(config_path),
    ))
}

pub(crate) fn mcp_json_object_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    name: &str,
    owner_tag: &str,
    config_path: Result<PathBuf, HookerError>,
) -> Result<UninstallPlan, HookerError> {
    McpSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes =
        mcp_json_object::plan_uninstall(&config_path, &ledger, name, owner_tag, "mcp server")?;
    Ok(UninstallPlan::from_changes(target, changes))
}

pub(crate) fn mcp_json_map_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &McpSpec,
    config_path: Result<PathBuf, HookerError>,
    servers_path: &[&str],
    build_server: mcp_json_map::ServerBuilder,
    format: mcp_json_map::ConfigFormat,
) -> Result<InstallPlan, HookerError> {
    spec.validate()?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    if let Some(plan) =
        mcp_local_inline_secret_refusal(target.clone(), scope, spec, Some(config_path.clone()))
    {
        return Ok(plan);
    }
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes = mcp_json_map::plan_install(
        &config_path,
        &ledger,
        spec,
        servers_path,
        build_server,
        format,
    )?;
    Ok(mcp_install_plan_from_changes(
        target,
        changes,
        scope,
        spec,
        Some(config_path),
    ))
}

pub(crate) fn mcp_json_map_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    name: &str,
    owner_tag: &str,
    config_path: Result<PathBuf, HookerError>,
    servers_path: &[&str],
    format: mcp_json_map::ConfigFormat,
) -> Result<UninstallPlan, HookerError> {
    McpSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes = mcp_json_map::plan_uninstall(
        &config_path,
        &ledger,
        name,
        owner_tag,
        "mcp server",
        servers_path,
        format,
    )?;
    Ok(UninstallPlan::from_changes(target, changes))
}

pub(crate) fn mcp_yaml_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &McpSpec,
    config_path: Result<PathBuf, HookerError>,
    servers_path: &[&str],
    build_server: yaml_mcp_map::ServerBuilder,
) -> Result<InstallPlan, HookerError> {
    spec.validate()?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    if let Some(plan) =
        mcp_local_inline_secret_refusal(target.clone(), scope, spec, Some(config_path.clone()))
    {
        return Ok(plan);
    }
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes =
        yaml_mcp_map::plan_install(&config_path, &ledger, spec, servers_path, build_server)?;
    Ok(mcp_install_plan_from_changes(
        target,
        changes,
        scope,
        spec,
        Some(config_path),
    ))
}

pub(crate) fn mcp_local_inline_secret_refusal(
    target: PlanTarget,
    scope: &Scope,
    spec: &McpSpec,
    path: Option<PathBuf>,
) -> Option<InstallPlan> {
    spec.refused_local_inline_secret_key(scope)?;
    Some(InstallPlan::refused(
        target,
        path,
        RefusalReason::InlineSecretInLocalScope,
    ))
}

pub(crate) fn mcp_install_plan_from_changes(
    target: PlanTarget,
    changes: Vec<crate::plan::PlannedChange>,
    scope: &Scope,
    spec: &McpSpec,
    path: Option<PathBuf>,
) -> InstallPlan {
    let mut plan = InstallPlan::from_changes(target, changes);
    if let Some(key) = spec.allowed_local_inline_secret_key(scope) {
        plan.warnings.push(PlanWarning {
            path,
            message: format!(
                "MCP server {:?} writes likely secret env var {:?} into a project-local config because local inline secrets were explicitly allowed",
                spec.name, key
            ),
        });
    }
    plan
}

pub(crate) fn mcp_yaml_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    name: &str,
    owner_tag: &str,
    config_path: Result<PathBuf, HookerError>,
    servers_path: &[&str],
) -> Result<UninstallPlan, HookerError> {
    McpSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Mcp {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let config_path = match config_path {
        Ok(path) => path,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let ledger = ownership::mcp_ledger_for(&config_path);
    let changes = yaml_mcp_map::plan_uninstall(
        &config_path,
        &ledger,
        name,
        owner_tag,
        "mcp server",
        servers_path,
    )?;
    Ok(UninstallPlan::from_changes(target, changes))
}

pub(crate) fn skill_install(
    integration_id: &'static str,
    scope: &Scope,
    spec: &SkillSpec,
    root: Result<PathBuf, HookerError>,
) -> Result<InstallPlan, HookerError> {
    spec.validate()?;
    let target = PlanTarget::Skill {
        integration_id,
        scope: scope.clone(),
        name: spec.name.clone(),
        owner: spec.owner_tag.clone(),
    };
    let root = match root {
        Ok(root) => root,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(InstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = skills_dir::plan_install(&root, spec)?;
    Ok(InstallPlan::from_changes(target, changes))
}

pub(crate) fn skill_uninstall(
    integration_id: &'static str,
    scope: &Scope,
    name: &str,
    owner_tag: &str,
    root: Result<PathBuf, HookerError>,
) -> Result<UninstallPlan, HookerError> {
    SkillSpec::validate_name(name)?;
    HookSpec::validate_tag(owner_tag)?;
    let target = PlanTarget::Skill {
        integration_id,
        scope: scope.clone(),
        name: name.to_string(),
        owner: owner_tag.to_string(),
    };
    let root = match root {
        Ok(root) => root,
        Err(HookerError::UnsupportedScope { .. }) => {
            return Ok(UninstallPlan::refused(
                target,
                None,
                RefusalReason::UnsupportedScope,
            ));
        }
        Err(e) => return Err(e),
    };
    let changes = skills_dir::plan_uninstall(&root, name, owner_tag)?;
    Ok(UninstallPlan::from_changes(target, changes))
}
