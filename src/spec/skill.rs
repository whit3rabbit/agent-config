//! Skill spec, builder, frontmatter, and supporting asset type.

use std::path::PathBuf;

use crate::error::AgentConfigError;

use super::validate::{validate_identifier, IdentifierKind};

/// Caller-supplied description of an agent skill to install.
///
/// Skills are directory-scoped: each one occupies a subdirectory under the
/// harness's `skills/` root, with a required `SKILL.md` and any number of
/// supporting files in `scripts/`, `references/`, and `assets/`.
///
/// Build via [`SkillSpec::builder`].
#[derive(Debug, Clone)]
pub struct SkillSpec {
    /// Skill directory name. Becomes the folder under the harness's
    /// `skills/` root. Must be lowercase kebab-case, max 64 chars.
    pub name: String,

    /// The consumer of this library that owns the skill. Recorded in the
    /// sidecar ownership ledger; refusal to remove a skill owned by another
    /// consumer matches the same `NotOwnedByCaller` model used for MCP.
    pub owner_tag: String,

    /// YAML frontmatter required by Claude / Antigravity. Written verbatim
    /// at the head of `SKILL.md`.
    pub frontmatter: SkillFrontmatter,

    /// Markdown body of `SKILL.md` (no frontmatter — that is rendered from
    /// [`SkillSpec::frontmatter`]).
    pub body: String,

    /// Optional supporting files under `scripts/`, `references/`, `assets/`.
    /// The `relative_path` is interpreted under the skill directory; any
    /// leading `scripts/`/`references/`/`assets/` prefix is honoured as-is.
    pub assets: Vec<SkillAsset>,
}

impl SkillSpec {
    /// Start building a skill spec.
    pub fn builder(name: impl Into<String>) -> SkillSpecBuilder {
        let name = name.into();
        SkillSpecBuilder {
            name: name.clone(),
            owner_tag: None,
            frontmatter: SkillFrontmatter {
                name,
                description: String::new(),
                allowed_tools: None,
            },
            body: String::new(),
            assets: Vec::new(),
        }
    }

    /// Validate `name` and `owner_tag`.
    pub(crate) fn validate(&self) -> Result<(), AgentConfigError> {
        Self::validate_name(&self.name)?;
        if self.frontmatter.description.trim().is_empty() {
            return Err(AgentConfigError::MissingSpecField {
                id: "<skill spec>",
                field: "frontmatter.description",
            });
        }
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)
    }

    /// Validate just the skill name (used by uninstall, which has no spec).
    pub(crate) fn validate_name(name: &str) -> Result<(), AgentConfigError> {
        validate_identifier(name, IdentifierKind::SkillName)
    }
}

/// YAML frontmatter prepended to `SKILL.md`. `description` is the field
/// harnesses use to decide when to activate the skill.
#[derive(Debug, Clone)]
pub struct SkillFrontmatter {
    /// Skill identifier surfaced in tooling (typically matches the directory
    /// name).
    pub name: String,

    /// Sentence (or short paragraph) explaining when the skill should
    /// activate. Required by both Claude and Antigravity; the activation
    /// model matches against this string.
    pub description: String,

    /// Optional `allowed-tools` list (Claude). When `None`, the field is
    /// omitted from the frontmatter.
    pub allowed_tools: Option<Vec<String>>,
}

/// One supporting file inside a skill directory.
#[derive(Debug, Clone)]
pub struct SkillAsset {
    /// Path relative to the skill directory, e.g.
    /// `PathBuf::from("scripts/run.sh")`. Must be a relative path; absolute
    /// paths or `..` segments are rejected at install time.
    pub relative_path: PathBuf,

    /// Raw bytes of the file. Lets callers ship binary references (e.g.
    /// images under `assets/`) as well as text scripts.
    pub bytes: Vec<u8>,

    /// On Unix, set the file mode to `0o755` after writing. No-op on
    /// Windows. Use for shell/python scripts under `scripts/`.
    pub executable: bool,
}

/// Builder for [`SkillSpec`].
#[derive(Debug, Clone)]
pub struct SkillSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    frontmatter: SkillFrontmatter,
    body: String,
    assets: Vec<SkillAsset>,
}

impl SkillSpecBuilder {
    /// Set the consumer's owner tag.
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Set the SKILL.md frontmatter `description`.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.frontmatter.description = d.into();
        self
    }

    /// Set the SKILL.md frontmatter `allowed-tools` list (Claude only).
    pub fn allowed_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.frontmatter.allowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Set the markdown body of SKILL.md.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Add one supporting file.
    pub fn asset(mut self, asset: SkillAsset) -> Self {
        self.assets.push(asset);
        self
    }

    /// Finalize the spec.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `description` were never set.
    pub fn build(self) -> SkillSpec {
        self.try_build().expect("SkillSpec missing required field")
    }

    /// Fallible variant of [`build`](Self::build).
    pub fn try_build(self) -> Result<SkillSpec, AgentConfigError> {
        let owner_tag = self.owner_tag.ok_or(AgentConfigError::MissingSpecField {
            id: "<skill builder>",
            field: "owner",
        })?;
        let spec = SkillSpec {
            name: self.name,
            owner_tag,
            frontmatter: self.frontmatter,
            body: self.body,
            assets: self.assets,
        };
        spec.validate()?;
        Ok(spec)
    }
}
