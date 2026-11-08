use std::path::Path;

use skills_store::error::SkillsError;
use skills_store::installer::{self, InstallOptions, InstallOutcome};
use skills_store::registry::AgentConfig;
use skills_store::sanitize::normalize_skill_name;
use skills_store::skills::Skill;

use crate::resolver::ResolvedSkills;

#[derive(Clone)]
pub(crate) enum SkillPayload {
    Disk(Skill),
    Blob(crate::blob::BlobSkill),
    WellKnown(crate::wellknown::WellKnownSkill),
}

impl SkillPayload {
    pub(crate) fn from_resolved(resolved: &ResolvedSkills) -> Vec<Self> {
        if let Some(blob) = &resolved.blob_result {
            blob.skills.iter().cloned().map(Self::Blob).collect()
        } else {
            resolved
                .discovery
                .skills
                .iter()
                .cloned()
                .map(Self::Disk)
                .collect()
        }
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Disk(skill) => &skill.name,
            Self::Blob(skill) => &skill.name,
            Self::WellKnown(skill) => &skill.install_name,
        }
    }

    pub(crate) fn display_name(&self) -> String {
        match self {
            Self::WellKnown(skill) => skill.name.clone(),
            _ => self.name().to_string(),
        }
    }

    pub(crate) fn description(&self) -> String {
        match self {
            Self::Disk(skill) => skill.description.clone(),
            Self::Blob(skill) => skill.description.clone(),
            Self::WellKnown(skill) => skill.description.clone(),
        }
    }

    pub(crate) fn plugin_name(&self) -> Option<String> {
        match self {
            Self::Disk(skill) => skill.plugin_name.clone(),
            _ => None,
        }
    }

    pub(crate) fn source_path(&self, root: Option<&Path>) -> Option<String> {
        match self {
            Self::Disk(skill) => {
                let path = skill.path.strip_prefix(root?).ok()?.join("SKILL.md");
                Some(path.to_string_lossy().replace('\\', "/"))
            }
            Self::Blob(skill) => Some(skill.repo_path.clone()),
            Self::WellKnown(_) => None,
        }
    }

    pub(crate) fn content_hash(&self) -> Result<String, SkillsError> {
        match self {
            Self::Disk(skill) => skills_store::locks::compute_folder_hash(&skill.path),
            Self::Blob(skill) => Ok(skill.snapshot_hash.clone()),
            Self::WellKnown(skill) => Ok(crate::wellknown::compute_well_known_skill_digest(skill)
                .trim_start_matches("sha256:")
                .to_string()),
        }
    }

    pub(crate) fn install(
        &self,
        name: &str,
        agent: &AgentConfig,
        options: &InstallOptions,
        warnings: &mut Vec<SkillsError>,
    ) -> InstallOutcome {
        match self {
            Self::Disk(skill) => {
                installer::install_named_skill_dir(name, &skill.path, agent, options, warnings)
            }
            Self::Blob(skill) => {
                installer::install_skill_snapshot(name, &skill.files, agent, options, warnings)
            }
            Self::WellKnown(skill) => {
                let files = skill
                    .files
                    .iter()
                    .map(|(path, content)| (path.clone(), content.as_bytes().to_vec()))
                    .collect::<Vec<_>>();
                installer::install_skill_files(name, &files, agent, options, warnings)
            }
        }
    }
}

pub(crate) fn find_locked<'a>(
    payloads: &'a [SkillPayload],
    root: Option<&Path>,
    name: &str,
    path: Option<&str>,
) -> Result<&'a SkillPayload, SkillsError> {
    if let Some(path) = path {
        let path = path.replace('\\', "/");
        if let Some(payload) = payloads
            .iter()
            .find(|payload| payload.source_path(root).as_deref() == Some(path.as_str()))
        {
            return Ok(payload);
        }
    }
    let normalized = normalize_skill_name(name);
    let mut matches = payloads.iter().filter(|payload| {
        normalize_skill_name(payload.name()) == normalized
            || matches!(payload, SkillPayload::Disk(skill) if skill.path.file_name().is_some_and(|folder| normalize_skill_name(&folder.to_string_lossy()) == normalized))
    });
    match (matches.next(), matches.next()) {
        (Some(payload), None) => Ok(payload),
        (None, _) => Err(SkillsError::new(
            "SKILLS_SOURCE_FETCH_FAILED",
            format!("{name}: skill not found in the recorded source"),
        )),
        _ => Err(SkillsError::new(
            "SKILLS_SOURCE_FETCH_FAILED",
            format!("{name}: several skills match the lock entry"),
        )),
    }
}
