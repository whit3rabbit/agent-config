//! The `skills list` command, ported from `src/list.ts`.

use std::collections::HashMap;

use skills_store::registry;
use skills_store::sanitize::{sanitize_metadata, sanitize_name};
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::prompt::log_warn;

#[derive(Debug, Default)]
struct ListOptions {
    global: bool,
    agent: Vec<String>,
    json: bool,
}

fn parse_list_options(args: &[String]) -> ListOptions {
    let mut options = ListOptions::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-g" | "--global" => options.global = true,
            "--json" => options.json = true,
            "-a" | "--agent" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.agent.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            _ => {}
        }
        index += 1;
    }
    options
}

struct LockEntry {
    source: String,
    source_url: Option<String>,
    source_type: String,
    plugin_name: Option<String>,
}

/// The `skills list` entry point (upstream `runList`).
pub fn run_list(args: &[String]) {
    let options = parse_list_options(args);
    let scope = options.global;
    let cwd = common::current_dir();

    let valid_agent_ids: Vec<&str> = registry::agents().iter().map(|a| a.name).collect();
    let agent_filter = if options.agent.is_empty() {
        None
    } else {
        let invalid: Vec<String> = options
            .agent
            .iter()
            .filter(|a| !valid_agent_ids.contains(&a.as_str()))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            println!(
                "{}",
                diag_invalid_agents(&invalid, &valid_agent_ids)
            );
            std::process::exit(1);
        }
        Some(options.agent.clone())
    };

    let installed_skills = installer::list_installed_skills(Some(scope), &cwd, agent_filter.as_deref());

    let locked_skills: HashMap<String, LockEntry> = if scope {
        locks::read_skill_lock()
            .map(|lock| {
                lock.skills
                    .into_iter()
                    .map(|(name, entry)| {
                        (
                            name,
                            LockEntry {
                                source: entry.source,
                                source_url: Some(entry.source_url),
                                source_type: entry.source_type,
                                plugin_name: entry.plugin_name,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        match locks::read_local_lock(&cwd) {
            Ok(lock) => lock
                .skills
                .into_iter()
                .map(|(name, entry)| {
                    (
                        name,
                        LockEntry {
                            source: entry.source,
                            source_url: entry.source_url,
                            source_type: entry.source_type,
                            plugin_name: None,
                        },
                    )
                })
                .collect(),
            Err(err) => {
                // Listing is read-only: an unusable lock only costs source info.
                log_warn(&format!("Warning: {}", err.plain()));
                HashMap::new()
            }
        }
    };

    let lock_by_sanitized: HashMap<String, &LockEntry> = locked_skills
        .iter()
        .map(|(name, entry)| (sanitize_name(name), entry))
        .collect();
    let get_lock_entry = |skill_name: &str| -> Option<&LockEntry> {
        locked_skills.get(skill_name).or_else(|| lock_by_sanitized.get(&sanitize_name(skill_name)).copied())
    };

    let installed_names: std::collections::HashSet<String> =
        installed_skills.iter().map(|skill| sanitize_name(&skill.name)).collect();
    let missing_skills: Vec<(&String, &LockEntry)> = if agent_filter.is_some() {
        Vec::new()
    } else {
        locked_skills
            .iter()
            .filter(|(name, _)| !installed_names.contains(&sanitize_name(name)))
            .collect()
    };

    let display_name_of = |agent_id: &str| -> String {
        registry::agent_by_id(agent_id)
            .map(|a| a.display_name.to_string())
            .unwrap_or_else(|| agent_id.to_string())
    };

    if options.json {
        let mut output: Vec<serde_json::Value> = Vec::new();
        for skill in &installed_skills {
            let entry = get_lock_entry(&skill.name);
            output.push(serde_json::json!({
                "name": skill.name,
                "path": skill.canonical_path.to_string_lossy(),
                "scope": if skill.scope == installer::Scope::Global { "global" } else { "project" },
                "agents": skill.agents.iter().map(|a| display_name_of(a)).collect::<Vec<_>>(),
                "source": entry.map(|e| e.source.clone()),
                "sourceUrl": entry.and_then(|e| e.source_url.clone()),
                "sourceType": entry.map(|e| e.source_type.clone()),
                "status": if entry.is_some() { "tracked" } else { "untracked" },
            }));
        }
        for (name, entry) in &missing_skills {
            output.push(serde_json::json!({
                "name": name,
                "path": serde_json::Value::Null,
                "scope": if scope { "global" } else { "project" },
                "agents": [],
                "source": entry.source,
                "sourceUrl": entry.source_url,
                "sourceType": entry.source_type,
                "status": "missing",
            }));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&output).unwrap_or_else(|_| "[]".to_string())
        );
        return;
    }

    let scope_label = if scope { "Global" } else { "Project" };
    if installed_skills.is_empty() && missing_skills.is_empty() {
        println!("{}No {} skills found.{}", color::dim(""), scope_label.to_lowercase(), color::reset(""));
        if scope {
            println!("{}Try listing project skills without -g{}", color::dim(""), color::reset(""));
        } else {
            println!("{}Try listing global skills with -g{}", color::dim(""), color::reset(""));
        }
        return;
    }

    let print_skill = |skill: &installer::InstalledSkill, indent: bool, name_width: usize, path_width: usize| {
        let prefix = if indent { "  " } else { "" };
        let short_path = common::shorten_path(&skill.canonical_path, &cwd);
        let agent_names: Vec<String> = skill.agents.iter().map(|a| display_name_of(a)).collect();
        let agent_info = if !skill.agents.is_empty() {
            common::format_list(&agent_names, 5)
        } else {
            color::yellow("not linked")
        };
        let padded_name = format!("{:<width$}", sanitize_metadata(&skill.name), width = name_width);
        let padded_path = format!("{:<width$}", short_path, width = path_width);

        let entry = get_lock_entry(&skill.name);
        let source_label = entry
            .map(|e| sanitize_metadata(&e.source))
            .unwrap_or_else(|| color::yellow("untracked"));

        println!(
            "{prefix}{} {} {}",
            color::cyan(&padded_name),
            color::dim(&padded_path),
            color::reset("")
        );
        println!(
            "{prefix}  {}: {}  {}: {}",
            color::dim("Agents"),
            agent_info,
            color::dim("Source"),
            source_label
        );
    };

    println!("{}{} Skills{}", color::bold(""), scope_label, color::reset(""));
    println!();

    let mut grouped: std::collections::BTreeMap<String, Vec<&installer::InstalledSkill>> =
        std::collections::BTreeMap::new();
    let mut ungrouped: Vec<&installer::InstalledSkill> = Vec::new();
    for skill in &installed_skills {
        match get_lock_entry(&skill.name).and_then(|e| e.plugin_name.clone()) {
            Some(plugin) => grouped.entry(plugin).or_default().push(skill),
            None => ungrouped.push(skill),
        }
    }

    let print_group = |skills: &[&installer::InstalledSkill]| {
        let mut max_name = 0;
        let mut max_path = 0;
        for skill in skills {
            max_name = max_name.max(sanitize_metadata(&skill.name).len());
            max_path = max_path.max(common::shorten_path(&skill.canonical_path, &cwd).len());
        }
        for skill in skills {
            print_skill(skill, true, max_name, max_path);
        }
    };

    if !grouped.is_empty() {
        for (group, skills) in &grouped {
            println!("{}{}{}", color::bold(""), common::kebab_to_title(group), color::reset(""));
            print_group(skills);
            println!();
        }
        if !ungrouped.is_empty() {
            println!("{}General{}", color::bold(""), color::reset(""));
            print_group(&ungrouped);
            println!();
        }
    } else {
        print_group(&installed_skills.iter().collect::<Vec<_>>());
        println!();
    }

    if !missing_skills.is_empty() {
        println!(
            "{}Missing{} {}(in the lock file, not on disk){}",
            color::bold(""),
            color::reset(""),
            color::dim(""),
            color::reset("")
        );
        for (name, entry) in &missing_skills {
            println!(
                "  {}  {}: {}",
                color::cyan(&sanitize_metadata(name)),
                color::dim("Source:"),
                sanitize_metadata(&entry.source)
            );
        }
        println!();
    }
}

fn diag_invalid_agents(invalid: &[String], valid: &[&str]) -> String {
    skills_store::error::diag::invalid_agents(
        invalid,
        &valid.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .plain()
}

/// Skill source lookup used by `--json` output (kept for tests).
#[allow(dead_code)]
fn lock_source(entry: &LockEntry) -> String {
    entry.source.clone()
}
