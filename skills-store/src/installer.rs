//! The canonical skills installer: writes one copy into `.agents/skills`,
//! symlinks agent-specific directories to it, and lists/removes installed
//! skills. Ported from `src/installer.ts` of the upstream `skills` CLI.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::error::{diag, SkillsError};
use crate::registry::{self, AgentConfig, DetectContext};
pub use crate::sanitize::sanitize_name;
use crate::skills::{has_skill_md, parse_skill_md};

/// Install scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// The current project (`.agents/skills` under `cwd`).
    #[default]
    Project,
    /// The user's home (`~/.agents/skills`).
    Global,
}

impl Scope {
    /// `true` for [`Scope::Global`].
    #[must_use]
    pub fn is_global(self) -> bool {
        self == Scope::Global
    }
}

/// How a skill is placed into agent directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstallMode {
    /// Copy into the canonical dir, symlink agent dirs to it (default).
    #[default]
    Symlink,
    /// Copy into each agent dir.
    Copy,
    /// Symlink the canonical dir to the skill's source, then link agents to it.
    Link,
}

impl InstallMode {
    /// From the upstream string form (`symlink` / `copy` / `link`).
    #[must_use]
    pub fn from_str_name(name: &str) -> Option<Self> {
        match name {
            "symlink" => Some(InstallMode::Symlink),
            "copy" => Some(InstallMode::Copy),
            "link" => Some(InstallMode::Link),
            _ => None,
        }
    }

    /// The upstream string form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMode::Symlink => "symlink",
            InstallMode::Copy => "copy",
            InstallMode::Link => "link",
        }
    }
}

/// Options for one install of a skill for one agent.
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Project or global scope.
    pub global: Scope,
    /// Symlink vs copy vs link (defaults to [`InstallMode::Symlink`]).
    pub mode: Option<InstallMode>,
    /// Project directory; defaults to the process cwd when absent.
    pub cwd: Option<PathBuf>,
    /// Eve subagent name (`None` = root agent); only meaningful for `eve`.
    pub eve_subagent: Option<String>,
    /// Create a missing agent-specific project root because the user selected
    /// this agent explicitly.
    pub create_missing_agent_root: bool,
}

/// Result of installing one skill for one agent.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    /// Agent registry id.
    pub agent: String,
    /// Whether the write succeeded.
    pub success: bool,
    /// The path the agent reads the skill from.
    pub path: PathBuf,
    /// The shared canonical copy, when one exists.
    pub canonical_path: Option<PathBuf>,
    /// Effective mode.
    pub mode: InstallMode,
    /// The symlink failed and a copy was written instead.
    pub symlink_failed: bool,
    /// Nothing was written for this agent.
    pub skipped: bool,
    /// Why the write was skipped (`missing-agent-project-directory`).
    pub skip_reason: Option<&'static str>,
    /// Error message when `success` is false.
    pub error: Option<String>,
}

/// Files of a skill held in memory (blob / well-known installs).
pub type SkillFiles = Vec<(String, Vec<u8>)>;

// ─── Path helpers ───────────────────────────────────────────────────────────

/// Lexically absolute + normalized form of `path` (mirrors JS
/// `normalize(resolve(p))`: no symlink resolution).
fn lexical_abs(path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_path_safe(base: &Path, target: &Path) -> bool {
    let base = lexical_abs(base);
    let target = lexical_abs(target);
    target.starts_with(&base)
}

fn paths_overlap(a: &Path, b: &Path) -> bool {
    is_path_safe(a, b) || is_path_safe(b, a)
}

/// Remove a file, symlink, or directory tree.
pub(crate) fn remove_path_any(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path);
    match meta {
        #[cfg(windows)]
        Ok(m) if std::os::windows::fs::FileTypeExt::is_symlink_dir(&m.file_type()) => {
            std::fs::remove_dir(path)
        }
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(path),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(err) => Err(err),
    }
}

// ─── Directory helpers ──────────────────────────────────────────────────────

/// The canonical `.agents/skills` directory for a scope.
#[must_use]
pub fn get_canonical_skills_dir(global: bool, cwd: &Path) -> PathBuf {
    let base = if global {
        crate::paths::home_dir()
    } else {
        cwd.to_path_buf()
    };
    base.join(".agents").join("skills")
}

/// Eve subagent skills dir: `agent/subagents/<name>/skills`.
#[must_use]
pub fn get_eve_subagent_skills_dir(subagent: &str, cwd: &Path) -> PathBuf {
    cwd.join("agent")
        .join("subagents")
        .join(sanitize_name(subagent))
        .join("skills")
}

