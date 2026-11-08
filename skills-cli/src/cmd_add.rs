//! The `skillz add` command: the full interactive install wizard, ported from
//! `src/add.ts` of the upstream CLI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use skills_store::error::{diag, SkillsError};
use skills_store::registry::{self, AgentConfig, DetectContext};
use skills_store::sanitize::sanitize_name;
use skills_store::{installer, locks};

use crate::color;
use crate::common;
use crate::detect_agent;
use crate::prompt::{self, Spinner};
use crate::resolver::{self, ResolveOptions};
use crate::search_multiselect::{LockedSection, SearchItem, SearchMultiselectOptions};
use crate::skill_payload::SkillPayload;
use crate::source_parser::{self, SourceKind};

/// Parsed `add` options.
#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    /// `-g/--global`.
    pub global: Option<bool>,
    /// `-a/--agent` values.
    pub agent: Vec<String>,
    /// `-y/--yes`.
    pub yes: bool,
    /// `-s/--skill` values.
    pub skill: Vec<String>,
    /// `--metadata` JSON string.
    pub metadata: Option<String>,
    /// `-l/--list`.
    pub list: bool,
    /// `--all`.
    pub all: bool,
    /// `--full-depth`.
    pub full_depth: bool,
    /// `--copy`.
    pub copy: bool,
    /// `--subagent` values.
    pub subagent: Vec<String>,
    /// `--json`.
    pub json: bool,
}

/// One JSON result entry in `add --json` output.
#[derive(serde::Serialize)]
struct AddJsonResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agents: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Parse `add` command-line options (upstream `parseAddOptions`).
#[must_use]
pub fn parse_add_options(args: &[String]) -> (Vec<String>, AddOptions, Vec<SkillsError>) {
    let mut options = AddOptions::default();
    let mut sources = Vec::new();
    let mut errors = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-g" | "--global" => options.global = Some(true),
            "-y" | "--yes" => options.yes = true,
            "-l" | "--list" => options.list = true,
            "--all" => options.all = true,
            "-a" | "--agent" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.agent.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            "-s" | "--skill" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.skill.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            "--metadata" => {
                index += 1;
                match args.get(index) {
                    None => errors.push(diag::option_requires_value("--metadata", "a JSON value")),
                    Some(metadata) => {
                        if serde_json::from_str::<serde_json::Value>(metadata).is_ok() {
                            options.metadata = Some(metadata.clone());
                        } else {
                            errors.push(diag::invalid_option_value("--metadata", "valid JSON"));
                        }
                    }
                }
            }
            "--full-depth" => options.full_depth = true,
            "--json" => options.json = true,
            "--copy" => options.copy = true,
            "--subagent" => {
                index += 1;
                while index < args.len() && !args[index].starts_with('-') {
                    options.subagent.push(args[index].clone());
                    index += 1;
                }
                index = index.saturating_sub(1);
            }
            other => {
                if !other.starts_with('-') {
                    sources.push(other.to_string());
                }
            }
        }
        index += 1;
    }

    (sources, options, errors)
}

/// A concrete install target (agent + optional Eve subagent).
#[derive(Clone)]
struct InstallTarget {
    agent: &'static AgentConfig,
    subagent: Option<String>,
}

struct TargetInstallResult {
    success: bool,
    path: PathBuf,
    canonical_path: Option<PathBuf>,
    mode: installer::InstallMode,
    symlink_failed: bool,
    skipped: bool,
    skip_reason: Option<&'static str>,
    error: Option<String>,
}

fn install_payload_for_target(
    payload: &SkillPayload,
    target: &InstallTarget,
    global: bool,
    cwd: &Path,
    mode: installer::InstallMode,
    create_missing_agent_root: bool,
    warnings: &mut Vec<SkillsError>,
) -> TargetInstallResult {
    let options = installer::InstallOptions {
        global: if global {
            installer::Scope::Global
        } else {
            installer::Scope::Project
        },
        mode: Some(mode),
        cwd: Some(cwd.to_path_buf()),
        eve_subagent: target.subagent.clone(),
        create_missing_agent_root,
    };
    let outcome = payload.install(payload.name(), target.agent, &options, warnings);
    let success = outcome.success
        && (outcome.skip_reason.is_some() || outcome.path.join("SKILL.md").is_file());
    TargetInstallResult {
        success,
        path: outcome.path,
        canonical_path: outcome.canonical_path,
        mode: outcome.mode,
        symlink_failed: outcome.symlink_failed,
        skipped: outcome.skipped,
        skip_reason: outcome.skip_reason,
        error: outcome
            .error
            .or_else(|| (!success).then(|| "target has no installed SKILL.md".to_string())),
    }
}

fn target_display_name(target: &InstallTarget) -> String {
    match &target.subagent {
        Some(subagent) => format!("{} ({subagent})", target.agent.display_name),
        None => target.agent.display_name.to_string(),
    }
}

fn default_install_mode_for(targets: &[InstallTarget]) -> installer::InstallMode {
    let all_eve = !targets.is_empty() && targets.iter().all(|t| t.agent.name == "eve");
    let unique_dirs: HashSet<&str> = targets.iter().map(|t| t.agent.skills_dir).collect();
    let mut unique = unique_dirs;
    let subagent_count = targets.iter().filter(|t| t.subagent.is_some()).count();
    if subagent_count > 1 {
        unique.insert("eve:subagents");
    }
    if unique.len() <= 1 || all_eve {
        installer::InstallMode::Copy
    } else {
        installer::InstallMode::Symlink
    }
}

