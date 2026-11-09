//! Source resolution: turn a [`ParsedSource`] into discoverable skills on
//! disk or in memory (upstream `resolveSkills` + `fetchSource` in add.ts).

use std::path::PathBuf;

use skills_store::error::{diag, SkillsError};
use skills_store::skills::{discover_skills, DiscoverOptions, Discovery, Skill};

use crate::blob;
use crate::download::download_source;
use crate::git;

/// A resolved source: skills + their materialization.
#[allow(dead_code)] // temp_dir/root_dir/commit feed lock entries in longer flows
pub struct ResolvedSkills {
    /// Discovered skills.
    pub discovery: Discovery,
    /// Blob result when the fast path succeeded.
    pub blob_result: Option<blob::BlobInstallResult>,
    /// Temp dir cleaned up when dropped.
    _temp_dir: Option<TempClone>,
    /// Directory skills were discovered in (the source root).
    pub root_dir: Option<PathBuf>,
    /// Commit checked out for git sources.
    pub commit: Option<String>,
}

/// A temp clone directory removed on drop (guard type so partial moves work).
pub struct TempClone(pub PathBuf);

impl Drop for TempClone {
    fn drop(&mut self) {
        let _ = git::cleanup_temp_dir(&self.0);
    }
}

/// Whether a requested skill name matched nothing (drives full-depth retry).
fn missing_requested(discovery: &Discovery, requested: &[String]) -> bool {
    requested.iter().any(|name| {
        name != "*"
            && skills_store::skills::filter_skills(&discovery.skills, std::slice::from_ref(name))
                .is_empty()
    })
}

/// Resolve a parsed source (upstream `resolveSkills`).
pub fn resolve_skills(
    parsed: &crate::source_parser::ParsedSource,
    options: &ResolveOptions,
    spinner: Option<&crate::prompt::Spinner>,
) -> Result<ResolvedSkills, SkillsError> {
    use crate::source_parser::SourceKind;
    #[allow(unused_assignments)]
    let mut discovery: Option<Discovery> = None;
    let mut blob_result: Option<blob::BlobInstallResult> = None;
    let mut temp_dir: Option<TempClone> = None;
    let mut root_dir: Option<PathBuf> = None;
    let mut commit: Option<String> = None;

    let spinner_message = |spinner: Option<&crate::prompt::Spinner>, message: &str| {
        if let Some(spinner) = spinner {
            spinner.message(message);
        }
    };

    match parsed.kind {
        SourceKind::Local => {
            let local_path = parsed
                .local_path
                .clone()
                .unwrap_or_else(|| PathBuf::from(&parsed.url));
            spinner_message(spinner, "Discovering skills…");
            root_dir = Some(local_path.clone());
            discovery = Some(discover(
                local_path,
                parsed.subpath.as_deref(),
                options,
                &mut missing_retry(parsed),
            )?);
        }
        SourceKind::WellKnown | SourceKind::Download => {
            spinner_message(spinner, "Downloading source...");
            let downloaded = download_source(&parsed.url)?;
            temp_dir = Some(TempClone(downloaded.temp_dir));
            root_dir = Some(downloaded.root_dir.clone());
            spinner_message(
                spinner,
                if downloaded.kind == "skill-md" {
                    "Downloaded SKILL.md file"
                } else {
                    "Downloaded archive"
                },
            );
            spinner_message(spinner, "Discovering skills...");
            discovery = Some(discover(
                downloaded.root_dir.clone(),
                None,
                options,
                &mut missing_retry(parsed),
            )?);
        }
        SourceKind::GitHub if !options.full_depth => {
            let owner_repo = crate::source_parser::get_owner_repo(parsed);
            let owner = owner_repo
                .as_ref()
                .and_then(|or| or.split('/').next())
                .map(str::to_lowercase);
            const BLOB_ALLOWED_OWNERS: [&str; 4] =
                ["vercel", "vercel-labs", "heygen-com", "remotion-dev"];
            if let (Some(owner_repo), Some(owner)) = (&owner_repo, owner) {
                if BLOB_ALLOWED_OWNERS.contains(&owner.as_str()) {
                    spinner_message(spinner, "Fetching skills…");
                    blob_result = blob::try_blob_install(
                        owner_repo,
                        parsed.subpath.as_deref(),
                        select_filter(parsed, options).as_deref(),
                        parsed.ref_.as_deref(),
                        options.include_internal,
                    );
                    if let Some(blob) = &blob_result {
                        if missing_blob(blob, parsed, options) {
                            blob_result = None;
                        }
                    }
                }
            }
            if let Some(blob) = &blob_result {
                discovery = Some(blob_to_discovery(blob, options));
                spinner_message(spinner, &format!("Found {} skill(s)", blob.skills.len()));
            } else {
                spinner_message(spinner, "Cloning repository…");
                let checkout = git::clone_repo(&parsed.url, parsed.ref_.as_deref())?;
                temp_dir = Some(TempClone(checkout.clone()));
                commit = git::get_head_commit(&checkout);
                root_dir = Some(checkout.clone());
                spinner_message(spinner, "Repository cloned");
                spinner_message(spinner, "Discovering skills…");
                discovery = Some(discover(
                    checkout,
                    parsed.subpath.as_deref(),
                    options,
                    &mut missing_retry(parsed),
                )?);
            }
        }
        SourceKind::GitHub | SourceKind::GitLab | SourceKind::Git => {
            spinner_message(spinner, "Cloning repository…");
            let checkout = git::clone_repo(&parsed.url, parsed.ref_.as_deref())?;
            temp_dir = Some(TempClone(checkout.clone()));
            commit = git::get_head_commit(&checkout);
            root_dir = Some(checkout.clone());
            spinner_message(spinner, "Repository cloned");
            spinner_message(spinner, "Discovering skills…");
            discovery = Some(discover(
                checkout,
                parsed.subpath.as_deref(),
                options,
                &mut missing_retry(parsed),
            )?);
        }
    }

    let discovery = discovery.unwrap_or_default();
    Ok(ResolvedSkills {
        discovery,
        blob_result,
        _temp_dir: temp_dir,
        root_dir,
        commit,
    })
}