/// Eve subagent directory names under `agent/subagents/`, sorted.
#[must_use]
pub fn eve_subagent_names(cwd: &Path) -> Vec<String> {
    let dir = cwd.join("agent").join("subagents");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// The base directory an agent reads skills from in the given scope.
#[must_use]
pub fn get_agent_base_dir(
    agent: &AgentConfig,
    global: bool,
    cwd: &Path,
    eve_subagent: Option<&str>,
) -> PathBuf {
    let ctx = DetectContext::from_env(cwd);
    if if global {
        agent.reads_canonical_globally()
    } else {
        agent.is_universal()
    } {
        return get_canonical_skills_dir(global, cwd);
    }
    if agent.name == "eve" {
        if let Some(subagent) = eve_subagent {
            return get_eve_subagent_skills_dir(subagent, cwd);
        }
    }
    if global {
        if let Some(global_dir) = agent.global_skills_dir(&ctx) {
            return global_dir;
        }
        // Callers check support first; mirror the upstream fallback.
        return cwd.join(agent.skills_dir);
    }
    cwd.join(agent.skills_dir)
}

/// Resolve a path's parent through symlinks, keeping the final component.
fn resolve_parent_symlinks(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("/"));
    let base = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    match std::fs::canonicalize(dir) {
        Ok(real_dir) => real_dir.join(base),
        Err(_) => path.to_path_buf(),
    }
}

/// Minimal `path.relative` for symlink targets. Unix-only: the Windows
/// branch of `create_symlink` delegates to `symlink_dir` instead.
#[cfg(not(windows))]
fn pathdiff(from: &Path, to: &Path) -> PathBuf {
    let from_abs: Vec<_> = from.components().collect();
    let to_abs: Vec<_> = to.components().collect();
    let mut common = 0;
    while common < from_abs.len() && common < to_abs.len() && from_abs[common] == to_abs[common] {
        common += 1;
    }
    let mut result = PathBuf::new();
    for _ in common..from_abs.len() {
        result.push("..");
    }
    for component in &to_abs[common..] {
        result.push(component.as_os_str());
    }
    if result.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        result
    }
}

/// Create a relative symlink, skipping no-op links and replacing stale ones.
/// Returns `false` when the caller should fall back to copying.
fn create_symlink(target: &Path, link_path: &Path) -> bool {
    let resolved_target = target.to_path_buf();
    let resolved_link = link_path.to_path_buf();

    let real_target =
        std::fs::canonicalize(&resolved_target).unwrap_or_else(|_| resolved_target.clone());
    let real_link = std::fs::canonicalize(&resolved_link).unwrap_or_else(|_| resolved_link.clone());
    if real_target == real_link {
        return true;
    }
    if resolve_parent_symlinks(&resolved_target) == resolve_parent_symlinks(&resolved_link) {
        return true;
    }

    match std::fs::symlink_metadata(link_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                if let Ok(existing) = std::fs::read_link(link_path) {
                    let existing_resolved =
                        link_path.parent().unwrap_or(Path::new("/")).join(existing);
                    if existing_resolved == resolved_target {
                        return true;
                    }
                }
                let _ = remove_path_any(link_path);
            } else {
                let _ = remove_path_any(link_path);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            // Possible ELOOP: try to force-remove the broken link.
            let _ = std::fs::remove_file(link_path);
        }
    }

    let link_dir = link_path.parent().unwrap_or(Path::new("/"));
    if std::fs::create_dir_all(link_dir).is_err() {
        return false;
    }
    #[cfg(windows)]
    {
        // Junction-equivalent: Windows symlink_dir requires developer mode;
        // failure makes the caller fall back to a copy, like upstream.
        std::os::windows::fs::symlink_dir(&resolved_target, link_path).is_ok()
    }
    #[cfg(not(windows))]
    {
        let real_link_dir = resolve_parent_symlinks(link_dir);
        let relative = pathdiff(&real_link_dir, &resolved_target);
        std::os::unix::fs::symlink(relative, link_path).is_ok()
    }
}

fn clean_and_create_directory(path: &Path) -> std::io::Result<()> {
    let _ = remove_path_any(path);
    std::fs::create_dir_all(path)
}

// ─── Copy semantics ─────────────────────────────────────────────────────────

const EXCLUDE_FILES: &[&str] = &["metadata.json"];
const EXCLUDE_DIRS: &[&str] = &[".git", "__pycache__", "__pypackages__"];

fn is_excluded(name: &str, is_directory: bool) -> bool {
    if EXCLUDE_FILES.contains(&name) {
        return true;
    }
    is_directory && EXCLUDE_DIRS.contains(&name)
}

fn is_within_root(root: &Path, target: &Path) -> bool {
    target == root || target.starts_with(root)
}

struct CopyCtx {
    root: PathBuf,
    ancestors: HashSet<PathBuf>,
}

/// Recursively copy a skill directory, dereferencing symlinks whose real
/// target stays inside the skill root and skipping everything else with a
/// warning (upstream `copyDirectory`).
pub fn copy_directory(
    src: &Path,
    dest: &Path,
    warnings: &mut Vec<SkillsError>,
) -> Result<(), SkillsError> {
    let mut ctx = CopyCtx {
        root: src.to_path_buf(),
        ancestors: HashSet::new(),
    };
    copy_directory_inner(src, dest, &mut ctx, warnings)
}

