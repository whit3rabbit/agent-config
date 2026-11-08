//! `agentconfig remove <name>` — uninstall a package recorded in the
//! lockfile.

use clap::Args as ClapArgs;

use crate::diag::{codes, CliResult, Diag};
use crate::engine::{self, OutcomeStatus};
use crate::lockfile;

use super::{confirm, fail, outcome_line, scope_choice};

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
    let lock = lockfile::load(&lock_path)?;

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
    let shared = engine::shared_removals(&items, &entry.items, &package_name, &scope_kind);
    let mut planned = engine::plan_uninstall(&items, &package_name, &scope_kind);
    for (item, outcome) in items.iter().zip(&mut planned) {
        if shared.contains(item) {
            outcome.status = OutcomeStatus::WouldChange {
                changes: vec!["remove tracking row (shared install retained)".into()],
                refusals: vec![],
            };
        }
    }
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

    let _guard = lockfile::MutationGuard::acquire(&lock_path)?;
    let mut lock = lockfile::load(&lock_path)?;
    if lock.packages.get(&package_name) != Some(&entry) {
        return Err(Diag::new(
            codes::IO,
            "package record changed while preparing removal, retry",
        ));
    }
    let shared = engine::shared_removals(&items, &entry.items, &package_name, &scope_kind);
    let physical: Vec<_> = items
        .into_iter()
        .filter(|item| !shared.contains(item))
        .collect();
    let applied = engine::apply_uninstall(&physical, &package_name, &scope_kind);
    let mut completed = shared;
    for (item, outcome) in physical.into_iter().zip(applied) {
        println!("{}", outcome_line(&outcome));
        if matches!(
            outcome.status,
            OutcomeStatus::NoOp | OutcomeStatus::Removed { .. }
        ) {
            completed.insert(item);
        }
    }
    let failed = planned.len() != completed.len();
    if !completed.is_empty() {
        let pkg = lock
            .packages
            .get_mut(&package_name)
            .expect("record was checked");
        pkg.items.retain(|item| !completed.contains(item));
        for agent in &args.agent {
            if completed.iter().any(|item| &item.agent == agent)
                && !pkg.items.iter().any(|item| &item.agent == agent)
                && !pkg.excluded_agents.contains(agent)
            {
                pkg.excluded_agents.push(agent.clone());
            }
        }
        if pkg.items.is_empty() {
            lock.packages.remove(&package_name);
        }
        lockfile::save(&lock_path, &lock)?;
    }
    Ok(i32::from(failed))
}
