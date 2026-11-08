//! Update-source derivation, ported from `src/update-source.ts`.

use skills_store::locks::LocalSkillLockEntry;

/// `url` with `#ref` appended.
#[must_use]
pub fn format_source_input(source_url: &str, ref_: Option<&str>) -> String {
    match ref_ {
        Some(ref_) => format!("{source_url}#{ref_}"),
        None => source_url.to_string(),
    }
}

/// Derive the skill's folder path from a SKILL.md-terminated skillPath.
fn derive_skill_folder(skill_path: &str) -> String {
    let mut folder = skill_path.to_string();
    if folder.ends_with("/SKILL.md") {
        folder.truncate(folder.len() - 9);
    } else if folder.ends_with("SKILL.md") {
        folder.truncate(folder.len() - 8);
    }
    if folder.ends_with('/') {
        folder.pop();
    }
    folder
}

/// Whether a skill folder can be appended to the source as a subpath.
fn supports_appended_subpath(source: &str) -> bool {
    if source.starts_with("git@") || source.starts_with("ssh://") {
        return false;
    }
    if source.ends_with(".git") {
        return false;
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return crate::source_parser::host_and_path(source)
            .map(|(host, _)| host == "github.com" || host == "gitlab.com")
            .unwrap_or(false);
    }
    true
}

fn is_bare_shorthand(source: &str) -> bool {
    !source.contains(':') && !source.starts_with('.') && !source.starts_with('/')
}

fn get_local_source(entry: &LocalSkillLockEntry) -> Option<String> {
    if let Some(source_url) = &entry.source_url {
        return Some(source_url.clone());
    }
    let requires_source_url = entry.source_type == "git" || entry.source_type == "gitlab";
    if requires_source_url && is_bare_shorthand(&entry.source) {
        return None;
    }
    Some(entry.source.clone())
}

/// Build a cloneable repository URL for project update source checks.
#[must_use]
pub fn build_local_clone_source(entry: &LocalSkillLockEntry) -> Option<String> {
    let source = get_local_source(entry)?;
    if entry.source_type == "github" && is_bare_shorthand(&source) {
        return Some(format!(
            "https://github.com/{}.git",
            source.trim_end_matches(".git")
        ));
    }
    Some(source)
}

/// Whether an update needs full-depth discovery (unappendable subpaths).
#[must_use]
pub fn should_use_full_depth_for_update(entry: &LocalSkillLockEntry) -> bool {
    let Some(_skill_path) = &entry.skill_path else {
        return false;
    };
    let source = if !entry.source_type.is_empty() && entry.source_type != "github" {
        get_local_source(entry)
    } else {
        Some(entry.source.clone())
    };
    source.is_some_and(|source| !supports_appended_subpath(&source))
}

fn append_folder_and_ref(source: &str, skill_path: &str, ref_: Option<&str>) -> String {
    if !supports_appended_subpath(source) {
        return format_source_input(source, ref_);
    }
    let folder = derive_skill_folder(skill_path);
    let with_folder = if folder.is_empty() {
        source.to_string()
    } else {
        format!("{source}/{folder}")
    };
    match ref_ {
        Some(ref_) => format!("{with_folder}#{ref_}"),
        None => with_folder,
    }
}

/// Build the `skills add` source argument for a global-lock update.
#[must_use]
pub fn build_update_install_source(entry: &LocalSkillLockEntry) -> Option<String> {
    let Some(skill_path) = &entry.skill_path else {
        let source = if !entry.source_type.is_empty() && entry.source_type != "github" {
            get_local_source(entry)
        } else {
            entry
                .source_url
                .clone()
                .or_else(|| Some(entry.source.clone()))
        }?;
        return Some(format_source_input(&source, entry.ref_.as_deref()));
    };
    let source = if !entry.source_type.is_empty() && entry.source_type != "github" {
        get_local_source(entry)
    } else {
        Some(entry.source.clone())
    }?;
    Some(append_folder_and_ref(
        &source,
        skill_path,
        entry.ref_.as_deref(),
    ))
}

/// Build the `skills add` source argument for a project-lock update.
#[must_use]
pub fn build_local_update_source(entry: &LocalSkillLockEntry) -> Option<String> {
    let source = get_local_source(entry)?;
    match &entry.skill_path {
        None => Some(format_source_input(&source, entry.ref_.as_deref())),
        Some(skill_path) => Some(append_folder_and_ref(
            &source,
            skill_path,
            entry.ref_.as_deref(),
        )),
    }
}

/// Build the `skills add` source argument from a global-lock entry.
#[must_use]
pub fn build_global_update_install_source(
    entry: &skills_store::locks::SkillLockEntry,
) -> Option<String> {
    let local = LocalSkillLockEntry {
        source: entry.source.clone(),
        source_url: None,
        ref_: entry.ref_.clone(),
        source_type: entry.source_type.clone(),
        skill_path: entry.skill_path.clone(),
        version: None,
        via: None,
        computed_hash: String::new(),
        subagents: None,
        well_known_digest: None,
        install_mode: entry.install_mode.clone(),
    };
    // Global entries always carry the original URL in `source_url`; use it
    // verbatim for sources where the shorthand would be ambiguous.
    if entry.source_url.starts_with("git@") || entry.source_url.starts_with("ssh://") {
        return Some(format_source_input(
            &entry.source_url,
            entry.ref_.as_deref(),
        ));
    }
    let mut out = build_update_install_source(&local)?;
    if out.is_empty() {
        out = format_source_input(&entry.source_url, entry.ref_.as_deref());
    }
    Some(out)
}

/// Whether a global-lock update needs full-depth discovery.
#[must_use]
pub fn should_use_full_depth_for_global(entry: &skills_store::locks::SkillLockEntry) -> bool {
    let local = LocalSkillLockEntry {
        source: entry.source.clone(),
        source_url: None,
        ref_: entry.ref_.clone(),
        source_type: entry.source_type.clone(),
        skill_path: entry.skill_path.clone(),
        version: None,
        via: None,
        computed_hash: String::new(),
        subagents: None,
        well_known_digest: None,
        install_mode: entry.install_mode.clone(),
    };
    should_use_full_depth_for_update(&local)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, source_type: &str, skill_path: Option<&str>) -> LocalSkillLockEntry {
        LocalSkillLockEntry {
            source: source.to_string(),
            source_url: None,
            ref_: None,
            source_type: source_type.to_string(),
            skill_path: skill_path.map(str::to_string),
            version: None,
            via: None,
            computed_hash: String::new(),
            subagents: None,
            well_known_digest: None,
            install_mode: None,
        }
    }

    #[test]
    fn shorthand_with_skill_path_appends_folder() {
        let e = entry("owner/repo", "github", Some("skills/web/SKILL.md"));
        assert_eq!(
            build_update_install_source(&e).as_deref(),
            Some("owner/repo/skills/web")
        );
    }

    #[test]
    fn generic_git_requires_source_url() {
        let e = entry("owner/repo", "git", Some("skills/web/SKILL.md"));
        assert_eq!(build_update_install_source(&e), None);
        assert_eq!(build_local_clone_source(&e), None);
    }

    #[test]
    fn ssh_sources_use_hash_refs() {
        let mut e = entry(
            "git@github.com:owner/repo.git",
            "git",
            Some("skills/web/SKILL.md"),
        );
        e.source_url = Some("git@github.com:owner/repo.git".to_string());
        e.ref_ = Some("main".to_string());
        // Unappendable sources fall back to the URL plus a `#ref` fragment.
        assert_eq!(
            build_update_install_source(&e).as_deref(),
            Some("git@github.com:owner/repo.git#main")
        );
    }
}
