//! Install orchestration: load a package from a source directory, resolve
//! targets, and drive the `agent-config` library's plan / install / uninstall
//! / status methods.
//!
//! The engine never writes harness files itself — every mutation goes
//! through the library, which owns ledgers, backups, atomic writes, and
//! scope containment. The only files this crate touches are its own
//! lockfiles (see [`crate::lockfile`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_config::{
    by_id, instruction_by_id, instruction_placements, mcp_by_id, skill_by_id, InstallStatus,
    InstructionSpec, McpSpec, Scope, SkillSpec,
};
use sha2::{Digest, Sha256};

use crate::diag::{codes, CliResult, Diag};
use crate::lockfile::LockItem;
use crate::manifest::{self, Manifest};
use crate::mapping::{self, Entry, ItemSpec, SurfaceKind};

/// Install scope chosen on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeChoice {
    /// User-global harness config dirs.
    Global,
    /// The current project directory.
    Local(PathBuf),
}

impl ScopeChoice {
    /// Convert to the library's [`Scope`].
    pub fn to_scope(&self) -> Scope {
        match self {
            ScopeChoice::Global => Scope::Global,
            ScopeChoice::Local(root) => Scope::Local(root.clone()),
        }
    }

    /// Stable name used in the lockfile.
    pub fn as_str(&self) -> &'static str {
        match self {
            ScopeChoice::Global => "global",
            ScopeChoice::Local(_) => "local",
        }
    }
}

/// A package loaded from a source directory, ready for targeting.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name (ledger owner tag).
    pub name: String,
    /// Installable entries.
    pub entries: Vec<Entry>,
    /// Files the package content is derived from, relative to the source
    /// directory; the basis of the content hash.
    pub files: Vec<PathBuf>,
    /// Non-fatal diagnostics from loading (e.g. manifest-less mode notes).
    pub warnings: Vec<Diag>,
}

/// Targeting options for an install.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// `--agent` filter; empty means "every capable agent".
    pub agent_filter: Vec<String>,
    /// `--only` surfaces; empty means "every surface in the manifest".
    pub only: Vec<SurfaceKind>,
}

/// One resolved (surface, name, agent) install target with its final spec.
#[derive(Debug, Clone)]
pub struct Target {
    /// Surface.
    pub surface: SurfaceKind,
    /// Hook tag / instruction name / MCP name / skill name.
    pub name: String,
    /// Agent id.
    pub agent: String,
    /// Fully built spec.
    pub spec: TargetSpec,
}

/// A fully resolved spec for one target.
#[derive(Debug, Clone)]
pub enum TargetSpec {
    /// Hook.
    Hook(Box<agent_config::HookSpec>),
    /// Instruction (placement resolved).
    Instruction(Box<InstructionSpec>),
    /// MCP server.
    Mcp(Box<McpSpec>),
    /// Skill.
    Skill(Box<SkillSpec>),
}

/// Outcome of planning or applying one target.
#[derive(Debug, Clone)]
pub struct TargetOutcome {
    /// Surface.
    pub surface: SurfaceKind,
    /// Item name.
    pub name: String,
    /// Agent id.
    pub agent: String,
    /// What happened / would happen.
    pub status: OutcomeStatus,
}

/// Status of one target after planning or applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutcomeStatus {
    /// Dry-run only: planned changes (rendered) plus refusals (rendered).
    WouldChange {
        /// Rendered planned-change lines.
        changes: Vec<String>,
        /// Rendered refusal lines.
        refusals: Vec<String>,
    },
    /// Nothing to do; already in the desired state.
    NoOp,
    /// Not attempted: refusal, unsupported scope/transport, or a plan error.
    Skipped(String),
    /// Install succeeded.
    Installed {
        /// Files created.
        created: usize,
        /// Files patched.
        patched: usize,
        /// First-touch backups made.
        backed_up: usize,
        /// True when the content was already installed byte-identically.
        already_installed: bool,
    },
    /// Uninstall succeeded.
    Removed {
        /// Files/dirs removed.
        removed: usize,
        /// Files patched (e.g. host memory file).
        patched: usize,
    },
    /// The library call failed.
    Failed(String),
}

