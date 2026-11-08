//! The `skills init` command and `skills install` (restore from
//! skills-lock.json), ported from `cli.ts`'s `runInit` and `src/install.ts`.

use std::path::Path;

use skills_store::error::{diag, SkillsError};
use skills_store::registry;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::prompt;

/// The `skills init [name]` command (upstream `runInit`).
pub fn run_init(args: &[String]) {
    let cwd = common::current_dir();
    let has_name = !args.is_empty();
    let skill_name = args.first().cloned().unwrap_or_else(|| {
        cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string())
    });

    let skill_dir = if has_name { cwd.join(&skill_name) } else { cwd.clone() };
    let skill_file = skill_dir.join("SKILL.md");
    let display_path = if has_name {
        format!("{skill_name}/SKILL.md")
    } else {
        "SKILL.md".to_string()
    };

    if skill_file.exists() {
        println!(
            "{}Skill already exists at {}{}",
            color::text(""),
            color::dim(&display_path),
            color::reset("")
        );
        return;
    }

    if has_name {
        let _ = std::fs::create_dir_all(&skill_dir);
    }

    let skill_content = format!(
        "---\nname: {skill_name}\ndescription: A brief description of what this skill does\n---\n\n\
# {skill_name}\n\nInstructions for the agent to follow when this skill is activated.\n\n\
## When to use\n\nDescribe when this skill should be used.\n\n\
## Instructions\n\n1. First step\n2. Second step\n3. Additional steps as needed\n"
    );
    if std::fs::write(&skill_file, skill_content).is_err() {
        prompt::log_error("Failed to write SKILL.md");
        std::process::exit(1);
    }

    println!(
        "{}Initialized skill: {}{}",
        color::text(""),
        color::dim(&skill_name),
        color::reset("")
    );
    println!();
    println!("{}Created:{}", color::dim(""), color::reset(""));
    println!("  {display_path}");
    println!();
    println!("{}Next steps:{}", color::dim(""), color::reset(""));
    println!("  1. Edit {} to define your skill instructions", color::text(&display_path));
    println!(
        "  2. Update the {} and {} in the frontmatter",
        color::text("name"),
        color::text("description")
    );
    println!();
    println!("{}Publishing:{}", color::dim(""), color::reset(""));
    println!(
        "  {}Push to a repo, then {}",
        color::dim("GitHub:"),
        color::text("skills add <owner>/<repo>")
    );
    println!(
        "  {}Host the file, then {}",
        color::dim("URL:"),
        color::text(&format!("skills add https://example.com/{display_path}"))
    );
    println!();
    println!(
        "{}Browse existing skills for inspiration at {}https://skills.sh/{}",
        color::dim(""),
        color::text(""),
        color::reset("")
    );
    println!();
}

// ─── install (restore from lock) ────────────────────────────────────────────

/// Options parsed for `experimental_install` (a subset of sync options).
#[derive(Debug, Default, Clone)]
pub struct InstallFromLockOptions {
    /// `-a/--agent` values.
    pub agent: Vec<String>,
    /// `--dry-run`.
    pub dry_run: bool,
    /// `--frozen-lockfile`.
    pub frozen_lockfile: Option<bool>,
    /// `--force`.
    pub force: bool,
}

/// Parse options shared with sync (upstream `parseSyncOptions` subset).
#[must_use]
pub fn parse_install_options(args: &[String]) -> InstallFromLockOptions {
    let mut options = InstallFromLockOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-a" | "--agent" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.agent.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            "--dry-run" => options.dry_run = true,
            "--frozen-lockfile" => options.frozen_lockfile = Some(true),
            "--no-frozen-lockfile" => options.frozen_lockfile = Some(false),
            "--force" => options.force = true,
            _ => {}
        }
        index += 1;
    }
    options
}