fn build_install_targets(
    target_agents: &[&'static AgentConfig],
    eve_subagent_targets: &[Option<String>],
) -> Vec<InstallTarget> {
    let mut targets = Vec::new();
    for agent in target_agents {
        if agent.name == "eve" {
            for subagent in eve_subagent_targets {
                targets.push(InstallTarget {
                    agent,
                    subagent: subagent.clone(),
                });
            }
        } else {
            targets.push(InstallTarget {
                agent,
                subagent: None,
            });
        }
    }
    targets
}

fn installation_cancelled() -> Result<(), SkillsError> {
    prompt::cancel("Installation cancelled");
    if !prompt::is_tty() {
        return Err(diag::prompt_requires_tty());
    }
    Ok(())
}

fn select_agents_interactive(global: Option<bool>) -> Option<Vec<&'static AgentConfig>> {
    let supports_global = |agent: &'static AgentConfig| -> bool {
        global
            .map(|g| !g || global_dir_supported(agent))
            .unwrap_or(true)
    };
    let universal_agents: Vec<&'static AgentConfig> = registry::universal_agents()
        .into_iter()
        .filter(|a| supports_global(a))
        .collect();
    let visible_universal: Vec<&'static AgentConfig> = registry::visible_universal_agents()
        .into_iter()
        .filter(|a| supports_global(a))
        .collect();
    let other_agents: Vec<&'static AgentConfig> = registry::non_universal_agents()
        .into_iter()
        .filter(|a| a.name != "eve" && supports_global(a))
        .collect();

    let cwd = common::current_dir();
    let ctx = DetectContext::from_env(&cwd);
    let locked_items: Vec<SearchItem> = visible_universal
        .iter()
        .map(|a| SearchItem {
            value: a.name.to_string(),
            label: a.display_name.to_string(),
            hint: None,
            group: None,
            detail: None,
        })
        .collect();

    let other_items: Vec<SearchItem> = other_agents
        .iter()
        .map(|a| {
            let hint = match global {
                Some(true) => a
                    .global_skills_dir(&ctx)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                _ => a.skills_dir.to_string(),
            };
            SearchItem {
                value: a.name.to_string(),
                label: a.display_name.to_string(),
                hint: Some(hint),
                group: None,
                detail: None,
            }
        })
        .collect();

    let last_selected = locks::read_skill_lock()
        .ok()
        .and_then(|lock| lock.last_selected_agents);
    let initial_selected: Vec<String> = last_selected
        .unwrap_or_default()
        .into_iter()
        .filter(|name| {
            other_agents.iter().any(|a| a.name == name)
                && !universal_agents.iter().any(|a| a.name == name)
        })
        .collect();

    let selected = crate::search_multiselect::search_multiselect(&SearchMultiselectOptions {
        message: "Which agents do you want to install to?".to_string(),
        items: other_items,
        initial_selected,
        required: true,
        locked_section: Some(LockedSection {
            title: "Universal (.agents/skills)".to_string(),
            items: locked_items,
            hidden_count: universal_agents
                .len()
                .saturating_sub(visible_universal.len()),
        }),
        searchable: true,
        show_detail: false,
        show_selected_summary: true,
        select_groups: false,
        select_all: false,
        max_visible: 8,
    })?;

    let resolved: Vec<&'static AgentConfig> = selected
        .iter()
        .filter_map(|name| registry::agent_by_id(name))
        .collect();
    // Save the selection for next time (all agents incl. universal).
    let mut all_selected: Vec<String> = selected;
    for universal in &universal_agents {
        if !all_selected.iter().any(|name| name == universal.name) {
            all_selected.push(universal.name.to_string());
        }
    }
    if let Ok(mut lock) = locks::read_skill_lock() {
        lock.last_selected_agents = Some(all_selected);
        let _ = locks::write_skill_lock(&lock);
    }
    Some(resolved)
}

fn global_dir_supported(agent: &AgentConfig) -> bool {
    let ctx = DetectContext::from_env(&common::current_dir());
    agent.global_skills_dir(&ctx).is_some()
}

/// The `skillz add` entry point (upstream `runAdd`).
#[allow(clippy::too_many_lines)]
pub fn run_add(sources: Vec<String>, mut options: AddOptions) {
    let _progress = prompt::StderrProgress::enable(options.json);
    let mut json_results: Vec<AddJsonResult> = Vec::new();

    let Some(source) = sources.first().cloned() else {
        prompt::log_error("Missing required argument: source. Usage: skillz add <source>");
        if options.json {
            emit_json(&json_results);
        }
        common::set_exit_code_1();
        return;
    };

    let explicitly_selected_agents: HashSet<String> = if options.agent.contains(&"*".to_string()) {
        HashSet::new()
    } else {
        options.agent.iter().cloned().collect()
    };

    if options.all {
        options.skill = vec!["*".to_string()];
        options.agent = vec!["*".to_string()];
        options.yes = true;
    }

    let agent_result = detect_agent::detect_agent();
    if agent_result.is_agent {
        options.yes = true;
        if options.agent.is_empty() {
            if let Some(name) = &agent_result.agent {
                if let Some(agent_id) = detect_agent::get_agent_type(name) {
                    if let Some(agent) = registry::agent_by_id(agent_id) {
                        options.agent = common::ensure_universal_agents(vec![agent])
                            .iter()
                            .map(|a| a.name.to_string())
                            .collect();
                    }
                }
            }
        }
    }

    if options.json && !options.yes {
        eprintln!("Error: The --json flag requires --yes (or --all) to run non-interactively.");
        println!("[]");
        common::set_exit_code_1();
        return;
    }
    if options.json && options.list {
        eprintln!("Error: The --json flag cannot be combined with --list.");
        println!("[]");
        common::set_exit_code_1();
        return;
    }

    prompt::line_break();
    if !agent_result.is_agent {
        prompt::intro(&color::bg_cyan(&color::black(" skillz ")));
    }

    if agent_result.is_agent {
        let name = agent_result.agent.clone().unwrap_or_default();
        prompt::log_info(&format!(
            "{} Agent detected — installing non-interactively",
            color::bg_cyan(&color::black(&color::bold(&format!(" {name} "))))
        ));
    }

    // Validate --agent values before fetching.
    let valid_agent_ids: Vec<&str> = registry::agents().iter().map(|a| a.name).collect();
    if !options.agent.is_empty() && !options.agent.contains(&"*".to_string()) {
        let invalid: Vec<String> = options
            .agent
            .iter()
            .filter(|a| !valid_agent_ids.contains(&a.as_str()))
            .cloned()
            .collect();
        if !invalid.is_empty() {
            let error = diag::invalid_agents(
                &invalid,
                &valid_agent_ids
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
            );
            prompt::log_error(&error.plain());
            if options.json {
                println!("[]");
            }
            common::set_exit_code_1();
            return;
        }
    }

    let mut options = options;
    let result = run_add_inner(
        &source,
        &mut options,
        &explicitly_selected_agents,
        &mut json_results,
    );
    if let Err(error) = result {
        let is_clone_error = matches!(
            error.code,
            "SKILLS_UNSUPPORTED_GIT_TRANSPORT"
                | "SKILLS_GIT_CLONE_TIMEOUT"
                | "SKILLS_GIT_AUTH_FAILED"
                | "SKILLS_GIT_SAML_SSO_BLOCKED"
                | "SKILLS_GIT_CLONE_FAILED"
        );
        if is_clone_error {
            prompt::log_error(&color::red("Failed to clone repository"));
            for line in error.plain().split('\n') {
                prompt::log_plain(&color::dim(line));
            }
        } else {
            prompt::log_error(&error.plain());
        }
        prompt::log_plain(&color::dim(
            "Tip: use the --yes (-y) and --global (-g) flags to install without prompts.",
        ));
        prompt::outro(&color::red("Installation failed"));
        if options.json {
            json_results.push(AddJsonResult {
                name: None,
                status: "failed",
                source: None,
                ref_: None,
                hash: None,
                path: None,
                scope: None,
                agents: None,
                mode: None,
                reason: None,
                error: Some(error.plain()),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&json_results).unwrap_or_else(|_| "[]".to_string())
            );
        }
        common::set_exit_code_1();
    }
}

