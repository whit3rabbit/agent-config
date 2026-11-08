//! Shared display helpers used across commands (ported from add.ts / list.ts).

use std::path::{Path, PathBuf};

use skills_store::registry::AgentConfig;
use skills_store::{installer, registry};

use crate::color;

/// Split target agents into universal vs symlinked display names.
#[must_use]
pub fn split_agents_by_type(agent_types: &[&AgentConfig], global: bool) -> (Vec<String>, Vec<String>) {
    let mut universal = Vec::new();
    let mut symlinked = Vec::new();
    for agent in agent_types {
        if if global { agent.reads_canonical_globally() } else { agent.is_universal() } {
            universal.push(agent.display_name.to_string());
        } else {
            symlinked.push(agent.display_name.to_string());
        }
    }
    (universal, symlinked)
}

/// Format a list, truncating past `max_show` items.
#[must_use]
pub fn format_list(items: &[String], max_show: usize) -> String {
    if items.len() <= max_show {
        return items.join(", ");
    }
    format!("{} +{} more", items[..max_show].join(", "), items.len() - max_show)
}

/// Build the `universal:` / `symlink →` / `copy →` summary lines.
#[must_use]
pub fn build_agent_summary_lines(
    target_agents: &[&AgentConfig],
    install_mode: installer::InstallMode,
    global: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let (universal, symlinked) = split_agents_by_type(target_agents, global);
    if install_mode == installer::InstallMode::Symlink {
        if !universal.is_empty() {
            lines.push(format!(
                "  {} {}",
                color::green("universal:"),
                format_list(&universal, 5)
            ));
        }
        if !symlinked.is_empty() {
            lines.push(format!(
                "  {} {}",
                color::dim("symlink →"),
                format_list(&symlinked, 5)
            ));
        }
    } else {
        let all_names: Vec<String> =
            target_agents.iter().map(|a| a.display_name.to_string()).collect();
        lines.push(format!("  {} {}", color::dim("copy →"), format_list(&all_names, 5)));
    }
    lines
}

/// Build the post-install result lines (universal/symlinked/copied/skipped).
#[must_use]
pub fn build_result_lines(
    results: &[(String, bool, bool, Option<String>)],
    target_agents: &[&AgentConfig],
    global: bool,
) -> Vec<String> {
    // Tuple: (agent display name, symlink_failed, skipped, skip_reason)
    let mut lines = Vec::new();
    let (universal, symlink_agents) = split_agents_by_type(target_agents, global);

    let successful_symlinks: Vec<String> = results
        .iter()
        .filter(|(agent, failed, skipped, _)| !failed && !skipped && !universal.contains(agent))
        .map(|(agent, _, _, _)| agent.clone())
        .collect();
    let failed_symlinks: Vec<String> = results
        .iter()
        .filter(|(agent, failed, skipped, _)| *failed && !skipped && symlink_agents.contains(agent))
        .map(|(agent, _, _, _)| agent.clone())
        .collect();
    let skipped_symlinks: Vec<String> = results
        .iter()
        .filter(|(agent, _, skipped, reason)| {
            *skipped && reason.as_deref() == Some("missing-agent-project-directory")
                && symlink_agents.contains(agent)
        })
        .map(|(agent, _, _, _)| agent.clone())
        .collect();

    if !universal.is_empty() {
        lines.push(format!("  {} {}", color::green("universal:"), format_list(&universal, 5)));
    }
    if !successful_symlinks.is_empty() {
        lines.push(format!(
            "  {} {}",
            color::dim("symlinked:"),
            format_list(&successful_symlinks, 5)
        ));
    }
    if !failed_symlinks.is_empty() {
        lines.push(format!("  {} {}", color::yellow("copied:"), format_list(&failed_symlinks, 5)));
    }
    if !skipped_symlinks.is_empty() {
        lines.push(format!(
            "  {} {} {}",
            color::yellow("skipped:"),
            format_list(&skipped_symlinks, 5),
            color::dim("(project directory not found)")
        ));
    }
    lines
}

