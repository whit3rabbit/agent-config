//! `agentconfig add <source>` — install a package from a repo or folder.

use clap::Args as ClapArgs;
use serde::Serialize;

use crate::diag::{codes, CliResult, Diag};
use crate::engine::{self, InstallOptions, ScopeChoice};
use crate::lockfile::{self, LockItem, LockPackage, Lockfile};
use crate::mapping::SurfaceKind;
use crate::source;

use super::{
    all_skipped, any_failed, confirm, fail, json_outcomes, outcome_line, print_diags, scope_choice,
};

/// Arguments for `add`.
#[derive(ClapArgs, Debug)]
pub struct Args {
    /// Source: owner/repo, owner/repo@ref, a git URL, or a local directory.
    pub source: String,

    /// Install into user-global harness config dirs instead of the project.
    #[arg(short, long)]
    pub global: bool,

    /// Restrict targets to these agent ids (comma-separated or repeated).
    #[arg(short, long, value_delimiter = ',')]
    pub agent: Vec<String>,

    /// Skip the confirmation prompt.
    #[arg(short, long)]
    pub yes: bool,

    /// Install only these surfaces (hooks, instructions, mcp, skills).
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Show the plan without touching anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit machine-readable JSON.
    #[arg(long, requires = "yes")]
    pub json: bool,

    /// Pin the source to a git ref (overrides owner/repo@ref).
    #[arg(long = "ref")]
    pub ref_name: Option<String>,

    /// Manifest-less mode: wrap one markdown file as an instruction.
    #[arg(long)]
    pub instruction_file: Option<String>,
}

/// Entry point for `add`.
pub fn run(args: &Args) -> i32 {
    match add(args) {
        Ok(code) => code,
        Err(diag) => fail(diag),
    }
}

/// JSON output shape for `add --json`.
#[derive(Serialize)]
struct AddJson {
    package: String,
    source: String,
    scope: String,
    commit: Option<String>,
    content_hash: String,
    outcomes: Vec<super::JsonOutcome>,
}

fn parse_only(only: &[String]) -> CliResult<Vec<SurfaceKind>> {
    only.iter().map(|s| SurfaceKind::parse(s)).collect()
}

fn add(args: &Args) -> CliResult<i32> {
    let only = parse_only(&args.only)?;
    let scope = scope_choice(args.global);
    let resolved = source::resolve(&args.source, args.ref_name.as_deref())?;
    let instruction_file = args.instruction_file.as_ref().map(std::path::PathBuf::from);
    let package = engine::load_package(&resolved.dir, instruction_file.as_deref())?;

    let opts = InstallOptions {
        agent_filter: args.agent.clone(),
        only,
    };
    let (targets, target_diags) = engine::resolve_targets(&package, &opts, &scope.to_scope())?;

    if !args.json {
        println!(
            "package {} from {} ({})",
            package.name,
            resolved.source.display,
            scope.as_str()
        );
    }
    print_diags(&package.warnings);
    print_diags(&target_diags);

    if targets.is_empty() {
        return Err(Diag::with_fix(
            codes::NO_TARGETS,
            "no applicable targets after filtering",
            "check --agent, entry `agents`/`skip` lists, and the surfaces this package declares",
        ));
    }

    let planned = engine::plan_install(&targets, &scope.to_scope());
    if !args.json {
        for outcome in &planned {
            println!("{}", outcome_line(outcome));
        }
    }

    if args.dry_run {
        return Ok(0);
    }

    if !args.json && !args.yes && !confirm("Proceed with install?") {
        println!("aborted; nothing was changed");
        return Ok(1);
    }

    let applied = engine::apply_install(&targets, &scope.to_scope());
    if !args.json {
        for outcome in &applied {
            println!("{}", outcome_line(outcome));
        }
    }

    let hash = engine::content_hash(&resolved.dir, &package.files)?;
    let mut lock = lockfile::load(&lockfile::lockfile_path(args.global)?)?;
    record_install(
        &mut lock,
        args.global,
        &LockEntry {
            package: &package,
            resolved: &resolved,
            scope: &scope,
            opts: &opts,
            instruction_file: args.instruction_file.as_deref(),
            hash: hash.clone(),
        },
        &targets,
    )?;

    if args.json {
        let out = AddJson {
            package: package.name.clone(),
            source: resolved.source.display.clone(),
            scope: scope.as_str().to_string(),
            commit: resolved.commit.clone(),
            content_hash: hash,
            outcomes: json_outcomes(&applied),
        };
        println!("{}", serde_json::to_string_pretty(&out).expect("json"));
    }

    if any_failed(&applied) || all_skipped(&applied) {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// Everything `record_install` needs to write one lockfile entry.
pub(crate) struct LockEntry<'a> {
    /// Loaded package (provides the name).
    pub package: &'a engine::Package,
    /// Resolved source (provides provenance).
    pub resolved: &'a source::ResolvedSource,
    /// Scope the install ran under.
    pub scope: &'a ScopeChoice,
    /// Install options (agent filter, only-surfaces).
    pub opts: &'a InstallOptions,
    /// `--instruction-file` used, if any.
    pub instruction_file: Option<&'a str>,
    /// Content hash of the package at install time.
    pub hash: String,
}

/// Shared helper for `add`/`update`: build and save a lockfile entry from a
/// resolved source and freshly applied targets.
pub(crate) fn record_install(
    lock: &mut Lockfile,
    global: bool,
    entry: &LockEntry<'_>,
    targets: &[engine::Target],
) -> CliResult<()> {
    let package = entry.package;
    let resolved = entry.resolved;
    let scope = entry.scope;
    let opts = entry.opts;
    let instruction_file = entry.instruction_file;
    let hash = entry.hash.clone();
    let mut items: Vec<LockItem> = targets
        .iter()
        .map(|t| LockItem {
            surface: engine::lock_surface(t.surface).to_string(),
            name: t.name.clone(),
            agent: t.agent.clone(),
        })
        .collect();
    items.sort();
    items.dedup();
    lock.packages.insert(
        package.name.clone(),
        LockPackage {
            source: resolved.source.display.clone(),
            source_type: match resolved.source.kind {
                source::SourceKind::Git => "git",
                source::SourceKind::Local => "local",
            }
            .to_string(),
            url: resolved.source.url.clone(),
            git_ref: resolved.source.git_ref.clone(),
            commit: resolved.commit.clone(),
            subpath: resolved.source.subpath.clone(),
            instruction_file: instruction_file.map(str::to_string),
            content_hash: hash,
            installed_at: engine::timestamp_now(),
            scope: scope.as_str().to_string(),
            agent_filter: opts.agent_filter.clone(),
            only: opts.only.iter().map(|s| s.plural().to_string()).collect(),
            items,
        },
    );
    lockfile::save(&lockfile::lockfile_path(global)?, lock)
}
