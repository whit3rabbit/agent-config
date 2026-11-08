//! The `skillz experimental_sync` command, ported from `src/sync.ts`:
//! install skills shipped by the project's node_modules dependencies and by
//! `skills` fields in package.json files.

use std::path::{Path, PathBuf};

use skills_store::error::{diag, SkillsError};
use skills_store::registry::{self, AgentConfig};
use skills_store::sanitize::sanitize_name;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::detect_agent;
use crate::prompt::{self, Spinner};
use crate::search_multiselect::{LockedSection, SearchItem, SearchMultiselectOptions};
use crate::source_parser;

#[derive(Debug, Default, Clone)]
pub struct SyncOptions {
    /// `-a/--agent` values.
    pub agent: Vec<String>,
    /// `-y/--yes`.
    pub yes: bool,
    /// `--copy`.
    pub copy: bool,
    /// `--dry-run`.
    pub dry_run: bool,
    /// `--no-cleanup`.
    pub cleanup: Option<bool>,
    /// `--include` patterns.
    pub include: Vec<String>,
    /// `--exclude` patterns.
    pub exclude: Vec<String>,
    /// `--no-remote`.
    pub remote: Option<bool>,
    /// `-r/--recursive`.
    pub recursive: bool,
    /// `--frozen-lockfile` tri-state.
    pub frozen_lockfile: Option<bool>,
}

/// Parse sync options (upstream `parseSyncOptions`).
#[must_use]
pub fn parse_sync_options(args: &[String]) -> SyncOptions {
    let mut options = SyncOptions::default();
    let mut index = 0;
    while index < args.len() {
        let take_values =
            |options: &mut SyncOptions, field: fn(&mut SyncOptions, String), index: &mut usize| {
                *index += 1;
                while *index < args.len() && !args[*index].starts_with('-') {
                    field(options, args[*index].clone());
                    *index += 1;
                }
                *index = index.saturating_sub(1);
            };
        match args[index].as_str() {
            "-y" | "--yes" => options.yes = true,
            "--copy" => options.copy = true,
            "--dry-run" => options.dry_run = true,
            "--no-cleanup" => options.cleanup = Some(false),
            "--no-remote" => options.remote = Some(false),
            "--frozen-lockfile" => options.frozen_lockfile = Some(true),
            "--no-frozen-lockfile" => options.frozen_lockfile = Some(false),
            "-r" | "--recursive" => options.recursive = true,
            "-a" | "--agent" => take_values(&mut options, |o, v| o.agent.push(v), &mut index),
            "--include" => take_values(&mut options, |o, v| o.include.push(v), &mut index),
            "--exclude" => take_values(&mut options, |o, v| o.exclude.push(v), &mut index),
            _ => {}
        }
        index += 1;
    }
    options
}

#[derive(Debug, Clone)]
struct PackageSkill {
    skill: skills_store::skills::Skill,
    package_name: String,
    package_version: Option<String>,
    /// SKILL.md path relative to the package root.
    skill_path: String,
    /// Package whose `skills` field requested this (`.` = project).
    via: Option<String>,
    depth: usize,
}