#[allow(clippy::too_many_lines)]
fn run_add_inner(
    source: &str,
    options: &mut AddOptions,
    explicitly_selected_agents: &HashSet<String>,
    json_results: &mut Vec<AddJsonResult>,
) -> Result<(), SkillsError> {
    let cwd = common::current_dir();

    // Parse the source.
    let spinner = Spinner::start("Parsing source…");
    let mut parsed = source_parser::parse_source(source);
    // `owner/repo@skill` merges into --skill (upstream behavior).
    if let Some(filter) = parsed.skill_filter.clone() {
        if !options.skill.contains(&filter) {
            options.skill.push(filter);
        }
    }
    spinner.stop(&format!(
        "Source: {}{}{}{}",
        match parsed.kind {
            SourceKind::Local => parsed
                .local_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&parsed.url))
                .to_string_lossy()
                .to_string(),
            _ => parsed.url.clone(),
        },
        parsed
            .ref_
            .as_ref()
            .map(|r| format!(" @ {}", color::yellow(r)))
            .unwrap_or_default(),
        parsed
            .subpath
            .as_ref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default(),
        parsed
            .skill_filter
            .as_ref()
            .map(|s| format!(" {}{}", color::dim("@"), color::cyan(s)))
            .unwrap_or_default(),
    ));

    // Well-known sources resolve through discovery instead of a fetch.
    let mut well_known_skills: Option<Vec<crate::wellknown::WellKnownSkill>> = None;
    if parsed.kind == SourceKind::WellKnown {
        spinner.message("Discovering skills from well-known endpoint...");
        match crate::wellknown::fetch_all_skills(&parsed.url, requested_internal(options)) {
            Ok(skills) if skills.is_empty() => {
                // Fall through to direct download.
                parsed.kind = SourceKind::Download;
            }
            Ok(skills) => {
                spinner.stop(&format!("Found {} skill(s)", skills.len()));
                well_known_skills = Some(skills);
            }
            Err(err) => {
                if err.code == "SKILLS_WELLKNOWN_DIGEST_MISMATCH"
                    || err.code == "SKILLS_WELLKNOWN_SCOPE_NOT_FOUND"
                {
                    return Err(err);
                }
                parsed.kind = SourceKind::Download;
            }
        }
    }

    let include_internal = !options.skill.is_empty() && !options.skill.contains(&"*".to_string());

    if parsed.kind == SourceKind::Local {
        spinner.message("Validating local path…");
        let local_path = parsed
            .local_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&parsed.url));
        if !local_path.exists() {
            spinner.stop(&color::red("Path not found"));
            let error = diag::local_path_not_found(&local_path.to_string_lossy());
            prompt::outro(&color::red(&error.plain()));
            return Err(error);
        }
        spinner.stop("Local path validated");
    }

    // Resolve the source into skills.
    let resolved = if well_known_skills.is_none() {
        let resolve_options = ResolveOptions {
            include_internal,
            full_depth: options.full_depth,
            include_duplicate_names: false,
            skill_filter: parsed.skill_filter.clone(),
        };
        let resolved = resolver::resolve_skills(&parsed, &resolve_options, Some(&spinner))?;
        for warning in &resolved.discovery.warnings {
            prompt::log_warn(&warning.plain());
        }
        Some(resolved)
    } else {
        None
    };
    let payloads = if let Some(resolved) = &resolved {
        SkillPayload::from_resolved(resolved)
    } else {
        well_known_skills
            .unwrap_or_default()
            .into_iter()
            .map(SkillPayload::WellKnown)
            .collect()
    };
    let root = resolved
        .as_ref()
        .and_then(|resolved| resolved.root_dir.as_deref());
    let blob_result = resolved
        .as_ref()
        .and_then(|resolved| resolved.blob_result.as_ref());

    if payloads.is_empty() {
        spinner.stop(&color::red("No skills found"));
        let error = diag::no_valid_skills();
        prompt::outro(&color::red(&error.plain()));
        return Err(error);
    }
    if resolved.is_some() && blob_result.is_none() {
        spinner.stop(&format!("Found {} skill(s)", payloads.len()));
    }

    // --list output.
    if options.list {
        prompt::line_break();
        prompt::log_step(&color::bold("Available Skills"));
        let mut grouped: std::collections::BTreeMap<String, Vec<&SkillPayload>> =
            std::collections::BTreeMap::new();
        let mut ungrouped: Vec<&SkillPayload> = Vec::new();
        for payload in &payloads {
            match payload.plugin_name() {
                Some(plugin) => grouped.entry(plugin).or_default().push(payload),
                None => ungrouped.push(payload),
            }
        }
        for (group, skills) in &grouped {
            println!("{}", color::bold(&common::kebab_to_title(group)));
            for skill in skills {
                prompt::log_plain(&format!("  {}", color::cyan(&skill.display_name())));
                prompt::log_plain(&format!("    {}", color::dim(&skill.description())));
            }
            prompt::line_break();
        }
        if !ungrouped.is_empty() {
            if !grouped.is_empty() {
                println!("{}", color::bold("General"));
            }
            for skill in &ungrouped {
                prompt::log_plain(&format!("  {}", color::cyan(&skill.display_name())));
                prompt::log_plain(&format!("    {}", color::dim(&skill.description())));
            }
        }
        prompt::line_break();
        prompt::outro("Use --skill <name> to install specific skills");
        return Ok(());
    }

    // Skill selection.
    #[allow(unused_assignments)] // reassigned in every branch below
    let mut selected_payloads: Vec<SkillPayload> = Vec::new();
    if options.skill.contains(&"*".to_string()) {
        selected_payloads = payloads.clone();
        log_auto_selected(&selected_payloads);
    } else if !options.skill.is_empty() {
        let mut selected: Vec<SkillPayload> = Vec::new();
        for requested in &options.skill {
            let exact: Vec<&SkillPayload> = payloads
                .iter()
                .filter(|p| p.display_name().to_lowercase() == requested.to_lowercase())
                .collect();
            let matches: Vec<&SkillPayload> = if exact.is_empty() {
                payloads
                    .iter()
                    .filter(|p| {
                        skills_store::sanitize::normalize_skill_name(&p.display_name())
                            == skills_store::sanitize::normalize_skill_name(requested)
                    })
                    .collect()
            } else {
                exact
            };
            for skill in matches {
                if !selected
                    .iter()
                    .any(|p| p.display_name() == skill.display_name())
                {
                    selected.push((*skill).clone());
                }
            }
            if options.json {
                let found = payloads.iter().any(|p| {
                    p.display_name().to_lowercase() == requested.to_lowercase()
                        || skills_store::sanitize::normalize_skill_name(&p.display_name())
                            == skills_store::sanitize::normalize_skill_name(requested)
                });
                if !found {
                    json_results.push(AddJsonResult {
                        name: Some(requested.clone()),
                        status: "skipped",
                        source: None,
                        ref_: None,
                        hash: None,
                        path: None,
                        scope: None,
                        agents: None,
                        mode: None,
                        reason: Some("No matching skill found in source"),
                        error: None,
                    });
                }
            }
        }
        if selected.is_empty() {
            let available: Vec<String> = payloads.iter().map(|p| p.display_name()).collect();
            let error = diag::no_matching_skills(&options.skill, &available);
            prompt::log_error(&error.plain());
            return Err(error);
        }
        selected_payloads = selected;
        let names: Vec<String> = selected_payloads
            .iter()
            .map(|p| color::cyan(&p.display_name()))
            .collect();
        prompt::log_info(&format!(
            "Selected {} skill(s): {}",
            selected_payloads.len(),
            names.join(", ")
        ));
    } else if payloads.len() == 1 || options.yes {
        selected_payloads = payloads.clone();
        log_auto_selected(&selected_payloads);
    } else {
        // Interactive multiselect with plugin groups.
        let mut sorted = payloads.clone();
        sorted.sort_by(|a, b| {
            let plugin_a = a.plugin_name();
            let plugin_b = b.plugin_name();
            match (plugin_a, plugin_b) {
                (Some(_pa), None) => std::cmp::Ordering::Less,
                (None, Some(_pb)) => std::cmp::Ordering::Greater,
                (Some(pa), Some(pb)) if pa != pb => pa.cmp(&pb),
                _ => a
                    .display_name()
                    .to_lowercase()
                    .cmp(&b.display_name().to_lowercase()),
            }
        });
        let has_groups = sorted.iter().any(|p| p.plugin_name().is_some());
        let items: Vec<SearchItem> = sorted
            .iter()
            .map(|p| SearchItem {
                value: p.display_name(),
                label: p.display_name(),
                hint: None,
                group: p.plugin_name().map(|g| common::kebab_to_title(g.as_str())),
                detail: Some(p.description()),
            })
            .collect();
        let selected = crate::search_multiselect::search_multiselect(&SearchMultiselectOptions {
            message: if has_groups {
                format!(
                    "Select skills to install {}",
                    color::dim("(space to toggle)")
                )
            } else {
                "Select skills to install".to_string()
            },
            items,
            initial_selected: Vec::new(),
            required: true,
            locked_section: None,
            searchable: !has_groups,
            show_detail: true,
            show_selected_summary: false,
            select_groups: has_groups,
            select_all: true,
            max_visible: 20,
        });
        let Some(selected) = selected else {
            return installation_cancelled();
        };
        selected_payloads = sorted
            .into_iter()
            .filter(|p| selected.contains(&p.display_name()))
            .collect();
    }

    // Agent selection.
    let mut target_agents: Vec<&'static AgentConfig>;
    if options.agent.contains(&"*".to_string()) {
        target_agents = registry::agents().iter().collect();
        prompt::log_info(&format!("Installing to all {} agents", target_agents.len()));
    } else if !options.agent.is_empty() {
        target_agents = options
            .agent
            .iter()
            .filter_map(|name| registry::agent_by_id(name))
            .collect();
    } else {
        spinner.message("Loading agents…");
        let installed_agents = registry::detect_installed_agents(&cwd);
        let total_agents = registry::agents().len();
        spinner.stop(&format!("{total_agents} agents"));

        if installed_agents.is_empty() {
            if options.yes {
                target_agents = registry::agents().iter().collect();
                prompt::log_info("Installing to all agents");
            } else {
                prompt::log_info("Select agents to install skills to");
                let items: Vec<SearchItem> = registry::agents()
                    .iter()
                    .filter(|a| a.name != "eve")
                    .map(|a| SearchItem {
                        value: a.name.to_string(),
                        label: a.display_name.to_string(),
                        hint: None,
                        group: None,
                        detail: None,
                    })
                    .collect();
                let selected = prompt_for_agents("Which agents do you want to install to?", items);
                let Some(selected) = selected else {
                    return installation_cancelled();
                };
                target_agents = selected;
            }
        } else if installed_agents.len() == 1 || options.yes {
            let names: Vec<String> = installed_agents
                .iter()
                .map(|a| color::cyan(a.display_name))
                .collect();
            target_agents = common::ensure_universal_agents(installed_agents);
            prompt::log_info(&format!("Installing to: {}", names.join(", ")));
        } else {
            let Some(selected) = select_agents_interactive(options.global) else {
                return installation_cancelled();
            };
            target_agents = selected;
        }
    }

    // Eve subagents.
    let mut eve_subagent_targets: Vec<Option<String>> = vec![None];
    if target_agents.iter().any(|a| a.name == "eve") && !options.subagent.is_empty() {
        eve_subagent_targets = options
            .subagent
            .iter()
            .map(|s| {
                if s == "root" || s == "." {
                    None
                } else {
                    Some(s.clone())
                }
            })
            .collect();
    }

    // Scope selection.
    let mut install_globally = options.global.unwrap_or(false);
    let supports_global = target_agents.iter().any(|a| global_dir_supported(a));
    if options.global.is_none() && !options.yes && supports_global {
        let scope = prompt::select(
            "Installation scope",
            &[
                prompt::SelectOption {
                    value: "project".to_string(),
                    label: "Project".to_string(),
                    hint: Some(
                        "Install in current directory (committed with your project)".to_string(),
                    ),
                },
                prompt::SelectOption {
                    value: "global".to_string(),
                    label: "Global".to_string(),
                    hint: Some(
                        "Install in home directory (available across all projects)".to_string(),
                    ),
                },
            ],
            None,
        );
        match scope {
            Some(value) => install_globally = value == "global",
            None => return installation_cancelled(),
        }
    }

    let explicit_agents = !options.agent.is_empty();
    target_agents =
        common::skip_agents_without_global_dir(target_agents, install_globally, explicit_agents);
    let install_targets = build_install_targets(&target_agents, &eve_subagent_targets);

    if install_targets.is_empty() {
        return Err(SkillsError::new(
            "SKILLS_INSTALL_FAILED",
            "no install targets for the selected scope",
        ));
    }
    // Validate the lock before touching any harness directory.
    if install_globally {
        locks::read_skill_lock()?;
    } else {
        locks::read_local_lock(&cwd)?;
    }

    // Install mode.
    let mut install_mode = if options.copy {
        installer::InstallMode::Copy
    } else {
        default_install_mode_for(&install_targets)
    };
    let unique_dirs: HashSet<&str> = target_agents.iter().map(|a| a.skills_dir).collect();
    if !options.copy && !options.yes && install_mode == installer::InstallMode::Symlink {
        let mode_choice = prompt::select(
            "Installation method",
            &[
                prompt::SelectOption {
                    value: "symlink".to_string(),
                    label: "Symlink (Recommended)".to_string(),
                    hint: Some("Single source of truth, easy updates".to_string()),
                },
                prompt::SelectOption {
                    value: "copy".to_string(),
                    label: "Copy to all agents".to_string(),
                    hint: Some("Independent copies for each agent".to_string()),
                },
            ],
            None,
        );
        match mode_choice {
            Some(value) => {
                install_mode = if value == "copy" {
                    installer::InstallMode::Copy
                } else {
                    installer::InstallMode::Symlink
                };
            }
            None => return installation_cancelled(),
        }
    } else if unique_dirs.len() <= 1 && !options.copy {
        install_mode = installer::InstallMode::Copy;
    }

    // Overwrite checks + summary.
    let mut summary_lines: Vec<String> = Vec::new();
    for payload in &selected_payloads {
        if !summary_lines.is_empty() {
            summary_lines.push(String::new());
        }
        let skill_name = sanitize_name(payload.name());
        let canonical_path = installer::get_canonical_path(&skill_name, install_globally, &cwd);
        summary_lines.push(color::cyan(&common::shorten_path(&canonical_path, &cwd)));
        summary_lines.extend(common::build_agent_summary_lines(
            &target_agents,
            install_mode,
            install_globally,
        ));
        let overwrite_agents: Vec<String> = install_targets
            .iter()
            .filter(|target| {
                installer::is_skill_installed(
                    &skill_name,
                    target.agent,
                    install_globally,
                    &cwd,
                    target.subagent.as_deref(),
                )
            })
            .map(target_display_name)
            .collect();
        if !overwrite_agents.is_empty() {
            summary_lines.push(format!(
                "  {} {}",
                color::yellow("overwrites:"),
                common::format_list(&overwrite_agents, 5)
            ));
        }
    }
    // Security audit results (public skills.sh data; never blocks install).
    {
        let audit_source = source_parser::get_owner_repo(&parsed);
        if let Some(owner_repo) = audit_source {
            let slugs: Vec<String> = selected_payloads.iter().map(|p| p.display_name()).collect();
            if let Some(audit) = crate::find_api::fetch_audit_data(&owner_repo, &slugs) {
                let mut security_lines: Vec<String> = Vec::new();
                let name_width = slugs
                    .iter()
                    .map(|s| s.chars().count())
                    .max()
                    .unwrap_or(0)
                    .min(36);
                let pad = |text: &str, width: usize| {
                    let visible = color::visible_len(text);
                    format!("{text}{}", " ".repeat(width.saturating_sub(visible)))
                };
                security_lines.push(format!(
                    "{}{}{}",
                    pad("", name_width + 2),
                    pad(color::dim("Gen").as_str(), 18),
                    pad(color::dim("Socket").as_str(), 18)
                ));
                for slug in &slugs {
                    let data = audit.get(slug);
                    let risk_label = |risk: Option<&String>| match risk {
                        Some(risk) if risk == "critical" => color::bold_red("Critical Risk"),
                        Some(risk) if risk == "high" => color::red("High Risk"),
                        Some(risk) if risk == "medium" => color::yellow("Med Risk"),
                        Some(risk) if risk == "low" => color::green("Low Risk"),
                        Some(risk) if risk == "safe" => color::green("Safe"),
                        _ => color::dim("--"),
                    };
                    let socket_label = |alerts: Option<u64>| match alerts {
                        Some(count) if count > 0 => color::red(&format!("{count} alerts")),
                        Some(_) => color::green("0 alerts"),
                        None => color::dim("--"),
                    };
                    let gen = data
                        .and_then(|d| d.get("gen"))
                        .map(|a| risk_label(Some(&a.risk)))
                        .unwrap_or_else(|| color::dim("--"));
                    let socket = data
                        .and_then(|d| d.get("socket"))
                        .map(|a| socket_label(a.alerts))
                        .unwrap_or_else(|| color::dim("--"));
                    let snyk = data
                        .and_then(|d| d.get("snyk"))
                        .map(|a| risk_label(Some(&a.risk)))
                        .unwrap_or_else(|| color::dim("--"));
                    security_lines.push(format!(
                        "{}{}{}{}",
                        pad(color::cyan(slug).as_str(), name_width + 2),
                        pad(gen.as_str(), 18),
                        pad(socket.as_str(), 18),
                        snyk
                    ));
                }
                security_lines.push(String::new());
                security_lines.push(format!(
                    "{} {}",
                    color::dim("Details:"),
                    color::dim(&format!("https://skills.sh/{owner_repo}"))
                ));
                prompt::line_break();
                prompt::note(&security_lines.join("\n"), "Security Risk Assessments");
            }
        }
    }

    prompt::line_break();
    prompt::note(&summary_lines.join("\n"), "Installation Summary");

    if !options.yes {
        match prompt::confirm("Proceed with installation?", true) {
            Some(true) => {}
            Some(false) | None => return installation_cancelled(),
        }
    }

    let hashes = selected_payloads
        .iter()
        .map(SkillPayload::content_hash)
        .collect::<Result<Vec<_>, _>>()?;
    spinner.message("Installing skills…");
    let mut results: Vec<(String, String, TargetInstallResult)> = Vec::new();
    let mut recorded = Vec::new();
    for (payload, hash) in selected_payloads.iter().zip(hashes) {
        let mut placements = Vec::new();
        for target in &install_targets {
            let result = install_payload_for_target(
                payload,
                target,
                install_globally,
                &cwd,
                install_mode,
                explicitly_selected_agents.contains(target.agent.name),
                &mut Vec::new(),
            );
            if result.success && (!result.skipped || result.path.join("SKILL.md").is_file()) {
                // A skipped agent may have materialized only the shared copy.
                let expected = installer::get_install_path(
                    payload.name(),
                    target.agent,
                    install_globally,
                    &cwd,
                    target.subagent.as_deref(),
                );
                if expected.join("SKILL.md").is_file() {
                    placements.push(locks::SkillPlacement {
                        agent: target.agent.name.to_string(),
                        subagent: target.subagent.clone(),
                        mode: if result.symlink_failed {
                            installer::InstallMode::Copy
                        } else {
                            result.mode
                        }
                        .as_str()
                        .to_string(),
                    });
                }
            }
            results.push((payload.display_name(), target_display_name(target), result));
        }
        recorded.push((payload, hash, placements));
    }
    spinner.stop("Installation complete");

    prompt::line_break();
    let successful: Vec<&(String, String, TargetInstallResult)> =
        results.iter().filter(|(_, _, r)| r.success).collect();
    let failed: Vec<&(String, String, TargetInstallResult)> =
        results.iter().filter(|(_, _, r)| !r.success).collect();

    // Lock files.
    let normalized_source = match parsed.kind {
        SourceKind::Local => None,
        _ => source_parser::get_owner_repo(&parsed),
    };
    let json_source = normalized_source
        .clone()
        .unwrap_or_else(|| match parsed.kind {
            SourceKind::Local => parsed
                .local_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&parsed.url))
                .to_string_lossy()
                .to_string(),
            _ => parsed.url.clone(),
        });

    let mut installed_hashes = std::collections::HashMap::new();
    let mut local = (!install_globally)
        .then(|| locks::read_local_lock(&cwd))
        .transpose()?;
    let mut global = install_globally.then(locks::read_skill_lock).transpose()?;
    for (payload, hash, placements) in recorded {
        if placements.is_empty() {
            continue;
        }
        let path = payload.source_path(root);
        let mut entry = lock_entry_for_payload(
            &parsed,
            payload,
            hash.clone(),
            path,
            placements,
            install_mode,
        );
        let name = payload.name();
        if let Some(lock) = &mut local {
            if let Some(old) = lock
                .skills
                .get(name)
                .filter(|old| old.source == entry.source && old.ref_ == entry.ref_)
            {
                if merge_placements(&mut entry.placements, &old.placements) {
                    // The previous fingerprint keeps the other placements eligible for update.
                    entry.computed_hash.clone_from(&old.computed_hash);
                }
            }
            lock.skills.insert(name.to_string(), entry);
        } else if let Some(lock) = &mut global {
            let mut installed_at = iso_now();
            let mut prior_fingerprint = None;
            if let Some(old) = lock.skills.get(name) {
                installed_at.clone_from(&old.installed_at);
                if old.source_url == parsed.url
                    && old.ref_ == entry.ref_
                    && merge_placements(&mut entry.placements, &old.placements)
                {
                    prior_fingerprint =
                        Some((old.computed_hash.clone(), old.skill_folder_hash.clone()));
                }
            }
            let tree_hash = blob_result
                .and_then(|blob| {
                    crate::blob::get_skill_folder_hash_from_tree(
                        &blob.tree,
                        entry.skill_path.as_deref()?,
                    )
                })
                .or_else(|| {
                    root.zip(entry.skill_path.as_deref())
                        .and_then(|(root, path)| {
                            resolved.as_ref()?.commit.as_ref()?;
                            crate::git::get_git_tree_hash(root, path)
                        })
                })
                .unwrap_or_default();
            let (computed_hash, tree_hash) =
                prior_fingerprint.unwrap_or((Some(hash.clone()), tree_hash));
            lock.skills.insert(
                name.to_string(),
                locks::SkillLockEntry {
                    source: entry.source,
                    source_type: entry.source_type,
                    source_url: parsed.url.clone(),
                    ref_: entry.ref_,
                    skill_path: entry.skill_path,
                    skill_folder_hash: tree_hash,
                    installed_at,
                    updated_at: iso_now(),
                    plugin_name: payload.plugin_name(),
                    source_base_url: None,
                    well_known_digest: entry.well_known_digest,
                    install_mode: entry.install_mode,
                    placements: entry.placements,
                    computed_hash,
                },
            );
        }
        installed_hashes.insert(payload.display_name(), hash);
    }
    if !installed_hashes.is_empty() {
        if let Some(lock) = local {
            locks::write_local_lock(&lock, &cwd)?;
        }
        if let Some(lock) = global {
            locks::write_skill_lock(&lock)?;
        }
    }
    if !failed.is_empty() {
        common::set_exit_code_1();
    }

    // JSON output.
    if options.json {
        for payload in &selected_payloads {
            let name = payload.display_name();
            let skill_results: Vec<&(String, String, TargetInstallResult)> = results
                .iter()
                .filter(|(skill, _, _)| skill == &name)
                .collect();
            let failures: Vec<&&(String, String, TargetInstallResult)> = skill_results
                .iter()
                .filter(|(_, _, r)| !r.success)
                .collect();
            if !failures.is_empty() {
                json_results.push(AddJsonResult {
                    name: Some(name),
                    status: "failed",
                    source: None,
                    ref_: None,
                    hash: None,
                    path: None,
                    scope: None,
                    agents: None,
                    mode: None,
                    reason: None,
                    error: failures
                        .first()
                        .and_then(|(_, _, r)| r.error.clone())
                        .or_else(|| Some("Installation failed".to_string())),
                });
                continue;
            }
            let first = skill_results.first().map(|(_, _, r)| r);
            json_results.push(AddJsonResult {
                name: Some(name.clone()),
                status: "installed",
                source: Some(json_source.clone()),
                ref_: parsed.ref_.clone(),
                hash: installed_hashes.get(&name).cloned(),
                path: first
                    .and_then(|r| r.canonical_path.clone().or_else(|| Some(r.path.clone())))
                    .map(|p| p.to_string_lossy().to_string()),
                scope: Some(if install_globally {
                    "global"
                } else {
                    "project"
                }),
                agents: Some(
                    skill_results
                        .iter()
                        .filter(|(_, _, r)| !r.skipped)
                        .map(|(_, agent, _)| agent.clone())
                        .collect(),
                ),
                mode: Some(
                    first
                        .map(|r| r.mode.as_str())
                        .unwrap_or(install_mode.as_str()),
                ),
                reason: None,
                error: None,
            });
        }
        emit_json(json_results);
        return Ok(());
    }

    // Result rendering.
    if !successful.is_empty() {
        #[allow(clippy::type_complexity)] // grouped install results, upstream shape
        let mut by_skill: Vec<(String, Vec<&(String, String, TargetInstallResult)>)> = Vec::new();
        for entry in &successful {
            match by_skill.iter_mut().find(|(skill, _)| skill == &entry.0) {
                Some((_, group)) => group.push(entry),
                None => by_skill.push((entry.0.clone(), vec![entry])),
            }
        }
        let skill_count = by_skill.len();
        let symlink_failures: Vec<String> = successful
            .iter()
            .filter(|(_, _, r)| r.mode == installer::InstallMode::Symlink && r.symlink_failed)
            .map(|(_, agent, _)| agent.clone())
            .collect();
        let mut result_lines: Vec<String> = Vec::new();
        for (skill_name, skill_results) in &by_skill {
            let first = &skill_results[0].2;
            if first.mode == installer::InstallMode::Copy {
                result_lines.push(format!(
                    "{} {} {}",
                    color::green("\u{2713}"),
                    skill_name,
                    color::dim("(copied)")
                ));
                let mut seen: HashSet<String> = HashSet::new();
                for (_, _, result) in skill_results {
                    let short = common::shorten_path(&result.path, &cwd);
                    if seen.insert(short.clone()) {
                        result_lines.push(format!("  {} {}", color::dim("\u{2192}"), short));
                    }
                }
            } else {
                match &first.canonical_path {
                    Some(canonical) => {
                        result_lines.push(format!(
                            "{} {}",
                            color::green("\u{2713}"),
                            common::shorten_path(canonical, &cwd)
                        ));
                    }
                    None => {
                        result_lines.push(format!("{} {}", color::green("\u{2713}"), skill_name));
                    }
                }
                let target_agent_names: Vec<&AgentConfig> = target_agents.to_vec();
                let tuple_results: Vec<(String, bool, bool, Option<String>)> = skill_results
                    .iter()
                    .map(|(_, agent, r)| {
                        (
                            agent.clone(),
                            r.symlink_failed,
                            r.skipped,
                            r.skip_reason.map(str::to_string),
                        )
                    })
                    .collect();
                result_lines.extend(common::build_result_lines(
                    &tuple_results,
                    &target_agent_names,
                    install_globally,
                ));
            }
        }
        let title = color::green(&format!(
            "Installed {skill_count} skill{}",
            if skill_count != 1 { "s" } else { "" }
        ));
        prompt::note(&result_lines.join("\n"), &title);
        if !symlink_failures.is_empty() {
            prompt::log_warn(&color::yellow(&format!(
                "Symlinks failed for: {}",
                common::format_list(&symlink_failures, 5)
            )));
            prompt::log_plain(&color::dim(
                "  Files were copied instead. On Windows, enable Developer Mode for symlink support.",
            ));
        }
    }

    if !failed.is_empty() {
        prompt::line_break();
        prompt::log_error(&color::red(&format!("Failed to install {}", failed.len())));
        for (skill, agent, result) in &failed {
            prompt::log_plain(&format!(
                "  {} {} → {}: {}",
                color::red("\u{2717}"),
                skill,
                agent,
                color::dim(result.error.as_deref().unwrap_or("unknown"))
            ));
        }
    }

    prompt::line_break();
    prompt::outro(&format!(
        "{}{}",
        color::green("Done!"),
        color::dim("  Review skills before use; they run with full agent permissions.")
    ));

    if !successful.is_empty() {
        prompt_for_find_skills(options, &target_agents);
    }
    Ok(())
}