fn copy_directory_inner(
    src: &Path,
    dest: &Path,
    ctx: &mut CopyCtx,
    warnings: &mut Vec<SkillsError>,
) -> Result<(), SkillsError> {
    let real_src = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    if ctx.ancestors.contains(&real_src) {
        warnings.push(diag::symlink_cycle(&src.to_string_lossy()));
        return Ok(());
    }
    std::fs::create_dir_all(dest)
        .map_err(|err| SkillsError::new("SKILLS_IO", format!("{}: {err}", dest.display())))?;
    let entries = std::fs::read_dir(src)
        .map_err(|err| SkillsError::new("SKILLS_IO", format!("{}: {err}", src.display())))?;
    ctx.ancestors.insert(real_src.clone());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let src_path = entry.path();
        let dest_path = dest.join(&name);

        let mut is_directory = file_type.is_dir();
        if file_type.is_symlink() {
            let real_target = match std::fs::canonicalize(&src_path) {
                Ok(target) => target,
                Err(_) => {
                    warnings.push(diag::broken_symlink(&src_path.to_string_lossy()));
                    continue;
                }
            };
            if !is_within_root(&ctx.root, &real_target) {
                warnings.push(diag::symlink_outside_skill(&src_path.to_string_lossy()));
                continue;
            }
            is_directory = real_target.is_dir();
            if is_directory && is_excluded(&name, true) {
                continue;
            }
        }

        if is_excluded(&name, is_directory) {
            continue;
        }

        if is_directory {
            copy_directory_inner(&src_path, &dest_path, ctx, warnings)?;
            continue;
        }

        std::fs::copy(&src_path, &dest_path).map_err(|err| {
            SkillsError::new("SKILLS_IO", format!("{}: {err}", src_path.display()))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&src_path) {
                let _ = std::fs::set_permissions(
                    &dest_path,
                    std::fs::Permissions::from_mode(meta.permissions().mode() & 0o777),
                );
            }
        }
    }
    ctx.ancestors.remove(&real_src);
    Ok(())
}

// ─── Install ────────────────────────────────────────────────────────────────

/// The effective install mode when the caller did not choose one: a single
/// target directory (or Eve) gets a copy, several get symlinks.
#[must_use]
pub fn default_install_mode(target_agents: &[&AgentConfig]) -> InstallMode {
    let all_eve = !target_agents.is_empty() && target_agents.iter().all(|a| a.name == "eve");
    let unique_dirs: HashSet<&str> = target_agents.iter().map(|a| a.skills_dir).collect();
    if unique_dirs.len() <= 1 || all_eve {
        InstallMode::Copy
    } else {
        InstallMode::Symlink
    }
}

fn should_skip_project_agent_symlink(
    agent: &AgentConfig,
    is_global: bool,
    cwd: &Path,
    create_missing_agent_root: bool,
) -> bool {
    if is_global || agent.is_universal() || create_missing_agent_root {
        return false;
    }
    if agent.flags.create_project_skills_dir_by_default {
        return false;
    }
    let agent_root = agent.skills_dir.split('/').next().unwrap_or("");
    !cwd.join(agent_root).exists()
}

