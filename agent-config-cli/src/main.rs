//! `agentconfig` binary entry point: clap dispatch plus process exit codes.

use clap::Parser;

use agent_config_cli::cmd::{add, check, init, list, remove, update};

/// Command-line interface for the `agentconfig` binary.
#[derive(Parser, Debug)]
#[command(
    name = "agentconfig",
    version,
    about = "Install hooks, prompt rules, MCP servers, skills, and instructions from an agent.config.toml manifest into AI coding harnesses.",
    after_help = "Run `agentconfig <command> --help` for per-command flags."
)]
struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands.
#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Install a package from a repo or local folder.
    Add(add::Args),
    /// Remove an installed package.
    Remove(remove::Args),
    /// List installed packages and their status.
    List(list::Args),
    /// Re-resolve recorded packages and reinstall changed ones.
    Update(update::Args),
    /// Exit 1 when upstream content differs from what was installed.
    Check(check::Args),
    /// Scaffold an agent.config.toml in the current directory.
    Init(init::Args),
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Commands::Add(args) => add::run(&args),
        Commands::Remove(args) => remove::run(&args),
        Commands::List(args) => list::run(&args),
        Commands::Update(args) => update::run(&args),
        Commands::Check(args) => check::run(&args),
        Commands::Init(args) => init::run(&args),
    };
    std::process::exit(code);
}
