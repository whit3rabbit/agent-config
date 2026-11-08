//! The `skills find` command, ported from `src/find.ts`: API search with a
//! non-interactive result list and an fzf-style interactive prompt.

use std::io::Write;

use skills_store::sanitize::sanitize_metadata;

use crate::cmd_add;
use crate::color;
use crate::find_api::{format_installs, search_skills_api, SearchSkill};
use crate::prompt::{is_tty, Key, RawSession};

const HIDE_CURSOR: &str = "\x1b[?25l";
#[allow(dead_code)] // used after prompt-driven flows restore the cursor
const SHOW_CURSOR: &str = "\x1b[?25h";

/// Parsed find options.
pub struct FindOptions {
    /// The search query.
    pub query: String,
    /// `--owner` GitHub owner filter.
    pub owner: Option<String>,
}

const GITHUB_OWNER_PATTERN_OK: fn(&str) -> bool = |owner: &str| {
    let bytes = owner.as_bytes();
    if bytes.is_empty() || bytes.len() > 39 {
        return false;
    }
    let first_ok = bytes[0].is_ascii_digit() || bytes[0].is_ascii_alphabetic();
    first_ok
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
};

/// Parse find options (upstream `parseFindOptions`).
#[must_use]
pub fn parse_find_options(args: &[String]) -> (FindOptions, Vec<skills_store::error::SkillsError>) {
    let mut query_parts: Vec<String> = Vec::new();
    let mut owner: Option<String> = None;
    let mut errors = Vec::new();

    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let mut owner_value: Option<String> = None;
        if arg == "--owner" {
            match args.get(index + 1) {
                Some(value) if !value.starts_with('-') => {
                    owner_value = Some(value.clone());
                    index += 1;
                }
                _ => {
                    errors.push(skills_store::error::diag::option_requires_value(
                        "--owner",
                        "a GitHub owner",
                    ));
                }
            }
        } else if let Some(value) = arg.strip_prefix("--owner=") {
            if value.is_empty() {
                errors.push(skills_store::error::diag::option_requires_value(
                    "--owner",
                    "a GitHub owner",
                ));
            } else {
                owner_value = Some(value.to_string());
            }
        } else {
            query_parts.push(arg.clone());
            index += 1;
            continue;
        }

        if let Some(owner_candidate) = owner_value {
            let owner_candidate = owner_candidate.trim().to_lowercase();
            if GITHUB_OWNER_PATTERN_OK(&owner_candidate) {
                owner = Some(owner_candidate);
            } else {
                errors.push(skills_store::error::diag::invalid_option_value(
                    "--owner",
                    "a valid GitHub owner",
                ));
            }
        }
        index += 1;
    }

    (
        FindOptions {
            query: query_parts.join(" "),
            owner,
        },
        errors,
    )
}

/// The `skills find` entry point (upstream `runFind`).
pub fn run_find(args: &[String]) {
    let (options, errors) = parse_find_options(args);
    let owner = options.owner.clone();
    let is_non_interactive = !is_tty();
    let agent_tip = format!(
        "{}Tip: if running in a coding agent, follow these steps:{}\n{}  1) skills find [query] [--owner <owner>]{}\n{}  2) skills add <owner/repo@skill>{}",
        color::dim_gray(""),
        color::reset(""),
        color::dim_gray(""),
        color::reset(""),
        color::dim_gray(""),
        color::reset("")
    );

    if !errors.is_empty() {
        for error in errors {
            eprintln!("{}", error.plain());
        }
        eprintln!("Usage: skills find <query> [--owner <owner>]");
        return;
    }

    // Non-interactive mode: print results and exit.
    if !options.query.is_empty() {
        let results = search_skills_api(&options.query, owner.as_deref());
        if results.is_empty() {
            let owner_suffix = owner
                .as_ref()
                .map(|owner| format!(" from owner \"{owner}\""))
                .unwrap_or_default();
            println!(
                "{}No skills found for \"{}\"{}{}",
                color::dim_gray(""),
                options.query,
                owner_suffix,
                color::reset("")
            );
            return;
        }
        println!(
            "{}Install with{} skills add <owner/repo@skill>",
            color::dim_gray(""),
            color::reset("")
        );
        println!();
        for skill in &results {
            let package = if skill.source.is_empty() {
                &skill.slug
            } else {
                &skill.source
            };
            let installs = format_installs(skill.installs);
            println!(
                "{}{}",
                color::text(&format!("{package}@{}", skill.name)),
                if installs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", color::cyan(&installs))
                }
            );
            println!(
                "{}└ https://skills.sh/{}{}",
                color::dim_gray(""),
                skill.slug,
                color::reset("")
            );
            println!();
        }
        return;
    }

    if is_non_interactive || crate::detect_agent::is_running_in_agent() {
        println!("{agent_tip}");
        println!();
        println!(
            "{}Usage: skills find <query> [--owner <owner>]{}",
            color::dim_gray(""),
            color::reset("")
        );
        return;
    }

    let selected = run_search_prompt("", owner.as_deref());

    let Some(selected) = selected else {
        println!(
            "{}Search cancelled{}",
            color::dim_gray(""),
            color::reset("")
        );
        println!();
        return;
    };

    let package = if selected.source.is_empty() {
        selected.slug.clone()
    } else {
        selected.source.clone()
    };
    let skill_name = selected.name.clone();

    println!();
    println!(
        "{}Installing {} from {}…{}",
        color::text(""),
        color::bold(&skill_name),
        color::dim_gray(&package),
        color::reset("")
    );
    println!();

    let (sources, add_options, _errors) =
        cmd_add::parse_add_options(&[package.clone(), "--skill".to_string(), skill_name.clone()]);
    cmd_add::run_add(sources, add_options);

    println!();

    let owner_repo = get_owner_repo_from_string(&package);
    let public = owner_repo
        .and_then(|(owner, repo)| is_repo_public(&owner, &repo))
        .unwrap_or(false);
    if public {
        println!(
            "{}View the skill at{} {}https://skills.sh/{}{}",
            color::dim_gray(""),
            color::reset(""),
            color::text(""),
            selected.slug,
            color::reset("")
        );
    } else {
        println!(
            "{}Discover more skills at{} {}https://skills.sh{}",
            color::dim_gray(""),
            color::reset(""),
            color::text(""),
            color::reset("")
        );
    }
    println!();
}

