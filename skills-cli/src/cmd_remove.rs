//! The `skills remove` command, ported from `src/remove.ts`.

use std::collections::HashMap;

use skills_store::error::{diag, SkillsError};
use skills_store::registry;
use skills_store::sanitize::sanitize_name;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::detect_agent;
use crate::prompt;

/// Parsed remove options.
#[derive(Debug, Clone, Default)]
pub struct RemoveOptions {
    /// `-g/--global`.
    pub global: bool,
    /// `-a/--agent` values.
    pub agent: Vec<String>,
    /// `-y/--yes`.
    pub yes: bool,
    /// `--all` (implies yes).
    pub all: bool,
}

/// Parse remove options (upstream `parseRemoveOptions`).
#[must_use]
pub fn parse_remove_options(args: &[String]) -> (Vec<String>, RemoveOptions) {
    let mut options = RemoveOptions::default();
    let mut skills = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-g" | "--global" => options.global = true,
            "-y" | "--yes" => options.yes = true,
            "--all" => {
                options.all = true;
                options.yes = true;
            }
            "-s" | "--skill" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    skills.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            "-a" | "--agent" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.agent.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            other => {
                if !other.starts_with('-') {
                    skills.push(other.to_string());
                }
            }
        }
        index += 1;
    }
    (skills, options)
}

