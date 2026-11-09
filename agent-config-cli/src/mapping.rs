//! Map manifest entries onto `agent-config` library specs.
//!
//! `build_entries` turns a parsed [`Manifest`] into installable [`Entry`]
//! values: it resolves referenced files against the source directory, parses
//! `SKILL.md` frontmatter, and builds the library specs with `try_build()`
//! so every caller-supplied value goes through the same validation the
//! library applies to hand-built specs. Invalid entries become diagnostics
//! and are skipped; the rest of the manifest still applies.

use std::path::{Path, PathBuf};

use agent_config::{
    Event, InstructionPlacement, Matcher, McpSpec, SkillAsset, SkillContext, SkillEffort,
    SkillShell, SkillSpec,
};
use serde::Deserialize;

use crate::diag::{codes, CliResult, Diag};
use crate::manifest::{HookCommandSpec, Manifest, MatcherSpec, McpEntry, SkillEntry};
use crate::source::{package_path, read_capped, read_text};

/// Upper bound for any single file the CLI ingests from a package, matching
/// the library's `read_capped` limit.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Installable surfaces, in the order they are presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurfaceKind {
    /// `[[hooks]]`.
    Hooks,
    /// `[[instructions]]`.
    Instructions,
    /// `[[mcp]]`.
    Mcp,
    /// `[[skills]]`.
    Skills,
}

impl SurfaceKind {
    /// Canonical singular name used in output and `--only` values.
    pub fn as_str(self) -> &'static str {
        match self {
            SurfaceKind::Hooks => "hook",
            SurfaceKind::Instructions => "instruction",
            SurfaceKind::Mcp => "mcp",
            SurfaceKind::Skills => "skill",
        }
    }

    /// Plural name matching the manifest section.
    pub fn plural(self) -> &'static str {
        match self {
            SurfaceKind::Hooks => "hooks",
            SurfaceKind::Instructions => "instructions",
            SurfaceKind::Mcp => "mcp",
            SurfaceKind::Skills => "skills",
        }
    }

    /// Parse an `--only` value (accepts singular and plural spellings).
    pub fn parse(s: &str) -> CliResult<Self> {
        match s {
            "hook" | "hooks" => Ok(SurfaceKind::Hooks),
            "instruction" | "instructions" | "rules" => Ok(SurfaceKind::Instructions),
            "mcp" => Ok(SurfaceKind::Mcp),
            "skill" | "skills" => Ok(SurfaceKind::Skills),
            other => Err(Diag::with_fix(
                codes::UNKNOWN_SURFACE,
                format!("unknown surface {other:?}"),
                "choose from: hooks, instructions, mcp, skills",
            )),
        }
    }
}

/// One installable item resolved from a manifest, before agent targeting.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Which surface this belongs to.
    pub surface: SurfaceKind,
    /// Hook tag / instruction name / MCP name / skill name.
    pub name: String,
    /// The library spec (instructions keep a template until the per-agent
    /// placement is resolved).
    pub spec: ItemSpec,
    /// Entry-level `agents` restriction.
    pub agents: Option<Vec<String>>,
    /// Entry-level exclusions.
    pub skip: Vec<String>,
}

/// A built spec for one entry.
#[derive(Debug, Clone)]
pub enum ItemSpec {
    /// Fully built hook spec.
    Hook(Box<agent_config::HookSpec>),
    /// Instruction template; the engine resolves placement per agent.
    Instruction(InstructionTemplate),
    /// Fully built MCP spec.
    Mcp(Box<McpSpec>),
    /// Fully built skill spec.
    Skill(Box<SkillSpec>),
}

/// Instruction placement choice from the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementChoice {
    /// Pick the first placement the target agent supports.
    Auto,
    /// Force one placement for every target.
    Fixed(InstructionPlacement),
}

/// Instruction fields before per-agent placement resolution.
#[derive(Debug, Clone)]
pub struct InstructionTemplate {
    /// Instruction name.
    pub name: String,
    /// Owner tag (package name).
    pub owner: String,
    /// Markdown body.
    pub body: String,
    /// Placement choice.
    pub placement: PlacementChoice,
}