/// Replace homedir with `~` and cwd with `.` for display.
#[must_use]
pub fn shorten_path(full_path: &Path, cwd: &Path) -> String {
    let text = full_path.to_string_lossy().to_string();
    let home = skills_store::paths::home_dir();
    let home_text = home.to_string_lossy().to_string();
    let cwd_text = cwd.to_string_lossy().to_string();
    if text == home_text || text.starts_with(&format!("{home_text}/")) || text.starts_with(&format!("{home_text}\\")) {
        return format!("~{}", &text[home_text.len()..]);
    }
    if text == cwd_text || text.starts_with(&format!("{cwd_text}/")) || text.starts_with(&format!("{cwd_text}\\")) {
        return format!(".{}", &text[cwd_text.len()..]);
    }
    text
}

/// The agents `add -y` installs to when none are named.
#[must_use]
pub fn non_interactive_target_agents(cwd: &Path) -> Vec<&'static AgentConfig> {
    let detected = crate::detect_agent::detect_agent();
    if detected.is_agent {
        if let Some(name) = &detected.agent {
            if let Some(agent_id) = crate::detect_agent::get_agent_type(name) {
                if let Some(agent) = registry::agent_by_id(agent_id) {
                    return ensure_universal_agents(vec![agent]);
                }
            }
        }
    }
    let installed = registry::detect_installed_agents(cwd);
    if installed.is_empty() {
        return registry::agents().iter().collect();
    }
    ensure_universal_agents(installed)
}

/// Ensure canonical-reading universal agents are always targeted
/// (upstream `ensureUniversalAgents`).
#[must_use]
pub fn ensure_universal_agents(target_agents: Vec<&'static AgentConfig>) -> Vec<&'static AgentConfig> {
    let mut result = target_agents;
    for universal in registry::universal_agents() {
        if universal.reads_canonical_globally() && !result.iter().any(|a| a.name == universal.name) {
            result.push(universal);
        }
    }
    result
}

/// Skip auto-selected agents without a global skills dir on global installs.
#[must_use]
pub fn skip_agents_without_global_dir(
    target_agents: Vec<&'static AgentConfig>,
    global: bool,
    explicit: bool,
) -> Vec<&'static AgentConfig> {
    if !global || explicit {
        return target_agents;
    }
    let skipped: Vec<&AgentConfig> =
        target_agents.iter().copied().filter(|a| global_dir_none(a)).collect();
    if skipped.is_empty() || skipped.len() == target_agents.len() {
        return target_agents;
    }
    let names: Vec<&str> = skipped.iter().map(|a| a.display_name).collect();
    crate::prompt::log_info(&format!(
        "Skipping {}: no global skills directory",
        names.join(", ")
    ));
    target_agents.into_iter().filter(|a| !global_dir_none(a)).collect()
}

fn global_dir_none(agent: &AgentConfig) -> bool {
    let ctx = registry::DetectContext::from_env(Path::new("."));
    agent.global_skills_dir(&ctx).is_none()
}

/// Kebab-case to Title Case.
#[must_use]
pub fn kebab_to_title(value: &str) -> String {
    value
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Default cwd for commands.
#[must_use]
pub fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Read the global skill lock (best effort).
#[allow(dead_code)]
#[must_use]
pub fn read_global_lock() -> skills_store::SkillLock {
    skills_store::read_skill_lock().unwrap_or_default()
}

/// Process exit code flag: set to 1 when a non-fatal failure occurred but
/// execution continued (mirrors upstream `process.exitCode = 1`).
pub static EXIT_CODE_ONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record that the process should exit 1 without aborting the current flow.
pub fn set_exit_code_1() {
    EXIT_CODE_ONE.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// The recorded exit code (1 when [`set_exit_code_1`] was called, else 0).
#[must_use]
pub fn exit_code() -> i32 {
    if EXIT_CODE_ONE.load(std::sync::atomic::Ordering::SeqCst) {
        1
    } else {
        0
    }
}