/// Install one skill (directory on disk, in-memory files, or snapshot) for
/// one agent. `source_path` is set for disk skills (never install onto the
/// source); `write_files` materializes memory-backed skills.
#[allow(clippy::too_many_lines)]
#[allow(clippy::type_complexity)] // mirrors the upstream multi-return shape
pub fn install_for_agent(
    skill_name: &str,
    agent: &AgentConfig,
    options: &InstallOptions,
    source_path: Option<&Path>,
    write_files: &dyn Fn(&Path, &mut Vec<SkillsError>) -> Result<(), SkillsError>,
    warnings: &mut Vec<SkillsError>,
) -> InstallOutcome {
    let is_global = options.global.is_global();
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let install_mode = options.mode.unwrap_or_default();

    let agent_ctx = DetectContext::from_env(&cwd);
    if is_global && agent.global_skills_dir(&agent_ctx).is_none() {
        return InstallOutcome {
            agent: agent.name.to_string(),
            success: false,
            path: PathBuf::new(),
            canonical_path: None,
            mode: install_mode,
            symlink_failed: false,
            skipped: false,
            skip_reason: None,
            error: Some(format!(
                "{} does not support global skill installation",
                agent.display_name
            )),
        };
    }

    let agent_base = get_agent_base_dir(agent, is_global, &cwd, options.eve_subagent.as_deref());
    let agent_dir = agent_base.join(skill_name);
    let canonical_base = if agent.name == "eve" && install_mode == InstallMode::Symlink {
        agent_base.clone()
    } else {
        get_canonical_skills_dir(is_global, &cwd)
    };
    let canonical_dir = canonical_base.join(skill_name);

    if !is_path_safe(&canonical_base, &canonical_dir) || !is_path_safe(&agent_base, &agent_dir) {
        return InstallOutcome {
            agent: agent.name.to_string(),
            success: false,
            path: agent_dir,
            canonical_path: None,
            mode: install_mode,
            symlink_failed: false,
            skipped: false,
            skip_reason: None,
            error: Some("Invalid skill name: potential path traversal detected".to_string()),
        };
    }

    let fail = |path: PathBuf, error: String| InstallOutcome {
        agent: agent.name.to_string(),
        success: false,
        path,
        canonical_path: None,
        mode: install_mode,
        symlink_failed: false,
        skipped: false,
        skip_reason: None,
        error: Some(error),
    };

    // Never install onto (or inside) the source directory.
    if let Some(source) = source_path {
        if paths_overlap(source, &agent_dir) {
            return InstallOutcome {
                agent: agent.name.to_string(),
                success: true,
                path: agent_dir,
                canonical_path: None,
                mode: install_mode,
                symlink_failed: false,
                skipped: true,
                skip_reason: None,
                error: None,
            };
        }
    }

    if install_mode == InstallMode::Copy {
        if clean_and_create_directory(&agent_dir).is_err() {
            return fail(agent_dir, "failed to create target directory".to_string());
        }
        if let Err(err) = write_files(&agent_dir, warnings) {
            return fail(agent_dir, err.plain());
        }
        return InstallOutcome {
            agent: agent.name.to_string(),
            success: true,
            path: agent_dir,
            canonical_path: None,
            mode: InstallMode::Copy,
            symlink_failed: false,
            skipped: false,
            skip_reason: None,
            error: None,
        };
    }

    let source_is_canonical = source_path
        .map(|s| {
            std::fs::canonicalize(s)
                .map(|c| c == canonical_dir)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if !source_is_canonical {
        if let Some(source) = source_path {
            if paths_overlap(source, &canonical_dir) {
                return InstallOutcome {
                    agent: agent.name.to_string(),
                    success: true,
                    path: canonical_dir.clone(),
                    canonical_path: Some(canonical_dir),
                    mode: install_mode,
                    symlink_failed: false,
                    skipped: true,
                    skip_reason: None,
                    error: None,
                };
            }
        }
        if install_mode == InstallMode::Link {
            if let Some(source) = source_path {
                if !create_symlink(source, &canonical_dir) {
                    if clean_and_create_directory(&canonical_dir).is_err() {
                        return fail(
                            agent_dir,
                            "failed to create canonical directory".to_string(),
                        );
                    }
                    if let Err(err) = write_files(&canonical_dir, warnings) {
                        return fail(agent_dir, err.plain());
                    }
                }
            }
        } else {
            if clean_and_create_directory(&canonical_dir).is_err() {
                return fail(
                    agent_dir,
                    "failed to create canonical directory".to_string(),
                );
            }
            if let Err(err) = write_files(&canonical_dir, warnings) {
                return fail(agent_dir, err.plain());
            }
        }
    }

    if is_global && agent.reads_canonical_globally() {
        return InstallOutcome {
            agent: agent.name.to_string(),
            success: true,
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: install_mode,
            symlink_failed: false,
            skipped: false,
            skip_reason: None,
            error: None,
        };
    }

    if should_skip_project_agent_symlink(agent, is_global, &cwd, options.create_missing_agent_root)
    {
        return InstallOutcome {
            agent: agent.name.to_string(),
            success: true,
            path: canonical_dir.clone(),
            canonical_path: Some(canonical_dir),
            mode: install_mode,
            symlink_failed: false,
            skipped: true,
            skip_reason: Some("missing-agent-project-directory"),
            error: None,
        };
    }

    if create_symlink(&canonical_dir, &agent_dir) {
        InstallOutcome {
            agent: agent.name.to_string(),
            success: true,
            path: agent_dir,
            canonical_path: Some(canonical_dir),
            mode: install_mode,
            symlink_failed: false,
            skipped: false,
            skip_reason: None,
            error: None,
        }
    } else {
        // Symlink failed, fall back to copy.
        if clean_and_create_directory(&agent_dir).is_err() {
            return fail(
                agent_dir,
                "symlink failed and copy fallback failed".to_string(),
            );
        }
        if let Err(err) = write_files(&agent_dir, warnings) {
            return fail(agent_dir, err.plain());
        }
        InstallOutcome {
            agent: agent.name.to_string(),
            success: true,
            path: agent_dir,
            canonical_path: Some(canonical_dir),
            mode: install_mode,
            symlink_failed: true,
            skipped: false,
            skip_reason: None,
            error: None,
        }
    }
}

/// Install one on-disk skill directory for one agent. The install name comes
/// from the directory name.
pub fn install_skill_dir(
    skill_path: &Path,
    agent: &AgentConfig,
    options: &InstallOptions,
    warnings: &mut Vec<SkillsError>,
) -> InstallOutcome {
    let name = skill_path.file_name().unwrap_or_default().to_string_lossy();
    install_named_skill_dir(&name, skill_path, agent, options, warnings)
}

/// Install an on-disk skill under its discovered or previously recorded name.
pub fn install_named_skill_dir(
    name: &str,
    skill_path: &Path,
    agent: &AgentConfig,
    options: &InstallOptions,
    warnings: &mut Vec<SkillsError>,
) -> InstallOutcome {
    let skill_name = sanitize_name(name);
    let source = skill_path.to_path_buf();
    install_for_agent(
        &skill_name,
        agent,
        options,
        Some(&source),
        &|dest, warnings| copy_directory(&source, dest, warnings),
        warnings,
    )
}

/// Install an in-memory file set (well-known skill) for one agent.
pub fn install_skill_files(
    install_name: &str,
    files: &[(String, Vec<u8>)],
    agent: &AgentConfig,
    options: &InstallOptions,
    warnings: &mut Vec<SkillsError>,
) -> InstallOutcome {
    let skill_name = sanitize_name(install_name);
    install_for_agent(
        &skill_name,
        agent,
        options,
        None,
        &|dest, _warnings| write_files_to(dest, files),
        warnings,
    )
}

/// Install a snapshot of text files (`{ path, contents }` strings), matching
/// the blob install path.
pub fn install_skill_snapshot(
    install_name: &str,
    files: &[(String, String)],
    agent: &AgentConfig,
    options: &InstallOptions,
    warnings: &mut Vec<SkillsError>,
) -> InstallOutcome {
    let bytes: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(path, contents)| (path.clone(), contents.clone().into_bytes()))
        .collect();
    install_skill_files(install_name, &bytes, agent, options, warnings)
}

fn write_files_to(target_dir: &Path, files: &[(String, Vec<u8>)]) -> Result<(), SkillsError> {
    for (file_path, contents) in files {
        let full = target_dir.join(file_path.replace('\\', "/"));
        if !is_path_safe(target_dir, &full) {
            continue; // Skip files that would escape the directory.
        }
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                SkillsError::new("SKILLS_IO", format!("{}: {err}", parent.display()))
            })?;
        }
        std::fs::write(&full, contents)
            .map_err(|err| SkillsError::new("SKILLS_IO", format!("{}: {err}", full.display())))?;
    }
    Ok(())
}

