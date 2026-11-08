//! The `skills use` command, ported from `src/use.ts`: generate a prompt for
//! using one skill without installing it.

use std::path::{Path, PathBuf};

use skills_store::error::{diag, SkillsError};
use skills_store::sanitize::sanitize_name;
use skills_store::skills::{discover_skills, filter_skills, DiscoverOptions, Skill};

use crate::git;
use crate::resolver::ResolveOptions;
use crate::source_parser::{self, SourceKind};

const BLOB_ALLOWED_OWNERS: [&str; 4] = ["vercel", "vercel-labs", "heygen-com", "remotion-dev"];

#[derive(Debug, Default)]
pub struct UseOptions {
    skill: Option<String>,
    agent: Vec<String>,
    full_depth: bool,
    help: bool,
}

fn supported_use_agents() -> Vec<&'static str> {
    vec!["claude-code", "codex", "sarvam-code"]
}

fn use_agent_command(agent: &str) -> Option<&'static str> {
    match agent {
        "claude-code" => Some("claude"),
        "codex" => Some("codex"),
        "sarvam-code" => Some("sarvam-code"),
        _ => None,
    }
}

/// Parse use options (upstream `parseUseOptions`).
#[must_use]
pub fn parse_use_options(args: &[String]) -> (Vec<String>, UseOptions, Vec<SkillsError>) {
    let mut sources = Vec::new();
    let mut options = UseOptions::default();
    let mut errors = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" || arg == "-h" {
            options.help = true;
        } else if arg == "--full-depth" {
            options.full_depth = true;
        } else if arg == "--skill" || arg == "-s" {
            match args.get(index + 1) {
                Some(value) if !value.starts_with('-') => {
                    if options.skill.is_some() {
                        errors.push(diag::duplicate_option("--skill"));
                    } else {
                        options.skill = Some(value.clone());
                    }
                    index += 1;
                }
                _ => {
                    errors.push(diag::option_requires_value(arg, "a skill name"));
                }
            }
        } else if arg == "--agent" || arg == "-a" {
            index += 1;
            let start = options.agent.len();
            while index < args.len() && !args[index].starts_with('-') {
                options.agent.push(args[index].clone());
                index += 1;
            }
            if options.agent.len() == start {
                errors.push(diag::option_requires_value(arg, "an agent name"));
            }
            index = index.saturating_sub(1);
        } else if arg.starts_with('-') {
            errors.push(diag::unknown_option(arg));
        } else {
            sources.push(arg.clone());
        }
        index += 1;
    }

    errors.extend(validate_use_agent_option(&options.agent));
    (sources, options, errors)
}

fn validate_use_agent_option(agent_values: &[String]) -> Vec<SkillsError> {
    let mut errors = Vec::new();
    if agent_values.is_empty() {
        return errors;
    }
    let valid: Vec<String> = skills_store::registry::agents()
        .iter()
        .map(|a| a.name.to_string())
        .collect();
    let invalid: Vec<String> = agent_values
        .iter()
        .filter(|agent| agent.as_str() != "*" && !valid.contains(agent))
        .cloned()
        .collect();
    if agent_values.iter().any(|agent| agent == "*") {
        errors.push(diag::use_agent_wildcard());
    }
    if agent_values.len() > 1 {
        errors.push(diag::use_multiple_agents());
    }
    if !invalid.is_empty() {
        errors.push(diag::invalid_agents(&invalid, &valid));
    }
    errors
}

fn get_use_help() -> String {
    format!(
        "Usage: skills use <source>[@<skill>] [options]\n\n\
         Generate a prompt for using one skill without installing it.\n\n\
         Options:\n  \
         -s, --skill <skill>   Select the skill to use\n  \
         -a, --agent <agent>   Start one supported agent interactively ({})\n  \
         --full-depth          Search nested directories like skills add --full-depth\n  \
         -h, --help            Show this help message\n\n\
         Examples:\n  \
         skills use vercel-labs/agent-skills@web-design-guidelines | claude\n  \
         skills use vercel-labs/agent-skills --skill web-design-guidelines --agent claude-code\n  \
         skills use vercel-labs/agent-skills@web-design-guidelines --agent codex",
        supported_use_agents().join(", ")
    )
}

