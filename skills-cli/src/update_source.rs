//! Resolve lockfile provenance without rebuilding a command-line source string.

use skills_store::error::SkillsError;
use skills_store::locks::LocalSkillLockEntry;

use crate::resolver::{self, ResolveOptions, ResolvedSkills};
use crate::source_parser::{self, SourceKind};

pub(crate) fn local_source(entry: &LocalSkillLockEntry) -> Result<&str, SkillsError> {
    if let Some(url) = &entry.source_url {
        return Ok(url);
    }
    let source = entry.source.as_str();
    let shorthand = !source.contains(':') && !source.starts_with(['.', '/']);
    if matches!(entry.source_type.as_str(), "git" | "gitlab") && shorthand {
        return Err(skills_store::error::diag::lock_missing_source_url(source));
    }
    Ok(source)
}

pub(crate) fn resolve(
    source: &str,
    kind: &str,
    git_ref: Option<&str>,
) -> Result<ResolvedSkills, SkillsError> {
    let mut parsed = if kind == "github" {
        source_parser::parse_source_with_host(source, "github.com")
    } else {
        source_parser::parse_source(source)
    };
    if let Some(git_ref) = git_ref {
        parsed.ref_ = Some(git_ref.to_string());
    }
    match kind {
        "git" => parsed.kind = SourceKind::Git,
        "gitlab" => parsed.kind = SourceKind::GitLab,
        "download" | "well-known" => {
            parsed.url = source.to_string();
            parsed.kind = SourceKind::Download;
        }
        _ => {}
    }
    // The lock path identifies the skill independently of how it was discovered.
    parsed.subpath = None;
    parsed.skill_filter = None;
    resolver::resolve_skills(
        &parsed,
        &ResolveOptions {
            full_depth: true,
            include_internal: true,
            include_duplicate_names: true,
            ..Default::default()
        },
        None,
    )
}