/// Build installable entries from a manifest. `base` is the directory the
/// manifest lives in; relative file references resolve against it.
pub fn build_entries(manifest: &Manifest, base: &Path) -> (Vec<Entry>, Vec<Diag>) {
    let (entries, diags, _) = build_entries_with_files(manifest, base);
    (entries, diags)
}

pub(crate) fn build_entries_with_files(
    manifest: &Manifest,
    base: &Path,
) -> (Vec<Entry>, Vec<Diag>, Vec<PathBuf>) {
    let owner = manifest.package.name.clone();
    let mut diags = Vec::new();
    let mut entries = Vec::new();
    let mut files = Vec::new();

    for hook in &manifest.hooks {
        match build_hook(hook, base, &mut diags) {
            Ok(spec) => {
                files.extend(hook.rules_file.iter().map(PathBuf::from));
                entries.push(Entry {
                    surface: SurfaceKind::Hooks,
                    name: hook.tag.clone(),
                    spec: ItemSpec::Hook(Box::new(spec)),
                    agents: hook.agents.clone(),
                    skip: hook.skip.clone(),
                });
            }
            Err(diag) => diags.push(entry_diag("hooks", &hook.tag, diag)),
        }
    }

    for instruction in &manifest.instructions {
        match build_instruction(instruction, base, &owner, &mut diags) {
            Ok(template) => {
                files.extend(instruction.file.iter().map(PathBuf::from));
                entries.push(Entry {
                    surface: SurfaceKind::Instructions,
                    name: instruction.name.clone(),
                    spec: ItemSpec::Instruction(template),
                    agents: instruction.agents.clone(),
                    skip: instruction.skip.clone(),
                });
            }
            Err(diag) => diags.push(entry_diag("instructions", &instruction.name, diag)),
        }
    }

    for mcp in &manifest.mcp {
        match build_mcp(mcp, &owner) {
            Ok(spec) => entries.push(Entry {
                surface: SurfaceKind::Mcp,
                name: mcp.name.clone(),
                spec: ItemSpec::Mcp(Box::new(spec)),
                agents: mcp.agents.clone(),
                skip: mcp.skip.clone(),
            }),
            Err(diag) => diags.push(entry_diag("mcp", &mcp.name, diag)),
        }
    }

    for skill in &manifest.skills {
        match build_skill(skill, base, &owner, &mut diags) {
            Ok(spec) => {
                if let Some(path) = &skill.path {
                    let dir = Path::new(path);
                    files.push(dir.join("SKILL.md"));
                    files.extend(
                        spec.assets
                            .iter()
                            .map(|asset| dir.join(&asset.relative_path)),
                    );
                }
                entries.push(Entry {
                    surface: SurfaceKind::Skills,
                    name: spec.name.clone(),
                    spec: ItemSpec::Skill(Box::new(spec)),
                    agents: skill.agents.clone(),
                    skip: skill.skip.clone(),
                });
            }
            Err(diag) => diags.push(entry_diag("skills", skill_name_label(skill), diag)),
        }
    }

    (entries, diags, files)
}

fn entry_diag(section: &str, name: &str, diag: Diag) -> Diag {
    diag.fix(format!(
        "fix or remove the {section} entry {name:?} in agent.config.toml"
    ))
}

fn skill_name_label(skill: &SkillEntry) -> &str {
    skill
        .name
        .as_deref()
        .or(skill.path.as_deref())
        .unwrap_or("<unnamed>")
}

