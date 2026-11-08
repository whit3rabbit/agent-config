//! The `skillz update` / `skillz check` command, ported from `src/update.ts`
//! (sequential source checks; same outputs and lock handling).

use std::path::Path;

use skills_store::error::SkillsError;
use skills_store::registry::AgentConfig;
use skills_store::sanitize::sanitize_metadata;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::prompt;
use crate::source_parser::{self, ParsedSource};
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

fn get_skip_reason(entry: &locks::SkillLockEntry) -> &'static str {
    if entry.source_type == "local" {
        return "Local path";
    }
    if entry.source_type == "git" {
        return "Git URL";
    }
    if entry.source_type == "well-known" {
        return "Well-known skill";
    }
    if entry.skill_folder_hash.is_empty() {
        return "Private or deleted repo";
    }
    if entry.skill_path.is_none() {
        return "No skill path recorded";
    }
    "No version tracking"
}

fn get_install_source(source_url: &str, source_type: &str, ref_: Option<&str>) -> String {
    let mut url = source_url.to_string();
    if source_type == "well-known" {
        if let Some(index) = url.find("/.well-known/") {
            url.truncate(index);
        }
    }
    update_source::format_source_input(&url, ref_)
}

fn print_skipped_skills(skipped: &[(&String, &str, &locks::SkillLockEntry)]) {
    if skipped.is_empty() {
        return;
    }
    println!();
    println!(
        "{}{} skill(s) cannot be checked automatically:{}",
        color::dim_gray(""),
        skipped.len(),
        color::reset("")
    );
    #[allow(clippy::type_complexity)]
    let mut grouped: Vec<(String, Vec<&str>, &str, Option<&str>, String)> = Vec::new();
    for (name, reason, entry) in skipped {
        let source =
            get_install_source(&entry.source_url, &entry.source_type, entry.ref_.as_deref());
        match grouped
            .iter_mut()
            .find(|(source_key, _, _, _, _)| *source_key == source)
        {
            Some((_, names, _, _, _)) => names.push(name),
            None => grouped.push((
                source,
                vec![name],
                reason,
                entry.ref_.as_deref(),
                entry.source_url.clone(),
            )),
        }
    }
    for (source, names, reason, ref_, source_url) in &grouped {
        if names.len() == 1 {
            println!(
                "  {}{} {}({}){}",
                color::text("•"),
                names[0],
                color::dim("("),
                reason,
                color::reset(")")
            );
        } else {
            println!(
                "  {}{} {}({}){}",
                color::text("•"),
                names.join(", "),
                color::dim("("),
                reason,
                color::reset(")")
            );
        }
        let update_source = update_source::format_source_input(source_url, *ref_);
        println!(
            "    {}To update: {}skillz add {update_source} -g -y{}",
            color::dim(""),
            color::text(""),
            color::reset("")
        );
        let _ = source;
    }
}

/// The `skillz update` / `skillz check` entry point.
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

fn placement_agents(global: bool) -> Vec<&'static AgentConfig> {
    let mut agents = common::non_interactive_target_agents(&common::current_dir());
    if global {
        let supported: Vec<&AgentConfig> = agents
            .iter()
            .copied()
            .filter(|a| registry_global_dir(a).is_some())
            .collect();
        if !supported.is_empty() {
            agents = supported;
        }
    }
    agents
}

fn registry_global_dir(agent: &AgentConfig) -> Option<std::path::PathBuf> {
    agent.global_skills_dir(&skills_store::registry::DetectContext::from_env(
        &common::current_dir(),
    ))
}

fn print_fetching(count: usize) {
    if count == 0 {
        return;
    }
    println!(
        "{}Fetching {} source{}…{}",
        color::dim_gray(""),
        count,
        if count == 1 { "" } else { "s" },
        color::reset("")
    );
}

fn report_install(name: &str, ok: bool, failures: &[String], error: Option<&str>) -> bool {
    let safe_name = sanitize_metadata(name);
    for failure in failures {
        println!(
            "  {}{}{}",
            color::dim_gray("\u{2717} "),
            failure,
            color::reset("")
        );
    }
    if ok && error.is_none() {
        println!(
            "  {}\u{2713}{} Updated {safe_name}",
            color::text(""),
            color::reset("")
        );
        return true;
    }
    println!(
        "  {}Failed to update {safe_name}{}{}",
        color::dim_gray("\u{2717}"),
        error.map(|err| format!(": {err}")).unwrap_or_default(),
        color::reset("")
    );
    false
}

