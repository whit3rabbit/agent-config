//! Skill discovery from a source tree, ported from `src/skills.ts`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{diag, SkillsError};
use crate::frontmatter::parse_frontmatter;
use crate::registry::agent_project_skill_dirs;
use crate::sanitize::{normalize_skill_name, sanitize_metadata};

/// Directories never descended into during discovery.
pub const SKIP_DIRS: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// Maximum skill-directory depth searched inside a known container.
pub const DEFAULT_SKILL_CONTAINER_DEPTH: usize = 3;

/// A discovered skill.
#[derive(Debug, Clone)]
pub struct Skill {
    /// Frontmatter `name` (sanitized).
    pub name: String,
    /// Frontmatter `description` (sanitized).
    pub description: String,
    /// Directory holding the SKILL.md.
    pub path: PathBuf,
    /// Raw SKILL.md content, when known.
    pub raw_content: Option<String>,
    /// Plugin grouping name, when the source declares one.
    pub plugin_name: Option<String>,
    /// Full frontmatter `metadata` object.
    pub metadata: Option<serde_json::Value>,
}

impl Skill {
    /// Upstream `getSkillDisplayName`: the name, or the folder name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.name
    }
}

/// Whether `INSTALL_INTERNAL_SKILLS` opts into `metadata.internal` skills.
#[must_use]
pub fn should_install_internal_skills() -> bool {
    matches!(
        std::env::var("INSTALL_INTERNAL_SKILLS").ok().as_deref(),
        Some("1") | Some("true")
    )
}

/// Whether `dir` directly contains a SKILL.md file.
#[must_use]
pub fn has_skill_md(dir: &Path) -> bool {
    dir.join("SKILL.md").is_file()
}

fn warn_skipped(skill_md_path: &Path, reason: &str, warnings: &mut Vec<SkillsError>) {
    warnings.push(diag::skill_skipped(
        &sanitize_metadata(&skill_md_path.to_string_lossy()),
        &sanitize_metadata(reason),
    ));
}