impl OutcomeStatus {
    /// Stable machine name for `--json` output.
    pub fn kind(&self) -> &'static str {
        match self {
            OutcomeStatus::WouldChange { .. } => "would_change",
            OutcomeStatus::NoOp => "noop",
            OutcomeStatus::Skipped(_) => "skipped",
            OutcomeStatus::Installed { .. } => "installed",
            OutcomeStatus::Removed { .. } => "removed",
            OutcomeStatus::Failed(_) => "failed",
        }
    }
}

/// Load a package from a resolved source directory.
///
/// Precedence: `--instruction-file` mode, then `agent.config.toml`, then the
/// Agent Plugins 1.0.0 layout (`plugin.json` / `mcp.json` / `skills/`).
pub fn load_package(dir: &Path, instruction_file: Option<&Path>) -> CliResult<Package> {
    if let Some(file) = instruction_file {
        return load_instruction_only(dir, file);
    }
    if let Some(path) = manifest::find_manifest(dir) {
        let (manifest, parse_diags) = manifest::load(&path)?;
        let (entries, build_diags) = mapping::build_entries(&manifest, dir);
        let mut warnings = parse_diags;
        warnings.extend(build_diags);
        let mut files = vec![PathBuf::from(manifest::MANIFEST_FILE)];
        files.extend(referenced_files(&manifest, dir)?);
        return Ok(Package {
            name: manifest.package.name,
            entries,
            files,
            warnings,
        });
    }
    if let Some(manifest) = load_foreign_layout(dir)? {
        let (entries, warnings) = mapping::build_entries(&manifest, dir);
        let mut files = Vec::new();
        if dir.join("plugin.json").is_file() {
            files.push(PathBuf::from("plugin.json"));
        }
        if dir.join("mcp.json").is_file() {
            files.push(PathBuf::from("mcp.json"));
        }
        files.extend(referenced_files(&manifest, dir)?);
        return Ok(Package {
            name: manifest.package.name,
            entries,
            files,
            warnings,
        });
    }
    Err(Diag::with_fix(
        codes::NO_MANIFEST,
        format!(
            "no {} and no recognized package layout in {}",
            manifest::MANIFEST_FILE,
            dir.display()
        ),
        "add an agent.config.toml (run `agentconfig init` for a scaffold) or use --instruction-file <path>",
    ))
}

fn load_instruction_only(dir: &Path, file: &Path) -> CliResult<Package> {
    let abs = if file.is_absolute() {
        file.to_path_buf()
    } else {
        dir.join(file)
    };
    let body = std::fs::read_to_string(&abs).map_err(|e| {
        Diag::new(
            codes::IO,
            format!("cannot read --instruction-file {}: {e}", abs.display()),
        )
    })?;
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "package".to_string());
    let name = sanitize_kebab(&name);
    let rel = abs
        .strip_prefix(dir)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| abs.clone());
    let manifest = Manifest {
        package: manifest::PackageInfo {
            name: name.clone(),
            version: None,
            description: None,
        },
        hooks: vec![],
        instructions: vec![manifest::InstructionEntry {
            name: name.clone(),
            body: Some(body),
            file: None,
            placement: None,
            agents: None,
            skip: vec![],
        }],
        mcp: vec![],
        skills: vec![],
    };
    let (entries, diags) = mapping::build_entries(&manifest, dir);
    Ok(Package {
        name,
        entries,
        files: vec![rel],
        warnings: diags,
    })
}