/// Agents a restore installs to, without prompting (upstream
/// `resolveRestoreAgents`).
pub fn resolve_restore_agents(
    requested: &[String],
    cwd: &Path,
) -> Result<Vec<&'static registry::AgentConfig>, SkillsError> {
    let valid: Vec<&str> = registry::agents().iter().map(|a| a.name).collect();
    if requested.iter().any(|agent| agent == "*") {
        return Ok(registry::agents().iter().collect());
    }
    let chosen: Vec<&'static registry::AgentConfig> = if !requested.is_empty() {
        let invalid: Vec<String> = requested
            .iter()
            .filter(|agent| !valid.contains(&agent.as_str()))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            return Err(diag::invalid_agents(
                &invalid,
                &valid.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            ));
        }
        requested.iter().filter_map(|name| registry::agent_by_id(name)).collect()
    } else {
        let last = locks::read_skill_lock()
            .ok()
            .and_then(|lock| lock.last_selected_agents)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| valid.contains(&name.as_str()))
            .filter_map(|name| registry::agent_by_id(&name))
            .collect::<Vec<_>>();
        if !last.is_empty() {
            last
        } else {
            registry::detect_installed_agents(cwd)
        }
    };
    let mut combined = chosen;
    for universal in registry::universal_agents() {
        if !combined.iter().any(|a| a.name == universal.name) {
            combined.push(universal);
        }
    }
    Ok(combined)
}

