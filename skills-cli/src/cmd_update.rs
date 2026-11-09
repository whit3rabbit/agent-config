//! The `skillz update` / `skillz check` command, ported from `src/update.ts`
//! (sequential source checks; same outputs and lock handling).

use std::collections::BTreeMap;
use std::path::Path;

use skills_store::error::SkillsError;
use skills_store::registry::AgentConfig;
use skills_store::sanitize::sanitize_metadata;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::prompt;
use crate::skill_payload::{find_locked, SkillPayload};
use crate::update_source;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum UpdateScope {
    #[default]
    Project,
    Global,
    Both,
}

struct UpdateCheckOptions {
    global: bool,
    project: bool,
    yes: bool,
    skills: Vec<String>,
    report_outdated: bool,
    outdated_names: std::cell::RefCell<Vec<String>>,
}

fn has_project_skills(cwd: &Path) -> bool {
    if cwd.join("skills-lock.json").exists() {
        return true;
    }
    let skills_dir = cwd.join(".agents").join("skills");
    let Ok(entries) = std::fs::read_dir(&skills_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() && entry.path().join("SKILL.md").exists() {
            return true;
        }
    }
    false
}

fn resolve_update_scope(options: &UpdateCheckOptions) -> UpdateScope {
    if !options.skills.is_empty() {
        if options.global {
            return UpdateScope::Global;
        }
        if options.project {
            return UpdateScope::Project;
        }
        return UpdateScope::Both;
    }
    if options.global && options.project {
        return UpdateScope::Both;
    }
    if options.global {
        return UpdateScope::Global;
    }
    if options.project {
        return UpdateScope::Project;
    }
    if options.yes || !prompt::is_tty() {
        return if has_project_skills(&common::current_dir()) {
            UpdateScope::Project
        } else {
            UpdateScope::Global
        };
    }
    let scope = prompt::select(
        "Update scope",
        &[
            prompt::SelectOption {
                value: "project".to_string(),
                label: "Project".to_string(),
                hint: Some("Update skills in current directory".to_string()),
            },
            prompt::SelectOption {
                value: "global".to_string(),
                label: "Global".to_string(),
                hint: Some("Update skills in home directory".to_string()),
            },
            prompt::SelectOption {
                value: "both".to_string(),
                label: "Both".to_string(),
                hint: Some("Update all skills".to_string()),
            },
        ],
        None,
    );
    match scope {
        Some(value) => match value.as_str() {
            "global" => UpdateScope::Global,
            "both" => UpdateScope::Both,
            _ => UpdateScope::Project,
        },
        None => {
            prompt::cancel("Cancelled");
            std::process::exit(0);
        }
    }
}

fn parse_update_options(args: &[String]) -> (UpdateCheckOptions, Vec<String>) {
    let mut options = UpdateCheckOptions {
        global: false,
        project: false,
        yes: false,
        skills: Vec::new(),
        report_outdated: false,
        outdated_names: std::cell::RefCell::new(Vec::new()),
    };
    let mut positional = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-g" | "--global" => options.global = true,
            "-p" | "--project" => options.project = true,
            "-y" | "--yes" => options.yes = true,
            other => {
                if !other.starts_with('-') {
                    positional.push(other.to_string());
                }
            }
        }
    }
    options.skills = positional.clone();
    (options, positional)
}

fn matches_skill_filter(name: &str, filter: &[String]) -> bool {
    filter.is_empty()
        || filter
            .iter()
            .any(|f| f.to_lowercase() == name.to_lowercase())
}