fn requested_internal(options: &AddOptions) -> bool {
    !options.skill.is_empty() && !options.skill.contains(&"*".to_string())
}

fn log_auto_selected(payloads: &[SkillPayload]) {
    if payloads.len() > 1 {
        prompt::log_info(&format!("Installing all {} skills", payloads.len()));
        return;
    }
    if let Some(only) = payloads.first() {
        prompt::log_info(&format!("Skill: {}", color::cyan(&only.display_name())));
        if !only.description().is_empty() {
            prompt::log_plain(&color::dim(&only.description()));
        }
    }
}

fn prompt_for_agents(message: &str, items: Vec<SearchItem>) -> Option<Vec<&'static AgentConfig>> {
    let last_selected = locks::read_skill_lock()
        .ok()
        .and_then(|lock| lock.last_selected_agents);
    let valid: HashSet<&str> = items.iter().map(|item| item.value.as_str()).collect();
    let defaults = ["claude-code", "opencode", "codex", "grok"];
    let initial: Vec<String> = match last_selected {
        Some(selected) if !selected.is_empty() => selected
            .into_iter()
            .filter(|name| valid.contains(name.as_str()))
            .collect(),
        _ => defaults
            .iter()
            .filter(|name| valid.contains(**name))
            .map(|name| name.to_string())
            .collect(),
    };
    let selected = crate::search_multiselect::search_multiselect(&SearchMultiselectOptions {
        message: message.to_string(),
        items,
        initial_selected: initial,
        required: true,
        locked_section: None,
        searchable: true,
        show_detail: false,
        show_selected_summary: true,
        select_groups: false,
        select_all: false,
        max_visible: 8,
    })?;
    if let Ok(mut lock) = locks::read_skill_lock() {
        lock.last_selected_agents = Some(selected.clone());
        let _ = locks::write_skill_lock(&lock);
    }
    Some(
        selected
            .iter()
            .filter_map(|name| registry::agent_by_id(name))
            .collect(),
    )
}