/// The `skills experimental_install` entry point (upstream `runInstallFromLock`).
pub fn run_install_from_lock(args: &[String]) {
    let cwd = common::current_dir();
    let options = parse_install_options(args);
    let frozen = options.frozen_lockfile.unwrap_or_else(crate::detect_agent::is_ci);
    let force = options.force;
    let lock = match locks::read_local_lock(&cwd) {
        Ok(lock) => lock,
        Err(err) => {
            prompt::log_error(&err.plain());
            std::process::exit(1);
        }
    };
    let skill_entries: Vec<(String, locks::LocalSkillLockEntry)> =
        lock.skills.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

    if skill_entries.is_empty() {
        if frozen {
            prompt::log_error(
                "Cannot install with --frozen-lockfile because skills-lock.json is absent or has no skills",
            );
            common::set_exit_code_1();
            return;
        }
        prompt::log_warn("No project skills found in skills-lock.json");
        prompt::log_info(&format!(
            "Add project-level skills with {} (without {})",
            color::cyan("skills add <package>"),
            color::cyan("-g")
        ));
        return;
    }

    let target_agents = match resolve_restore_agents(&options.agent, &cwd) {
        Ok(agents) => agents,
        Err(error) => {
            prompt::log_error(&error.plain());
            common::set_exit_code_1();
            return;
        }
    };
    let linked_agents: Vec<&registry::AgentConfig> =
        target_agents.iter().copied().filter(|a| !a.is_universal()).collect();

    let mut node_module_skills: Vec<String> = Vec::new();
    let mut already_installed: Vec<String> = Vec::new();
    let mut failed_skills: Vec<String> = Vec::new();
    let mut by_source: Vec<(String, bool, bool, Vec<String>)> = Vec::new();

    for (skill_name, entry) in &skill_entries {
        if entry.source_type == "node_modules" {
            node_module_skills.push(skill_name.clone());
            continue;
        }
        if !force
            && installer::get_canonical_path(skill_name, false, &cwd)
                .join("SKILL.md")
                .exists()
        {
            already_installed.push(skill_name.clone());
            continue;
        }
        let install_source = if entry.source_type == "download" {
            entry.source_url.clone()
        } else {
            crate::update_source::build_local_update_source(entry)
        };
        let Some(install_source) = install_source else {
            prompt::log_error(
                &diag::lock_missing_source_url(&color::cyan(skill_name)).plain(),
            );
            failed_skills.push(skill_name.clone());
            continue;
        };
        let copy = entry.install_mode.as_deref() == Some("copy");
        let download = entry.source_type == "download";
        let key = format!(
            "{}:{}:{install_source}",
            if copy { "copy" } else { "symlink" },
            if download { "download" } else { "source" }
        );
        match by_source.iter_mut().find(|(k, _, _, _)| *k == key) {
            Some((_, _, _, skills)) => skills.push(skill_name.clone()),
            None => by_source.push((key, copy, download, vec![skill_name.clone()])),
        }
    }

    if !already_installed.is_empty() {
        prompt::log_info(&format!(
            "{} skill(s) already installed {}",
            color::cyan(&already_installed.len().to_string()),
            color::dim("(use --force to reinstall)")
        ));
    }

    let remote_count = skill_entries.len() - node_module_skills.len() - already_installed.len();
    if remote_count > 0 {
        prompt::log_info(&format!(
            "{} {} skill(s) from skills-lock.json into {}",
            if options.dry_run { "Would restore" } else { "Restoring" },
            color::cyan(&remote_count.to_string()),
            color::dim(".agents/skills/")
        ));
    }
    if !linked_agents.is_empty() && (remote_count > 0 || !already_installed.is_empty()) {
        prompt::log_info(&format!(
            "{} to {}",
            if options.dry_run { "Would link" } else { "Linking" },
            linked_agents.iter().map(|a| a.display_name).collect::<Vec<_>>().join(", ")
        ));
    }

    if options.dry_run {
        for (source, copy, _, skills) in &by_source {
            let names: Vec<String> = skills.iter().map(|n| color::cyan(n)).collect();
            prompt::log_plain(&format!(
                "{} {}",
                names.join(", "),
                color::dim(&format!("from {source}{}", if *copy { " (copy)" } else { "" }))
            ));
        }
        return;
    }

    link_installed_skills(&already_installed, &lock, &target_agents, &cwd);

    for (source, copy, download, skills) in &by_source {
        let parsed = if *download {
            source_parser_for_download(source)
        } else {
            crate::source_parser::parse_source(source)
        };
        let result = install_group_from_source(&parsed, skills, *copy, &target_agents, frozen, &lock, &cwd);
        match result {
            Ok(installed) => {
                prompt::log_success(&format!(
                    "Restored {}",
                    installed.iter().map(|n| color::cyan(n)).collect::<Vec<_>>().join(", ")
                ));
                let installed_lower: Vec<String> =
                    installed.iter().map(|n| n.to_lowercase()).collect();
                let not_found: Vec<&String> = skills
                    .iter()
                    .filter(|name| !installed_lower.contains(&name.to_lowercase()))
                    .collect();
                if !not_found.is_empty() {
                    let names = not_found.iter().map(|n| color::cyan(n)).collect::<Vec<_>>();
                    prompt::log_error(&format!(
                        "{} not found in {}",
                        names.join(", "),
                        color::cyan(source)
                    ));
                    failed_skills.extend(not_found.iter().map(|s| (*s).clone()));
                }
            }
            Err(error) => {
                let names = skills.iter().map(|n| color::cyan(n)).collect::<Vec<_>>();
                prompt::log_error(&format!(
                    "Failed to restore {} from {}: {}",
                    names.join(", "),
                    color::cyan(source),
                    error.plain()
                ));
                failed_skills.extend(skills.iter().cloned());
            }
        }
    }

    if !node_module_skills.is_empty() {
        prompt::log_info(&format!(
            "{} skill(s) from node_modules",
            color::cyan(&node_module_skills.len().to_string())
        ));
        prompt::log_warn(
            "node_modules skills are synced by `skills experimental_sync` (run it separately).",
        );
        failed_skills.extend(node_module_skills);
    }

    if !failed_skills.is_empty() {
        let restored_note = String::new();
        let _ = restored_note;
        prompt::log_error(&format!(
            "Failed to restore {} skill(s) from skills-lock.json: {}",
            failed_skills.len(),
            failed_skills.iter().map(|n| color::cyan(n)).collect::<Vec<_>>().join(", ")
        ));
        common::set_exit_code_1();
    }
}

fn source_parser_for_download(url: &str) -> crate::source_parser::ParsedSource {
    crate::source_parser::ParsedSource {
        kind: crate::source_parser::SourceKind::Download,
        url: url.to_string(),
        subpath: None,
        local_path: None,
        ref_: None,
        skill_filter: None,
    }
}