fn build_hook(
    hook: &crate::manifest::HookEntry,
    base: &Path,
    diags: &mut Vec<Diag>,
) -> CliResult<agent_config::HookSpec> {
    let mut builder = agent_config::HookSpec::builder(hook.tag.clone());
    builder = match &hook.command {
        HookCommandSpec::Program(argv) => {
            let (program, args) = argv.split_first().ok_or_else(|| {
                Diag::new(codes::INVALID_ENTRY, "command array must not be empty")
            })?;
            builder.command_program(program.clone(), args.iter().cloned())
        }
        HookCommandSpec::Shell(raw) => {
            diags.push(Diag::with_fix(
                codes::SHELL_UNCHECKED,
                format!(
                    "hook {:?} uses a raw shell command, which is passed through unquoted",
                    hook.tag
                ),
                "prefer command = [\"program\", \"arg\"] so arguments are POSIX-quoted",
            ));
            builder.command_shell_unchecked(raw.clone())
        }
    };
    builder = builder
        .event(match hook.event.as_deref() {
            Some(e) => parse_event(e),
            None => Event::PreToolUse,
        })
        .matcher(match &hook.matcher {
            Some(spec) => parse_matcher(spec)?,
            None => Matcher::All,
        });

    if hook.rules.is_some() && hook.rules_file.is_some() {
        return Err(Diag::new(
            codes::INVALID_ENTRY,
            "rules and rules_file are mutually exclusive",
        ));
    }
    if let Some(inline) = &hook.rules {
        builder = builder.rules(inline.clone());
    }
    if let Some(rel) = &hook.rules_file {
        let text = read_text(&package_path(base, Path::new(rel))?)?;
        builder = builder.rules(text);
    }
    if let Some(timeout) = hook.timeout {
        builder = builder.timeout_seconds(timeout);
    }
    if let Some(msg) = &hook.status_message {
        builder = builder.status_message(msg.clone());
    }

    builder.try_build().map_err(|e| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("invalid hook {:?}: {e}", hook.tag),
        )
    })
}

fn build_instruction(
    instruction: &crate::manifest::InstructionEntry,
    base: &Path,
    owner: &str,
    _diags: &mut Vec<Diag>,
) -> CliResult<InstructionTemplate> {
    let body = match (&instruction.body, &instruction.file) {
        (Some(_), Some(_)) => {
            return Err(Diag::new(
                codes::INVALID_ENTRY,
                "body and file are mutually exclusive",
            ));
        }
        (Some(inline), None) => inline.clone(),
        (None, Some(rel)) => read_text(&package_path(base, Path::new(rel))?)?,
        (None, None) => {
            return Err(Diag::new(
                codes::INVALID_ENTRY,
                "one of body or file is required",
            ));
        }
    };
    let placement = match instruction.placement.as_deref() {
        None | Some("auto") => PlacementChoice::Auto,
        Some("inline") => PlacementChoice::Fixed(InstructionPlacement::InlineBlock),
        Some("referenced") => PlacementChoice::Fixed(InstructionPlacement::ReferencedFile),
        Some("standalone") => PlacementChoice::Fixed(InstructionPlacement::StandaloneFile),
        Some(other) => {
            return Err(Diag::with_fix(
                codes::INVALID_ENTRY,
                format!("unknown placement {other:?}"),
                format!("choose one of: {}", crate::manifest::PLACEMENTS.join(", ")),
            ));
        }
    };
    Ok(InstructionTemplate {
        name: instruction.name.clone(),
        owner: owner.to_string(),
        body,
        placement,
    })
}

fn build_mcp(mcp: &McpEntry, owner: &str) -> CliResult<McpSpec> {
    let kind = mcp.kind.as_deref().unwrap_or("stdio");
    let mut builder = McpSpec::builder(mcp.name.clone()).owner(owner.to_string());
    match kind {
        "stdio" => {
            let argv = mcp.command.as_deref().ok_or_else(|| {
                Diag::new(
                    codes::INVALID_ENTRY,
                    "stdio mcp entries require command = [\"cmd\", \"args...\"]",
                )
            })?;
            let (command, args) = argv.split_first().ok_or_else(|| {
                Diag::new(codes::INVALID_ENTRY, "mcp command array must not be empty")
            })?;
            builder = builder.stdio(command.clone(), args.iter().cloned());
            if let Some(env) = &mcp.env {
                for (k, v) in env {
                    builder = builder.env(k.clone(), v.clone());
                }
            }
            if mcp.headers.is_some() {
                return Err(Diag::new(
                    codes::INVALID_ENTRY,
                    "headers are only valid on http/sse mcp entries",
                ));
            }
            if mcp.url.is_some() {
                return Err(Diag::new(
                    codes::INVALID_ENTRY,
                    "url is only valid on http/sse mcp entries",
                ));
            }
        }
        "http" | "sse" => {
            let url = mcp.url.as_deref().ok_or_else(|| {
                Diag::new(
                    codes::INVALID_ENTRY,
                    format!("{kind} mcp entries require url = \"...\""),
                )
            })?;
            builder = if kind == "http" {
                builder.http(url)
            } else {
                builder.sse(url)
            };
            if let Some(headers) = &mcp.headers {
                for (k, v) in headers {
                    builder = builder.header(k.clone(), v.clone());
                }
            }
            if mcp.env.is_some() {
                return Err(Diag::new(
                    codes::INVALID_ENTRY,
                    "env is only valid on stdio mcp entries",
                ));
            }
            if mcp.command.is_some() {
                return Err(Diag::new(
                    codes::INVALID_ENTRY,
                    "command is only valid on stdio mcp entries",
                ));
            }
        }
        other => {
            return Err(Diag::with_fix(
                codes::INVALID_ENTRY,
                format!("unknown mcp type {other:?}"),
                "choose one of: stdio, http, sse",
            ));
        }
    }
    if let Some(friendly) = &mcp.friendly_name {
        builder = builder.friendly_name(friendly.clone());
    }
    builder.try_build().map_err(|e| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("invalid mcp {:?}: {e}", mcp.name),
        )
    })
}