fn merge_placements(
    current: &mut Vec<locks::SkillPlacement>,
    previous: &[locks::SkillPlacement],
) -> bool {
    let original_count = current.len();
    for placement in previous {
        if !current
            .iter()
            .any(|new| new.agent == placement.agent && new.subagent == placement.subagent)
        {
            current.push(placement.clone());
        }
    }
    current.len() != original_count
}

fn lock_entry_for_payload(
    parsed: &source_parser::ParsedSource,
    payload: &SkillPayload,
    computed_hash: String,
    skill_path: Option<String>,
    placements: Vec<locks::SkillPlacement>,
    mode: installer::InstallMode,
) -> locks::LocalSkillLockEntry {
    let source_url = (parsed.kind != SourceKind::Local).then(|| parsed.url.clone());
    locks::LocalSkillLockEntry {
        source: if parsed.kind == SourceKind::WellKnown {
            crate::wellknown::get_source_identifier(&parsed.url)
        } else {
            get_lock_source(
                &parsed.url,
                source_parser::get_owner_repo(parsed).as_deref(),
            )
            .unwrap_or_else(|| parsed.url.clone())
        },
        source_url,
        ref_: parsed.ref_.clone(),
        source_type: parsed.kind.as_str().to_string(),
        skill_path,
        version: None,
        via: None,
        computed_hash,
        subagents: placements.iter().any(|p| p.agent == "eve").then(|| {
            placements
                .iter()
                .filter(|p| p.agent == "eve")
                .map(|p| p.subagent.clone().unwrap_or_default())
                .collect()
        }),
        well_known_digest: match payload {
            SkillPayload::WellKnown(skill) => {
                Some(crate::wellknown::compute_well_known_skill_digest(skill))
            }
            _ => None,
        },
        install_mode: Some(mode.as_str().to_string()),
        placements,
    }
}

