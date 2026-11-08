//! The `skills` CLI — the open agent skills ecosystem. Wizard-compatible with
//! vercel-labs/skills; the on-disk engine lives in `skills-store`.

mod archive;
mod blob;
mod cmd_add;
mod cmd_find;
mod cmd_init;
mod cmd_list;
mod cmd_remove;
mod cmd_sync;
mod cmd_update;
mod cmd_use;
mod color;
mod common;
mod detect_agent;
mod download;
mod find_api;
mod git;
mod http;
mod prompt;
mod resolver;
mod search_multiselect;
mod source_parser;
mod update_source;
mod wellknown;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[38;5;102m";
const TEXT: &str = "\x1b[38;5;145m";

const LOGO_LINES: [&str; 6] = [
    "███████╗██╗  ██╗██╗██╗     ██╗     ███████╗",
    "██╔════╝██║ ██╔╝██║██║     ██║     ██╔════╝",
    "███████╗█████╔╝ ██║██║     ██║     ███████╗",
    "╚════██║██╔═██╗ ██║██║     ██║     ╚════██║",
    "███████║██║  ██╗██║███████╗███████╗███████║",
    "╚══════╝╚═╝  ╚═╝╚═╝╚══════╝╚══════╝╚══════╝",
];

const GRAYS: [&str; 6] = [
    "\x1b[38;5;250m",
    "\x1b[38;5;248m",
    "\x1b[38;5;245m",
    "\x1b[38;5;243m",
    "\x1b[38;5;240m",
    "\x1b[38;5;238m",
];

fn show_logo() {
    println!();
    for (index, line) in LOGO_LINES.iter().enumerate() {
        println!("{}{line}{RESET}", GRAYS[index]);
    }
}

fn show_banner() {
    show_logo();
    println!();
    println!("{DIM}The open agent skills ecosystem{RESET}");
    println!();
    println!(
        "  {DIM}${RESET} {TEXT}skills add {DIM}<package>{RESET}        {DIM}Add a new skill{RESET}"
    );
    println!("  {DIM}${RESET} {TEXT}skills use {DIM}<package>@<skill>{RESET} {DIM}Use a skill without installing{RESET}");
    println!("  {DIM}${RESET} {TEXT}skills remove{RESET}               {DIM}Remove installed skills{RESET}");
    println!("  {DIM}${RESET} {TEXT}skills list{RESET}                 {DIM}List installed skills{RESET}");
    println!("  {DIM}${RESET} {TEXT}skills find {DIM}[query]{RESET}         {DIM}Search for skills{RESET}");
    println!();
    println!("  {DIM}${RESET} {TEXT}skills update{RESET}               {DIM}Update installed skills{RESET}");
    println!();
    println!("  {DIM}${RESET} {TEXT}skills experimental_install{RESET} {DIM}Restore from skills-lock.json{RESET}");
    println!("  {DIM}${RESET} {TEXT}skills init {DIM}[name]{RESET}          {DIM}Create a new skill{RESET}");
    println!("  {DIM}${RESET} {TEXT}skills experimental_sync{RESET}    {DIM}Sync skills from node_modules{RESET}");
    println!();
    println!("{DIM}try:{RESET} skills add vercel-labs/agent-skills");
    println!();
    println!("Discover more skills at {TEXT}https://skills.sh/{RESET}");
    println!();
}

#[allow(clippy::too_many_lines)]
fn show_help() {
    let examples: Vec<(&str, Option<&str>)> = vec![
        ("skills add vercel-labs/agent-skills", None),
        (
            "skills use vercel-labs/agent-skills@vercel-optimize | claude",
            None,
        ),
        (
            "skills use vercel-labs/agent-skills --skill vercel-optimize --agent claude-code",
            None,
        ),
        ("skills add vercel-labs/agent-skills -g", None),
        (
            "skills add vercel-labs/agent-skills --agent claude-code cursor",
            None,
        ),
        (
            "skills add vercel-labs/agent-skills --skill pr-review commit",
            None,
        ),
        (
            "skills add vercel-labs/agent-skills --json -y",
            Some("JSON output"),
        ),
        ("skills remove", Some("interactive remove")),
        ("skills remove web-design", Some("remove by name")),
        ("skills rm --global frontend-design", None),
        ("skills list", Some("list project skills")),
        ("skills ls -g", Some("list global skills")),
        ("skills ls -a claude-code", Some("filter by agent")),
        ("skills ls --json", Some("JSON output")),
        ("skills find", Some("interactive search")),
        ("skills find typescript", Some("search by keyword")),
        (
            "skills find react --owner vercel",
            Some("search within an owner"),
        ),
        ("skills update", None),
        ("skills update my-skill", Some("update a single skill")),
        ("skills update -g", Some("update global skills only")),
        (
            "skills experimental_install",
            Some("restore from skills-lock.json"),
        ),
        ("skills init my-skill", None),
        (
            "skills experimental_sync",
            Some("sync from node_modules (experimental)"),
        ),
        ("skills experimental_sync -y", Some("sync without prompts")),
    ];
    let example_width = examples
        .iter()
        .filter(|(_, description)| description.is_some())
        .map(|(command, _)| command.len())
        .max()
        .unwrap_or(0);
    let formatted_examples = examples
        .iter()
        .map(|(command, description)| match description {
            Some(description) => format!(
                "  {DIM}${RESET} {command}{}{DIM}# {description}{RESET}",
                " ".repeat(example_width - command.len() + 2)
            ),
            None => format!("  {DIM}${RESET} {command}"),
        })
        .collect::<Vec<_>>()
        .join("\n");

    println!(
        r#"
{BOLD}Usage:{RESET} skills <command> [options]

{BOLD}Manage Skills:{RESET}
  add <package>        Add a skill package (alias: a)
                       e.g. vercel-labs/agent-skills
                            notion
                            https://notion.so/<skill-page>
                            https://github.com/vercel-labs/agent-skills
  use <package>@<skill>
                       Generate a prompt for using one skill without installing it
  remove [skills]      Remove installed skills
  list, ls             List installed skills
  find [query]         Search for skills interactively

{BOLD}Find Options:{RESET}
  --owner <owner>        Search only repositories from a GitHub owner

{BOLD}Updates:{RESET}
  update [skills...]   Update skills to latest versions (alias: upgrade)

{BOLD}Update Options:{RESET}
  -g, --global           Update global skills only
  -p, --project          Update project skills only
  -y, --yes              Skip scope prompt (auto-detect: project if in a project, else global)

{BOLD}Project:{RESET}
  experimental_install Restore skills from skills-lock.json (alias: install, i)
  init [name]          Initialize a skill (creates <name>/SKILL.md or ./SKILL.md)
  experimental_sync    Sync skills from node_modules into agent directories (alias: sync)

{BOLD}Experimental Install Options:{RESET}
  -a, --agent <agents>   Link to these agents (default: last selected, else detected)
  --dry-run              Show what would be restored without installing anything
  --frozen-lockfile      Fail instead of changing skills-lock.json (default in CI)
  --no-frozen-lockfile   Allow skills-lock.json to change in CI
  --force                Reinstall skills that are already installed

{BOLD}Add Options:{RESET}
  -g, --global           Install skill globally (user-level) instead of project-level
  -a, --agent <agents>   Specify agents to install to (use '*' for all agents)
  -s, --skill <skills>   Specify skill names to install (use '*' for all skills)
  -l, --list             List available skills in the repository without installing
  -y, --yes              Skip confirmation prompts
  --copy                 Copy files instead of symlinking to agent directories
  --metadata <json>      Attach valid JSON to the install telemetry event
  --subagent <names>     Install to Eve subagents (use 'root' for the root agent)
  --all                  Shorthand for --skill '*' --agent '*' -y
  --full-depth           Search all subdirectories even when a root SKILL.md exists
  --json                 Output results as JSON (machine-readable, no ANSI codes)

{BOLD}Use Options:{RESET}
  -s, --skill <skill>    Specify the skill to use
  -a, --agent <agent>    Start one supported agent interactively
  --full-depth           Search all subdirectories even when a root SKILL.md exists

{BOLD}Remove Options:{RESET}
  -g, --global           Remove from global scope
  -a, --agent <agents>   Remove from specific agents (omit to clean all agent links)
  -s, --skill <skills>   Specify skills to remove (use '*' for all skills)
  -y, --yes              Skip confirmation prompts
  --all                  Remove every installed skill (-y implied). Do not combine with named skills.

{BOLD}Experimental Sync Options:{RESET}
  -a, --agent <agents>   Specify agents to install to (use '*' for all agents)
  -y, --yes              Skip confirmation prompts
  --copy                 Copy skills instead of linking them to node_modules
  --dry-run              Show what would change without changing anything
  --no-cleanup           Keep skills whose package no longer ships them
  --no-remote            Skip git sources listed in package.json skills fields
  --frozen-lockfile      Fail instead of changing skills-lock.json (default in CI)
  --no-frozen-lockfile   Allow skills-lock.json to change in CI
  -r, --recursive        Also sync the dependencies of each workspace package
  --include <patterns>   Only sync matching packages (<pkg>) or skills (<pkg>#<skill>)
  --exclude <patterns>   Skip matching packages (<pkg>) or skills (<pkg>#<skill>)

{BOLD}List Options:{RESET}
  -g, --global           List global skills (default: project)
  -a, --agent <agents>   Filter by specific agents
  --json                 Output as JSON (machine-readable, no ANSI codes)

{BOLD}Options:{RESET}
  --help, -h        Show this help message
  --version, -v     Show version number

{BOLD}Examples:{RESET}
{formatted_examples}

Discover more skills at {TEXT}https://skills.sh/{RESET}
"#
    );
}

fn show_remove_help() {
    println!(
        r#"
{BOLD}Usage:{RESET} skills remove [skills...] [options]

{BOLD}Description:{RESET}
  Remove installed skills from agents. If no skill names are provided,
  an interactive selection menu will be shown.

{BOLD}Arguments:{RESET}
  skills            Optional skill names to remove (space-separated)

{BOLD}Options:{RESET}
  -g, --global       Remove from global scope (~/) instead of project scope
  -a, --agent        Remove from specific agents (omit to clean all agent links)
  -s, --skill        Specify skills to remove (use '*' for all skills)
  -y, --yes          Skip confirmation prompts
  --all              Remove every installed skill (-y implied). Do not combine with named skills.

{BOLD}Examples:{RESET}
  {DIM}${RESET} skills remove                           {DIM}# interactive selection{RESET}
  {DIM}${RESET} skills remove my-skill                   {DIM}# remove specific skill{RESET}
  {DIM}${RESET} skills remove skill1 skill2 -y           {DIM}# remove multiple skills{RESET}
  {DIM}${RESET} skills remove --global my-skill          {DIM}# remove from global scope{RESET}
  {DIM}${RESET} skills rm --agent claude-code my-skill   {DIM}# remove from specific agent{RESET}
  {DIM}${RESET} skills remove --all                      {DIM}# remove all skills{RESET}
  {DIM}${RESET} skills remove --skill '*' -a cursor      {DIM}# remove all skills from cursor{RESET}

Discover more skills at {TEXT}https://skills.sh/{RESET}
"#
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let in_agent = detect_agent::is_running_in_agent();

    if args.is_empty() {
        if !in_agent {
            show_banner();
        }
        std::process::exit(common::exit_code());
    }

    let command = args[0].clone();
    let rest: Vec<String> = args[1..].to_vec();

    // Subcommand --help short-circuits dispatch.
    if command != "--help"
        && command != "-h"
        && command != "--version"
        && command != "-v"
        && rest.iter().any(|arg| arg == "--help" || arg == "-h")
    {
        if command == "remove" || command == "rm" || command == "r" {
            show_remove_help();
        } else {
            show_help();
        }
        std::process::exit(common::exit_code());
    }

    match command.as_str() {
        "find" | "search" | "f" | "s" => {
            if !in_agent {
                show_logo();
            }
            println!();
            cmd_find::run_find(&rest);
        }
        "init" => {
            if !in_agent {
                show_logo();
            }
            println!();
            cmd_init::run_init(&rest);
        }
        "experimental_install" => {
            if !in_agent {
                show_logo();
            }
            cmd_init::run_install_from_lock(&rest);
        }
        "i" | "install" | "a" | "add" => {
            let (sources, options, errors) = cmd_add::parse_add_options(&rest);
            if (command == "i" || command == "install") && sources.is_empty() {
                if !in_agent {
                    show_logo();
                }
                cmd_init::run_install_from_lock(&rest);
            } else {
                if !in_agent && !options.json {
                    show_logo();
                }
                if !errors.is_empty() {
                    for error in errors {
                        eprintln!("Error: {}", error.plain());
                    }
                    if options.json {
                        println!("[]");
                    }
                    std::process::exit(1);
                }
                cmd_add::run_add(sources, options);
            }
        }
        "use" => {
            let (sources, options, errors) = cmd_use::parse_use_options(&rest);
            cmd_use::run_use(sources, options, errors);
        }
        "remove" | "rm" | "r" => {
            let (skills, options) = cmd_remove::parse_remove_options(&rest);
            cmd_remove::remove_command(skills, options);
        }
        "sync" | "experimental_sync" => {
            if !in_agent {
                show_logo();
            }
            let options = cmd_sync::parse_sync_options(&rest);
            cmd_sync::run_sync(&rest, options);
        }
        "list" | "ls" => {
            cmd_list::run_list(&rest);
        }
        "check" => {
            cmd_update::run_update(&rest, "check");
        }
        "update" | "upgrade" => {
            cmd_update::run_update(&rest, "update");
        }
        "--help" | "-h" => show_help(),
        "--version" | "-v" => println!("{VERSION}"),
        other => {
            println!(
                "{}",
                skills_store::error::diag::unknown_command(other).plain()
            );
            std::process::exit(1);
        }
    }

    std::process::exit(common::exit_code());
}
