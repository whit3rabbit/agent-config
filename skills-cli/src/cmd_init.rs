//! The `skillz init` command and `skillz install` (restore from
//! skills-lock.json), ported from `cli.ts`'s `runInit` and `src/install.ts`.

use std::path::Path;

use skills_store::error::{diag, SkillsError};
use skills_store::registry;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::prompt;

/// The `skillz init [name]` command (upstream `runInit`).
pub fn run_init(args: &[String]) {
    let cwd = common::current_dir();
    let has_name = !args.is_empty();
    let skill_name = args.first().cloned().unwrap_or_else(|| {
        cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "skill".to_string())
    });

    let skill_dir = if has_name {
        cwd.join(&skill_name)
    } else {
        cwd.clone()
    };
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
    println!(
        "  1. Edit {} to define your skill instructions",
        color::text(&display_path)
    );
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
        color::text("skillz add <owner>/<repo>")
    );
    println!(
        "  {}Host the file, then {}",
        color::dim("URL:"),
        color::text(&format!("skillz add https://example.com/{display_path}"))
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
        requested
            .iter()
            .filter_map(|name| registry::agent_by_id(name))
            .collect()
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

/// The `skillz experimental_install` entry point (upstream `runInstallFromLock`).
pub fn run_install_from_lock(args: &[String]) {
    let cwd = common::current_dir();
    let options = parse_install_options(args);
    let frozen = options
        .frozen_lockfile
        .unwrap_or_else(crate::detect_agent::is_ci);
    if let Err(error) = restore_from_lock(&cwd, &options, frozen) {
        prompt::log_error(&error.plain());
        common::set_exit_code_1();
    }
}

fn restore_from_lock(
    cwd: &Path,
    options: &InstallFromLockOptions,
    frozen: bool,
) -> Result<(), SkillsError> {
    use crate::skill_payload::{find_locked, SkillPayload};
    use crate::update_source;
    use std::collections::BTreeMap;

    let mut lock = locks::read_local_lock(cwd)?;
    if lock.skills.is_empty() {
        if frozen {
            return Err(SkillsError::new(
                "SKILLS_FROZEN_LOCKFILE",
                "skills-lock.json is absent or has no skills",
            ));
        }
        prompt::log_warn("No project skills found in skills-lock.json");
        return Ok(());
    }
    let agents = resolve_restore_agents(&options.agent, cwd)?;
    type RestoreGroup<'a> = (&'a str, &'a str, Option<&'a str>);
    let mut groups: BTreeMap<RestoreGroup<'_>, Vec<_>> = BTreeMap::new();
    for (name, entry) in &lock.skills {
        if entry.source_type == "node_modules" {
            prompt::log_error(&format!("{name}: restore with skillz experimental_sync"));
            common::set_exit_code_1();
            continue;
        }
        let placements = if options.agent.is_empty() && !entry.placements.is_empty() {
            entry.placements.clone()
        } else {
            restore_placements(&agents, entry)
        };
        if !options.force
            && !placements.is_empty()
            && placements.iter().all(|placement| {
                registry::agent_by_id(&placement.agent).is_some_and(|agent| {
                    installer::get_install_path(
                        name,
                        agent,
                        false,
                        cwd,
                        placement.subagent.as_deref(),
                    )
                    .join("SKILL.md")
                    .is_file()
                })
            })
        {
            prompt::log_info(&format!("{name}: already installed"));
            continue;
        }
        match update_source::local_source(entry) {
            Ok(source) => groups
                .entry((&entry.source_type, source, entry.ref_.as_deref()))
                .or_default()
                .push((name, entry, placements)),
            Err(error) => {
                prompt::log_error(&error.plain());
                common::set_exit_code_1();
            }
        }
    }

    let mut changes = Vec::new();
    for ((kind, source, git_ref), entries) in groups {
        if options.dry_run {
            for (name, _, _) in &entries {
                let reference = git_ref.map(|r| format!(" @ {r}")).unwrap_or_default();
                prompt::log_info(&format!("Would restore {name} from {source}{reference}"));
            }
            continue;
        }
        let fetched = if kind == "well-known" {
            crate::wellknown::fetch_all_skills(source, true).map(|skills| {
                (
                    None,
                    skills
                        .into_iter()
                        .map(SkillPayload::WellKnown)
                        .collect::<Vec<_>>(),
                )
            })
        } else {
            update_source::resolve(source, kind, git_ref).map(|resolved| {
                let payloads = SkillPayload::from_resolved(&resolved);
                (Some(resolved), payloads)
            })
        };
        let (resolved, payloads) = match fetched {
            Ok(source) => source,
            Err(error) => {
                prompt::log_error(&format!(
                    "Could not restore from {source}: {}",
                    error.plain()
                ));
                common::set_exit_code_1();
                continue;
            }
        };
        let root = resolved
            .as_ref()
            .and_then(|resolved| resolved.root_dir.as_deref());
        let mut prepared = Vec::new();
        let mut refusals = Vec::new();
        for (name, entry, placements) in entries {
            match find_locked(&payloads, root, name, entry.skill_path.as_deref())
                .and_then(|payload| Ok((payload, payload.content_hash()?)))
            {
                Ok((payload, hash)) => {
                    if frozen && hash != entry.computed_hash {
                        refusals.push(format!("{name}: content hash changed"));
                    }
                    prepared.push((name, payload, hash, placements));
                }
                Err(error) => refusals.push(error.plain()),
            }
        }
        if !refusals.is_empty() {
            common::set_exit_code_1();
            if frozen {
                prompt::log_error(&locks::frozen_lockfile_error(&refusals));
                continue;
            }
            for refusal in refusals {
                prompt::log_error(&refusal);
            }
        }
        for (name, payload, hash, placements) in prepared {
            match crate::cmd_update::install_placements(payload, name, &placements, false, cwd) {
                Ok(placements) => {
                    prompt::log_success(&format!("Restored {name}"));
                    changes.push((name.clone(), hash, payload.source_path(root), placements));
                }
                Err(error) => {
                    prompt::log_error(&format!("{name}: {}", error.plain()));
                    common::set_exit_code_1();
                }
            }
        }
    }
    if !frozen && !changes.is_empty() {
        for (name, hash, path, placements) in changes {
            let entry = lock
                .skills
                .get_mut(&name)
                .expect("restore came from lockfile");
            entry.computed_hash = hash;
            entry.skill_path = path;
            entry.placements = placements;
        }
        locks::write_local_lock(&lock, cwd)?;
    }
    Ok(())
}

fn restore_placements(
    agents: &[&registry::AgentConfig],
    entry: &locks::LocalSkillLockEntry,
) -> Vec<locks::SkillPlacement> {
    let mut placements = Vec::new();
    let mode = entry.install_mode.as_deref().unwrap_or("symlink");
    for agent in agents {
        if agent.name == "eve" {
            if let Some(subagents) = &entry.subagents {
                placements.extend(subagents.iter().map(|name| locks::SkillPlacement {
                    agent: agent.name.into(),
                    subagent: (!name.is_empty()).then(|| name.clone()),
                    mode: mode.into(),
                }));
                continue;
            }
        }
        placements.push(locks::SkillPlacement {
            agent: agent.name.into(),
            subagent: None,
            mode: mode.into(),
        });
    }
    placements
}