/// Whether a sanitized skill directory already exists for an agent.
#[must_use]
pub fn is_skill_installed(
    skill_name: &str,
    agent: &AgentConfig,
    global: bool,
    cwd: &Path,
    eve_subagent: Option<&str>,
) -> bool {
    let sanitized = sanitize_name(skill_name);
    if global
        && agent
            .global_skills_dir(&DetectContext::from_env(cwd))
            .is_none()
    {
        return false;
    }
    let target_base = if global {
        get_agent_base_dir(agent, true, cwd, None)
    } else if agent.name == "eve" {
        get_agent_base_dir(agent, false, cwd, eve_subagent)
    } else {
        cwd.join(agent.skills_dir)
    };
    target_base.join(sanitized).exists()
}

/// The path an agent reads a skill from (upstream `getInstallPath`).
#[must_use]
pub fn get_install_path(
    skill_name: &str,
    agent: &AgentConfig,
    global: bool,
    cwd: &Path,
    eve_subagent: Option<&str>,
) -> PathBuf {
    get_agent_base_dir(agent, global, cwd, eve_subagent).join(sanitize_name(skill_name))
}

/// The canonical `.agents/skills/<skill>` path.
#[must_use]
pub fn get_canonical_path(skill_name: &str, global: bool, cwd: &Path) -> PathBuf {
    get_canonical_skills_dir(global, cwd).join(sanitize_name(skill_name))
}

/// A listed installed skill.
#[derive(Debug, Clone)]
pub struct InstalledSkill {
    /// Frontmatter name.
    pub name: String,
    /// Frontmatter description.
    pub description: String,
    /// Directory holding the skill.
    pub path: PathBuf,
    /// Canonical copy (same as `path` for scanned agent dirs).
    pub canonical_path: PathBuf,
    /// Scope it was found in.
    pub scope: Scope,
    /// Agent registry ids the skill is linked for.
    pub agents: Vec<String>,
}

