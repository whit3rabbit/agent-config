//! `agentconfig check [name]` — exit 1 when upstream package content
//! differs from what was installed. Designed for CI gates.

use clap::Args as ClapArgs;

use crate::diag::{codes, CliResult, Diag};
use crate::engine;
use crate::lockfile;
use crate::source;

use super::fail;

/// Arguments for `check`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Optional package name; omitted means every package in the lockfile.
    pub name: Option<String>,

    /// Check the globally-installed packages instead of the project's.
    #[arg(short, long)]
    pub global: bool,
}

/// Entry point for `check`.
pub fn run(args: &Args) -> i32 {
    match check(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

fn check(args: &Args) -> CliResult<i32> {
    let lock_path = lockfile::lockfile_path(args.global)?;
    let lock = lockfile::load(&lock_path)?;

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

    if names.is_empty() {
        println!("no packages recorded in {}", lock_path.display());
        return Ok(0);
    }

    let mut stale = 0;
    for name in &names {
        let Some(pkg) = lock.packages.get(name) else {
            continue;
        };
        let resolved = source::replay(pkg)?;
        let instruction_file = pkg.instruction_file.as_ref().map(std::path::PathBuf::from);
        let package =
            engine::load_package_with_name(&resolved.dir, instruction_file.as_deref(), name)?;
        let hash = engine::content_hash(&resolved.dir, &package.files)?;
        if hash == pkg.content_hash && !pkg.pending {
            println!("{name}: up to date");
        } else {
            stale += 1;
            println!(
                "{name}: updates available (installed {}, upstream {})",
                pkg.content_hash.get(..12).unwrap_or(&pkg.content_hash),
                &hash[..hash.len().min(12)]
            );
        }
    }

    Ok(if stale > 0 { 1 } else { 0 })
}
