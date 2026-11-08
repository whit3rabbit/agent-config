//! `agentconfig add <source>` — install a package from a repo or folder.

use clap::Args as ClapArgs;
use serde::Serialize;

use crate::diag::{codes, CliResult, Diag};
use crate::engine::{self, InstallOptions, OutcomeStatus, ScopeChoice, TargetOutcome};
use crate::lockfile::{self, InstallSelection, LockItem, LockPackage, Lockfile};
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
    let package = engine::load_package_with_name(
        &resolved.dir,
        instruction_file.as_deref(),
        &resolved.fallback_name(),
    )?;
    let hash = engine::content_hash(&resolved.dir, &package.files)?;

    let opts = InstallOptions {
        agent_filter: args.agent.clone(),
        only,
    };
    let lock_path = lockfile::lockfile_path(args.global)?;
    let original = lockfile::load(&lock_path)?;
    let previous = original.packages.get(&package.name);
    let (mut targets, target_diags) = engine::resolve_targets(&package, &opts, &scope.to_scope())?;
    if let Some(previous) = previous {
        targets.retain(|target| {
            !previous.excluded_agents.contains(&target.agent) || args.agent.contains(&target.agent)
        });
    }

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
        if args.json {
            print_json(&package, &resolved, &scope, &hash, &planned);
        }
        return Ok(0);
    }

    if !args.json && !args.yes && !confirm("Proceed with install?") {
        println!("aborted; nothing was changed");
        return Ok(1);
    }

    let _guard = lockfile::MutationGuard::acquire(&lock_path)?;
    let mut lock = lockfile::load(&lock_path)?;
    if lock.packages.get(&package.name) != previous {
        return Err(Diag::new(
            codes::IO,
            "package record changed while preparing install, retry",
        ));
    }
    let applied = engine::apply_install(&targets, &scope.to_scope());
    if !args.json {
        for outcome in &applied {
            println!("{}", outcome_line(outcome));
        }
    }

    record_install(
        &mut lock,
        &lock_path,
        &LockEntry {
            package: &package,
            resolved: &resolved,
            scope: &scope,
            selection: Some(InstallSelection {
                agents: opts.agent_filter,
                only: opts
                    .only
                    .iter()
                    .map(|surface| surface.plural().to_string())
                    .collect(),
            }),
            instruction_file: args.instruction_file.as_deref(),
            hash: hash.clone(),
        },
        &applied,
    )?;

    if args.json {
        print_json(&package, &resolved, &scope, &hash, &applied);
    }

    if any_failed(&applied) || all_skipped(&applied) {
        Ok(1)
    } else {
        Ok(0)
    }
}

fn print_json(
    package: &engine::Package,
    resolved: &source::ResolvedSource,
    scope: &ScopeChoice,
    hash: &str,
    outcomes: &[TargetOutcome],
) {
    let out = AddJson {
        package: package.name.clone(),
        source: resolved.source.display.clone(),
        scope: scope.as_str().to_string(),
        commit: resolved.commit.clone(),
        content_hash: hash.to_string(),
        outcomes: json_outcomes(outcomes),
    };
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}

/// Everything `record_install` needs to write one lockfile entry.
pub(crate) struct LockEntry<'a> {
    /// Loaded package (provides the name).
    pub package: &'a engine::Package,
    /// Resolved source (provides provenance).
    pub resolved: &'a source::ResolvedSource,
    /// Scope the install ran under.
    pub scope: &'a ScopeChoice,
    /// Explicit selection for an add, or None when replaying existing selections.
    pub selection: Option<InstallSelection>,
    /// `--instruction-file` used, if any.
    pub instruction_file: Option<&'a str>,
    /// Content hash of the package at install time.
    pub hash: String,
}

/// Shared helper for `add`/`update`: build and save a lockfile entry from a
/// resolved source and freshly applied targets.
pub(crate) fn record_install(
    lock: &mut Lockfile,
    lock_path: &std::path::Path,
    entry: &LockEntry<'_>,
    outcomes: &[TargetOutcome],
) -> CliResult<()> {
    let package = entry.package;
    let resolved = entry.resolved;
    let scope = entry.scope;
    let instruction_file = entry.instruction_file;
    let previous = lock.packages.get(&package.name);
    let successes: Vec<_> = outcomes
        .iter()
        .filter(|outcome| matches!(outcome.status, OutcomeStatus::Installed { .. }))
        .map(|outcome| LockItem {
            surface: engine::lock_surface(outcome.surface).to_string(),
            name: outcome.name.clone(),
            agent: outcome.agent.clone(),
        })
        .collect();
    let mut selections = previous
        .map(LockPackage::install_selections)
        .unwrap_or_default();
    let mut replay_needed = false;
    if let Some(selection) = &entry.selection {
        let covers_all = selections.iter().all(|prior| {
            (selection.agents.is_empty()
                || (!prior.agents.is_empty()
                    && prior
                        .agents
                        .iter()
                        .all(|agent| selection.agents.contains(agent))))
                && (selection.only.is_empty()
                    || (!prior.only.is_empty()
                        && prior
                            .only
                            .iter()
                            .all(|surface| selection.only.contains(surface))))
        });
        replay_needed = !covers_all
            && previous.is_some_and(|pkg| pkg.pending || pkg.content_hash != entry.hash);
        if !selections.contains(selection) {
            selections.push(selection.clone());
        }
    }
    if successes.is_empty() && !outcomes.is_empty() {
        if let Some(previous) = lock.packages.get_mut(&package.name) {
            previous.pending = true;
            if previous.url == resolved.source.url
                && previous.git_ref == resolved.source.git_ref
                && previous.subpath == resolved.source.subpath
                && (resolved.source.kind == source::SourceKind::Git
                    || previous.source == resolved.source.display)
            {
                previous.selections = selections;
            }
            lockfile::save(lock_path, lock)?;
        }
        return Ok(());
    }
    let mut items = previous.map(|pkg| pkg.items.clone()).unwrap_or_default();
    let pending = replay_needed || successes.len() < outcomes.len();
    let mut excluded_agents = previous
        .map(|pkg| pkg.excluded_agents.clone())
        .unwrap_or_default();
    excluded_agents.retain(|agent| !successes.iter().any(|item| &item.agent == agent));
    items.extend(successes);
    let first = selections
        .first()
        .expect("an install records its selection");
    items.sort();
    items.dedup();
    lock.packages.insert(
        package.name.clone(),
        LockPackage {
            source: if matches!(scope, ScopeChoice::Global)
                && resolved.source.kind == source::SourceKind::Local
            {
                resolved.dir.to_string_lossy().into_owned()
            } else {
                resolved.source.display.clone()
            },
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
            content_hash: entry.hash.clone(),
            installed_at: engine::timestamp_now(),
            scope: scope.as_str().to_string(),
            pending,
            agent_filter: first.agents.clone(),
            only: first.only.clone(),
            excluded_agents,
            selections,
            items,
        },
    );
    lockfile::save(lock_path, lock)
}