fn build_skill(
    skill: &SkillEntry,
    base: &Path,
    owner: &str,
    diags: &mut Vec<Diag>,
) -> CliResult<SkillSpec> {
    match (&skill.path, &skill.name) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(Diag::new(
                codes::INVALID_ENTRY,
                "skills need either path = \"dir\" (SKILL.md folder) or inline name/description/body",
            ));
        }
        (Some(_), None) => {}
        (None, Some(_)) => {
            return Err(Diag::new(
                codes::INVALID_ENTRY,
                "inline skills need name, description, and body together",
            ));
        }
    }

    if let Some(rel) = &skill.path {
        read_skill_dir(base, rel, owner, diags)
    } else {
        let name = skill.name.clone().expect("checked above");
        let description = skill
            .description
            .clone()
            .ok_or_else(|| Diag::new(codes::INVALID_ENTRY, "inline skills need a description"))?;
        let body = skill
            .body
            .clone()
            .ok_or_else(|| Diag::new(codes::INVALID_ENTRY, "inline skills need a body"))?;
        SkillSpec::builder(name)
            .owner(owner.to_string())
            .description(description)
            .body(body)
            .try_build()
            .map_err(|e| Diag::new(codes::INVALID_ENTRY, format!("invalid inline skill: {e}")))
    }
}

/// Raw SKILL.md frontmatter as parsed from YAML. Mirrors the fields the
/// library's `SkillFrontmatter` supports.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SkillFrontmatterRaw {
    name: Option<String>,
    description: Option<String>,
    when_to_use: Option<String>,
    #[serde(rename = "argument-hint")]
    argument_hint: Option<String>,
    arguments: Option<Vec<String>>,
    #[serde(rename = "disable-model-invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(rename = "user-invocable")]
    user_invocable: Option<bool>,
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "disallowed-tools")]
    disallowed_tools: Option<Vec<String>>,
    model: Option<String>,
    effort: Option<String>,
    context: Option<String>,
    agent: Option<String>,
    paths: Option<Vec<String>>,
    shell: Option<String>,
}

/// Split a `SKILL.md` into `(frontmatter_yaml, body)`. Returns `None` when no
/// frontmatter fence is present.
fn split_frontmatter(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("---")?;
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    let rest = rest.strip_prefix('\n')?;
    let end = rest.find("\n---")?;
    let yaml = &rest[..end];
    let after = rest[end + 1..].strip_prefix("---")?;
    let after = after.strip_prefix('\r').unwrap_or(after);
    let body = after.strip_prefix('\n').unwrap_or(after);
    Some((yaml.to_string(), body.to_string()))
}