/// List installed skills by scanning canonical + agent directories
/// (upstream `listInstalledSkills`).
#[must_use]
pub fn list_installed_skills(
    global: Option<bool>,
    cwd: &Path,
    agent_filter: Option<&[String]>,
) -> Vec<InstalledSkill> {
    let ctx = DetectContext::from_env(cwd);
    let detected = registry::detect_installed_agents(cwd);
    let agents_to_check: Vec<&AgentConfig> = match agent_filter {
        Some(filter) => detected
            .into_iter()
            .filter(|a| filter.iter().any(|f| f == a.name))
            .collect(),
        None => detected,
    };

    let scope_types: Vec<bool> = match global {
        Some(g) => vec![g],
        None => vec![false, true],
    };

    struct ScanScope {
        global: bool,
        path: PathBuf,
        agent: Option<String>,
    }

    let mut scopes: Vec<ScanScope> = Vec::new();
    for is_global in scope_types {
        scopes.push(ScanScope {
            global: is_global,
            path: get_canonical_skills_dir(is_global, cwd),
            agent: None,
        });
        for agent in &agents_to_check {
            if is_global && agent.global_skills_dir(&ctx).is_none() {
                continue;
            }
            let agent_dir = if is_global {
                agent.global_skills_dir(&ctx).unwrap()
            } else {
                cwd.join(agent.skills_dir)
            };
            if !scopes
                .iter()
                .any(|s| s.path == agent_dir && s.global == is_global)
            {
                scopes.push(ScanScope {
                    global: is_global,
                    path: agent_dir,
                    agent: Some(agent.name.to_string()),
                });
            }
            if agent.name == "eve" && !is_global {
                for subagent in eve_subagent_names(cwd) {
                    let dir = get_eve_subagent_skills_dir(&subagent, cwd);
                    if !scopes
                        .iter()
                        .any(|s| s.path == dir && s.global == is_global)
                    {
                        scopes.push(ScanScope {
                            global: is_global,
                            path: dir,
                            agent: Some("eve".to_string()),
                        });
                    }
                }
            }
        }
        // Also scan dirs for agents no longer detected but present on disk.
        for agent in registry::agents() {
            if agent_filter.is_some_and(|f| !f.iter().any(|x| x == agent.name)) {
                continue;
            }
            if agents_to_check.iter().any(|a| a.name == agent.name) {
                continue;
            }
            if is_global && agent.global_skills_dir(&ctx).is_none() {
                continue;
            }
            let agent_dir = if is_global {
                match agent.global_skills_dir(&ctx) {
                    Some(dir) => dir,
                    None => continue,
                }
            } else {
                cwd.join(agent.skills_dir)
            };
            if scopes
                .iter()
                .any(|s| s.path == agent_dir && s.global == is_global)
            {
                continue;
            }
            if agent_dir.exists() {
                scopes.push(ScanScope {
                    global: is_global,
                    path: agent_dir,
                    agent: Some(agent.name.to_string()),
                });
            }
        }
    }

    let mut skills_map: Vec<(String, InstalledSkill)> = Vec::new();
    for scope in scopes {
        let Ok(entries) = std::fs::read_dir(&scope.path) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let skill_dir = entry.path();
            let is_dir = file_type.is_dir()
                || (file_type.is_symlink()
                    && std::fs::metadata(&skill_dir)
                        .map(|m| m.is_dir())
                        .unwrap_or(false));
            if !is_dir {
                continue;
            }
            if !has_skill_md(&skill_dir) {
                continue;
            }
            let Some(skill) = parse_skill_md(&skill_dir.join("SKILL.md")) else {
                continue;
            };
            let scope_key = if scope.global {
                Scope::Global
            } else {
                Scope::Project
            };
            let skill_key = format!(
                "{}:{}",
                if scope.global { "global" } else { "project" },
                skill.name
            );

            if let Some(agent_name) = &scope.agent {
                match skills_map.iter_mut().find(|(k, _)| *k == skill_key) {
                    Some((_, existing)) => {
                        if !existing.agents.contains(agent_name) {
                            existing.agents.push(agent_name.clone());
                        }
                    }
                    None => skills_map.push((
                        skill_key.clone(),
                        InstalledSkill {
                            name: skill.name.clone(),
                            description: skill.description.clone(),
                            path: skill_dir.clone(),
                            canonical_path: skill_dir.clone(),
                            scope: scope_key,
                            agents: vec![agent_name.clone()],
                        },
                    )),
                }
                continue;
            }

            // Canonical directory: determine which agents have this skill.
            let sanitized_skill_name = sanitize_name(&skill.name);
            let mut installed_agents: Vec<String> = Vec::new();
            for agent in &agents_to_check {
                if scope.global && agent.global_skills_dir(&ctx).is_none() {
                    continue;
                }
                let agent_base = get_agent_base_dir(agent, scope.global, cwd, None);
                let mut found = false;
                let mut possible_names: Vec<String> = vec![
                    entry.file_name().to_string_lossy().to_string(),
                    sanitized_skill_name.clone(),
                ];
                let collapsed = skill
                    .name
                    .to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-");
                let collapsed = collapsed.replace(['/', '\\', ':', '\0'], "");
                if !possible_names.contains(&collapsed) {
                    possible_names.push(collapsed);
                }
                for possible in &possible_names {
                    if agent_base.join(possible).exists() {
                        found = true;
                        break;
                    }
                }
                if !found {
                    if let Ok(agent_entries) = std::fs::read_dir(&agent_base) {
                        for agent_entry in agent_entries.flatten() {
                            let Ok(ft) = agent_entry.file_type() else {
                                continue;
                            };
                            let candidate = agent_entry.path();
                            let candidate_is_dir = ft.is_dir()
                                || (ft.is_symlink()
                                    && std::fs::metadata(&candidate)
                                        .map(|m| m.is_dir())
                                        .unwrap_or(false));
                            if !candidate_is_dir || !has_skill_md(&candidate) {
                                continue;
                            }
                            if let Some(candidate_skill) =
                                parse_skill_md(&candidate.join("SKILL.md"))
                            {
                                if candidate_skill.name == skill.name {
                                    found = true;
                                    break;
                                }
                            }
                        }
                    }
                }
                if found {
                    installed_agents.push(agent.name.to_string());
                }
            }

            match skills_map.iter_mut().find(|(k, _)| *k == skill_key) {
                Some((_, existing)) => {
                    for agent in installed_agents {
                        if !existing.agents.contains(&agent) {
                            existing.agents.push(agent);
                        }
                    }
                }
                None => skills_map.push((
                    skill_key,
                    InstalledSkill {
                        name: skill.name,
                        description: skill.description,
                        path: skill_dir.clone(),
                        canonical_path: skill_dir,
                        scope: scope_key,
                        agents: installed_agents,
                    },
                )),
            }
        }
    }

    skills_map.into_iter().map(|(_, v)| v).collect()
}

