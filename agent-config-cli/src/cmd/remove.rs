//! `agentconfig remove <name>` — uninstall a package recorded in the
//! lockfile.

use clap::Args as ClapArgs;

use crate::diag::{codes, CliResult, Diag};
use crate::engine;
use crate::lockfile;

use super::{any_failed, confirm, fail, outcome_line, scope_choice};

/// Arguments for `remove`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Package name (or source string) as recorded in the lockfile.
    pub name: String,

    /// Remove the globally-installed copy instead of the project copy.
    #[arg(short, long)]
    pub global: bool,

    /// Only remove items on these agent ids (comma-separated or repeated).
    #[arg(short, long, value_delimiter = ',')]
    pub agent: Vec<String>,

    /// Show the removal plan without touching anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip the confirmation prompt.
    #[arg(short, long)]
    pub yes: bool,
}

/// Entry point for `remove`.
pub fn run(args: &Args) -> i32 {
    match remove(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

fn remove(args: &Args) -> CliResult<i32> {
    let scope = scope_choice(args.global);
    let scope_kind = scope.to_scope();
    let lock_path = lockfile::lockfile_path(args.global)?;
    let mut lock = lockfile::load(&lock_path)?;

    let found = lock
        .packages
        .iter()
        .find(|(_, p)| p.source == args.name)
        .map(|(k, v)| (k.clone(), v.clone()))
        .or_else(|| {
            lock.packages
                .get(&args.name)
                .map(|v| (args.name.clone(), v.clone()))
        });
    let Some((package_name, entry)) = found else {
        return Err(Diag::with_fix(
            codes::NOT_IN_LOCKFILE,
            format!("{:?} is not recorded in {}", args.name, lock_path.display()),
            "run `agentconfig list` to see installed packages",
        ));
    };

    let mut items = entry.items.clone();
    if !args.agent.is_empty() {
        items.retain(|item| args.agent.iter().any(|a| a == &item.agent));
    }
    if items.is_empty() {
        return Err(Diag::with_fix(
            codes::NOT_IN_LOCKFILE,
            format!("no lockfile items for {package_name:?} match the filters"),
            "run `agentconfig list` to see what is recorded",
        ));
    }

    println!("removing {package_name} ({})", entry.source);
    let planned = engine::plan_uninstall(&items, &package_name, &scope_kind);
    for outcome in &planned {
        println!("{}", outcome_line(outcome));
    }
    if args.dry_run {
        return Ok(0);
    }
    if !args.yes && !confirm("Proceed with removal?") {
        println!("aborted; nothing was changed");
        return Ok(1);
    }

    let applied = engine::apply_uninstall(&items, &package_name, &scope_kind);
    for outcome in &applied {
        println!("{}", outcome_line(outcome));
    }

    if let Some(pkg) = lock.packages.get_mut(&package_name) {
        if args.agent.is_empty() {
            lock.packages.remove(&package_name);
        } else {
            pkg.items
                .retain(|item| !args.agent.iter().any(|a| a == &item.agent));
            if pkg.items.is_empty() {
                lock.packages.remove(&package_name);
            }
        }
    }
    lockfile::save(&lock_path, &lock)?;

    if any_failed(&applied) {
        Ok(1)
    } else {
        Ok(0)
    }
}