/// Parse one SKILL.md into a [`Skill`], or `None` when invalid / internal.
pub fn parse_skill_md_with_warnings(
    skill_md_path: &Path,
    options: &DiscoverOptions,
    warnings: &mut Vec<SkillsError>,
) -> Option<Skill> {
    let content = match std::fs::read_to_string(skill_md_path) {
        Ok(content) => content,
        Err(err) => {
            warn_skipped(skill_md_path, &format!("failed to read file: {err}"), warnings);
            return None;
        }
    };
    let fm = parse_frontmatter(&content);
    let (Some(name_value), Some(description_value)) =
        (fm.data.get("name"), fm.data.get("description"))
    else {
        let mut missing = Vec::new();
        if fm.data.get("name").is_none() {
            missing.push("name");
        }
        if fm.data.get("description").is_none() {
            missing.push("description");
        }
        warn_skipped(
            skill_md_path,
            &format!("missing required frontmatter field(s): {}", missing.join(", ")),
            warnings,
        );
        return None;
    };
    let (Some(name_str), Some(desc_str)) = (name_value.as_str(), description_value.as_str()) else {
        warn_skipped(
            skill_md_path,
            &format!(
                "frontmatter \"name\" and \"description\" must be strings (got {} and {})",
                json_type_name(name_value),
                json_type_name(description_value)
            ),
            warnings,
        );
        return None;
    };
    let name = sanitize_metadata(name_str);
    let description = sanitize_metadata(desc_str);
    if name.is_empty() || description.is_empty() {
        let mut empty = Vec::new();
        if name.is_empty() {
            empty.push("name");
        }
        if description.is_empty() {
            empty.push("description");
        }
        warn_skipped(
            skill_md_path,
            &format!("empty required frontmatter field(s): {}", empty.join(", ")),
            warnings,
        );
        return None;
    }

    let metadata = fm.data.get("metadata").filter(|m| m.is_object()).cloned();
    let is_internal = metadata
        .as_ref()
        .and_then(|m| m.get("internal"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_internal && !should_install_internal_skills() && !options.include_internal {
        return None;
    }

    Some(Skill {
        name,
        description,
        path: skill_md_path.parent().unwrap_or(Path::new(".")).to_path_buf(),
        raw_content: Some(content),
        plugin_name: None,
        metadata,
    })
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "undefined",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Convenience wrapper collecting warnings into a discardable buffer.
#[must_use]
pub fn parse_skill_md(skill_md_path: &Path) -> Option<Skill> {
    let mut warnings = Vec::new();
    parse_skill_md_with_warnings(skill_md_path, &DiscoverOptions::default(), &mut warnings)
}

/// Discovery options (upstream `DiscoverSkillsOptions`).
#[derive(Debug, Clone, Default)]
pub struct DiscoverOptions {
    /// Include skills marked `metadata.internal: true`.
    pub include_internal: bool,
    /// Search all subdirectories even when a root SKILL.md exists.
    pub full_depth: bool,
    /// Preserve duplicate skill names instead of keeping the first.
    pub include_duplicate_names: bool,
}

/// Result of [`discover_skills`]: skills plus non-fatal warnings.
#[derive(Debug, Default)]
pub struct Discovery {
    /// Skills found, in upstream discovery order.
    pub skills: Vec<Skill>,
    /// Non-fatal problems (`SKILLS_SKILL_SKIPPED` diagnostics).
    pub warnings: Vec<SkillsError>,
}

fn is_subpath_safe(base: &Path, subpath: &str) -> bool {
    let joined = base.join(subpath);
    match (std::fs::canonicalize(base), joined.canonicalize()) {
        (Ok(base), Ok(target)) => target.starts_with(&base),
        // Match the upstream behavior loosely when canonicalization fails.
        _ => !subpath.split('/').any(|s| s == ".."),
    }
}

fn validate_subpath(subpath: &str) -> Result<(), SkillsError> {
    if subpath.replace('\\', "/").split('/').any(|s| s == "..") {
        return Err(diag::unsafe_subpath(subpath));
    }
    Ok(())
}

/// Parse the `paths` field of a `skills.sh.json` file (upstream
/// `parseSkillsShPaths`): `./`-prefixed repo-relative container dirs.
#[must_use]
pub fn parse_skills_sh_paths(content: &str) -> Vec<String> {
    let Ok(config) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(paths) = config.get("paths").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    let mut result: Vec<String> = Vec::new();
    for entry in paths {
        let Some(entry) = entry.as_str() else { continue };
        if !entry.starts_with("./") || entry.contains('\\') {
            continue;
        }
        // POSIX normalize: resolve `.` and `..` against a virtual root.
        let mut parts: Vec<&str> = Vec::new();
        for segment in entry.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        let normalized = parts.join("/");
        if normalized.is_empty() || normalized == ".." || normalized.starts_with("../") {
            continue;
        }
        if !result.contains(&normalized) {
            result.push(normalized);
        }
    }
    result
}

/// Read `skills.sh.json` container dirs from `basePath`.
fn skills_sh_search_dirs(base: &Path) -> Vec<PathBuf> {
    let Ok(content) = std::fs::read_to_string(base.join("skills.sh.json")) else {
        return Vec::new();
    };
    parse_skills_sh_paths(&content).into_iter().map(|p| base.join(p)).collect()
}

/// Plugin manifest extraction, ported from `src/plugin-manifest.ts`.
fn plugin_manifests(base: &Path) -> (Vec<PathBuf>, HashMap<PathBuf, String>) {
    let mut search_dirs = Vec::new();
    let mut groupings = HashMap::new();

    let read_manifest = |rel: &str| -> Option<serde_json::Value> {
        let text = std::fs::read_to_string(base.join(rel)).ok()?;
        serde_json::from_str(&text).ok()
    };

    let is_contained = |candidate: &Path| -> bool {
        candidate.canonicalize().map(|c| c.starts_with(base)).unwrap_or_else(|_| candidate.starts_with(base))
    };

    // marketplace.json (multi-plugin catalog)
    if let Some(manifest) = read_manifest(".claude-plugin/marketplace.json") {
        let plugin_root = manifest
            .get("metadata")
            .and_then(|m| m.get("pluginRoot"))
            .and_then(|p| p.as_str())
            .filter(|p| p.starts_with("./"));
        let plugins = manifest.get("plugins").and_then(|p| p.as_array()).cloned().unwrap_or_default();
        for plugin in &plugins {
            let name = plugin.get("name").and_then(|n| n.as_str());
            let source = match plugin.get("source") {
                None => "",
                Some(s) if s.is_string() => s.as_str().unwrap_or(""),
                Some(_) => continue, // remote source objects are skipped
            };
            if source.is_empty() && plugin.get("source").is_some() {
                continue;
            }
            if !source.is_empty() && !source.starts_with("./") {
                continue;
            }
            let mut plugin_base = base.to_path_buf();
            if let Some(root) = plugin_root {
                plugin_base = plugin_base.join(root.trim_start_matches("./"));
            }
            if !source.is_empty() {
                plugin_base = plugin_base.join(source.trim_start_matches("./"));
            }
            if !is_contained(&plugin_base) {
                continue;
            }
            if let Some(skills) = plugin.get("skills").and_then(|s| s.as_array()) {
                for skill_path in skills {
                    let Some(skill_path) = skill_path.as_str() else { continue };
                    if !skill_path.starts_with("./") {
                        continue;
                    }
                    let skill_dir = plugin_base.join(skill_path.trim_start_matches("./"));
                    if is_contained(&skill_dir) {
                        search_dirs.push(skill_dir.parent().unwrap_or(&skill_dir).to_path_buf());
                        if let Some(name) = name {
                            groupings.insert(skill_dir, name.to_string());
                        }
                    }
                }
            }
            search_dirs.push(plugin_base.join("skills"));
        }
    }

    // plugin.json (single plugin at root)
    if let Some(manifest) = read_manifest(".claude-plugin/plugin.json") {
        let name = manifest.get("name").and_then(|n| n.as_str());
        let skills = manifest.get("skills").and_then(|s| s.as_array()).cloned().unwrap_or_default();
        for skill_path in &skills {
            let Some(skill_path) = skill_path.as_str() else { continue };
            if !skill_path.starts_with("./") {
                continue;
            }
            let skill_dir = base.join(skill_path.trim_start_matches("./"));
            if is_contained(&skill_dir) {
                search_dirs.push(skill_dir.parent().unwrap_or(&skill_dir).to_path_buf());
                if let Some(name) = name {
                    groupings.insert(skill_dir, name.to_string());
                }
            }
        }
        search_dirs.push(base.join("skills"));
    }

    (search_dirs, groupings)
}

/// Discover skills under `base_path` (optionally `subpath`), ported from
/// upstream `discoverSkills`.
pub fn discover_skills(
    base_path: &Path,
    subpath: Option<&str>,
    options: &DiscoverOptions,
) -> Result<Discovery, SkillsError> {
    let mut warnings = Vec::new();
    let mut skills: Vec<Skill> = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();
    let mut parsed_skill_paths: HashSet<PathBuf> = HashSet::new();

    if let Some(subpath) = subpath {
        validate_subpath(subpath)?;
        if !is_subpath_safe(base_path, subpath) {
            return Err(diag::subpath_outside_repo(subpath));
        }
    }

    let search_path = match subpath {
        Some(subpath) => base_path.join(subpath),
        None => base_path.to_path_buf(),
    };

    let (mut plugin_skill_dirs, plugin_groupings) = plugin_manifests(&search_path);

    // The source repo's own lock marks skills it merely installed; skip them.
    let locked_skill_names: HashSet<String> = crate::locks::read_local_lock(base_path)
        .map(|lock| lock.skills.keys().map(|k| normalize_skill_name(k)).collect())
        .unwrap_or_default();
    let skip_installed_project_skill = |skill: &Skill| -> bool {
        if locked_skill_names.is_empty() {
            return false;
        }
        let Ok(relative_dir) = skill.path.strip_prefix(base_path) else {
            return false;
        };
        let relative_dir = relative_dir.to_string_lossy().replace('\\', "/");
        let is_agent_skill_path = agent_project_skill_dirs()
            .iter()
            .any(|dir| relative_dir == *dir || relative_dir.starts_with(&format!("{dir}/")));
        if !is_agent_skill_path {
            return false;
        }
        let directory_name = normalize_skill_name(
            &skill.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        );
        locked_skill_names.contains(&normalize_skill_name(&skill.name))
            || locked_skill_names.contains(&directory_name)
    };

    fn parse_skill_at(
        skill_dir: &Path,
        parsed_skill_paths: &mut HashSet<PathBuf>,
        options: &DiscoverOptions,
        warnings: &mut Vec<SkillsError>,
    ) -> Option<Skill> {
        let skill_md_path = skill_dir.join("SKILL.md");
        if parsed_skill_paths.contains(&skill_md_path) {
            return None;
        }
        parsed_skill_paths.insert(skill_md_path.clone());
        parse_skill_md_with_warnings(&skill_md_path, options, warnings)
    }

    let enhance = |skill: &mut Skill| {
        if let Some(plugin) = plugin_groupings.get(&skill.path) {
            skill.plugin_name = Some(plugin.clone());
        }
    };

    // Direct skill at the search path (early return unless full depth).
    if has_skill_md(&search_path) {
        if let Some(mut skill) = parse_skill_at(&search_path, &mut parsed_skill_paths, options, &mut warnings) {
            if !skip_installed_project_skill(&skill) {
                enhance(&mut skill);
                seen_names.insert(skill.name.clone());
                if !options.full_depth {
                    return Ok(Discovery { skills: vec![skill], warnings });
                }
                skills.push(skill);
            }
        }
    }

    // Priority containers: root, then the standard layout dirs, then declared.
    let mut priority_search_dirs: Vec<PathBuf> =
        vec![search_path.clone(), search_path.join("skills")];
    priority_search_dirs.push(search_path.join("skills/.curated"));
    priority_search_dirs.push(search_path.join("skills/.experimental"));
    priority_search_dirs.push(search_path.join("skills/.system"));
    for dir in agent_project_skill_dirs() {
        priority_search_dirs.push(search_path.join(dir));
    }
    let declared_dirs = skills_sh_search_dirs(&search_path);
    priority_search_dirs.extend(declared_dirs);
    // Manifest-declared dirs stay at depth 1.
    priority_search_dirs.append(&mut plugin_skill_dirs);

    let deep_container_dirs: HashSet<usize> = (1..priority_search_dirs.len()).collect();

    for (index, dir) in priority_search_dirs.iter().enumerate() {
        let max_depth = if deep_container_dirs.contains(&index) {
            DEFAULT_SKILL_CONTAINER_DEPTH
        } else {
            1
        };
        walk_skill_dirs(dir, max_depth, 1, options, &mut skills, &mut seen_names, &mut parsed_skill_paths, &mut warnings, &plugin_groupings, &skip_installed_project_skill);
    }

    // Recursive fallback when nothing matched, or with full depth.
    if skills.is_empty() || options.full_depth {
        let mut all_skill_dirs = Vec::new();
        find_skill_dirs(&search_path, 0, 5, &mut all_skill_dirs);
        for skill_dir in all_skill_dirs {
            if let Some(mut skill) = parse_skill_at(&skill_dir, &mut parsed_skill_paths, options, &mut warnings) {
                if (options.include_duplicate_names || !seen_names.contains(&skill.name))
                    && !skip_installed_project_skill(&skill)
                {
                    enhance(&mut skill);
                    seen_names.insert(skill.name.clone());
                    skills.push(skill);
                }
            }
        }
    }

    Ok(Discovery { skills, warnings })
}

#[allow(clippy::too_many_arguments)]
fn walk_skill_dirs(
    dir: &Path,
    max_depth: usize,
    depth: usize,
    options: &DiscoverOptions,
    skills: &mut Vec<Skill>,
    seen_names: &mut HashSet<String>,
    parsed_skill_paths: &mut HashSet<PathBuf>,
    warnings: &mut Vec<SkillsError>,
    plugin_groupings: &HashMap<PathBuf, String>,
    skip_installed: &dyn Fn(&Skill) -> bool,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_dir() {
            continue;
        }
        let child = entry.path();
        // Upstream tryAddSkillAt: a directory holding a SKILL.md always stops
        // the descent below it, whether or not a skill was pushed.
        let found_at_child = has_skill_md(&child);
        if found_at_child {
            let skill_md_path = child.join("SKILL.md");
            if !parsed_skill_paths.contains(&skill_md_path) {
                parsed_skill_paths.insert(skill_md_path.clone());
                if let Some(mut skill) = parse_skill_md_with_warnings(&skill_md_path, options, warnings) {
                    let duplicate = !options.include_duplicate_names && seen_names.contains(&skill.name);
                    if !duplicate && !skip_installed(&skill) {
                        if let Some(plugin) = plugin_groupings.get(&skill.path) {
                            skill.plugin_name = Some(plugin.clone());
                        }
                        seen_names.insert(skill.name.clone());
                        skills.push(skill);
                    }
                }
            }
        }
        if found_at_child
            || depth >= max_depth
            || SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
        {
            continue;
        }
        walk_skill_dirs(&child, max_depth, depth + 1, options, skills, seen_names, parsed_skill_paths, warnings, plugin_groupings, skip_installed);
    }
}

fn find_skill_dirs(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    if has_skill_md(dir) {
        out.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if file_type.is_dir() && !SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref()) {
            find_skill_dirs(&entry.path(), depth + 1, max_depth, out);
        }
    }
}

/// Filter skills by name, falling back to slug matching (upstream
/// `filterSkills`); preserves source order.
#[must_use]
pub fn filter_skills(skills: &[Skill], input_names: &[String]) -> Vec<Skill> {
    let mut selected: HashSet<usize> = HashSet::new();
    for input in input_names {
        let exact: Vec<usize> = skills
            .iter()
            .enumerate()
            .filter(|(_, s)| s.name.to_lowercase() == input.to_lowercase())
            .map(|(i, _)| i)
            .collect();
        let matches: Vec<usize> = if exact.is_empty() {
            skills
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    normalize_skill_name(&s.name) == normalize_skill_name(input)
                })
                .map(|(i, _)| i)
                .collect()
        } else {
            exact
        };
        selected.extend(matches);
    }
    skills
        .iter()
        .enumerate()
        .filter(|(i, _)| selected.contains(i))
        .map(|(_, s)| s.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn skill_md(name: &str, desc: &str) -> String {
        format!("---\nname: {name}\ndescription: {desc}\n---\nbody\n")
    }

    #[test]
    fn discovers_flat_and_nested() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "skills/alpha/SKILL.md", &skill_md("alpha", "A"));
        write(root, "skills/beta/SKILL.md", &skill_md("beta", "B"));
        // Depth-2 under docs/ is outside the depth-1 root walk.
        write(root, "docs/nested/SKILL.md", &skill_md("docs-skill", "D"));

        let found = discover_skills(root, None, &DiscoverOptions::default()).unwrap();
        let mut names: Vec<&str> = found.skills.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["alpha", "beta"]);
    }

    #[test]
    fn root_skill_early_return() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "SKILL.md", &skill_md("root-skill", "R"));
        write(root, "skills/other/SKILL.md", &skill_md("other", "O"));
        let found = discover_skills(root, None, &DiscoverOptions::default()).unwrap();
        assert_eq!(found.skills.len(), 1);
        assert_eq!(found.skills[0].name, "root-skill");
    }

    #[test]
    fn deep_container_layout() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(root, "skills/web/tools/fmt/SKILL.md", &skill_md("fmt", "F"));
        let found = discover_skills(root, None, &DiscoverOptions::default()).unwrap();
        assert_eq!(found.skills.len(), 1);
        assert_eq!(found.skills[0].name, "fmt");
    }

    #[test]
    fn internal_skills_hidden_unless_requested() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        write(
            root,
            "skills/internal-one/SKILL.md",
            "---\nname: internal-one\ndescription: I\nmetadata:\n  internal: true\n---\nx",
        );
        let hidden = discover_skills(root, None, &DiscoverOptions::default()).unwrap();
        assert!(hidden.skills.is_empty());
        let shown = discover_skills(
            root,
            None,
            &DiscoverOptions { include_internal: true, ..Default::default() },
        )
        .unwrap();
        assert_eq!(shown.skills.len(), 1);
    }

    #[test]
    fn subpath_traversal_rejected() {
        let tmp = TempDir::new().unwrap();
        let err = discover_skills(tmp.path(), Some("../outside"), &DiscoverOptions::default());
        assert!(err.is_err());
    }

    #[test]
    fn filter_matches_slug() {
        let a = Skill {
            name: "alpha-skill".into(),
            description: "A".into(),
            path: PathBuf::from("/x"),
            raw_content: None,
            plugin_name: None,
            metadata: None,
        };
        let picked = filter_skills(std::slice::from_ref(&a), &["Alpha Skill".to_string()]);
        assert_eq!(picked.len(), 1);
    }

    #[test]
    fn skills_sh_paths_parsed() {
        assert_eq!(parse_skills_sh_paths(r#"{"paths":["./tools/skills/","./skills"]}"#), vec!["tools/skills", "skills"]);
        assert!(parse_skills_sh_paths("not json").is_empty());
        assert!(parse_skills_sh_paths(r#"{"paths":["../esc"]}"#).is_empty());
    }
}