/// The `skills remove` entry point (upstream `removeCommand`).
pub fn remove_command(skill_names: Vec<String>, mut options: RemoveOptions) {
    let agent_result = detect_agent::detect_agent();
    if agent_result.is_agent {
        options.yes = true;
        let name = agent_result.agent.clone().unwrap_or_default();
        prompt::log_info(&format!(
            "{} Agent detected — removing non-interactively",
            color::bg_cyan(&color::black(&color::bold(&format!(" {name} "))))
        ));
    }

    let mut skill_names = skill_names;
    if skill_names.contains(&"*".to_string()) {
        options.all = true;
        skill_names.retain(|name| name != "*");
    }

    let named_skills: Vec<String> = skill_names
        .iter()
        .filter(|name| name.as_str() != "*")
        .cloned()
        .collect();
    if options.all && !named_skills.is_empty() {
        prompt::log_error(&diag::remove_all_with_names(&named_skills[0]).plain());
        std::process::exit(1);
    }

    let is_global = options.global;
    let cwd = common::current_dir();

    let spinner = prompt::Spinner::start("Scanning for installed skills…");
    let installed_skills = installer::scan_installed_skill_names(is_global, &cwd);
    spinner.stop(&format!(
        "Found {} unique installed skill(s)",
        installed_skills.len()
    ));

    let lock_entries: HashMap<String, (String, String)> = if is_global {
        locks::read_skill_lock()
            .map(|lock| {
                lock.skills
                    .into_iter()
                    .map(|(name, entry)| (name, (entry.source, entry.source_type)))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        match locks::read_local_lock(&cwd) {
            Ok(lock) => lock
                .skills
                .into_iter()
                .map(|(name, entry)| (name, (entry.source, entry.source_type)))
                .collect(),
            Err(err) => {
                prompt::log_error(&err.plain());
                std::process::exit(1);
            }
        }
    };
    let lock_skill_keys: Vec<String> = lock_entries.keys().cloned().collect();
    let source_by_key: HashMap<String, String> = lock_entries
        .iter()
        .map(|(key, (source, _))| (key.clone(), source.clone()))
        .collect();

    let requested_skills: Vec<String> = if options.all {
        if is_global {
            lock_skill_keys.clone()
        } else {
            [installed_skills.clone(), lock_skill_keys.clone()].concat()
        }
    } else {
        skill_names.clone()
    };
    let resolved_requested = if options.all || !skill_names.is_empty() {
        installer::resolve_skills_to_remove(
            &requested_skills,
            &installed_skills,
            &lock_skill_keys,
            &source_by_key,
        )
    } else {
        Vec::new()
    };

    if installed_skills.is_empty() && resolved_requested.is_empty() {
        prompt::outro(&color::yellow("No skills found to remove."));
        return;
    }

    // Validate agent options before prompting.
    let valid_agent_ids: Vec<&str> = registry::agents().iter().map(|a| a.name).collect();
    if !options.agent.is_empty() {
        let invalid: Vec<String> = options
            .agent
            .iter()
            .filter(|a| !valid_agent_ids.contains(&a.as_str()))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            prompt::log_error(
                &diag::invalid_agents(
                    &invalid,
                    &valid_agent_ids
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                )
                .plain(),
            );
            std::process::exit(1);
        }
    }

    let selected_skills: Vec<String>;
    if options.all {
        selected_skills = resolved_requested;
    } else if !skill_names.is_empty() {
        selected_skills = resolved_requested;
        if selected_skills.is_empty() {
            prompt::log_error(&diag::no_matching_skills(&skill_names, &[]).plain());
            return;
        }
    } else {
        let choices: Vec<prompt::MultiOption> = installed_skills
            .iter()
            .map(|name| prompt::MultiOption {
                value: name.clone(),
                label: name.clone(),
                hint: None,
            })
            .collect();
        let selected = prompt::multiselect(
            &format!(
                "Select skills to remove {}",
                color::dim("(space to toggle)")
            ),
            &choices,
            &[],
            true,
        );
        let Some(selected) = selected else {
            prompt::cancel("Removal cancelled");
            std::process::exit(0);
        };
        selected_skills = installer::resolve_skills_to_remove(
            &selected,
            &installed_skills,
            &lock_skill_keys,
            &source_by_key,
        );
    }

    // Refuse removing globally tracked skills from a project whose canonical
    // dir is the global one.
    if !is_global {
        let globally_tracked =
            installer::find_globally_tracked_skills(&selected_skills, &cwd).unwrap_or_default();
        if !globally_tracked.is_empty() {
            prompt::log_error(&diag::remove_globally_tracked(&globally_tracked).plain());
            std::process::exit(1);
        }
    }

    if selected_skills.is_empty() {
        if options.all && is_global {
            prompt::outro(&color::yellow(
                "No globally managed skills found to remove.",
            ));
        } else {
            prompt::outro(&color::yellow("No skills found to remove."));
        }
        return;
    }

    let target_agents: Vec<String> = if !options.agent.is_empty() {
        options.agent.clone()
    } else {
        registry::agents()
            .iter()
            .map(|a| a.name.to_string())
            .collect()
    };

    if !options.yes {
        println!();
        prompt::log_info("Skills to remove:");
        for skill in &selected_skills {
            prompt::log_plain(&format!("  {} {}", color::red("\u{2022}"), skill));
        }
        println!();
        match prompt::confirm(
            &format!(
                "Are you sure you want to uninstall {} skill(s)?",
                selected_skills.len()
            ),
            true,
        ) {
            Some(true) => {}
            Some(false) | None => {
                prompt::cancel("Removal cancelled");
                std::process::exit(0);
            }
        }
    }

    let spinner = prompt::Spinner::start("Removing skills…");
    let mut warnings: Vec<SkillsError> = Vec::new();
    let results = installer::remove_skills(
        &selected_skills,
        &installer::RemoveOptions {
            global: if is_global {
                installer::Scope::Global
            } else {
                installer::Scope::Project
            },
            cwd: Some(cwd.clone()),
            target_agents,
            lockfile: Some(true),
        },
        &mut warnings,
    );
    spinner.stop("Removal process complete");

    let results = match results {
        Ok(results) => results,
        Err(err) => {
            prompt::log_error(&err.plain());
            std::process::exit(1);
        }
    };

    let successful = results.iter().filter(|r| r.success).count();
    let failed: Vec<&installer::RemoveOutcome> = results.iter().filter(|r| !r.success).collect();

    if successful > 0 {
        prompt::log_success(&color::green(&format!(
            "Successfully removed {successful} skill(s)"
        )));
    }
    if !failed.is_empty() {
        prompt::log_error(&color::red(&format!(
            "Failed to remove {} skill(s)",
            failed.len()
        )));
        for result in failed {
            prompt::log_plain(&format!(
                "  {} {}: {}",
                color::red("\u{2717}"),
                result.skill,
                result.error.as_deref().unwrap_or("unknown error")
            ));
        }
    }

    println!();
    prompt::outro(&color::green("Done!"));
    let _ = sanitize_name; // keep import parity with upstream helpers
}
