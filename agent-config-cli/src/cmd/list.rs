//! `agentconfig list` — show installed packages and their on-disk status.

use clap::Args as ClapArgs;
use serde::Serialize;

use crate::engine;
use crate::lockfile;

use super::{fail, scope_choice};

/// Arguments for `list`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// List the globally-installed packages instead of the project's.
    #[arg(short, long)]
    pub global: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,

    /// Optional package name filter.
    pub name: Option<String>,
}

/// Entry point for `list`.
pub fn run(args: &Args) -> i32 {
    match list(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

/// One probed lockfile item.
#[derive(Serialize)]
struct ListItem {
    surface: String,
    name: String,
    agent: String,
    status: String,
}

/// One package in `list` output.
#[derive(Serialize)]
struct ListPackage {
    name: String,
    source: String,
    source_type: String,
    git_ref: Option<String>,
    commit: Option<String>,
    content_hash: String,
    installed_at: String,
    items: Vec<ListItem>,
}

fn list(args: &Args) -> Result<i32, crate::diag::Diag> {
    let scope = scope_choice(args.global);
    let scope_kind = scope.to_scope();
    let lock_path = lockfile::lockfile_path(args.global)?;
    let lock = lockfile::load(&lock_path)?;

    let mut packages: Vec<ListPackage> = Vec::new();
    for (name, pkg) in &lock.packages {
        if let Some(filter) = &args.name {
            if name != filter && pkg.source != *filter {
                continue;
            }
        }
        let items = pkg
            .items
            .iter()
            .map(|item| ListItem {
                surface: item.surface.clone(),
                name: item.name.clone(),
                agent: item.agent.clone(),
                status: engine::probe_status(item, name, &scope_kind),
            })
            .collect();
        packages.push(ListPackage {
            name: name.clone(),
            source: pkg.source.clone(),
            source_type: pkg.source_type.clone(),
            git_ref: pkg.git_ref.clone(),
            commit: pkg.commit.clone(),
            content_hash: pkg.content_hash.clone(),
            installed_at: pkg.installed_at.clone(),
            items,
        });
    }

    if packages.is_empty() {
        println!("no packages recorded in {}", lock_path.display());
        return Ok(0);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&packages).expect("json"));
        return Ok(0);
    }

    for pkg in &packages {
        let pin = match (&pkg.git_ref, &pkg.commit) {
            (Some(r), Some(c)) => format!("@{r} ({})", &c[..c.len().min(12)]),
            (Some(r), None) => format!("@{r}"),
            (None, Some(c)) => format!("({})", &c[..c.len().min(12)]),
            (None, None) => String::new(),
        };
        println!(
            "{} from {} {} [{} scope, installed {}]",
            pkg.name,
            pkg.source,
            pin,
            scope.as_str(),
            pkg.installed_at
        );
        for item in &pkg.items {
            println!(
                "  {} {} -> {}: {}",
                item.surface, item.name, item.agent, item.status
            );
        }
    }
    Ok(0)
}