fn read_skill_dir(
    base: &Path,
    rel: &str,
    owner: &str,
    diags: &mut Vec<Diag>,
) -> CliResult<SkillSpec> {
    let dir = package_path(base, Path::new(rel))?;
    let meta = std::fs::symlink_metadata(base.join(rel))
        .map_err(|e| Diag::new(codes::IO, format!("cannot stat {}: {e}", dir.display())))?;
    if meta.is_symlink() || !meta.is_dir() {
        return Err(Diag::new(
            codes::INVALID_ENTRY,
            format!(
                "skill path {} must be a real directory (not a symlink)",
                dir.display()
            ),
        ));
    }

    let skill_md = dir.join("SKILL.md");
    let text = read_text(&package_path(base, &Path::new(rel).join("SKILL.md"))?)?;
    let (yaml, body) = split_frontmatter(&text).ok_or_else(|| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("{} has no YAML frontmatter fence", skill_md.display()),
        )
    })?;
    let fm: SkillFrontmatterRaw = yaml_serde::from_str(&yaml).map_err(|e| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("invalid SKILL.md frontmatter: {e}"),
        )
    })?;

    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = fm
        .name
        .clone()
        .unwrap_or_else(|| {
            if rel == "." {
                owner.to_string()
            } else {
                dir_name
            }
        })
        .trim()
        .to_string();
    let description = fm.description.clone().ok_or_else(|| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("{} frontmatter needs a description", skill_md.display()),
        )
    })?;

    let mut builder = SkillSpec::builder(name)
        .owner(owner.to_string())
        .description(description)
        .body(body);

    if let Some(v) = fm.when_to_use.clone() {
        builder = builder.when_to_use(v);
    }
    if let Some(v) = fm.argument_hint.clone() {
        builder = builder.argument_hint(v);
    }
    if let Some(v) = fm.arguments.clone() {
        builder = builder.arguments(v);
    }
    if let Some(v) = fm.disable_model_invocation {
        builder = builder.disable_model_invocation(v);
    }
    if let Some(v) = fm.user_invocable {
        builder = builder.user_invocable(v);
    }
    if let Some(v) = fm.allowed_tools.clone() {
        builder = builder.allowed_tools(v);
    }
    if let Some(v) = fm.disallowed_tools.clone() {
        builder = builder.disallowed_tools(v);
    }
    if let Some(v) = fm.model.clone() {
        builder = builder.model(v);
    }
    if let Some(v) = fm.effort.clone() {
        builder = builder.effort(parse_effort(&v)?);
    }
    if let Some(v) = fm.context.clone() {
        builder = builder.context(parse_context(&v)?);
    }
    if let Some(v) = fm.agent.clone() {
        builder = builder.agent(v);
    }
    if let Some(v) = fm.paths.clone() {
        builder = builder.paths(v);
    }
    if let Some(v) = fm.shell.clone() {
        builder = builder.shell(parse_shell(&v)?);
    }

    for asset in collect_assets(&dir, &dir, diags)? {
        builder = builder.asset(asset);
    }

    builder.try_build().map_err(|e| {
        Diag::new(
            codes::INVALID_ENTRY,
            format!("invalid skill {:?}: {e}", dir.display()),
        )
    })
}

fn parse_effort(v: &str) -> CliResult<SkillEffort> {
    match v {
        "low" => Ok(SkillEffort::Low),
        "medium" => Ok(SkillEffort::Medium),
        "high" => Ok(SkillEffort::High),
        "xhigh" => Ok(SkillEffort::XHigh),
        "max" => Ok(SkillEffort::Max),
        other => Err(Diag::with_fix(
            codes::INVALID_ENTRY,
            format!("unknown skill effort {other:?}"),
            "choose one of: low, medium, high, xhigh, max",
        )),
    }
}

fn parse_context(v: &str) -> CliResult<SkillContext> {
    match v {
        "fork" => Ok(SkillContext::Fork),
        other => Err(Diag::with_fix(
            codes::INVALID_ENTRY,
            format!("unknown skill context {other:?}"),
            "the only supported value is: fork",
        )),
    }
}

fn parse_shell(v: &str) -> CliResult<SkillShell> {
    match v {
        "bash" => Ok(SkillShell::Bash),
        "powershell" => Ok(SkillShell::PowerShell),
        other => Err(Diag::with_fix(
            codes::INVALID_ENTRY,
            format!("unknown skill shell {other:?}"),
            "choose one of: bash, powershell",
        )),
    }
}