fn update_global_skills(options: &UpdateCheckOptions) -> (usize, usize, usize) {
    let lock = locks::read_skill_lock().unwrap_or_default();
    let skill_names: Vec<String> = lock
        .skills
        .keys()
        .filter(|name| matches_skill_filter(name, &options.skills))
        .cloned()
        .collect();
    let mut success_count = 0;
    let mut fail_count = 0;

    if skill_names.is_empty() && options.skills.is_empty() {
        println!(
            "{}No global skills tracked in lock file.{}",
            color::dim_gray(""),
            color::reset("")
        );
        println!(
            "{}Install skills with{} {}skillz add <package> -g{}",
            color::dim_gray(""),
            color::reset(""),
            color::text(""),
            color::reset("")
        );
        return (0, 0, 0);
    }

    let mut skipped: Vec<(String, &'static str, locks::SkillLockEntry)> = Vec::new();
    let mut checkable: Vec<(String, locks::SkillLockEntry)> = Vec::new();

    for skill_name in &skill_names {
        let Some(entry) = lock.skills.get(skill_name) else {
            continue;
        };
        if entry.skill_folder_hash.is_empty() || entry.skill_path.is_none() {
            skipped.push((skill_name.clone(), get_skip_reason(entry), entry.clone()));
            continue;
        }
        checkable.push((skill_name.clone(), entry.clone()));
    }

    let on_disk: std::collections::HashSet<String> =
        installer::list_installed_skills(Some(true), &common::current_dir(), None)
            .into_iter()
            .map(|skill| skill.name)
            .collect();

    // Group by source + ref.
    type GlobalGroup = (String, Option<String>, Vec<(String, locks::SkillLockEntry)>);
    let mut groups: Vec<GlobalGroup> = Vec::new();
    for (name, entry) in &checkable {
        let name = name.clone();
        let entry = entry.clone();
        let key_source = entry.source.clone();
        match groups
            .iter_mut()
            .find(|(source, ref_, _)| *source == key_source && *ref_ == entry.ref_)
        {
            Some((_, _, items)) => items.push((name, entry)),
            None => groups.push((key_source, entry.ref_.clone(), vec![(name, entry)])),
        }
    }

    print_fetching(groups.len());
    let mut updates: Vec<(String, locks::SkillLockEntry, String)> = Vec::new();
    for (source, _ref_, items) in &groups {
        println!(
            "{}Checking skills from source: {}{}\x1b[K",
            color::dim_gray(""),
            source,
            color::reset("")
        );
        let first_entry = &items[0].1;
        let _parsed = parse_update_source(&first_entry.source_url, &first_entry.source_type);
        let checkout: Result<(Option<crate::blob::RepoTree>, Option<std::path::PathBuf>), ()> =
            (|| {
                if first_entry.source_type == "github" {
                    if let Some(tree) =
                        crate::blob::fetch_repo_tree(source, first_entry.ref_.as_deref())
                    {
                        let tree_paths: std::collections::HashSet<&str> = tree
                            .tree
                            .iter()
                            .filter(|entry| entry.kind == "blob")
                            .map(|entry| entry.path.as_str())
                            .collect();
                        let all_present = items.iter().all(|(_, entry)| {
                            entry
                                .skill_path
                                .as_ref()
                                .map(|path| tree_paths.contains(path.as_str()))
                                .unwrap_or(true)
                        });
                        if all_present {
                            return Ok((Some(tree), None));
                        }
                        println!(
                            "  {}Skill paths changed; resolving via Git clone{}",
                            color::dim_gray(""),
                            color::reset("")
                        );
                    } else {
                        println!(
                            "  {}GitHub API unavailable; checking via Git clone{}",
                            color::dim_gray(""),
                            color::reset("")
                        );
                    }
                }
                let temp =
                    crate::git::clone_repo(&first_entry.source_url, first_entry.ref_.as_deref())
                        .map_err(|_| ())?;
                Ok((None, Some(temp)))
            })();

        match checkout {
            Err(()) => {
                println!(
                    "  {}Failed to check skills from {source}{}",
                    color::dim_gray(""),
                    color::reset("")
                );
                fail_count += items.len();
                continue;
            }
            Ok((tree, temp_dir)) => {
                if let Some(tree) = &tree {
                    for (name, entry) in items {
                        let latest = entry.skill_path.as_ref().and_then(|path| {
                            crate::blob::get_skill_folder_hash_from_tree(tree, path)
                        });
                        if let Some(latest) = latest {
                            if latest != entry.skill_folder_hash || !on_disk.contains(name) {
                                updates.push((name.clone(), entry.clone(), latest));
                            }
                        }
                    }
                } else if let Some(temp_dir) = temp_dir {
                    let discovery = skills_store::skills::discover_skills(
                        &temp_dir,
                        None,
                        &skills_store::skills::DiscoverOptions {
                            full_depth: true,
                            include_duplicate_names: true,
                            ..Default::default()
                        },
                    );
                    match discovery {
                        Ok(discovered) => {
                            for (name, entry) in items {
                                let Some(locked_path) = &entry.skill_path else {
                                    continue;
                                };
                                // Find the skill with the same name (exact path or relocation).
                                let matches: Vec<&skills_store::skills::Skill> = discovered
                                    .skills
                                    .iter()
                                    .filter(|skill| {
                                        skills_store::sanitize::normalize_skill_name(&skill.name)
                                            == skills_store::sanitize::normalize_skill_name(name)
                                    })
                                    .collect();
                                if matches.is_empty() {
                                    continue;
                                }
                                if matches.len() > 1 {
                                    println!(
                                        "  {}Multiple current paths match {name}; skipping{}",
                                        color::dim_gray(""),
                                        color::reset("")
                                    );
                                    continue;
                                }
                                let current = matches[0];
                                let folder_hash = locks::compute_folder_hash(&current.path).ok();
                                let relocated = format!(
                                    "{}/SKILL.md",
                                    current.path.to_string_lossy().replace('\\', "/")
                                ) != *locked_path;
                                if relocated
                                    || folder_hash.as_ref().is_some_and(|hash| {
                                        *hash != entry.skill_folder_hash || !on_disk.contains(name)
                                    })
                                {
                                    let mut entry = entry.clone();
                                    entry.skill_path = Some(format!(
                                        "{}/SKILL.md",
                                        relative_path(&temp_dir, &current.path)
                                    ));
                                    updates.push((
                                        name.clone(),
                                        entry,
                                        folder_hash.unwrap_or_default(),
                                    ));
                                }
                            }
                        }
                        Err(_) => {
                            println!(
                                "  {}Failed to check skills from {source}{}",
                                color::dim_gray(""),
                                color::reset("")
                            );
                        }
                    }
                    let _ = crate::git::cleanup_temp_dir(&temp_dir);
                }
            }
        }
    }

    let checked_count = skill_names.len();
    if checkable.is_empty() && skipped.is_empty() {
        return (0, 0, 0);
    }
    if updates.is_empty() {
        println!(
            "{}\u{2713} All global skills are up to date{}",
            color::text(""),
            color::reset("")
        );
        print_skipped_skills(
            &skipped
                .iter()
                .map(|(name, reason, entry)| (name, *reason, entry))
                .collect::<Vec<_>>(),
        );
        return (0, 0, checked_count);
    }
    if options.report_outdated {
        for (name, _, _) in &updates {
            report_outdated(options, name);
        }
        print_skipped_skills(
            &skipped
                .iter()
                .map(|(name, reason, entry)| (name, *reason, entry))
                .collect::<Vec<_>>(),
        );
        return (0, 0, checked_count);
    }

    println!(
        "{}Found {} global update(s){}",
        color::text(""),
        updates.len(),
        color::reset("")
    );
    println!();

    let agents = placement_agents(true);
    for (name, entry, new_hash) in &updates {
        println!(
            "{}Updating {}…{}",
            color::text(""),
            sanitize_metadata(name),
            color::reset("")
        );
        let install_url = update_source::build_global_update_install_source(entry);
        let Some(install_url) = install_url else {
            println!(
                "  {}Cannot update {}: lock file is missing sourceUrl for this generic Git source{}",
                color::dim_gray("\u{2717}"),
                sanitize_metadata(name),
                color::reset("")
            );
            fail_count += 1;
            continue;
        };
        let parsed = parse_update_source(&install_url, &entry.source_type);
        let full_depth = update_source::should_use_full_depth_for_global(entry);
        match reinstall_skill(
            &parsed,
            name,
            &agents,
            false,
            entry.install_mode.as_deref() == Some("copy"),
            full_depth,
        ) {
            Ok(()) => {
                if let Ok(mut lock) = locks::read_skill_lock() {
                    if let Some(mut entry) = lock.skills.get(name).cloned() {
                        entry.skill_folder_hash = new_hash.clone();
                        entry.updated_at = crate::cmd_add::iso_now();
                        lock.skills.insert(name.clone(), entry);
                        let _ = locks::write_skill_lock(&lock);
                    }
                }
                if report_install(name, true, &[], None) {
                    success_count += 1;
                }
            }
            Err(error) => {
                if report_install(name, false, &[error], None) {
                    success_count += 1;
                } else {
                    fail_count += 1;
                }
            }
        }
    }

    print_skipped_skills(
        &skipped
            .iter()
            .map(|(name, reason, entry)| (name, *reason, entry))
            .collect::<Vec<_>>(),
    );
    (success_count, fail_count, checked_count)
}

fn parse_update_source(install_url: &str, source_type: &str) -> ParsedSource {
    if source_type == "github" {
        source_parser::parse_source_with_host(install_url, "github.com")
    } else {
        source_parser::parse_source(install_url)
    }
}

fn relative_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn reinstall_skill(
    parsed: &ParsedSource,
    skill_name: &str,
    agents: &[&'static AgentConfig],
    global: bool,
    copy: bool,
    full_depth: bool,
) -> Result<(), String> {
    let resolve_options = crate::resolver::ResolveOptions {
        full_depth,
        include_internal: true,
        ..Default::default()
    };
    let resolved = crate::resolver::resolve_skills(parsed, &resolve_options, None)
        .map_err(|error| error.plain())?;
    let present = if resolved.blob_result.is_some() {
        resolved
            .blob_result
            .as_ref()
            .map(|blob| {
                blob.skills
                    .iter()
                    .any(|skill| skill.name.eq_ignore_ascii_case(skill_name))
            })
            .unwrap_or(false)
    } else {
        resolved
            .discovery
            .skills
            .iter()
            .any(|skill| skill.name.eq_ignore_ascii_case(skill_name))
    };
    if !present {
        return Err(format!(
            "skill {skill_name} not found in the fetched source"
        ));
    }

    let mode = if copy {
        skills_store::installer::InstallMode::Copy
    } else {
        common_default_mode(agents)
    };
    let cwd = common::current_dir();
    let mut warnings: Vec<SkillsError> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut any_success = false;
    let skill_path = match resolved.blob_result.as_ref() {
        Some(blob) => {
            let skill = blob
                .skills
                .iter()
                .find(|skill| skill.name.eq_ignore_ascii_case(skill_name));
            match skill {
                Some(skill) => {
                    for agent in agents {
                        let outcome = skills_store::installer::install_skill_snapshot(
                            &skill.name,
                            &skill.files,
                            agent,
                            &skills_store::installer::InstallOptions {
                                global: if global {
                                    skills_store::installer::Scope::Global
                                } else {
                                    skills_store::installer::Scope::Project
                                },
                                cwd: Some(cwd.clone()),
                                mode: Some(mode),
                                ..Default::default()
                            },
                            &mut warnings,
                        );
                        if outcome.success {
                            any_success = true;
                        } else if let Some(error) = outcome.error {
                            failures.push(format!("{}: {error}", agent.display_name));
                        }
                    }
                    skill.name.clone()
                }
                None => return Err("skill not found".to_string()),
            }
        }
        None => {
            let skill = resolved
                .discovery
                .skills
                .iter()
                .find(|skill| skill.name.eq_ignore_ascii_case(skill_name));
            match skill {
                Some(skill) => {
                    for agent in agents {
                        let outcome = skills_store::installer::install_skill_dir(
                            &skill.path,
                            agent,
                            &skills_store::installer::InstallOptions {
                                global: if global {
                                    skills_store::installer::Scope::Global
                                } else {
                                    skills_store::installer::Scope::Project
                                },
                                cwd: Some(cwd.clone()),
                                mode: Some(mode),
                                ..Default::default()
                            },
                            &mut warnings,
                        );
                        if outcome.success {
                            any_success = true;
                        } else if let Some(error) = outcome.error {
                            failures.push(format!("{}: {error}", agent.display_name));
                        }
                    }
                    skill.name.clone()
                }
                None => return Err("skill not found".to_string()),
            }
        }
    };
    let _ = skill_path;
    if !any_success && !failures.is_empty() {
        return Err(failures.join("; "));
    }
    Ok(())
}

fn common_default_mode(agents: &[&'static AgentConfig]) -> skills_store::installer::InstallMode {
    skills_store::installer::default_install_mode(agents)
}

fn update_project_skills(options: &UpdateCheckOptions) -> (usize, usize, usize) {
    let cwd = common::current_dir();
    let local_lock = locks::read_local_lock(&cwd).unwrap_or_default();
    let project_skills: Vec<(String, locks::LocalSkillLockEntry)> = local_lock
        .skills
        .iter()
        .filter(|(name, entry)| {
            matches_skill_filter(name, &options.skills)
                && entry.source_type != "node_modules"
                && entry.source_type != "local"
        })
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect();
    let mut success_count = 0;
    let mut fail_count = 0;

    if project_skills.is_empty() && options.skills.is_empty() {
        println!(
            "{}No project skills to update.{}",
            color::dim_gray(""),
            color::reset("")
        );
        println!(
            "{}Install project skills with{} {}skillz add <package>{}",
            color::dim_gray(""),
            color::reset(""),
            color::text(""),
            color::reset("")
        );
        return (0, 0, 0);
    }

    let updatable: Vec<&(String, locks::LocalSkillLockEntry)> = project_skills
        .iter()
        .filter(|(_, entry)| entry.skill_path.is_some())
        .collect();
    let legacy_count = project_skills.len() - updatable.len();

    if updatable.is_empty() {
        println!(
            "{}No project skills can be updated in place.{}",
            color::dim_gray(""),
            color::reset("")
        );
        return (0, 0, project_skills.len());
    }

    // "Updating for:" banner.
    let mut target_parts: Vec<String> = Vec::new();
    let mut has_universal = false;
    let mut agent_names: Vec<String> = Vec::new();
    for agent in skills_store::registry::agents() {
        if agent.is_universal() {
            if !has_universal && cwd.join(".agents").exists() {
                has_universal = true;
            }
        } else {
            let agent_root = agent.skills_dir.split('/').next().unwrap_or("");
            if cwd.join(agent_root).exists() {
                agent_names.push(agent.display_name.to_string());
            }
        }
    }
    if has_universal {
        target_parts.push("Universal".to_string());
    }
    target_parts.extend(agent_names);
    if !target_parts.is_empty() {
        println!(
            "{}Updating for: {}{}",
            color::text(""),
            target_parts.join(", "),
            color::reset("")
        );
    }

    println!(
        "{}Checking {} skill(s)…{}",
        color::text(""),
        updatable.len(),
        color::reset("")
    );
    println!();

    let on_disk: std::collections::HashSet<String> =
        installer::list_installed_skills(Some(false), &cwd, None)
            .into_iter()
            .map(|skill| skill.name)
            .collect();
    let agents = placement_agents(false);

    // Group by source + ref.
    type LocalGroup = (
        String,
        Option<String>,
        Vec<(String, locks::LocalSkillLockEntry)>,
    );
    let mut groups: Vec<LocalGroup> = Vec::new();
    for (name, entry) in &updatable {
        let source = entry
            .source_url
            .clone()
            .unwrap_or_else(|| entry.source.clone());
        match groups
            .iter_mut()
            .find(|(source_key, ref_, _)| *source_key == source && *ref_ == entry.ref_)
        {
            Some((_, _, items)) => items.push((name.clone(), entry.clone())),
            None => groups.push((
                source,
                entry.ref_.clone(),
                vec![(name.clone(), entry.clone())],
            )),
        }
    }

    print_fetching(groups.len());
    let mut up_to_date_count = 0;
    for (source, _ref_, items) in &groups {
        let first_entry = &items[0].1;
        let Some(clone_source) = update_source::build_local_clone_source(first_entry) else {
            fail_count += items.len();
            println!(
                "{}Cannot update {source}: skills-lock.json is missing sourceUrl for this generic Git source{}",
                color::dim_gray("\u{2717}"),
                color::reset("")
            );
            continue;
        };
        println!(
            "{}Checking skills from source: {}{}\x1b[K",
            color::dim_gray(""),
            source,
            color::reset("")
        );
        let temp_dir = match crate::git::clone_repo(&clone_source, first_entry.ref_.as_deref()) {
            Ok(temp) => temp,
            Err(_) => {
                fail_count += items.len();
                println!(
                    "{}Failed to check for deleted skills from {source}{}",
                    color::dim_gray(""),
                    color::reset("")
                );
                continue;
            }
        };
        let discovery = skills_store::skills::discover_skills(
            &temp_dir,
            None,
            &skills_store::skills::DiscoverOptions {
                full_depth: true,
                include_duplicate_names: true,
                ..Default::default()
            },
        );
        match discovery {
            Ok(discovered) => {
                for (name, entry) in items {
                    let matches: Vec<&skills_store::skills::Skill> = discovered
                        .skills
                        .iter()
                        .filter(|skill| {
                            skills_store::sanitize::normalize_skill_name(&skill.name)
                                == skills_store::sanitize::normalize_skill_name(name)
                        })
                        .collect();
                    if matches.is_empty() {
                        continue;
                    }
                    if matches.len() > 1 {
                        continue;
                    }
                    let current = matches[0];
                    let computed_hash = locks::compute_folder_hash(&current.path).ok();
                    let same_path = entry
                        .skill_path
                        .as_ref()
                        .map(|locked| {
                            format!("{}/SKILL.md", relative_path(&temp_dir, &current.path))
                                == *locked
                        })
                        .unwrap_or(false);
                    if same_path
                        && computed_hash.as_deref() == Some(entry.computed_hash.as_str())
                        && on_disk.contains(name)
                    {
                        up_to_date_count += 1;
                        continue;
                    }
                    if options.report_outdated {
                        report_outdated(options, name);
                        continue;
                    }
                    println!(
                        "{}Updating {}…{}",
                        color::text(""),
                        sanitize_metadata(name),
                        color::reset("")
                    );
                    let copy = entry.install_mode.as_deref() == Some("copy");
                    let parsed = source_parser::parse_source(source);
                    let full_depth = update_source::should_use_full_depth_for_update(entry);
                    match reinstall_skill(&parsed, name, &agents, false, copy, full_depth) {
                        Ok(()) => {
                            if let Ok(hash) = locks::compute_folder_hash(&current.path) {
                                if let Ok(mut lock) = locks::read_local_lock(&cwd) {
                                    if let Some(mut current_entry) = lock.skills.get(name).cloned()
                                    {
                                        current_entry.computed_hash = hash;
                                        current_entry.skill_path = Some(format!(
                                            "{}/SKILL.md",
                                            relative_path(&temp_dir, &current.path)
                                        ));
                                        lock.skills.insert(name.clone(), current_entry);
                                        let _ = locks::write_local_lock(&lock, &cwd);
                                    }
                                }
                            }
                            if report_install(name, true, &[], None) {
                                success_count += 1;
                            }
                        }
                        Err(error) => {
                            if report_install(name, false, &[], Some(&error)) {
                                success_count += 1;
                            } else {
                                fail_count += 1;
                            }
                        }
                    }
                }
            }
            Err(_) => {
                fail_count += items.len();
                println!(
                    "{}Failed to check for deleted skills from {source}{}",
                    color::dim_gray(""),
                    color::reset("")
                );
            }
        }
        let _ = crate::git::cleanup_temp_dir(&temp_dir);
    }

    if up_to_date_count > 0 {
        println!(
            "{}{} project skill(s) already up to date{}",
            color::dim_gray(""),
            up_to_date_count,
            color::reset("")
        );
    }
    if legacy_count > 0 {
        println!(
            "{}{legacy_count} project skill(s) cannot be updated automatically (installed before skillPath tracking):{}",
            color::dim_gray(""),
            color::reset("")
        );
    }
    (success_count, fail_count, project_skills.len())
}