fn read_package_json(dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn discover_package_skills(pkg_dir: &Path, package_name: &str, depth: usize) -> Vec<PackageSkill> {
    let Some(pkg) = read_package_json(pkg_dir) else {
        return Vec::new();
    };
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    if skills_store::skills::has_skill_md(pkg_dir) {
        if let Some(skill) = skills_store::skills::parse_skill_md(&pkg_dir.join("SKILL.md")) {
            return vec![PackageSkill {
                skill,
                package_name: package_name.to_string(),
                package_version: version,
                skill_path: "SKILL.md".to_string(),
                via: None,
                depth,
            }];
        }
    }

    let mut skills = Vec::new();
    for dir in ["skills", "dist/skills"] {
        let Ok(entries) = std::fs::read_dir(pkg_dir.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let skill_dir = entry.path();
            if !skills_store::skills::has_skill_md(&skill_dir) {
                continue;
            }
            let Some(skill) = skills_store::skills::parse_skill_md(&skill_dir.join("SKILL.md"))
            else {
                continue;
            };
            skills.push(PackageSkill {
                skill,
                package_name: package_name.to_string(),
                package_version: version.clone(),
                skill_path: format!("{dir}/{}/SKILL.md", entry.file_name().to_string_lossy()),
                via: None,
                depth,
            });
        }
    }
    skills
}

fn find_installed_package(from: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = from.to_path_buf();
    loop {
        let candidate = dir.join("node_modules").join(name);
        if candidate.join("package.json").exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

const DEPENDENCY_FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// Discover skills shipped by dependencies plus `npm:` targets (upstream
/// `discoverNodeModuleSkills`, simplified to its core: project + remotes).
fn discover_node_module_skills(
    cwd: &Path,
    recursive: bool,
) -> (Vec<PackageSkill>, Vec<SkillsError>) {
    let mut skills: Vec<PackageSkill> = Vec::new();
    let mut warnings: Vec<SkillsError> = Vec::new();
    let Some(pkg) = read_package_json(cwd) else {
        return (skills, warnings);
    };

    let mut seen = std::collections::HashSet::new();
    let add = |found: Vec<PackageSkill>,
               skills: &mut Vec<PackageSkill>,
               seen: &mut std::collections::HashSet<PathBuf>| {
        for skill in found {
            let key = std::fs::canonicalize(&skill.skill.path)
                .unwrap_or_else(|_| skill.skill.path.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            skills.push(skill);
        }
    };

    let mut declarers: Vec<(String, PathBuf, usize)> =
        vec![(".".to_string(), cwd.to_path_buf(), 0)];

    // Direct dependencies.
    let mut deps: Vec<String> = Vec::new();
    for field in DEPENDENCY_FIELDS {
        if let Some(map) = pkg.get(field).and_then(|v| v.as_object()) {
            for name in map.keys() {
                deps.push(name.clone());
            }
        }
    }
    for name in &deps {
        if let Some(dir) = find_installed_package(cwd, name) {
            add(
                discover_package_skills(&dir, name, 0),
                &mut skills,
                &mut seen,
            );
            declarers.push((name.clone(), dir, 1));
        }
    }

    // Workspace packages with --recursive.
    if recursive {
        for workspace in find_workspace_packages(cwd, &pkg) {
            if let Some(workspace_pkg) = read_package_json(&workspace) {
                let name = workspace_pkg
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| "workspace".to_string());
                let mut deps: Vec<String> = Vec::new();
                for field in DEPENDENCY_FIELDS {
                    if let Some(map) = workspace_pkg.get(field).and_then(|v| v.as_object()) {
                        for dep in map.keys() {
                            deps.push(dep.clone());
                        }
                    }
                }
                add(
                    discover_package_skills(&workspace, &name, 1),
                    &mut skills,
                    &mut seen,
                );
                for dep in deps {
                    if let Some(dir) = find_installed_package(&workspace, &dep) {
                        add(
                            discover_package_skills(&dir, &dep, 1),
                            &mut skills,
                            &mut seen,
                        );
                    }
                }
            }
        }
    }

    // package.json `skills` fields (npm: targets only; remotes are handled by
    // the caller through the add engine).
    let mut visited = std::collections::HashSet::new();
    for (name, dir, depth) in declarers {
        let Ok(real_dir) = std::fs::canonicalize(&dir) else {
            continue;
        };
        if visited.contains(&real_dir) {
            continue;
        }
        visited.insert(real_dir);
        let Some(declarer_pkg) = read_package_json(&dir) else {
            continue;
        };
        let Some(field) = declarer_pkg.get("skills") else {
            continue;
        };
        let Some(entries) = field.as_array() else {
            if name == "." {
                warnings.push(diag_skills_not_array());
            }
            continue;
        };
        for raw in entries {
            let source = raw
                .as_str()
                .or_else(|| raw.get("source").and_then(|s| s.as_str()));
            let Some(source) = source else { continue };
            if let Some(package) = source.strip_prefix("npm:") {
                let target = find_installed_package(&dir, package);
                let Some(target) = target else {
                    warnings.push(SkillsError::new(
                        "SKILLS_PACKAGE_SKILLS_UNRESOLVED_NPM",
                        format!("{name}: cannot resolve \"npm:{package}\"; is it a dependency?"),
                    ));
                    continue;
                };
                let found = discover_package_skills(&target, package, depth)
                    .into_iter()
                    .map(|mut skill| {
                        skill.via = Some(name.clone());
                        skill
                    })
                    .collect();
                add(found, &mut skills, &mut seen);
            }
        }
    }

    (skills, warnings)
}

fn diag_skills_not_array() -> SkillsError {
    SkillsError::new(
        "SKILLS_PACKAGE_SKILLS_NOT_ARRAY",
        "package.json: \"skills\" must be an array",
    )
}

fn find_workspace_packages(cwd: &Path, pkg: &serde_json::Value) -> Vec<PathBuf> {
    let mut patterns: Vec<String> = Vec::new();
    match pkg.get("workspaces") {
        Some(serde_json::Value::Array(items)) => {
            for item in items {
                if let Some(text) = item.as_str() {
                    patterns.push(text.to_string());
                }
            }
        }
        Some(serde_json::Value::Object(map)) => {
            if let Some(items) = map.get("packages").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(text) = item.as_str() {
                        patterns.push(text.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    if let Ok(text) = std::fs::read_to_string(cwd.join("pnpm-workspace.yaml")) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("- ") {
                patterns.push(rest.trim().trim_matches(['"', '\'']).to_string());
            }
        }
    }
    let mut dirs = std::collections::BTreeSet::new();
    for pattern in patterns {
        if pattern.starts_with('!') {
            continue;
        }
        // Support simple `packages/*`-style globs without full glob semantics.
        if let Some((prefix, _)) = pattern.split_once('*') {
            let base = cwd.join(prefix.trim_end_matches('/'));
            let Ok(entries) = std::fs::read_dir(&base) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.path().join("package.json").exists() {
                    dirs.insert(entry.path());
                }
            }
        } else if cwd.join(&pattern).join("package.json").exists() {
            dirs.insert(cwd.join(&pattern));
        }
    }
    dirs.into_iter().collect()
}

fn matches_skill(skill: &PackageSkill, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        let (package_pattern, skill_pattern) = match pattern.split_once('#') {
            Some((package, skill)) => (package.to_string(), skill.to_string()),
            None => (pattern.clone(), "**".to_string()),
        };
        let package_matches = package_pattern == "**"
            || package_pattern == skill.package_name
            || glob_simple(&package_pattern, &skill.package_name);
        let skill_matches =
            skill_pattern == "**" || glob_simple(&skill_pattern, &sanitize_name(&skill.skill.name));
        package_matches && skill_matches
    })
}

/// Simple glob: `*` and `**` wildcards, otherwise exact/prefix matching.
fn glob_simple(pattern: &str, value: &str) -> bool {
    if pattern == value {
        return true;
    }
    let mut pattern_parts = pattern.split('/');
    let mut value_parts = value.split('/');
    loop {
        match (pattern_parts.next(), value_parts.next()) {
            (None, None) => return true,
            (None, Some(_)) | (Some(_), None) => return false,
            (Some(p), Some(v)) => {
                if p == "**" {
                    return true;
                }
                if p.contains('*') {
                    let (before, after) = p.split_once('*').unwrap();
                    if !(v.starts_with(before) && v.ends_with(after)) {
                        return false;
                    }
                } else if p != v {
                    return false;
                }
            }
        }
    }
}

fn allowed_by_filters(skill: &PackageSkill, options: &SyncOptions) -> bool {
    (!options.include.iter().any(|p| !p.contains('#')) || matches_skill(skill, &options.include))
        && !(!options.exclude.is_empty() && matches_skill(skill, &options.exclude))
}

fn is_under_node_modules(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "node_modules")
}

fn foreign_destination(
    dest: &Path,
    canonical_dir: &Path,
    lock_entry: Option<&locks::LocalSkillLockEntry>,
    cwd: &Path,
) -> Option<String> {
    let Ok(stats) = std::fs::symlink_metadata(dest) else {
        return None;
    };
    if stats.file_type().is_symlink() {
        let Ok(target) = std::fs::read_link(dest) else {
            return None;
        };
        let resolved = dest.parent().unwrap_or(Path::new("/")).join(target);
        if is_under_node_modules(&resolved) || resolved == canonical_dir {
            return None;
        }
        return Some(format!(
            "{} is a symlink to {}",
            common::shorten_path(dest, cwd),
            common::shorten_path(&resolved, cwd)
        ));
    }
    let installed_by_sync = lock_entry
        .map(|entry| entry.source_type == "node_modules" || entry.via.is_some())
        .unwrap_or(false);
    if installed_by_sync {
        return None;
    }
    Some(format!(
        "{} already exists and was not installed by sync",
        common::shorten_path(dest, cwd)
    ))
}

fn installed_by_sync(entry: Option<&locks::LocalSkillLockEntry>) -> bool {
    entry
        .map(|entry| entry.source_type == "node_modules" || entry.via.is_some())
        .unwrap_or(false)
}

fn blocked_reason(
    name: &str,
    target_agents: &[&'static AgentConfig],
    lock_entry: Option<&locks::LocalSkillLockEntry>,
    cwd: &Path,
) -> Option<String> {
    if let Some(entry) = lock_entry {
        if !installed_by_sync(Some(entry)) {
            return Some(format!("installed with `skillz add` from {}", entry.source));
        }
    }
    let canonical_dir = installer::get_canonical_path(name, false, cwd);
    let mut destinations = vec![canonical_dir];
    for agent in target_agents {
        destinations.push(installer::get_install_path(name, agent, false, cwd, None));
    }
    for dest in destinations {
        if let Some(reason) = foreign_destination(
            &dest,
            &installer::get_canonical_path(name, false, cwd),
            lock_entry,
            cwd,
        ) {
            return Some(reason);
        }
    }
    None
}

/// The `skillz experimental_sync` entry point (upstream `runSync`).
#[allow(clippy::too_many_lines)]
pub fn run_sync(_args: &[String], mut options: SyncOptions) {
    let cwd = common::current_dir();

    let agent_result = detect_agent::detect_agent();
    if agent_result.is_agent {
        options.yes = true;
        if options.agent.is_empty() {
            if let Some(name) = &agent_result.agent {
                if let Some(agent_id) = detect_agent::get_agent_type(name) {
                    if let Some(agent) = registry::agent_by_id(agent_id) {
                        let mut agent_list = vec![agent];
                        for universal in registry::universal_agents() {
                            if !agent_list.iter().any(|a| a.name == universal.name) {
                                agent_list.push(universal);
                            }
                        }
                        options.agent = agent_list.iter().map(|a| a.name.to_string()).collect();
                    }
                }
            }
        }
    }

    println!();
    if !agent_result.is_agent {
        prompt::intro(&color::bg_cyan(&color::black(" skillz experimental_sync ")));
    }
    if agent_result.is_agent {
        let name = agent_result.agent.clone().unwrap_or_default();
        prompt::log_info(&format!(
            "{} Agent detected — installing non-interactively",
            color::bg_cyan(&color::black(&color::bold(&format!(" {name} "))))
        ));
    }

    let spinner = Spinner::start("Scanning node_modules for skills…");
    let (discovered, warnings) = discover_node_module_skills(&cwd, options.recursive);
    let discovered: Vec<PackageSkill> = discovered
        .into_iter()
        .filter(|skill| allowed_by_filters(skill, &options))
        .collect();
    let stop_message = if discovered.is_empty() {
        color::yellow("No skills found")
    } else {
        format!(
            "Found {} skill(s) in node_modules",
            color::green(&discovered.len().to_string())
        )
    };
    spinner.stop(&stop_message);
    for warning in &warnings {
        prompt::log_warn(&warning.plain());
    }

    let local_lock = locks::read_local_lock(&cwd).unwrap_or_default();
    let frozen = options.frozen_lockfile.unwrap_or_else(detect_agent::is_ci);

    // Prune stale skills.
    if options.cleanup != Some(false) && !options.dry_run && !frozen {
        let keep: std::collections::HashSet<String> = discovered
            .iter()
            .map(|skill| sanitize_name(&skill.skill.name))
            .collect();
        let stale: Vec<String> = local_lock
            .skills
            .iter()
            .filter(|(name, entry)| {
                installed_by_sync(Some(entry)) && !keep.contains(&sanitize_name(name))
            })
            .map(|(name, _)| sanitize_name(name))
            .filter(|name| !keep.contains(name))
            .collect();
        if !stale.is_empty() {
            let all_agents = registry::agents();
            let mut warnings = Vec::new();
            let _ = installer::remove_skills(
                &stale,
                &installer::RemoveOptions {
                    global: installer::Scope::Project,
                    cwd: Some(cwd.clone()),
                    target_agents: all_agents.iter().map(|a| a.name.to_string()).collect(),
                    lockfile: Some(true),
                },
                &mut warnings,
            );
            for name in &stale {
                prompt::log_info(&format!(
                    "Removed {} {}",
                    color::cyan(name),
                    color::dim("(no longer provided by a dependency)")
                ));
            }
        }
    }

    if discovered.is_empty() {
        prompt::outro(&color::dim("Nothing to sync."));
        return;
    }

    // Show discovered skills.
    for skill in &discovered {
        prompt::log_info(&format!(
            "{} {}",
            color::cyan(&skill.skill.name),
            color::dim(&format!("from {}", skill.package_name))
        ));
        if !skill.skill.description.is_empty() {
            prompt::log_plain(&color::dim(&format!("  {}", skill.skill.description)));
        }
    }

    // Agent selection.
    let mut target_agents: Vec<&'static AgentConfig>;
    let valid_agent_ids: Vec<&str> = registry::agents().iter().map(|a| a.name).collect();
    if options.agent.contains(&"*".to_string()) {
        target_agents = registry::agents().iter().collect();
        prompt::log_info(&format!("Installing to all {} agents", target_agents.len()));
    } else if !options.agent.is_empty() {
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
        target_agents = options
            .agent
            .iter()
            .filter_map(|name| registry::agent_by_id(name))
            .collect();
    } else {
        let spinner = Spinner::start("Loading agents…");
        let installed_agents = registry::detect_installed_agents(&cwd);
        spinner.stop(&format!("{} agents", registry::agents().len()));
        if installed_agents.is_empty() && options.yes {
            target_agents = registry::universal_agents();
            prompt::log_info("Installing to universal agents");
        } else if installed_agents.len() == 1 || options.yes {
            target_agents = installed_agents;
            for universal in registry::universal_agents() {
                if !target_agents.iter().any(|a| a.name == universal.name) {
                    target_agents.push(universal);
                }
            }
        } else {
            let choices = registry::non_universal_agents()
                .into_iter()
                .filter(|a| {
                    installed_agents.is_empty() || installed_agents.iter().any(|i| i.name == a.name)
                })
                .collect::<Vec<_>>();
            let items: Vec<SearchItem> = choices
                .iter()
                .map(|a| SearchItem {
                    value: a.name.to_string(),
                    label: a.display_name.to_string(),
                    hint: Some(a.skills_dir.to_string()),
                    group: None,
                    detail: None,
                })
                .collect();
            let last = locks::read_skill_lock()
                .ok()
                .and_then(|lock| lock.last_selected_agents);
            let remembered: Vec<String> = last
                .unwrap_or_default()
                .into_iter()
                .filter(|name| choices.iter().any(|a| a.name == name))
                .collect();
            let selected =
                crate::search_multiselect::search_multiselect(&SearchMultiselectOptions {
                    message: "Which agents do you want to install to?".to_string(),
                    items,
                    initial_selected: remembered,
                    required: true,
                    locked_section: Some(LockedSection {
                        title: "Universal (.agents/skills)".to_string(),
                        items: registry::visible_universal_agents()
                            .iter()
                            .map(|a| SearchItem {
                                value: a.name.to_string(),
                                label: a.display_name.to_string(),
                                hint: None,
                                group: None,
                                detail: None,
                            })
                            .collect(),
                        hidden_count: registry::universal_agents().len()
                            - registry::visible_universal_agents().len(),
                    }),
                    searchable: true,
                    show_detail: false,
                    show_selected_summary: true,
                    select_groups: false,
                    select_all: false,
                    max_visible: 8,
                });
            let Some(selected) = selected else {
                prompt::cancel("Sync cancelled");
                std::process::exit(0);
            };
            target_agents = selected
                .iter()
                .filter_map(|name| registry::agent_by_id(name))
                .collect();
            if let Ok(mut lock) = locks::read_skill_lock() {
                lock.last_selected_agents = Some(selected);
                let _ = locks::write_skill_lock(&lock);
            }
        }
    }

    // Conflict resolution.
    let mut to_install: Vec<&PackageSkill> = Vec::new();
    let mut by_name: std::collections::HashMap<String, Vec<&PackageSkill>> =
        std::collections::HashMap::new();
    for skill in &discovered {
        by_name
            .entry(sanitize_name(&skill.skill.name))
            .or_default()
            .push(skill);
    }
    for (name, candidates) in &by_name {
        let min_depth = candidates.iter().map(|c| c.depth).min().unwrap_or(0);
        let closest: Vec<&&PackageSkill> =
            candidates.iter().filter(|c| c.depth == min_depth).collect();
        if closest.len() > 1 {
            let packages: Vec<String> = closest.iter().map(|c| c.package_name.clone()).collect();
            prompt::log_warn(&format!(
                "Skipped {} {}: shipped by {}; drop one with --exclude",
                color::cyan(name),
                color::dim(""),
                packages.join(", ")
            ));
            continue;
        }
        let skill = closest[0];
        let reason = blocked_reason(name, &target_agents, local_lock.skills.get(name), &cwd);
        match reason {
            Some(reason) => prompt::log_warn(&format!(
                "Skipped {} {}: {reason}",
                color::cyan(&skill.skill.name),
                color::dim(&format!("from {}", skill.package_name))
            )),
            None => to_install.push(skill),
        }
    }

    if to_install.is_empty() {
        println!();
        prompt::outro(&color::yellow("Nothing to sync."));
        return;
    }

    // Summary.
    let mode = if options.copy { "copied from" } else { "→" };
    let mut summary_lines: Vec<String> = Vec::new();
    for skill in &to_install {
        let canonical_path = installer::get_canonical_path(&skill.skill.name, false, &cwd);
        summary_lines.push(format!(
            "{} {}",
            color::cyan(&skill.skill.name),
            color::dim(&format!("\u{2190} {}", skill.package_name))
        ));
        summary_lines.push(format!(
            "  {}",
            color::dim(&format!(
                "{} {mode} {}",
                common::shorten_path(&canonical_path, &cwd),
                common::shorten_path(&skill.skill.path, &cwd)
            ))
        ));
    }
    println!();
    prompt::note(&summary_lines.join("\n"), "Sync Summary");

    if options.dry_run {
        prompt::outro(&color::dim("Dry run — nothing changed."));
        return;
    }

    if !options.yes {
        match prompt::confirm("Proceed with sync?", true) {
            Some(true) => {}
            Some(false) | None => {
                prompt::cancel("Sync cancelled");
                std::process::exit(0);
            }
        }
    }

    // Install.
    let spinner = Spinner::start("Syncing skills…");
    let install_mode = if options.copy {
        installer::InstallMode::Copy
    } else {
        installer::InstallMode::Link
    };
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut installed: Vec<(String, String)> = Vec::new();
    {
        let mut warnings: Vec<SkillsError> = Vec::new();
        for skill in &to_install {
            for agent in &target_agents {
                let outcome = installer::install_skill_dir(
                    &skill.skill.path,
                    agent,
                    &installer::InstallOptions {
                        global: installer::Scope::Project,
                        cwd: Some(cwd.clone()),
                        mode: Some(if agent.name == "eve" {
                            installer::InstallMode::Copy
                        } else {
                            install_mode
                        }),
                        ..Default::default()
                    },
                    &mut warnings,
                );
                if outcome.success {
                    installed.push((skill.skill.name.clone(), skill.package_name.clone()));
                } else if let Some(error) = outcome.error {
                    failures.push((skill.skill.name.clone(), error));
                }
            }
        }
    }
    spinner.stop("Sync complete");

    // Lock entries.
    if !frozen {
        if let Ok(mut lock) = locks::read_local_lock(&cwd) {
            for skill in &to_install {
                if !installed.iter().any(|(name, _)| name == &skill.skill.name) {
                    continue;
                }
                let Ok(hash) = locks::compute_folder_hash(&skill.skill.path) else {
                    continue;
                };
                lock.skills.insert(
                    skill.skill.name.clone(),
                    locks::LocalSkillLockEntry {
                        source: skill.package_name.clone(),
                        source_url: None,
                        ref_: None,
                        source_type: "node_modules".to_string(),
                        skill_path: Some(skill.skill_path.clone()),
                        version: skill.package_version.clone(),
                        via: skill.via.clone(),
                        computed_hash: hash,
                        subagents: None,
                        well_known_digest: None,
                        install_mode: None,
                    },
                );
            }
            let _ = locks::write_local_lock(&lock, &cwd);
        }
    }

    // Results.
    println!();
    if !installed.is_empty() {
        let mut unique: Vec<(String, String)> = Vec::new();
        for entry in &installed {
            if !unique.iter().any(|(name, _)| name == &entry.0) {
                unique.push(entry.clone());
            }
        }
        let mut result_lines: Vec<String> = Vec::new();
        for (name, package) in &unique {
            result_lines.push(format!(
                "{} {} {}",
                color::green("\u{2713}"),
                name,
                color::dim(&format!("\u{2190} {package}"))
            ));
        }
        let count = unique.len();
        let title = color::green(&format!(
            "Synced {count} skill{}",
            if count != 1 { "s" } else { "" }
        ));
        prompt::note(&result_lines.join("\n"), &title);
    }
    if !failures.is_empty() {
        common::set_exit_code_1();
        println!();
        prompt::log_error(&color::red(&format!(
            "Failed to install {}",
            failures.len()
        )));
        for (name, error) in &failures {
            prompt::log_plain(&format!(
                "  {} {}: {}",
                color::red("\u{2717}"),
                name,
                color::dim(error)
            ));
        }
    }

    println!();
    prompt::outro(&format!(
        "{}{}",
        color::green("Done!"),
        color::dim("  Review skills before use; they run with full agent permissions.")
    ));
    let _ = source_parser::parse_source; // parity: remotes route through add
}
