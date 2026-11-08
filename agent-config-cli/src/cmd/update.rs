//! `agentconfig update [name]` — re-resolve recorded packages and reinstall
//! when upstream content changed.

use clap::Args as ClapArgs;

use crate::diag::{codes, CliResult, Diag};
use crate::engine::{self, InstallOptions, ScopeChoice};
use crate::lockfile;
use crate::source;

use super::add::record_install;
use super::{all_skipped, any_failed, confirm, fail, outcome_line, print_diags};

/// Arguments for `update`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Optional package name; omitted means every package in the lockfile.
    pub name: Option<String>,

    /// Update the globally-installed packages instead of the project's.
    #[arg(short, long)]
    pub global: bool,

    /// Skip the confirmation prompt.
    #[arg(short, long)]
    pub yes: bool,

    /// Show what would change without touching anything.
    #[arg(long)]
    pub dry_run: bool,
}

/// Entry point for `update`.
pub fn run(args: &Args) -> i32 {
    match update(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

fn update(args: &Args) -> CliResult<i32> {
    let lock_path = lockfile::lockfile_path(args.global)?;
    let mut lock = lockfile::load(&lock_path)?;

    let names: Vec<String> = match &args.name {
        Some(name) => {
            if !lock.packages.contains_key(name) {
                return Err(Diag::with_fix(
                    codes::NOT_IN_LOCKFILE,
                    format!("{name:?} is not recorded in {}", lock_path.display()),
                    "run `agentconfig list` to see installed packages",
                ));
            }
            vec![name.clone()]
        }
        None => lock.packages.keys().cloned().collect(),
    };

    let mut failures = 0;
    for name in &names {
        let Some(pkg) = lock.packages.get(name).cloned() else {
            continue;
        };
        let resolved = source::resolve(&pkg.source, None)?;
        let instruction_file = pkg.instruction_file.as_ref().map(std::path::PathBuf::from);
        let package = engine::load_package(&resolved.dir, instruction_file.as_deref())?;
        let hash = engine::content_hash(&resolved.dir, &package.files)?;

        if hash == pkg.content_hash {
            println!("{name}: up to date");
            continue;
        }
        println!(
            "{name}: upstream changed ({} -> reinstalled from {})",
            &pkg.content_hash[..pkg.content_hash.len().min(12)],
            resolved.source.display
        );

        let replay_scope = match pkg.scope.as_str() {
            "global" => ScopeChoice::Global,
            _ => ScopeChoice::Local(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            ),
        };
        let replay_scope_kind = replay_scope.to_scope();
        let opts = InstallOptions {
            agent_filter: pkg.agent_filter.clone(),
            only: pkg
                .only
                .iter()
                .filter_map(|s| crate::mapping::SurfaceKind::parse(s).ok())
                .collect(),
        };
        let (targets, target_diags) = engine::resolve_targets(&package, &opts, &replay_scope_kind)?;
        print_diags(&target_diags);
        let planned = engine::plan_install(&targets, &replay_scope_kind);
        for outcome in &planned {
            println!("{}", outcome_line(outcome));
        }
        if args.dry_run {
            continue;
        }
        if !args.yes && !confirm("Proceed with update?") {
            println!("aborted {name}; nothing was changed");
            continue;
        }
        let applied = engine::apply_install(&targets, &replay_scope_kind);
        for outcome in &applied {
            println!("{}", outcome_line(outcome));
        }
        record_install(
            &mut lock,
            args.global,
            &super::add::LockEntry {
                package: &package,
                resolved: &resolved,
                scope: &replay_scope,
                opts: &opts,
                instruction_file: pkg.instruction_file.as_deref(),
                hash,
            },
            &targets,
        )?;
        if any_failed(&applied) || all_skipped(&applied) {
            failures += 1;
        }
    }

    if !args.dry_run {
        lockfile::save(&lock_path, &lock)?;
    }

    if failures > 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}