// ─── Removal ────────────────────────────────────────────────────────────────

/// A single removal result.
#[derive(Debug, Clone)]
pub struct RemoveOutcome {
    /// Skill name.
    pub skill: String,
    /// Whether removal succeeded.
    pub success: bool,
    /// The effective source recorded in the lock (`local` when unknown).
    pub source: String,
    /// The effective source type.
    pub source_type: String,
    /// Error message on failure.
    pub error: Option<String>,
}

/// Options for [`remove_skills`].
#[derive(Debug, Clone, Default)]
pub struct RemoveOptions {
    /// Project or global scope.
    pub global: Scope,
    /// Project directory (defaults to the process cwd).
    pub cwd: Option<PathBuf>,
    /// Only remove from these agents; all agents when empty (cleans ghost
    /// symlinks too).
    pub target_agents: Vec<String>,
    /// Update the lock file (defaults to true when `None`).
    pub lockfile: Option<bool>,
}

/// Resolve requested skill names to canonical removal targets
/// (upstream `resolveSkillsToRemove`).
#[must_use]
pub fn resolve_skills_to_remove(
    requested: &[String],
    folder_names: &[String],
    lock_keys: &[String],
    source_by_key: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut identity_by_sanitized: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for folder in folder_names {
        identity_by_sanitized.insert(sanitize_name(folder), folder.clone());
    }
    for key in lock_keys {
        identity_by_sanitized.insert(sanitize_name(key), key.clone());
    }
    let mut matched: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for name in requested {
        if let Some(hit) = identity_by_sanitized.get(&sanitize_name(name)) {
            matched.insert(hit.clone());
            continue;
        }
        for key in lock_keys {
            if source_by_key.get(key).map(String::as_str) == Some(name.as_str()) {
                matched.insert(key.clone());
            }
        }
    }
    matched.into_iter().collect()
}

/// Names of skill folders (holding a SKILL.md) installed in `cwd` or globally.
#[must_use]
pub fn scan_installed_skill_names(is_global: bool, cwd: &Path) -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    let scan_dir = |dir: &Path, names: &mut std::collections::BTreeSet<String>| {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if !file_type.is_dir() || name.starts_with('.') {
                continue;
            }
            if !has_skill_md(&entry.path()) {
                continue;
            }
            names.insert(name);
        }
    };

    if is_global {
        scan_dir(&get_canonical_skills_dir(true, cwd), &mut names);
        let ctx = DetectContext::from_env(cwd);
        for agent in registry::agents() {
            if let Some(global_dir) = agent.global_skills_dir(&ctx) {
                scan_dir(&global_dir, &mut names);
            }
        }
    } else {
        scan_dir(&get_canonical_skills_dir(false, cwd), &mut names);
        for agent in registry::agents() {
            scan_dir(&cwd.join(agent.skills_dir), &mut names);
        }
        for subagent in eve_subagent_names(cwd) {
            scan_dir(&get_eve_subagent_skills_dir(&subagent, cwd), &mut names);
        }
    }

    names.into_iter().collect()
}

/// Skills in `selected` a project-scoped remove must refuse: when the project
/// skills dir is the global one (cwd is home) (upstream
/// `findGloballyTrackedSkills`).
pub fn find_globally_tracked_skills(
    selected: &[String],
    cwd: &Path,
) -> Result<Vec<String>, SkillsError> {
    let project_dir = get_canonical_skills_dir(false, cwd);
    let global_dir = get_canonical_skills_dir(true, cwd);
    let same_dir = std::fs::canonicalize(&project_dir)
        .map(|p| p == std::fs::canonicalize(&global_dir).unwrap_or_else(|_| global_dir.clone()))
        .unwrap_or(project_dir == global_dir);
    if !same_dir {
        return Ok(Vec::new());
    }
    let lock_entries = crate::locks::read_local_lock(cwd)?;
    let global_names: HashSet<String> = crate::locks::read_skill_lock()?
        .skills
        .keys()
        .map(|n| sanitize_name(n))
        .collect();
    Ok(selected
        .iter()
        .filter(|s| {
            !lock_entries.skills.contains_key(*s) && global_names.contains(&sanitize_name(s))
        })
        .cloned()
        .collect())
}