/// Check for updates or reinstall changed skills in their recorded scope.
pub fn run_update(args: &[String], mode: &'static str) {
    let (mut options, positional) = parse_update_options(args);
    if mode == "check" {
        options.yes = true;
        options.report_outdated = true;
    }
    let scope = resolve_update_scope(&options);

    if !options.skills.is_empty() && mode == "update" {
        println!(
            "{}Updating {}…{}",
            color::text(""),
            options.skills.join(", "),
            color::reset("")
        );
    } else {
        println!(
            "{}Checking for skill updates…{}",
            color::text(""),
            color::reset("")
        );
    }
    println!();

    let mut total_success = 0;
    let mut total_fail = 0;
    let mut total_found = 0;

    if scope == UpdateScope::Global || scope == UpdateScope::Both {
        if scope == UpdateScope::Both && options.skills.is_empty() {
            println!("{}Global Skills{}", color::bold(""), color::reset(""));
        }
        let (success, fail, checked) = update_global_skills(&options);
        total_success += success;
        total_fail += fail;
        total_found += checked;
        if scope == UpdateScope::Both && options.skills.is_empty() {
            println!();
        }
    }

    if scope == UpdateScope::Project || scope == UpdateScope::Both {
        if scope == UpdateScope::Both && options.skills.is_empty() {
            println!("{}Project Skills{}", color::bold(""), color::reset(""));
        }
        let (success, fail, found) = update_project_skills(&options);
        total_success += success;
        total_fail += fail;
        total_found += found;
    }

    if !options.skills.is_empty() && total_found == 0 {
        println!(
            "{}No installed skills found matching: {}{}",
            color::dim_gray(""),
            options.skills.join(", "),
            color::reset("")
        );
    }

    println!();
    if total_success > 0 {
        println!(
            "{}\u{2713} Updated {} skill(s){}",
            color::text(""),
            total_success,
            color::reset("")
        );
    }
    if total_fail > 0 {
        println!(
            "{}Failed to update {} skill(s){}",
            color::dim_gray(""),
            total_fail,
            color::reset("")
        );
        common::set_exit_code_1();
    }
    let outdated = options.outdated_names.borrow();
    if mode == "check" && !outdated.is_empty() {
        for name in outdated.iter() {
            println!(
                "{}{} has an update{}",
                color::text("\u{2191}"),
                sanitize_metadata(name),
                color::reset("")
            );
        }
        println!(
            "{}{} skill(s) can be updated; run {}skillz update{} to install{}",
            color::text(""),
            outdated.len(),
            color::bold(""),
            color::reset(""),
            color::reset("")
        );
        common::set_exit_code_1();
    }

    println!();
    let _ = positional;
}

fn report_outdated(options: &UpdateCheckOptions, name: &str) {
    if options.report_outdated {
        options.outdated_names.borrow_mut().push(name.to_string());
    }
}

fn report_failure(name: &str, error: &SkillsError) {
    prompt::log_error(&format!("{}: {}", sanitize_metadata(name), error.plain()));
}

struct TrackedSkill<'a> {
    name: &'a str,
    path: Option<&'a str>,
    hash: Option<&'a str>,
    tree_hash: Option<&'a str>,
    mode: Option<&'a str>,
    placements: &'a [locks::SkillPlacement],
    subagents: &'a [String],
}

struct UpdatedSkill {
    name: String,
    hash: String,
    tree_hash: Option<String>,
    path: Option<String>,
    placements: Vec<locks::SkillPlacement>,
}

type SourceGroups<'a> = BTreeMap<(&'a str, &'a str, Option<&'a str>), Vec<TrackedSkill<'a>>>;

fn is_git(kind: &str) -> bool {
    matches!(kind, "github" | "gitlab" | "git")
}

fn update_global_skills(options: &UpdateCheckOptions) -> (usize, usize, usize) {
    let mut lock = match locks::read_skill_lock() {
        Ok(lock) => lock,
        Err(err) => {
            report_failure("global lockfile", &err);
            return (0, 1, 0);
        }
    };
    let mut groups = SourceGroups::new();
    let mut found = 0;
    for (name, entry) in &lock.skills {
        if !matches_skill_filter(name, &options.skills) {
            continue;
        }
        found += 1;
        if !is_git(&entry.source_type) {
            println!(
                "{}: source type {} has no Git update",
                sanitize_metadata(name),
                entry.source_type
            );
            continue;
        }
        groups
            .entry((&entry.source_type, &entry.source_url, entry.ref_.as_deref()))
            .or_default()
            .push(TrackedSkill {
                name,
                path: entry.skill_path.as_deref(),
                hash: entry.computed_hash.as_deref(),
                tree_hash: Some(entry.skill_folder_hash.as_str()).filter(|hash| !hash.is_empty()),
                mode: entry.install_mode.as_deref(),
                placements: &entry.placements,
                subagents: &[],
            });
    }
    let (updates, mut failed) = update_groups(groups, options, true);
    for update in &updates {
        let entry = lock
            .skills
            .get_mut(&update.name)
            .expect("update came from the lock");
        entry.computed_hash = Some(update.hash.clone());
        entry.skill_folder_hash = update.tree_hash.clone().unwrap_or_default();
        entry.skill_path = update.path.clone();
        entry.placements = update.placements.clone();
        entry.updated_at = crate::cmd_add::iso_now();
    }
    if !updates.is_empty() {
        if let Err(err) = locks::write_skill_lock(&lock) {
            report_failure("global lockfile", &err);
            failed += updates.len();
            return (0, failed, found);
        }
    }
    (updates.len(), failed, found)
}