fn build_use_prompt(skill_md: &str, support_dir: &Path, has_supporting_files: bool) -> String {
    let mut sections = vec![
        "You are being given a Skill to execute for the user's next request.".to_string(),
        "Use the following SKILL.md as your instructions:".to_string(),
        format!("<SKILL.md>\n{skill_md}\n</SKILL.md>"),
    ];
    if has_supporting_files {
        sections.push(format!(
            "Supporting files for this skill were downloaded to:\n{}\n\nWhen the SKILL.md references relative paths, read them from that directory.",
            support_dir.to_string_lossy()
        ));
    }
    format!("{}\n", sections.join("\n\n"))
}

enum UseSkill {
    Blob(crate::blob::BlobSkill),
    Disk {
        name: String,
        raw_content: Option<String>,
        path: PathBuf,
    },
    WellKnown(crate::wellknown::WellKnownSkill),
}

impl UseSkill {
    fn name(&self) -> String {
        match self {
            UseSkill::Blob(skill) => skill.name.clone(),
            UseSkill::Disk { name, .. } => name.clone(),
            UseSkill::WellKnown(skill) => skill.name.clone(),
        }
    }

    fn directory_name(&self) -> String {
        match self {
            UseSkill::Blob(skill) => skill.name.clone(),
            UseSkill::Disk { name, .. } => name.clone(),
            UseSkill::WellKnown(skill) => skill.install_name.clone(),
        }
    }
}

/// The `skills use` entry point (upstream `runUse`).
pub fn run_use(sources: Vec<String>, options: UseOptions, parse_errors: Vec<SkillsError>) {
    if options.help {
        println!("{}", get_use_help());
        return;
    }
    if !parse_errors.is_empty() {
        for error in parse_errors {
            eprintln!("{}", error.plain());
        }
        std::process::exit(1);
    }
    if sources.is_empty() {
        eprintln!(
            "{}\n\n{}",
            diag::missing_argument("source").plain(),
            get_use_help()
        );
        std::process::exit(1);
    }
    if sources.len() > 1 {
        eprintln!("{}", diag::multiple_sources(&sources).plain());
        std::process::exit(1);
    }

    let use_agent = options.agent.first().cloned();
    if let Some(agent) = &use_agent {
        if use_agent_command(agent).is_none() {
            let display = skills_store::registry::agent_by_id(agent)
                .map(|a| a.display_name.to_string())
                .unwrap_or_else(|| agent.clone());
            eprintln!(
                "{}",
                diag::use_agent_unsupported(
                    &display,
                    &supported_use_agents()
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                )
                .plain()
            );
            std::process::exit(1);
        }
    }

    let source = sources[0].clone();
    let parsed = source_parser::parse_source(&source);
    let selector = resolve_selector(parsed.skill_filter.clone(), options.skill.clone());
    let include_internal = selector.is_some();

    let result: Result<UseSkill, SkillsError> = (|| match parsed.kind {
        SourceKind::WellKnown => {
            let skills = crate::wellknown::fetch_all_skills(&parsed.url, include_internal)?;
            select_well_known_skill(skills, selector.as_deref(), &source)
        }
        SourceKind::Download => {
            let downloaded = crate::download::download_source(&parsed.url)?;
            let discovery = discover_skills(
                &downloaded.root_dir,
                None,
                &DiscoverOptions {
                    include_internal,
                    full_depth: options.full_depth,
                    ..Default::default()
                },
            )?;
            select_disk_skill(discovery.skills, selector.as_deref(), &source)
        }
        SourceKind::Local => {
            let local_path = parsed
                .local_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&parsed.url));
            if !local_path.exists() {
                return Err(diag::local_path_not_found(&local_path.to_string_lossy()));
            }
            let discovery = discover_skills(
                &local_path,
                parsed.subpath.as_deref(),
                &DiscoverOptions {
                    include_internal,
                    full_depth: options.full_depth,
                    ..Default::default()
                },
            )?;
            select_disk_skill(discovery.skills, selector.as_deref(), &source)
        }
        SourceKind::GitHub if !options.full_depth => {
            let owner_repo = source_parser::get_owner_repo(&parsed);
            let owner = owner_repo
                .as_ref()
                .and_then(|or| or.split('/').next())
                .map(str::to_lowercase);
            let mut blob_result = None;
            if let (Some(owner_repo), Some(owner)) = (&owner_repo, owner) {
                if BLOB_ALLOWED_OWNERS.contains(&owner.as_str()) {
                    blob_result = crate::blob::try_blob_install(
                        owner_repo,
                        parsed.subpath.as_deref(),
                        selector.as_deref(),
                        parsed.ref_.as_deref(),
                        include_internal,
                    );
                }
            }
            if let Some(blob) = blob_result {
                let skills: Vec<Skill> = blob
                    .skills
                    .iter()
                    .map(|skill| Skill {
                        name: skill.name.clone(),
                        description: skill.description.clone(),
                        path: PathBuf::new(),
                        raw_content: Some(skill.raw_content.clone()),
                        plugin_name: None,
                        metadata: skill.metadata.clone(),
                    })
                    .collect();
                let selected = select_disk_skill(skills, selector.as_deref(), &source)?;
                let blob_skill = blob
                    .skills
                    .into_iter()
                    .find(|skill| skill.name == selected.name())
                    .ok_or_else(diag::no_valid_skills)?;
                return Ok(UseSkill::Blob(blob_skill));
            }
            let temp_dir = git::clone_repo(&parsed.url, parsed.ref_.as_deref())?;
            let discovery = discover_skills(
                &temp_dir,
                parsed.subpath.as_deref(),
                &DiscoverOptions {
                    include_internal,
                    full_depth: options.full_depth,
                    ..Default::default()
                },
            );
            let _ = git::cleanup_temp_dir(&temp_dir);
            select_disk_skill(discovery?.skills, selector.as_deref(), &source)
        }
        SourceKind::GitHub | SourceKind::GitLab | SourceKind::Git => {
            let temp_dir = git::clone_repo(&parsed.url, parsed.ref_.as_deref())?;
            let discovery = discover_skills(
                &temp_dir,
                parsed.subpath.as_deref(),
                &DiscoverOptions {
                    include_internal,
                    full_depth: options.full_depth,
                    ..Default::default()
                },
            );
            let _ = git::cleanup_temp_dir(&temp_dir);
            select_disk_skill(discovery?.skills, selector.as_deref(), &source)
        }
    })();

    let selected = match result {
        Ok(selected) => selected,
        Err(error) => {
            eprintln!("{}", error.plain());
            std::process::exit(1);
        }
    };

    // Materialize the skill into a temp dir.
    let materialized = materialize_use_skill(&selected);
    let prompt_text = build_use_prompt(
        &materialized.skill_md,
        &materialized.skill_dir,
        materialized.has_supporting_files,
    );

    if let Some(agent) = &use_agent {
        let command = use_agent_command(agent).unwrap_or("claude");
        let exit = launch_agent(command, &prompt_text);
        std::process::exit(exit);
    }

    print!("{prompt_text}");
}