/// Detect an Agent Plugins 1.0.0 layout and synthesize a manifest from it.
/// Returns `Ok(None)` when no recognizable layout exists.
fn load_foreign_layout(dir: &Path) -> CliResult<Option<Manifest>> {
    let plugin_name = plugin_json_name(dir);
    let mcp_path = dir.join("mcp.json");
    let has_mcp = mcp_path.is_file();
    let has_skills = dir.join("skills").is_dir() || dir.join("SKILL.md").is_file();
    if plugin_name.is_none() && !has_mcp && !has_skills {
        return Ok(None);
    }
    let name = plugin_name
        .or_else(|| {
            dir.file_name()
                .map(|n| sanitize_kebab(&n.to_string_lossy()))
        })
        .ok_or_else(|| {
            Diag::new(
                codes::NO_MANIFEST,
                "cannot derive a package name from the source directory",
            )
        })?;

    let mut mcp = Vec::new();
    if has_mcp {
        let text = std::fs::read_to_string(&mcp_path)
            .map_err(|e| Diag::new(codes::IO, format!("cannot read mcp.json: {e}")))?;
        mcp = parse_foreign_mcp_json(&text)?;
    }

    let mut skills = Vec::new();
    let skills_dir = dir.join("skills");
    if skills_dir.is_dir() {
        let mut kids: Vec<PathBuf> = std::fs::read_dir(&skills_dir)
            .map_err(|e| {
                Diag::new(
                    codes::IO,
                    format!("cannot read {}: {e}", skills_dir.display()),
                )
            })?
            .filter_map(|e| e.map(|e| e.path()).ok())
            .filter(|p| p.is_dir())
            .collect();
        kids.sort();
        for kid in kids {
            if kid.join("SKILL.md").is_file() {
                let rel = kid
                    .strip_prefix(dir)
                    .map(Path::to_path_buf)
                    .unwrap_or(kid.clone());
                skills.push(manifest::SkillEntry {
                    path: Some(rel.to_string_lossy().to_string()),
                    name: None,
                    description: None,
                    body: None,
                    agents: None,
                    skip: vec![],
                });
            }
        }
    } else if dir.join("SKILL.md").is_file() {
        skills.push(manifest::SkillEntry {
            path: Some(".".to_string()),
            name: None,
            description: None,
            body: None,
            agents: None,
            skip: vec![],
        });
    }

    Ok(Some(Manifest {
        package: manifest::PackageInfo {
            name,
            version: None,
            description: None,
        },
        hooks: vec![],
        instructions: vec![],
        mcp,
        skills,
    }))
}

fn plugin_json_name(dir: &Path) -> Option<String> {
    for candidate in ["plugin.json", ".claude-plugin/plugin.json"] {
        let path = dir.join(candidate);
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(name) = value.get("name").and_then(|n| n.as_str()) {
            return Some(sanitize_kebab(name));
        }
    }
    None
}

/// Parse a standard `{ "mcpServers": { name: cfg } }` document into manifest
/// entries. Accepts the Claude/Agent Plugins shapes: `command` (string) +
/// `args` + `env`, or `url` + optional `type` (`http`, `sse`,
/// `streamable-http`) + `headers`.
fn parse_foreign_mcp_json(text: &str) -> CliResult<Vec<manifest::McpEntry>> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
        Diag::with_fix(
            codes::INVALID_ENTRY,
            format!("mcp.json is not valid JSON: {e}"),
            "fix mcp.json in the source package",
        )
    })?;
    let servers = value
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .ok_or_else(|| {
            Diag::with_fix(
                codes::INVALID_ENTRY,
                "mcp.json has no \"mcpServers\" object",
                "the recognized shape is { \"mcpServers\": { ... } }",
            )
        })?;
    let mut out = Vec::new();
    for (name, cfg) in servers {
        let kind = cfg.get("type").and_then(|t| t.as_str()).map(|t| match t {
            "streamable-http" => "http".to_string(),
            other => other.to_string(),
        });
        let command: Option<Vec<String>> = match cfg.get("command") {
            Some(serde_json::Value::String(c)) => {
                let args: Vec<String> = cfg
                    .get("args")
                    .and_then(|a| a.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(std::iter::once(c.clone()).chain(args).collect())
            }
            Some(serde_json::Value::Array(a)) => Some(
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        };
        let env = cfg.get("env").and_then(|e| e.as_object()).map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect::<std::collections::BTreeMap<_, _>>()
        });
        let headers = cfg.get("headers").and_then(|h| h.as_object()).map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect::<std::collections::BTreeMap<_, _>>()
        });
        out.push(manifest::McpEntry {
            name: sanitize_kebab(name),
            kind,
            command,
            url: cfg.get("url").and_then(|u| u.as_str()).map(str::to_string),
            env,
            headers,
            friendly_name: None,
            agents: None,
            skip: vec![],
        });
    }
    Ok(out)
}

/// Every file referenced by manifest entries (rules files, instruction
/// files, skill directories), relative to `dir`. Inline bodies live in the
/// manifest itself and need no extra entries.
fn referenced_files(manifest: &Manifest, dir: &Path) -> CliResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for hook in &manifest.hooks {
        if let Some(rel) = &hook.rules_file {
            files.push(PathBuf::from(rel));
        }
    }
    for instruction in &manifest.instructions {
        if let Some(rel) = &instruction.file {
            files.push(PathBuf::from(rel));
        }
    }
    for skill in &manifest.skills {
        if let Some(rel) = &skill.path {
            let root = dir.join(rel);
            if root.is_dir() {
                let mut skill_files = Vec::new();
                source_collect(&root, &root, &mut skill_files)?;
                for f in skill_files {
                    files.push(PathBuf::from(rel).join(f));
                }
            }
        }
    }
    Ok(files)
}