fn install_group_from_source(
    parsed: &crate::source_parser::ParsedSource,
    skills: &[String],
    copy: bool,
    target_agents: &[&'static registry::AgentConfig],
    frozen: bool,
    lock: &locks::LocalSkillLock,
    cwd: &Path,
) -> Result<Vec<String>, SkillsError> {
    let resolve_options = crate::resolver::ResolveOptions {
        full_depth: parsed.kind == crate::source_parser::SourceKind::Download,
        include_internal: !skills.is_empty(),
        ..Default::default()
    };
    let resolved = crate::resolver::resolve_skills(parsed, &resolve_options, None)?;
    if crate::resolver::no_skills(&resolved) {
        return Err(SkillsError::new(
            "SKILLS_SOURCE_FETCH_FAILED",
            format!("Could not fetch {}: no skills found", parsed.url),
        ));
    }

    // Frozen lock: refuse changed content before writing.
    if frozen {
        let mut changes: Vec<String> = Vec::new();
        for skill in &resolved.discovery.skills {
            if !skills.iter().any(|name| name.eq_ignore_ascii_case(&skill.name)) {
                continue;
            }
            if let Ok(hash) = locks::compute_folder_hash(&skill.path) {
                if let Some(reason) = locks::lock_change_reason(lock, &skill.name, (&hash, None)) {
                    changes.push(reason);
                }
            }
        }
        if !changes.is_empty() {
            return Err(SkillsError::new("SKILLS_FROZEN_LOCKFILE", locks::frozen_lockfile_error(&changes)));
        }
    }

    let mut installed_names: Vec<String> = Vec::new();
    let mode = if copy {
        installer::InstallMode::Copy
    } else {
        installer::InstallMode::Symlink
    };
    let mut warnings: Vec<SkillsError> = Vec::new();
    for skill in &resolved.discovery.skills {
        if !skills.iter().any(|name| name.eq_ignore_ascii_case(&skill.name)) {
            continue;
        }
        let mut any_success = false;
        for agent in target_agents {
            let outcome = installer::install_skill_dir(
                &skill.path,
                agent,
                &installer::InstallOptions {
                    global: installer::Scope::Project,
                    cwd: Some(cwd.to_path_buf()),
                    mode: Some(mode),
                    create_missing_agent_root: true,
                    ..Default::default()
                },
                &mut warnings,
            );
            if outcome.success {
                any_success = true;
            } else if let Some(error) = outcome.error {
                prompt::log_error(&format!(
                    "Failed to install {} for {}: {error}",
                    color::cyan(&skill.name),
                    agent.display_name
                ));
            }
        }
        if any_success {
            installed_names.push(skill.name.clone());
            // Update the local lock (unless frozen).
            if !frozen {
                if let Ok(hash) = locks::compute_folder_hash(&skill.path) {
                    if let Ok(mut current) = locks::read_local_lock(cwd) {
                        if let Some(entry) = current.skills.get(&skill.name).cloned() {
                            let mut entry = entry;
                            entry.computed_hash = hash;
                            current.skills.insert(skill.name.clone(), entry);
                            let _ = locks::write_local_lock(&current, cwd);
                        }
                    }
                }
            }
        }
    }
    Ok(installed_names)
}

/// Give already installed skills the agent links they lack (upstream
/// `linkInstalledSkills`).
fn link_installed_skills(
    skill_names: &[String],
    lock: &locks::LocalSkillLock,
    target_agents: &[&'static registry::AgentConfig],
    cwd: &Path,
) {
    for name in skill_names {
        let path = installer::get_canonical_path(name, false, cwd);
        let mode = if lock.skills.get(name).and_then(|e| e.install_mode.as_deref()) == Some("copy") {
            installer::InstallMode::Copy
        } else {
            installer::InstallMode::Symlink
        };
        for agent in target_agents {
            if installer::get_install_path(name, agent, false, cwd, None).exists() {
                continue;
            }
            let mut warnings = Vec::new();
            let result = installer::install_skill_dir(
                &path,
                agent,
                &installer::InstallOptions {
                    global: installer::Scope::Project,
                    cwd: Some(cwd.to_path_buf()),
                    mode: Some(mode),
                    create_missing_agent_root: true,
                    ..Default::default()
                },
                &mut warnings,
            );
            if !result.success {
                prompt::log_error(&format!(
                    "Failed to link {} to {}: {}",
                    color::cyan(name),
                    agent.display_name,
                    result.error.as_deref().unwrap_or("unknown")
                ));
                common::set_exit_code_1();
            }
        }
    }
}