/// Depth-first collection of every regular file under `root`, returned as
/// [`SkillAsset`]s with paths relative to `root`. Symlinks are skipped with a
/// diagnostic.
fn collect_assets(root: &Path, dir: &Path, diags: &mut Vec<Diag>) -> CliResult<Vec<SkillAsset>> {
    let mut assets = Vec::new();
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
            diags.push(Diag::new(
                codes::SYMLINK_SKIPPED,
                format!("skipping symlink in skill dir: {}", path.display()),
            ));
            continue;
        }
        // SKILL.md is written from frontmatter + body, never as an asset.
        if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md")
            && path.parent() == Some(root)
        {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("asset came from the skill directory");
        let canonical = match package_path(root, relative) {
            Ok(canonical) if canonical == path => canonical,
            Ok(_) => {
                diags.push(Diag::new(
                    codes::SYMLINK_SKIPPED,
                    format!("skipping linked skill asset {}", path.display()),
                ));
                continue;
            }
            Err(diag) => {
                diags.push(diag);
                continue;
            }
        };
        if meta.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, ".git" | "node_modules") {
                continue;
            }
            assets.extend(collect_assets(root, &canonical, diags)?);
            continue;
        }
        let bytes = read_capped(&canonical)?;
        let executable = is_executable(&meta);
        assets.push(SkillAsset {
            relative_path: relative.to_path_buf(),
            bytes,
            executable,
        });
    }
    Ok(assets)
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    false
}

fn parse_event(event: &str) -> Event {
    match event {
        "PreToolUse" => Event::PreToolUse,
        "PostToolUse" => Event::PostToolUse,
        "Notification" => Event::Notification,
        "UserPromptSubmit" => Event::UserPromptSubmit,
        "Stop" => Event::Stop,
        "SubagentStop" => Event::SubagentStop,
        "PreCompact" => Event::PreCompact,
        "SessionStart" => Event::SessionStart,
        "SessionEnd" => Event::SessionEnd,
        "PermissionRequest" => Event::PermissionRequest,
        "PermissionDenied" => Event::PermissionDenied,
        "ConfigChange" => Event::ConfigChange,
        "FileChanged" => Event::FileChanged,
        "InstructionsLoaded" => Event::InstructionsLoaded,
        other => Event::Custom(other.to_string()),
    }
}

fn looks_like_regex(value: &str) -> bool {
    value.chars().any(|c| "|*+?()[]{}.^$\\".contains(c))
}