fn get_owner_repo_from_string(package: &str) -> Option<(String, String)> {
    let at_index = package.rfind('@');
    let repo_path = match at_index {
        Some(index) if index > 0 => &package[..index],
        _ => package,
    };
    let (owner, repo) = repo_path.split_once('/')?;
    Some((owner.to_string(), repo.to_string()))
}

fn is_repo_public(owner: &str, repo: &str) -> Option<bool> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}");
    let response = crate::http::get(
        &url,
        std::time::Duration::from_secs(10),
        &[("User-Agent", "skills-cli")],
    )
    .ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    let value = response.json().ok()?;
    Some(
        !value
            .get("private")
            .and_then(|p| p.as_bool())
            .unwrap_or(false),
    )
}

/// Interactive fzf-style search prompt (upstream `runSearchPrompt`).
#[allow(clippy::too_many_lines)]
fn run_search_prompt(initial_query: &str, owner: Option<&str>) -> Option<SearchSkill> {
    let mut results: Vec<SearchSkill> = Vec::new();
    let mut selected_index = 0usize;
    let mut query = initial_query.to_string();
    let mut last_rendered_lines = 0usize;

    let session = RawSession::new();
    print!("{HIDE_CURSOR}");
    let _ = std::io::stdout().flush();

    let render = |results: &[SearchSkill],
                  selected_index: usize,
                  query: &str,
                  last_rendered_lines: &mut usize| {
        if *last_rendered_lines > 0 {
            print!("\x1b[{}A\x1b[1G", *last_rendered_lines);
        }
        print!("\x1b[J");
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "{}Search skills:{} {}{}",
            color::text(""),
            color::reset(""),
            query,
            color::bold("_")
        ));
        lines.push(String::new());
        if query.len() < 2 {
            lines.push(format!(
                "{}Start typing to search (min 2 chars){}",
                color::dim_gray(""),
                color::reset("")
            ));
        } else if results.is_empty() {
            lines.push(format!(
                "{}No skills found{}",
                color::dim_gray(""),
                color::reset("")
            ));
        } else {
            let max_visible = 8;
            for (index, skill) in results.iter().enumerate().take(max_visible) {
                let is_selected = index == selected_index;
                let arrow = if is_selected {
                    color::bold(">")
                } else {
                    " ".to_string()
                };
                let name = if is_selected {
                    color::bold(&skill.name)
                } else {
                    color::text(&skill.name)
                };
                let source = if skill.source.is_empty() {
                    String::new()
                } else {
                    format!(" {}", color::dim_gray(&skill.source))
                };
                let installs = format_installs(skill.installs);
                let installs_badge = if installs.is_empty() {
                    String::new()
                } else {
                    format!(" {}", color::cyan(&installs))
                };
                lines.push(format!("  {arrow} {name}{source}{installs_badge}"));
            }
        }
        lines.push(String::new());
        lines.push(format!(
            "{}up/down navigate | enter select | esc cancel{}",
            color::dim_gray(""),
            color::reset("")
        ));
        for line in &lines {
            println!("{line}");
        }
        let _ = std::io::stdout().flush();
        *last_rendered_lines = lines.len();
    };

    // Initial search.
    if !initial_query.is_empty() && initial_query.len() >= 2 {
        results = search_skills_api(initial_query, owner);
    }
    render(&results, selected_index, &query, &mut last_rendered_lines);

    loop {
        let Some(key) = session.next_key() else {
            break None;
        };
        match key {
            Key::Escape => break None,
            Key::CtrlC => break None,
            Key::Enter => break results.get(selected_index).cloned(),
            Key::Up => {
                selected_index = selected_index.saturating_sub(1);
            }
            Key::Down => {
                if !results.is_empty() {
                    selected_index = (selected_index + 1).min(results.len() - 1);
                }
            }
            Key::Backspace => {
                query.pop();
                if query.len() >= 2 {
                    results = search_skills_api(&query, owner);
                } else {
                    results.clear();
                }
                selected_index = 0;
            }
            Key::Char(c) if c != '\0' => {
                query.push(c);
                if query.len() >= 2 {
                    results = search_skills_api(&query, owner);
                } else {
                    results.clear();
                }
                selected_index = 0;
            }
            _ => {}
        }
        render(&results, selected_index, &query, &mut last_rendered_lines);
    }
}

/// Sanitized result metadata (shared with tests).
#[allow(dead_code)]
#[must_use]
pub fn sanitize_result_name(name: &str) -> String {
    sanitize_metadata(name)
}