/// Remove `skills` from `target_agents` without prompting. The canonical copy
/// and lock entry go only when no other installed agent uses them.
pub fn remove_skills(
    skills: &[String],
    options: &RemoveOptions,
    warnings: &mut Vec<SkillsError>,
) -> Result<Vec<RemoveOutcome>, SkillsError> {
    let is_global = options.global.is_global();
    let cwd = options
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let ctx = DetectContext::from_env(&cwd);
    let target_agents: Vec<&AgentConfig> = if options.target_agents.is_empty() {
        registry::agents().iter().collect()
    } else {
        options
            .target_agents
            .iter()
            .filter_map(|name| registry::agent_by_id(name))
            .collect()
    };

    let mut results = Vec::new();
    for skill_name in skills {
        let canonical_path = get_canonical_path(skill_name, is_global, &cwd);
        for agent in &target_agents {
            let skill_path = get_install_path(skill_name, agent, is_global, &cwd, None);
            let mut cleanup_paths: Vec<PathBuf> = vec![skill_path];
            let sanitized = sanitize_name(skill_name);
            if is_global {
                if let Some(global_dir) = agent.global_skills_dir(&ctx) {
                    cleanup_paths.push(global_dir.join(&sanitized));
                }
            } else {
                cleanup_paths.push(cwd.join(agent.skills_dir).join(&sanitized));
                if agent.name == "eve" {
                    for subagent in eve_subagent_names(&cwd) {
                        cleanup_paths
                            .push(get_eve_subagent_skills_dir(&subagent, &cwd).join(&sanitized));
                    }
                }
            }

            for path_to_cleanup in &cleanup_paths {
                if *path_to_cleanup == canonical_path {
                    continue;
                }
                if std::fs::symlink_metadata(path_to_cleanup).is_ok() {
                    if let Err(err) = remove_path_any(path_to_cleanup) {
                        warnings.push(SkillsError::new(
                            "SKILLS_IO",
                            format!("Could not remove skill from {}: {err}", agent.display_name),
                        ));
                    }
                }
            }
        }

        // Only remove the canonical copy when no other installed agent uses it.
        let installed_agents = registry::detect_installed_agents(&cwd);
        let mut is_still_used = false;
        for agent in installed_agents
            .iter()
            .filter(|a| !target_agents.iter().any(|t| t.name == a.name))
        {
            let path = get_install_path(skill_name, agent, is_global, &cwd, None);
            if std::fs::symlink_metadata(&path).is_ok() {
                is_still_used = true;
                break;
            }
        }

        if !is_still_used {
            let _ = remove_path_any(&canonical_path);
        }

        let track_remove = options.lockfile != Some(false);
        let prune_placements = |placements: &mut Vec<crate::locks::SkillPlacement>| {
            let original_len = placements.len();
            placements.retain(|placement| {
                let Some(agent) = target_agents
                    .iter()
                    .find(|agent| agent.name == placement.agent)
                else {
                    return true;
                };
                let path = get_install_path(
                    skill_name,
                    agent,
                    is_global,
                    &cwd,
                    placement.subagent.as_deref(),
                );
                path != canonical_path && path.join("SKILL.md").is_file()
            });
            original_len != placements.len()
        };
        let (source, source_type) = if is_global {
            let mut lock = crate::locks::read_skill_lock()?;
            let entry = lock.skills.get_mut(skill_name);
            let source = entry
                .as_ref()
                .map(|e| (e.source.clone(), e.source_type.clone()));
            if track_remove {
                let changed = if is_still_used {
                    entry.is_some_and(|e| prune_placements(&mut e.placements))
                } else {
                    lock.skills.remove(skill_name).is_some()
                };
                if changed {
                    crate::locks::write_skill_lock(&lock)?;
                }
            }
            source
        } else {
            let mut lock = crate::locks::read_local_lock(&cwd)?;
            let entry = lock.skills.get_mut(skill_name);
            let source = entry
                .as_ref()
                .map(|e| (e.source.clone(), e.source_type.clone()));
            if track_remove {
                let changed = if is_still_used {
                    entry.is_some_and(|e| prune_placements(&mut e.placements))
                } else {
                    lock.skills.remove(skill_name).is_some()
                };
                if changed {
                    crate::locks::write_local_lock(&lock, &cwd)?;
                }
            }
            source
        }
        .unwrap_or_else(|| ("local".into(), "local".into()));

        results.push(RemoveOutcome {
            skill: skill_name.clone(),
            success: true,
            source,
            source_type,
            error: None,
        });
    }
    Ok(results)
}