/// Preserve SSH URLs and non-github.com hosts in lock sources
/// (upstream `getLockSource`).
#[must_use]
pub fn get_lock_source(parsed_url: &str, normalized_source: Option<&str>) -> Option<String> {
    if parsed_url.starts_with("git@") || parsed_url.starts_with("ssh://") {
        return Some(parsed_url.to_string());
    }
    if parsed_url.starts_with("http://") || parsed_url.starts_with("https://") {
        if let Some((host, _)) = source_parser::host_and_path(parsed_url) {
            if host != "github.com" {
                return Some(parsed_url.to_string());
            }
        } else {
            return normalized_source.map(str::to_string);
        }
    }
    normalized_source.map(str::to_string)
}

/// RFC 3339 UTC timestamp matching `new Date().toISOString()`.
pub fn iso_now() -> String {
    // RFC 3339 UTC timestamp with millisecond precision, matching JS
    // `new Date().toISOString()`.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = secs / 86_400;
    let time_secs = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time_secs / 3600,
        (time_secs % 3600) / 60,
        time_secs % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn emit_json(json_results: &[AddJsonResult]) {
    println!(
        "{}",
        serde_json::to_string_pretty(json_results).unwrap_or_else(|_| "[]".to_string())
    );
}

/// One-time find-skills prompt after a successful install (upstream
/// `promptForFindSkills`).
fn prompt_for_find_skills(options: &AddOptions, target_agents: &[&'static AgentConfig]) {
    if !prompt::is_tty() || options.yes {
        return;
    }
    let dismissed = locks::read_skill_lock()
        .ok()
        .and_then(|lock| lock.dismissed)
        .and_then(|d| d.find_skills_prompt)
        .unwrap_or(false);
    if dismissed {
        return;
    }
    let find_skills_installed = registry::agent_by_id("claude-code").is_some_and(|agent| {
        installer::is_skill_installed("find-skills", agent, true, &common::current_dir(), None)
    });
    if find_skills_installed {
        if let Ok(mut lock) = locks::read_skill_lock() {
            let dismissed = lock.dismissed.get_or_insert_with(Default::default);
            dismissed.find_skills_prompt = Some(true);
            let _ = locks::write_skill_lock(&lock);
        }
        return;
    }
    prompt::line_break();
    prompt::log_plain(&color::dim(
        "One-time prompt - you won't be asked again if you dismiss.",
    ));
    match prompt::confirm(
        &format!(
            "Install the {} skill? It helps your agent discover and suggest skills.",
            color::cyan("find-skills")
        ),
        true,
    ) {
        Some(true) => {
            if let Ok(mut lock) = locks::read_skill_lock() {
                let dismissed = lock.dismissed.get_or_insert_with(Default::default);
                dismissed.find_skills_prompt = Some(true);
                let _ = locks::write_skill_lock(&lock);
            }
            let agents: Vec<&'static AgentConfig> = target_agents
                .iter()
                .copied()
                .filter(|a| a.name != "replit")
                .collect();
            if agents.is_empty() {
                return;
            }
            prompt::line_break();
            prompt::log_step("Installing find-skills skill…");
            let agent_flags = agents.iter().map(|a| a.name.to_string()).collect();
            let add_options = AddOptions {
                skill: vec!["find-skills".to_string()],
                global: Some(true),
                yes: true,
                agent: agent_flags,
                ..Default::default()
            };
            run_add(vec!["vercel-labs/skills".to_string()], add_options);
        }
        Some(false) => {
            if let Ok(mut lock) = locks::read_skill_lock() {
                let dismissed = lock.dismissed.get_or_insert_with(Default::default);
                dismissed.find_skills_prompt = Some(true);
                let _ = locks::write_skill_lock(&lock);
            }
            prompt::log_plain(&color::dim(
                "You can install it later with: skillz add vercel-labs/skills@find-skills",
            ));
        }
        None => {
            if let Ok(mut lock) = locks::read_skill_lock() {
                let dismissed = lock.dismissed.get_or_insert_with(Default::default);
                dismissed.find_skills_prompt = Some(true);
                let _ = locks::write_skill_lock(&lock);
            }
        }
    }
}