fn update_project_skills(options: &UpdateCheckOptions) -> (usize, usize, usize) {
    let cwd = common::current_dir();
    let mut lock = match locks::read_local_lock(&cwd) {
        Ok(lock) => lock,
        Err(err) => {
            report_failure("project lockfile", &err);
            return (0, 1, 0);
        }
    };
    let mut groups = SourceGroups::new();
    let mut found = 0;
    let mut failed = 0;
    for (name, entry) in &lock.skills {
        if !matches_skill_filter(name, &options.skills) || !is_git(&entry.source_type) {
            continue;
        }
        found += 1;
        match update_source::local_source(entry) {
            Ok(source) => groups
                .entry((&entry.source_type, source, entry.ref_.as_deref()))
                .or_default()
                .push(TrackedSkill {
                    name,
                    path: entry.skill_path.as_deref(),
                    hash: Some(&entry.computed_hash),
                    tree_hash: None,
                    mode: entry.install_mode.as_deref(),
                    placements: &entry.placements,
                    subagents: entry.subagents.as_deref().unwrap_or_default(),
                }),
            Err(err) => {
                report_failure(name, &err);
                failed += 1;
            }
        }
    }
    let (updates, group_failures) = update_groups(groups, options, false);
    failed += group_failures;
    for update in &updates {
        let entry = lock
            .skills
            .get_mut(&update.name)
            .expect("update came from the lock");
        entry.computed_hash.clone_from(&update.hash);
        entry.skill_path.clone_from(&update.path);
        entry.placements.clone_from(&update.placements);
    }
    if !updates.is_empty() {
        if let Err(err) = locks::write_local_lock(&lock, &cwd) {
            report_failure("project lockfile", &err);
            failed += updates.len();
            return (0, failed, found);
        }
    }
    (updates.len(), failed, found)
}

fn update_groups(
    groups: SourceGroups<'_>,
    options: &UpdateCheckOptions,
    global: bool,
) -> (Vec<UpdatedSkill>, usize) {
    let cwd = common::current_dir();
    let mut updates = Vec::new();
    let mut failed = 0;
    for ((kind, source, git_ref), items) in groups {
        let resolved = match update_source::resolve(source, kind, git_ref) {
            Ok(resolved) => resolved,
            Err(err) => {
                report_failure(source, &err);
                failed += items.len();
                continue;
            }
        };
        let payloads = SkillPayload::from_resolved(&resolved);
        let root = resolved.root_dir.as_deref();
        for entry in items {
            let result = (|| {
                let payload = find_locked(&payloads, root, entry.name, entry.path)?;
                let hash = payload.content_hash()?;
                let path = payload.source_path(root);
                let tree_hash = root
                    .zip(path.as_deref())
                    .and_then(|(root, path)| crate::git::get_git_tree_hash(root, path));
                let placements = if entry.placements.is_empty() {
                    infer_placements(entry.name, global, &cwd, entry.mode, entry.subagents)
                } else {
                    entry.placements.to_vec()
                };
                let current = match entry.hash {
                    Some(old) => hash == old,
                    None => entry.tree_hash.is_some() && entry.tree_hash == tree_hash.as_deref(),
                };
                if current
                    && entry.path == path.as_deref()
                    && !placements.is_empty()
                    && placements.iter().all(|placement| {
                        placement_path(entry.name, placement, global, &cwd)
                            .is_some_and(|path| path.join("SKILL.md").is_file())
                    })
                {
                    println!("{}: up to date", sanitize_metadata(entry.name));
                    return Ok(None);
                }
                if options.report_outdated {
                    report_outdated(options, entry.name);
                    return Ok(None);
                }
                let placements =
                    install_placements(payload, entry.name, &placements, global, &cwd)?;
                println!("{}: updated", sanitize_metadata(entry.name));
                Ok(Some(UpdatedSkill {
                    name: entry.name.to_string(),
                    hash,
                    tree_hash,
                    path,
                    placements,
                }))
            })();
            match result {
                Ok(Some(update)) => updates.push(update),
                Ok(None) => {}
                Err(err) => {
                    report_failure(entry.name, &err);
                    failed += 1;
                }
            }
        }
    }
    (updates, failed)
}