fn resolve_selector(
    source_selector: Option<String>,
    option_selector: Option<String>,
) -> Option<String> {
    match (source_selector, option_selector) {
        (Some(from_source), Some(from_option)) => {
            if from_source.to_lowercase() != from_option.to_lowercase() {
                eprintln!(
                    "{}",
                    diag::conflicting_skill_selectors(&from_source, &from_option).plain()
                );
                std::process::exit(1);
            }
            Some(from_option)
        }
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn select_disk_skill(
    skills: Vec<Skill>,
    selector: Option<&str>,
    _source: &str,
) -> Result<UseSkill, SkillsError> {
    if skills.is_empty() {
        return Err(diag::no_valid_skills());
    }
    match selector {
        None => {
            if skills.len() == 1 {
                let owned = skills[0].clone();
                return Ok(UseSkill::Disk {
                    name: owned.name.clone(),
                    raw_content: owned.raw_content.clone(),
                    path: owned.path.clone(),
                });
            }
            let names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
            Err(diag::multiple_skills_in_source(&names))
        }
        Some(selector) => {
            let matches = filter_skills(&skills, &[(selector).to_string()]);
            if matches.is_empty() {
                let available: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
                return Err(diag::use_no_matching_skill(selector, &available));
            }
            if matches.len() > 1 {
                return Err(diag::ambiguous_skill_selector(selector));
            }
            let owned = matches[0].clone();
            Ok(UseSkill::Disk {
                name: owned.name.clone(),
                raw_content: owned.raw_content.clone(),
                path: owned.path.clone(),
            })
        }
    }
}

fn select_well_known_skill(
    skills: Vec<crate::wellknown::WellKnownSkill>,
    selector: Option<&str>,
    source: &str,
) -> Result<UseSkill, SkillsError> {
    if skills.is_empty() {
        return Err(diag::wellknown_index_not_found());
    }
    let selected: Vec<crate::wellknown::WellKnownSkill> = match selector {
        None => {
            if skills.len() != 1 {
                let names: Vec<String> = skills.iter().map(|s| s.install_name.clone()).collect();
                return Err(diag::multiple_skills_in_source(&names));
            }
            skills
        }
        Some(selector) => {
            let matches: Vec<_> = skills
                .into_iter()
                .filter(|skill| {
                    skill.install_name.to_lowercase() == selector.to_lowercase()
                        || skill.name.to_lowercase() == selector.to_lowercase()
                })
                .collect();
            if matches.is_empty() {
                let available: Vec<String> = Vec::new();
                return Err(diag::use_no_matching_skill(selector, &available));
            }
            if matches.len() > 1 {
                return Err(diag::ambiguous_skill_selector(selector));
            }
            matches
        }
    };
    let _ = source;
    Ok(UseSkill::WellKnown(selected.into_iter().next().unwrap()))
}

struct MaterializedUseSkill {
    skill_dir: PathBuf,
    skill_md: String,
    has_supporting_files: bool,
}

fn materialize_use_skill(skill: &UseSkill) -> MaterializedUseSkill {
    let temp_root = tempfile::Builder::new()
        .prefix("skills-use-")
        .tempdir()
        .expect("tempdir");
    let skill_dir = temp_root
        .path()
        .join(sanitize_name(&skill.directory_name()));
    std::fs::create_dir_all(&skill_dir).expect("create skill dir");
    let temp_path = skill_dir.clone();

    let skill_md = match skill {
        UseSkill::Blob(blob) => {
            for (path, contents) in &blob.files {
                let full = temp_path.join(path.replace('\\', "/"));
                if let Some(parent) = full.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(full, contents);
            }
            blob.raw_content.clone()
        }
        UseSkill::WellKnown(well_known) => {
            for (path, content) in &well_known.files {
                let full = temp_path.join(path.replace('\\', "/"));
                if let Some(parent) = full.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let bytes = match content {
                    crate::wellknown::FileContent::Text(text) => text.clone().into_bytes(),
                    crate::wellknown::FileContent::Bytes(bytes) => bytes.clone(),
                };
                let _ = std::fs::write(full, bytes);
            }
            well_known.content.clone()
        }
        UseSkill::Disk {
            path, raw_content, ..
        } => {
            let mut warnings = Vec::new();
            let _ = skills_store::installer::copy_directory(path, &temp_path, &mut warnings);
            raw_content.clone().unwrap_or_else(|| {
                std::fs::read_to_string(path.join("SKILL.md")).unwrap_or_default()
            })
        }
    };

    let has_supporting_files = contains_supporting_files(&temp_path, &temp_path);
    // Keep the directory alive for the process lifetime; the OS cleans tmp.
    let temp_root_path = temp_root.keep();
    std::mem::forget(temp_root_path);
    MaterializedUseSkill {
        skill_dir: temp_path,
        skill_md,
        has_supporting_files,
    }
}

fn contains_supporting_files(root_dir: &Path, current_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(current_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if contains_supporting_files(root_dir, &entry_path) {
                return true;
            }
        } else {
            let relative = entry_path
                .strip_prefix(root_dir)
                .map(|p| p.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if relative != "skill.md" {
                return true;
            }
        }
    }
    false
}

fn launch_agent(command: &str, prompt: &str) -> i32 {
    use std::process::{Command, Stdio};
    let mut child = match Command::new(command)
        .arg(prompt)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let display = skills_store::registry::agents()
                .iter()
                .find(|a| a.name == command)
                .map(|a| a.display_name.to_string())
                .unwrap_or_else(|| command.to_string());
            eprintln!(
                "{}",
                diag::use_agent_command_not_found(&display, command).plain()
            );
            return 1;
        }
        Err(err) => {
            eprintln!("{}", SkillsError::new("SKILLS_IO", err.to_string()).plain());
            return 1;
        }
    };
    child
        .wait()
        .map(|status| status.code().unwrap_or(1))
        .unwrap_or(1)
}

/// Re-exported resolver options for command parity tests.
#[allow(dead_code)]
fn _assert_resolver_options_default(_: ResolveOptions) {}