fn parse_matcher(spec: &MatcherSpec) -> CliResult<Matcher> {
    match spec {
        MatcherSpec::List(values) => Ok(Matcher::AnyOf(values.clone())),
        MatcherSpec::Explicit(explicit) => {
            if let Some(exact) = &explicit.exact {
                return Ok(Matcher::Exact(exact.clone()));
            }
            if let Some(regex) = &explicit.regex {
                return Ok(Matcher::Regex(regex.clone()));
            }
            if let Some(any) = &explicit.any {
                return Ok(Matcher::AnyOf(any.clone()));
            }
            Err(Diag::new(
                codes::INVALID_ENTRY,
                "matcher table needs one of: exact, regex, any",
            ))
        }
        MatcherSpec::Plain(value) => {
            if value.is_empty() {
                Err(Diag::new(codes::INVALID_ENTRY, "matcher must not be empty"))
            } else if value == "*" {
                Ok(Matcher::All)
            } else if value.eq_ignore_ascii_case("bash") || value.eq_ignore_ascii_case("shell") {
                Ok(Matcher::Bash)
            } else if looks_like_regex(value) {
                Ok(Matcher::Regex(value.clone()))
            } else {
                Ok(Matcher::Exact(value.clone()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse;

    fn build(text: &str, base: &Path) -> (Vec<Entry>, Vec<Diag>) {
        let (manifest, parse_diags) = parse(text).expect("manifest parses");
        assert!(parse_diags.is_empty(), "parse diags: {parse_diags:?}");
        build_entries(&manifest, base)
    }

    #[test]
    fn builds_hook_with_program_command_and_matcher() {
        let text = r#"
[package]
name = "pkg"

[[hooks]]
tag = "guard"
command = ["bin/guard.sh", "--strict"]
matcher = "Bash"
timeout = 10
rules = "be safe"
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(entries.len(), 1);
        let ItemSpec::Hook(spec) = &entries[0].spec else {
            panic!("expected hook");
        };
        assert_eq!(spec.tag, "guard");
        assert!(matches!(spec.matcher, Matcher::Bash));
        assert!(matches!(spec.event, Event::PreToolUse));
        assert_eq!(spec.options.timeout_seconds, Some(10));
        assert!(spec.rules.is_some());
    }

    #[test]
    fn shell_command_warns_but_builds() {
        let text = r#"
[package]
name = "pkg"

[[hooks]]
tag = "raw"
command = "make lint"
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert_eq!(entries.len(), 1);
        assert!(diags.iter().any(|d| d.code == codes::SHELL_UNCHECKED));
    }

    #[test]
    fn regex_matcher_detected_from_metacharacters() {
        let text = r#"
[package]
name = "pkg"

[[hooks]]
tag = "h"
command = ["run"]
matcher = "Write|Edit"
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert!(diags.is_empty());
        let ItemSpec::Hook(spec) = &entries[0].spec else {
            panic!("expected hook");
        };
        assert!(matches!(spec.matcher, Matcher::Regex(_)));
    }

    #[test]
    fn mcp_stdio_with_env_builds() {
        let text = r#"
[package]
name = "pkg"

[[mcp]]
name = "ctx7"
command = ["npx", "-y", "@upstash/context7-mcp"]
env = { API_KEY = "${CONTEXT7_API_KEY}" }
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert!(diags.is_empty(), "{diags:?}");
        let ItemSpec::Mcp(spec) = &entries[0].spec else {
            panic!("expected mcp");
        };
        assert_eq!(spec.name, "ctx7");
        assert_eq!(spec.owner_tag, "pkg");
        let agent_config::McpTransport::Stdio { command, env, .. } = &spec.transport else {
            panic!("expected stdio");
        };
        assert_eq!(command, "npx");
        assert_eq!(
            env.get("API_KEY").map(String::as_str),
            Some("${CONTEXT7_API_KEY}")
        );
    }

    #[test]
    fn mcp_http_rejects_env() {
        let text = r#"
[package]
name = "pkg"

[[mcp]]
name = "remote"
type = "http"
url = "https://mcp.example.com"
env = { A = "b" }
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert!(entries.is_empty());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].why.contains("env is only valid on stdio"));
    }

    #[test]
    fn instruction_file_body_resolved_against_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("STYLE.md"), "# Style\n\nBe terse.").unwrap();
        let text = r#"
[package]
name = "pkg"

[[instructions]]
name = "code-style"
file = "STYLE.md"
"#;
        let (entries, diags) = build(text, dir.path());
        assert!(diags.is_empty(), "{diags:?}");
        let ItemSpec::Instruction(template) = &entries[0].spec else {
            panic!("expected instruction");
        };
        assert!(template.body.contains("Be terse."));
        assert_eq!(template.placement, PlacementChoice::Auto);
    }

    #[test]
    fn skill_dir_parses_frontmatter_and_assets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let skill_dir = dir.path().join("skills").join("pdf");
        std::fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: pdf\ndescription: Handle PDFs.\neffort: high\nallowed-tools:\n  - Read\n---\n# PDF\n\nDo things.\n",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("scripts").join("run.sh"),
            "#!/bin/sh\ntrue\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                skill_dir.join("scripts").join("run.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let text = r#"
[package]
name = "pkg"

[[skills]]
path = "skills/pdf"
"#;
        let (entries, diags) = build(text, dir.path());
        assert!(diags.is_empty(), "{diags:?}");
        let ItemSpec::Skill(spec) = &entries[0].spec else {
            panic!("expected skill");
        };
        assert_eq!(spec.name, "pdf");
        assert_eq!(spec.frontmatter.effort, Some(SkillEffort::High));
        assert_eq!(
            spec.frontmatter.allowed_tools.as_deref(),
            Some(["Read".to_string()].as_slice())
        );
        assert!(spec.body.contains("Do things."));
        assert_eq!(spec.assets.len(), 1);
        // The exec bit is a Unix-only concept here: `is_executable` is
        // hardcoded `false` on non-Unix platforms, so the expected value
        // follows the platform.
        #[cfg(unix)]
        assert!(spec.assets[0].executable);
        #[cfg(not(unix))]
        assert!(!spec.assets[0].executable);
        assert_eq!(
            spec.assets[0].relative_path,
            PathBuf::from("scripts/run.sh")
        );
    }

    #[test]
    fn instruction_requires_body_or_file() {
        let text = r#"
[package]
name = "pkg"

[[instructions]]
name = "empty"
"#;
        let (entries, diags) = build(text, Path::new("/tmp"));
        assert!(entries.is_empty());
        assert!(diags[0].why.contains("body or file"));
    }
}