fn placement_path(
    name: &str,
    placement: &locks::SkillPlacement,
    global: bool,
    cwd: &Path,
) -> Option<std::path::PathBuf> {
    let agent = skills_store::registry::agent_by_id(&placement.agent)?;
    Some(installer::get_install_path(
        name,
        agent,
        global,
        cwd,
        placement.subagent.as_deref(),
    ))
}

fn infer_placements(
    name: &str,
    global: bool,
    cwd: &Path,
    mode: Option<&str>,
    subagents: &[String],
) -> Vec<locks::SkillPlacement> {
    use skills_store::registry::{self, DetectContext};
    let ctx = DetectContext::from_env(cwd);
    let supported = |agent: &&AgentConfig| !global || agent.global_skills_dir(&ctx).is_some();
    let fallback = common::non_interactive_target_agents(cwd)
        .into_iter()
        .filter(supported)
        .collect::<Vec<_>>();
    let fallback_mode = mode.unwrap_or_else(|| installer::default_install_mode(&fallback).as_str());
    let mut existing = Vec::new();
    for agent in registry::agents().iter().filter(supported) {
        let mut locations = vec![None];
        if agent.name == "eve" && !global {
            locations.extend(
                subagents
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .map(Some),
            );
        }
        for subagent in locations {
            let path = installer::get_install_path(name, agent, global, cwd, subagent.as_deref());
            if path.join("SKILL.md").is_file() {
                let observed_mode = if path.is_symlink()
                    || path == installer::get_canonical_path(name, global, cwd)
                {
                    installer::InstallMode::Symlink
                } else {
                    installer::InstallMode::Copy
                };
                existing.push(locks::SkillPlacement {
                    agent: agent.name.into(),
                    subagent,
                    mode: mode.unwrap_or(observed_mode.as_str()).into(),
                });
            }
        }
    }
    if !existing.is_empty() {
        return existing;
    }
    fallback
        .into_iter()
        .map(|agent| locks::SkillPlacement {
            agent: agent.name.into(),
            subagent: None,
            mode: fallback_mode.into(),
        })
        .collect()
}

pub(crate) fn install_placements(
    payload: &SkillPayload,
    name: &str,
    placements: &[locks::SkillPlacement],
    global: bool,
    cwd: &Path,
) -> Result<Vec<locks::SkillPlacement>, SkillsError> {
    let mut installed = Vec::new();
    let mut failures = Vec::new();
    for placement in placements {
        let Some(agent) = skills_store::registry::agent_by_id(&placement.agent) else {
            failures.push(format!("unknown agent {}", placement.agent));
            continue;
        };
        let mode = if placement.mode == "copy" {
            installer::InstallMode::Copy
        } else {
            installer::InstallMode::Symlink
        };
        let options = installer::InstallOptions {
            global: if global {
                installer::Scope::Global
            } else {
                installer::Scope::Project
            },
            cwd: Some(cwd.to_path_buf()),
            mode: Some(mode),
            eve_subagent: placement.subagent.clone(),
            create_missing_agent_root: true,
        };
        let mut warnings = Vec::new();
        let outcome = payload.install(name, agent, &options, &mut warnings);
        for warning in warnings {
            prompt::log_warn(&warning.plain());
        }
        if outcome.success
            && placement_path(name, placement, global, cwd)
                .is_some_and(|path| path.join("SKILL.md").is_file())
        {
            let mut actual = placement.clone();
            actual.mode = if outcome.symlink_failed {
                "copy"
            } else {
                mode.as_str()
            }
            .into();
            installed.push(actual);
        } else {
            failures.push(format!(
                "{}: {}",
                placement.agent,
                outcome.error.as_deref().unwrap_or("skill was not placed")
            ));
        }
    }
    if !failures.is_empty() || installed.is_empty() {
        let reason = if failures.is_empty() {
            "no install targets".to_string()
        } else {
            failures.join("; ")
        };
        return Err(SkillsError::new("SKILLS_INSTALL_FAILED", reason));
    }
    Ok(installed)
}