fn missing_retry(
    parsed: &crate::source_parser::ParsedSource,
) -> impl FnMut(&Discovery) -> bool + '_ {
    let requested: Vec<String> = parsed.skill_filter.clone().into_iter().collect();
    move |discovery: &Discovery| !requested.is_empty() && missing_requested(discovery, &requested)
}

fn missing_blob(
    blob: &blob::BlobInstallResult,
    parsed: &crate::source_parser::ParsedSource,
    options: &ResolveOptions,
) -> bool {
    if let Some(filter) = select_filter(parsed, options) {
        let filter_slug = skills_store::sanitize::to_skill_slug(&filter);
        let matches = blob
            .skills
            .iter()
            .any(|skill| skills_store::sanitize::to_skill_slug(&skill.name) == filter_slug);
        if !matches {
            return true;
        }
    }
    false
}

fn select_filter(
    parsed: &crate::source_parser::ParsedSource,
    options: &ResolveOptions,
) -> Option<String> {
    parsed
        .skill_filter
        .clone()
        .or_else(|| options.skill_filter.clone())
}

fn discover(
    root: PathBuf,
    subpath: Option<&str>,
    options: &ResolveOptions,
    needs_full_depth: &mut dyn FnMut(&Discovery) -> bool,
) -> Result<Discovery, SkillsError> {
    let discover_options = DiscoverOptions {
        include_internal: options.include_internal,
        full_depth: options.full_depth,
        include_duplicate_names: options.include_duplicate_names,
    };
    let found = discover_skills(&root, subpath, &discover_options)?;
    if !options.full_depth && needs_full_depth(&found) {
        let deep = DiscoverOptions {
            full_depth: true,
            ..discover_options
        };
        return discover_skills(&root, subpath, &deep);
    }
    Ok(found)
}

fn blob_to_discovery(blob: &blob::BlobInstallResult, options: &ResolveOptions) -> Discovery {
    let skills = blob
        .skills
        .iter()
        .filter(|skill| {
            options
                .skill_filter
                .as_ref()
                .map(|filter| {
                    skills_store::sanitize::to_skill_slug(&skill.name)
                        == skills_store::sanitize::to_skill_slug(filter)
                })
                .unwrap_or(true)
        })
        .map(|skill| Skill {
            name: skill.name.clone(),
            description: skill.description.clone(),
            path: PathBuf::new(),
            raw_content: Some(skill.raw_content.clone()),
            plugin_name: None,
            metadata: skill.metadata.clone(),
        })
        .collect();
    Discovery {
        skills,
        warnings: Vec::new(),
    }
}

/// Options for [`resolve_skills`].
#[derive(Debug, Clone, Default)]
pub struct ResolveOptions {
    /// Include `metadata.internal` skills.
    pub include_internal: bool,
    /// Search the whole tree.
    pub full_depth: bool,
    /// Keep duplicate skill names.
    pub include_duplicate_names: bool,
    /// Skill-name filter applied to blob results.
    pub skill_filter: Option<String>,
}

/// The "no skills" diagnostic.
#[allow(dead_code)]
#[must_use]
pub fn no_valid_skills_error() -> SkillsError {
    diag::no_valid_skills()
}