// Reuse of `source::collect_files` logic (kept local to avoid widening that
// module's API): recursive relative-path listing, skipping symlinks and .git.
fn source_collect(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) -> CliResult<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| Diag::new(codes::IO, format!("cannot read {}: {e}", dir.display())))?
        .filter_map(|e| e.map(|e| e.path()).ok())
        .collect();
    entries.sort();
    for path in entries {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(".git") {
                continue;
            }
            source_collect(root, &path, files)?;
        } else {
            files.push(
                path.strip_prefix(root)
                    .map(Path::to_path_buf)
                    .unwrap_or(path),
            );
        }
    }
    Ok(())
}

/// SHA-256 over every contributed file (sorted, length-prefixed), hex
/// encoded. This is the `check` / `update` fingerprint of a package.
pub fn content_hash(dir: &Path, files: &[PathBuf]) -> CliResult<String> {
    let mut sorted: Vec<&PathBuf> = files.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut hasher = Sha256::new();
    for rel in sorted {
        let abs = dir.join(rel);
        let bytes = std::fs::read(&abs)
            .map_err(|e| Diag::new(codes::IO, format!("cannot read {}: {e}", abs.display())))?;
        hasher.update((rel.to_string_lossy().len() as u64).to_le_bytes());
        hasher.update(rel.to_string_lossy().as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn capable_ids(surface: SurfaceKind) -> Vec<&'static str> {
    match surface {
        SurfaceKind::Hooks => agent_config::all().iter().map(|a| a.id()).collect(),
        SurfaceKind::Instructions => agent_config::instruction_capable()
            .iter()
            .map(|a| a.id())
            .collect(),
        SurfaceKind::Mcp => agent_config::mcp_capable().iter().map(|a| a.id()).collect(),
        SurfaceKind::Skills => agent_config::skill_capable()
            .iter()
            .map(|a| a.id())
            .collect(),
    }
}

fn supports_scope(surface: SurfaceKind, agent: &str, scope: &Scope) -> bool {
    let kinds: &[agent_config::ScopeKind] = match surface {
        SurfaceKind::Hooks => by_id(agent).map(|a| a.supported_scopes()).unwrap_or(&[]),
        SurfaceKind::Instructions => instruction_by_id(agent)
            .map(|a| a.supported_instruction_scopes())
            .unwrap_or(&[]),
        SurfaceKind::Mcp => mcp_by_id(agent)
            .map(|a| a.supported_mcp_scopes())
            .unwrap_or(&[]),
        SurfaceKind::Skills => skill_by_id(agent)
            .map(|a| a.supported_skill_scopes())
            .unwrap_or(&[]),
    };
    kinds.contains(&scope.kind())
}

/// Resolve which agents each entry installs onto.
pub fn resolve_targets(
    package: &Package,
    opts: &InstallOptions,
    scope: &Scope,
) -> CliResult<(Vec<Target>, Vec<Diag>)> {
    if !opts.agent_filter.is_empty() {
        let known: BTreeSet<&str> = agent_config::all().iter().map(|a| a.id()).collect();
        let unknown: Vec<&str> = opts
            .agent_filter
            .iter()
            .map(String::as_str)
            .filter(|id| !known.contains(id))
            .collect();
        if !unknown.is_empty() {
            return Err(Diag::with_fix(
                codes::UNKNOWN_AGENT,
                format!("unknown agent id(s): {}", unknown.join(", ")),
                format!(
                    "valid ids: {}",
                    agent_config::all()
                        .iter()
                        .map(|a| a.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
    }

    let mut diags = Vec::new();
    let mut targets = Vec::new();
    let filter: BTreeSet<&str> = opts.agent_filter.iter().map(String::as_str).collect();

    for entry in &package.entries {
        if !opts.only.is_empty() && !opts.only.contains(&entry.surface) {
            continue;
        }
        let capable = capable_ids(entry.surface);
        let mut candidates: BTreeSet<&str> = capable.iter().copied().collect();

        if let Some(requested) = &entry.agents {
            for id in requested {
                if !capable.contains(&id.as_str()) {
                    let why = if capable_ids(SurfaceKind::Hooks).contains(&id.as_str()) {
                        format!(
                            "{id} does not support the {} surface",
                            entry.surface.as_str()
                        )
                    } else {
                        format!("{id} is not a registered agent id")
                    };
                    diags.push(Diag::new(
                        codes::TARGET_WARNING,
                        format!("{} {:?}: {why}", entry.surface.as_str(), entry.name),
                    ));
                }
            }
            candidates.retain(|id| requested.iter().any(|r| r == id));
        }
        for id in &entry.skip {
            if !agent_config::all().iter().any(|a| a.id() == id.as_str()) {
                diags.push(Diag::new(
                    codes::TARGET_WARNING,
                    format!(
                        "{} {:?}: skip list contains unknown agent id {id:?}",
                        entry.surface.as_str(),
                        entry.name
                    ),
                ));
            }
        }
        candidates.retain(|id| !entry.skip.iter().any(|s| s == id));
        if !filter.is_empty() {
            candidates.retain(|id| filter.contains(id));
        }

        for agent in candidates {
            if !supports_scope(entry.surface, agent, scope) {
                diags.push(Diag::new(
                    codes::TARGET_WARNING,
                    format!(
                        "{} {:?}: skipping {agent} (surface does not support {} scope)",
                        entry.surface.as_str(),
                        entry.name,
                        match scope.kind() {
                            agent_config::ScopeKind::Global => "global",
                            _ => "local",
                        }
                    ),
                ));
                continue;
            }
            let Some(spec) = build_target_spec(entry, agent)? else {
                continue;
            };
            targets.push(Target {
                surface: entry.surface,
                name: entry.name.clone(),
                agent: agent.to_string(),
                spec,
            });
        }
    }
    Ok((targets, diags))
}

fn build_target_spec(entry: &Entry, agent: &str) -> CliResult<Option<TargetSpec>> {
    Ok(Some(match &entry.spec {
        ItemSpec::Hook(spec) => TargetSpec::Hook(spec.clone()),
        ItemSpec::Mcp(spec) => TargetSpec::Mcp(spec.clone()),
        ItemSpec::Skill(spec) => TargetSpec::Skill(spec.clone()),
        ItemSpec::Instruction(template) => {
            let placement = match template.placement {
                mapping::PlacementChoice::Fixed(p) => p,
                mapping::PlacementChoice::Auto => {
                    let Some(p) = instruction_placements(agent).first().copied() else {
                        return Ok(None);
                    };
                    p
                }
            };
            let spec = InstructionSpec::builder(template.name.clone())
                .owner(template.owner.clone())
                .placement(placement)
                .body(template.body.clone())
                .try_build()
                .map_err(|e| {
                    Diag::new(
                        codes::INVALID_ENTRY,
                        format!("invalid instruction {:?}: {e}", template.name),
                    )
                })?;
            TargetSpec::Instruction(Box::new(spec))
        }
    }))
}

/// Dry-run: plan every target without mutating anything.
pub fn plan_install(targets: &[Target], scope: &Scope) -> Vec<TargetOutcome> {
    targets
        .iter()
        .map(|t| TargetOutcome {
            surface: t.surface,
            name: t.name.clone(),
            agent: t.agent.clone(),
            status: plan_one(t, scope),
        })
        .collect()
}

fn plan_one(target: &Target, scope: &Scope) -> OutcomeStatus {
    let plan = match (&target.spec, target.surface) {
        (TargetSpec::Hook(spec), _) => by_id(&target.agent).map(|a| a.plan_install(scope, spec)),
        (TargetSpec::Instruction(spec), _) => {
            instruction_by_id(&target.agent).map(|a| a.plan_install_instruction(scope, spec))
        }
        (TargetSpec::Mcp(spec), _) => {
            mcp_by_id(&target.agent).map(|a| a.plan_install_mcp(scope, spec))
        }
        (TargetSpec::Skill(spec), _) => {
            skill_by_id(&target.agent).map(|a| a.plan_install_skill(scope, spec))
        }
    };
    let plan = match plan {
        Some(Ok(p)) => p,
        Some(Err(e)) => return OutcomeStatus::Skipped(e.to_string()),
        None => return OutcomeStatus::Skipped("agent lookup failed".to_string()),
    };
    match plan.status {
        agent_config::PlanStatus::NoOp => OutcomeStatus::NoOp,
        agent_config::PlanStatus::WillChange => {
            let mut changes = Vec::new();
            let mut refusals = Vec::new();
            for change in &plan.changes {
                match describe_change(change) {
                    ChangeLine::Change(line) => changes.push(line),
                    ChangeLine::Refusal(line) => refusals.push(line),
                    ChangeLine::Silent => {}
                }
            }
            OutcomeStatus::WouldChange { changes, refusals }
        }
        agent_config::PlanStatus::Refused => {
            let refusals: Vec<String> = plan
                .changes
                .iter()
                .filter_map(|c| match c {
                    agent_config::PlannedChange::Refuse { reason, .. } => {
                        Some(describe_refusal(*reason))
                    }
                    _ => None,
                })
                .collect();
            OutcomeStatus::Skipped(if refusals.is_empty() {
                "refused".to_string()
            } else {
                refusals.join("; ")
            })
        }
        _ => OutcomeStatus::Skipped("unrecognized plan status".to_string()),
    }
}

/// Apply installs for every target.
pub fn apply_install(targets: &[Target], scope: &Scope) -> Vec<TargetOutcome> {
    targets
        .iter()
        .map(|t| TargetOutcome {
            surface: t.surface,
            name: t.name.clone(),
            agent: t.agent.clone(),
            status: apply_one(t, scope),
        })
        .collect()
}

fn apply_one(target: &Target, scope: &Scope) -> OutcomeStatus {
    let applied = match &target.spec {
        TargetSpec::Hook(spec) => by_id(&target.agent).map(|a| a.install(scope, spec)),
        TargetSpec::Instruction(spec) => {
            instruction_by_id(&target.agent).map(|a| a.install_instruction(scope, spec))
        }
        TargetSpec::Mcp(spec) => mcp_by_id(&target.agent).map(|a| a.install_mcp(scope, spec)),
        TargetSpec::Skill(spec) => skill_by_id(&target.agent).map(|a| a.install_skill(scope, spec)),
    };
    let applied = match applied {
        Some(Ok(report)) => report,
        Some(Err(e)) => return OutcomeStatus::Failed(e.to_string()),
        None => return OutcomeStatus::Skipped("agent lookup failed".to_string()),
    };
    OutcomeStatus::Installed {
        created: applied.created.len(),
        patched: applied.patched.len(),
        backed_up: applied.backed_up.len(),
        already_installed: applied.already_installed,
    }
}

/// Dry-run an uninstall of lockfile items.
pub fn plan_uninstall(items: &[LockItem], owner: &str, scope: &Scope) -> Vec<TargetOutcome> {
    items
        .iter()
        .map(|item| TargetOutcome {
            surface: surface_of(item),
            name: item.name.clone(),
            agent: item.agent.clone(),
            status: plan_uninstall_one(item, owner, scope),
        })
        .collect()
}

fn plan_uninstall_one(item: &LockItem, owner: &str, scope: &Scope) -> OutcomeStatus {
    let surface = surface_of(item);
    let planned = match surface {
        SurfaceKind::Hooks => by_id(&item.agent).map(|a| a.plan_uninstall(scope, &item.name)),
        SurfaceKind::Instructions => instruction_by_id(&item.agent)
            .map(|a| a.plan_uninstall_instruction(scope, &item.name, owner)),
        SurfaceKind::Mcp => {
            mcp_by_id(&item.agent).map(|a| a.plan_uninstall_mcp(scope, &item.name, owner))
        }
        SurfaceKind::Skills => {
            skill_by_id(&item.agent).map(|a| a.plan_uninstall_skill(scope, &item.name, owner))
        }
    };
    let plan = match planned {
        Some(Ok(p)) => p,
        Some(Err(e)) => return OutcomeStatus::Skipped(e.to_string()),
        None => return OutcomeStatus::Skipped("agent lookup failed".to_string()),
    };
    match plan.status {
        agent_config::PlanStatus::NoOp => OutcomeStatus::NoOp,
        agent_config::PlanStatus::WillChange => {
            let changes = plan
                .changes
                .iter()
                .map(describe_change)
                .filter_map(|line| match line {
                    ChangeLine::Change(c) => Some(c),
                    _ => None,
                })
                .collect();
            OutcomeStatus::WouldChange {
                changes,
                refusals: Vec::new(),
            }
        }
        agent_config::PlanStatus::Refused => OutcomeStatus::Skipped("refused".to_string()),
        _ => OutcomeStatus::Skipped("unrecognized plan status".to_string()),
    }
}

/// Apply an uninstall of lockfile items.
pub fn apply_uninstall(items: &[LockItem], owner: &str, scope: &Scope) -> Vec<TargetOutcome> {
    items
        .iter()
        .map(|item| TargetOutcome {
            surface: surface_of(item),
            name: item.name.clone(),
            agent: item.agent.clone(),
            status: apply_uninstall_one(item, owner, scope),
        })
        .collect()
}

fn apply_uninstall_one(item: &LockItem, owner: &str, scope: &Scope) -> OutcomeStatus {
    let surface = surface_of(item);
    let removed = match surface {
        SurfaceKind::Hooks => by_id(&item.agent).map(|a| a.uninstall(scope, &item.name)),
        SurfaceKind::Instructions => instruction_by_id(&item.agent)
            .map(|a| a.uninstall_instruction(scope, &item.name, owner)),
        SurfaceKind::Mcp => {
            mcp_by_id(&item.agent).map(|a| a.uninstall_mcp(scope, &item.name, owner))
        }
        SurfaceKind::Skills => {
            skill_by_id(&item.agent).map(|a| a.uninstall_skill(scope, &item.name, owner))
        }
    };
    let removed = match removed {
        Some(Ok(report)) => report,
        Some(Err(e)) => return OutcomeStatus::Failed(e.to_string()),
        None => return OutcomeStatus::Skipped("agent lookup failed".to_string()),
    };
    if removed.not_installed {
        OutcomeStatus::NoOp
    } else {
        OutcomeStatus::Removed {
            removed: removed.removed.len(),
            patched: removed.patched.len(),
        }
    }
}

/// Probe the on-disk status of one lockfile item for `list`.
pub fn probe_status(item: &LockItem, owner: &str, scope: &Scope) -> String {
    let surface = surface_of(item);
    let result = match surface {
        SurfaceKind::Hooks => by_id(&item.agent).map(|a| a.status(scope, &item.name)),
        SurfaceKind::Instructions => {
            instruction_by_id(&item.agent).map(|a| a.instruction_status(scope, &item.name, owner))
        }
        SurfaceKind::Mcp => mcp_by_id(&item.agent).map(|a| a.mcp_status(scope, &item.name, owner)),
        SurfaceKind::Skills => {
            skill_by_id(&item.agent).map(|a| a.skill_status(scope, &item.name, owner))
        }
    };
    match result {
        Some(Ok(report)) => describe_status(&report.status),
        Some(Err(e)) => format!("error: {e}"),
        None => "unknown agent".to_string(),
    }
}

fn surface_of(item: &LockItem) -> SurfaceKind {
    match item.surface.as_str() {
        "instruction" => SurfaceKind::Instructions,
        "mcp" => SurfaceKind::Mcp,
        "skill" => SurfaceKind::Skills,
        _ => SurfaceKind::Hooks,
    }
}

/// Lockfile surface name for a surface kind.
pub fn lock_surface(surface: SurfaceKind) -> &'static str {
    surface.as_str()
}

enum ChangeLine {
    Change(String),
    Refusal(String),
    Silent,
}

fn describe_change(change: &agent_config::PlannedChange) -> ChangeLine {
    use agent_config::PlannedChange as P;
    match change {
        P::CreateFile { path } => ChangeLine::Change(format!("create {}", path.display())),
        P::PatchFile { path } => ChangeLine::Change(format!("patch {}", path.display())),
        P::RemoveFile { path } => ChangeLine::Change(format!("remove {}", path.display())),
        P::RestoreBackup { backup, target } => ChangeLine::Change(format!(
            "restore {} -> {}",
            backup.display(),
            target.display()
        )),
        P::CreateBackup { backup, target } => ChangeLine::Change(format!(
            "backup {} -> {}",
            target.display(),
            backup.display()
        )),
        P::CreateDir { path } => ChangeLine::Change(format!("create dir {}", path.display())),
        P::RemoveDir { path } => ChangeLine::Change(format!("remove dir {}", path.display())),
        P::WriteLedger { path, key, .. } => {
            ChangeLine::Change(format!("ledger {key} in {}", path.display()))
        }
        P::RemoveLedgerEntry { path, key } => {
            ChangeLine::Change(format!("ledger remove {key} from {}", path.display()))
        }
        P::SetPermissions { path, .. } => {
            ChangeLine::Change(format!("set permissions on {}", path.display()))
        }
        P::NoOp { .. } => ChangeLine::Silent,
        P::Refuse { reason, .. } => ChangeLine::Refusal(describe_refusal(*reason)),
        _ => ChangeLine::Silent,
    }
}

/// Human-readable refusal text, used in plan output and `--json` details.
pub fn describe_refusal(reason: agent_config::RefusalReason) -> String {
    use agent_config::RefusalReason as R;
    match reason {
        R::OwnerMismatch => "owned by another consumer".to_string(),
        R::UserInstalledEntry => {
            "config entry exists without an agent-config ledger entry".to_string()
        }
        R::InvalidConfig => "existing config is invalid or has an unsupported shape".to_string(),
        R::BackupAlreadyExists => "a .bak backup already exists".to_string(),
        R::UnsupportedScope => "scope not supported by this agent".to_string(),
        R::MissingRequiredSpecField => "spec missing a field this agent requires".to_string(),
        R::InlineSecretInLocalScope => "inline secret refused in local scope".to_string(),
        R::UnsupportedTransport => "transport not supported by this agent".to_string(),
        R::UnsupportedPlatform => "surface unsupported on this platform".to_string(),
        R::UnsupportedSpecField => "spec field not supported by this agent".to_string(),
        other => format!("refused: {other:?}"),
    }
}

/// Human-readable install status text for `list`.
pub fn describe_status(status: &InstallStatus) -> String {
    match status {
        InstallStatus::Absent => "missing".to_string(),
        InstallStatus::InstalledOwned { owner } => format!("installed (owner: {owner})"),
        InstallStatus::InstalledOtherOwner { owner } => {
            format!("installed by another consumer (owner: {owner})")
        }
        InstallStatus::PresentUnowned => "present, unowned".to_string(),
        InstallStatus::LedgerOnly { owner } => {
            format!("ledger-only, config entry missing (owner: {owner})")
        }
        InstallStatus::Drifted { issues } => {
            format!("drifted ({} issue(s))", issues.len())
        }
        InstallStatus::Unknown => "unknown".to_string(),
        other => format!("{other:?}"),
    }
}

/// RFC 3339-ish UTC timestamp for lockfile `installed_at` fields.
pub fn timestamp_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Days-to-civil conversion (Howard Hinnant's algorithm); no chrono dep.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// Lowercase-kebab sanitize for names derived from directories or foreign
/// manifests.
pub fn sanitize_kebab(name: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in name.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            sep = false;
        } else if c.is_ascii_uppercase() {
            out.push(c.to_ascii_lowercase());
            sep = false;
        } else if !sep && !out.is_empty() {
            out.push('-');
            sep = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "package".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_kebab_normalizes() {
        assert_eq!(sanitize_kebab("My Toolkit!"), "my-toolkit");
        assert_eq!(sanitize_kebab("  --weird__name--  "), "weird-name");
        assert_eq!(sanitize_kebab("!!!"), "package");
        assert_eq!(sanitize_kebab("acme-toolkit"), "acme-toolkit");
    }

    #[test]
    fn timestamps_are_rfc3339_shaped() {
        let ts = timestamp_now();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn content_hash_is_stable_and_order_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "alpha").unwrap();
        std::fs::write(dir.path().join("b.md"), "bravo").unwrap();
        let h1 = content_hash(dir.path(), &[PathBuf::from("a.md"), PathBuf::from("b.md")]).unwrap();
        let h2 = content_hash(dir.path(), &[PathBuf::from("b.md"), PathBuf::from("a.md")]).unwrap();
        assert_eq!(h1, h2);
        std::fs::write(dir.path().join("a.md"), "ALPHA").unwrap();
        let h3 = content_hash(dir.path(), &[PathBuf::from("a.md"), PathBuf::from("b.md")]).unwrap();
        assert_ne!(h1, h3);
    }
}
