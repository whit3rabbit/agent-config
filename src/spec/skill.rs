//! Skill spec, builder, frontmatter, and supporting asset type.

use std::path::PathBuf;

use crate::error::AgentConfigError;

use super::validate::{validate_identifier, IdentifierKind};

// User-supplied frontmatter strings are rendered into single-line YAML at the
// top of `SKILL.md`. Reject newlines, tabs, C0 control bytes, and DEL up
// front so the renderer never has to choose between mangling input and
// emitting broken YAML.
fn validate_frontmatter_scalar(value: &str) -> Result<(), AgentConfigError> {
    for c in value.chars() {
        if c == '\n' || c == '\r' || c == '\t' {
            return Err(AgentConfigError::InvalidTag {
                tag: value.to_string(),
                reason: "skill frontmatter must not contain newlines or tabs",
            });
        }
        if (c as u32) < 0x20 && c != ' ' {
            return Err(AgentConfigError::InvalidTag {
                tag: value.to_string(),
                reason: "skill frontmatter must not contain control characters",
            });
        }
        if c == '\u{007F}' {
            return Err(AgentConfigError::InvalidTag {
                tag: value.to_string(),
                reason: "skill frontmatter must not contain DEL (0x7F)",
            });
        }
    }
    Ok(())
}

fn validate_optional_frontmatter_scalar(value: Option<&String>) -> Result<(), AgentConfigError> {
    if let Some(value) = value {
        validate_frontmatter_scalar(value)?;
    }
    Ok(())
}

fn validate_optional_frontmatter_list(
    values: Option<&Vec<String>>,
) -> Result<(), AgentConfigError> {
    if let Some(values) = values {
        for value in values {
            validate_frontmatter_scalar(value)?;
        }
    }
    Ok(())
}

/// Claude skill effort override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillEffort {
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra-high reasoning effort.
    XHigh,
    /// Maximum reasoning effort.
    Max,
}

impl SkillEffort {
    pub(crate) const fn as_yaml(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// Claude skill context mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillContext {
    /// Run the skill in a forked subagent context.
    Fork,
}

impl SkillContext {
    pub(crate) const fn as_yaml(self) -> &'static str {
        match self {
            Self::Fork => "fork",
        }
    }
}

/// Claude skill shell for inline shell snippets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillShell {
    /// Use Bash for inline shell commands.
    Bash,
    /// Use PowerShell for inline shell commands.
    PowerShell,
}

impl SkillShell {
    pub(crate) const fn as_yaml(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::PowerShell => "powershell",
        }
    }
}

/// Caller-supplied description of an agent skill to install.
///
/// Skills are directory-scoped: each one occupies a subdirectory under the
/// harness's `skills/` root, with a required `SKILL.md` and any number of
/// supporting files in `scripts/`, `references/`, and `assets/`.
///
/// Build via [`SkillSpec::builder`]. For fallible construction see
/// [`SkillSpecBuilder::try_build`].
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

    /// When true, install adopts a skill directory that exists on disk but
    /// has no recorded owner instead of refusing. Use after a crash between
    /// skill-directory write and ledger record. Default `false`.
    pub adopt_unowned: bool,
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
                when_to_use: None,
                argument_hint: None,
                arguments: None,
                disable_model_invocation: None,
                user_invocable: None,
                allowed_tools: None,
                disallowed_tools: None,
                model: None,
                effort: None,
                context: None,
                agent: None,
                paths: None,
                shell: None,
            },
            body: String::new(),
            assets: Vec::new(),
            adopt_unowned: false,
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
        if self.body.trim().is_empty() {
            return Err(AgentConfigError::MissingSpecField {
                id: "<skill spec>",
                field: "body",
            });
        }
        validate_frontmatter_scalar(&self.frontmatter.name)?;
        validate_frontmatter_scalar(&self.frontmatter.description)?;
        validate_optional_frontmatter_scalar(self.frontmatter.when_to_use.as_ref())?;
        validate_optional_frontmatter_scalar(self.frontmatter.argument_hint.as_ref())?;
        validate_optional_frontmatter_list(self.frontmatter.arguments.as_ref())?;
        validate_optional_frontmatter_list(self.frontmatter.allowed_tools.as_ref())?;
        validate_optional_frontmatter_list(self.frontmatter.disallowed_tools.as_ref())?;
        validate_optional_frontmatter_scalar(self.frontmatter.model.as_ref())?;
        validate_optional_frontmatter_scalar(self.frontmatter.agent.as_ref())?;
        validate_optional_frontmatter_list(self.frontmatter.paths.as_ref())?;
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)
    }

    /// Validate just the skill name (used by uninstall, which has no spec).
    pub(crate) fn validate_name(name: &str) -> Result<(), AgentConfigError> {
        validate_identifier(name, IdentifierKind::SkillName)
    }
}

/// YAML frontmatter prepended to `SKILL.md`.
///
/// `description` remains required by this crate for cross-harness safety,
/// even though Claude can infer it in some cases.
#[derive(Debug, Clone)]
pub struct SkillFrontmatter {
    /// Skill identifier surfaced in tooling (typically matches the directory
    /// name).
    pub name: String,

    /// Sentence (or short paragraph) explaining when the skill should
    /// activate. Required by both Claude and Antigravity; the activation
    /// model matches against this string.
    pub description: String,

    /// Optional Claude `when_to_use` field.
    pub when_to_use: Option<String>,

    /// Optional Claude `argument-hint` field.
    pub argument_hint: Option<String>,

    /// Optional Claude `arguments` list.
    pub arguments: Option<Vec<String>>,

    /// Optional Claude `disable-model-invocation` field.
    pub disable_model_invocation: Option<bool>,

    /// Optional Claude `user-invocable` field.
    pub user_invocable: Option<bool>,

    /// Optional `allowed-tools` list (Claude). When `None`, the field is
    /// omitted from the frontmatter.
    pub allowed_tools: Option<Vec<String>>,

    /// Optional Claude `disallowed-tools` list.
    pub disallowed_tools: Option<Vec<String>>,

    /// Optional Claude `model` override.
    pub model: Option<String>,

    /// Optional Claude `effort` override.
    pub effort: Option<SkillEffort>,

    /// Optional Claude `context` mode.
    pub context: Option<SkillContext>,

    /// Optional Claude `agent` to use with forked context.
    pub agent: Option<String>,

    /// Optional Claude `paths` list.
    pub paths: Option<Vec<String>>,

    /// Optional Claude `shell` override.
    pub shell: Option<SkillShell>,
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
    adopt_unowned: bool,
}

impl SkillSpecBuilder {
    /// Set the consumer's owner tag.
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Adopt a skill directory that exists on disk but has no recorded owner.
    /// See [`SkillSpec::adopt_unowned`].
    pub fn adopt_unowned(mut self, adopt: bool) -> Self {
        self.adopt_unowned = adopt;
        self
    }

    /// Set the SKILL.md frontmatter `description`.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.frontmatter.description = d.into();
        self
    }

    /// Set the SKILL.md frontmatter `when_to_use` field.
    pub fn when_to_use(mut self, value: impl Into<String>) -> Self {
        self.frontmatter.when_to_use = Some(value.into());
        self
    }

    /// Set the SKILL.md frontmatter `argument-hint` field.
    pub fn argument_hint(mut self, value: impl Into<String>) -> Self {
        self.frontmatter.argument_hint = Some(value.into());
        self
    }

    /// Set the SKILL.md frontmatter `arguments` list.
    pub fn arguments<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.frontmatter.arguments = Some(arguments.into_iter().map(Into::into).collect());
        self
    }

    /// Set the SKILL.md frontmatter `disable-model-invocation` field.
    pub fn disable_model_invocation(mut self, disable: bool) -> Self {
        self.frontmatter.disable_model_invocation = Some(disable);
        self
    }

    /// Set the SKILL.md frontmatter `user-invocable` field.
    pub fn user_invocable(mut self, invocable: bool) -> Self {
        self.frontmatter.user_invocable = Some(invocable);
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

    /// Set the SKILL.md frontmatter `disallowed-tools` list.
    pub fn disallowed_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.frontmatter.disallowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Set the SKILL.md frontmatter `model` override.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.frontmatter.model = Some(model.into());
        self
    }

    /// Set the SKILL.md frontmatter `effort` override.
    pub fn effort(mut self, effort: SkillEffort) -> Self {
        self.frontmatter.effort = Some(effort);
        self
    }

    /// Set the SKILL.md frontmatter `context` mode.
    pub fn context(mut self, context: SkillContext) -> Self {
        self.frontmatter.context = Some(context);
        self
    }

    /// Set the SKILL.md frontmatter `agent` field.
    pub fn agent(mut self, agent: impl Into<String>) -> Self {
        self.frontmatter.agent = Some(agent.into());
        self
    }

    /// Set the SKILL.md frontmatter `paths` list.
    pub fn paths<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.frontmatter.paths = Some(paths.into_iter().map(Into::into).collect());
        self
    }

    /// Set the SKILL.md frontmatter `shell` override.
    pub fn shell(mut self, shell: SkillShell) -> Self {
        self.frontmatter.shell = Some(shell);
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

    /// Finalize the spec, panicking on missing or invalid fields.
    ///
    /// Convenience wrapper around [`try_build()`](Self::try_build) for tests
    /// and examples. Production code should prefer [`try_build()`](Self::try_build)
    /// to propagate errors instead of panicking.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `description` were never set.
    pub fn build(self) -> SkillSpec {
        self.try_build().expect("SkillSpec missing required field")
    }

    /// Finalize the spec, returning [`Result`] on missing or invalid fields.
    ///
    /// This is the recommended way to build a spec in production code.
    /// See [crate-level documentation](crate#production-usage) for a full example.
    ///
    /// # Errors
    ///
    /// - [`AgentConfigError::MissingSpecField`] with `field = "owner"` when
    ///   [`SkillSpecBuilder::owner`] was never called,
    ///   `field = "frontmatter.description"` when the skill frontmatter
    ///   description is empty, or `field = "body"` when the body is empty.
    /// - [`AgentConfigError::InvalidTag`] when `name` violates the kebab-case
    ///   skill-name contract or `owner_tag` is malformed, or when a
    ///   frontmatter scalar contains a control character.
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
            adopt_unowned: self.adopt_unowned,
        };
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_newline_in_description() {
        let err = SkillSpec::builder("alpha")
            .owner("appA")
            .description("line1\nline2")
            .body("body")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn validate_rejects_tab_in_name() {
        // The frontmatter.name defaults to the SkillSpec name; build via
        // try_build and inject the tab through the frontmatter directly,
        // because the builder routes the SkillSpec name through the
        // kebab-case validator first.
        let mut spec = SkillSpec::builder("alpha")
            .owner("appA")
            .description("ok")
            .body("body")
            .try_build()
            .expect("base spec valid");
        spec.frontmatter.name = "bad\tname".into();
        let err = spec.validate().unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn validate_rejects_control_char_in_allowed_tools() {
        let err = SkillSpec::builder("alpha")
            .owner("appA")
            .description("ok")
            .body("body")
            .allowed_tools(["ed\u{0001}it"])
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn validate_rejects_control_char_in_current_frontmatter_fields() {
        let newline = SkillSpec::builder("alpha")
            .owner("appA")
            .description("ok")
            .when_to_use("line1\nline2")
            .body("body")
            .try_build()
            .unwrap_err();
        assert!(matches!(newline, AgentConfigError::InvalidTag { .. }));

        let tab = SkillSpec::builder("alpha")
            .owner("appA")
            .description("ok")
            .disallowed_tools(["Write\tFile"])
            .body("body")
            .try_build()
            .unwrap_err();
        assert!(matches!(tab, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn validate_rejects_del_in_description() {
        let err = SkillSpec::builder("alpha")
            .owner("appA")
            .description("evil\u{007F}")
            .body("body")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn validate_accepts_normal_description_with_punctuation() {
        SkillSpec::builder("alpha")
            .owner("appA")
            .description("Format Git commit messages: subject + body.")
            .body("body")
            .try_build()
            .expect("valid");
    }

    #[test]
    fn validate_rejects_empty_body() {
        let err = SkillSpec::builder("alpha")
            .owner("appA")
            .description("Use this skill")
            .try_build()
            .unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::MissingSpecField { field: "body", .. }
        ));
    }

    #[test]
    fn validate_rejects_whitespace_only_body() {
        let err = SkillSpec::builder("alpha")
            .owner("appA")
            .description("Use this skill")
            .body("   \n\t  \n")
            .try_build()
            .unwrap_err();
        assert!(matches!(
            err,
            AgentConfigError::MissingSpecField { field: "body", .. }
        ));
    }

    #[test]
    fn adopt_unowned_defaults_false_and_round_trips() {
        let default_spec = SkillSpec::builder("alpha")
            .owner("appA")
            .description("Use this skill")
            .body("body")
            .build();
        assert!(!default_spec.adopt_unowned);

        let opted = SkillSpec::builder("alpha")
            .owner("appA")
            .description("Use this skill")
            .body("body")
            .adopt_unowned(true)
            .build();
        assert!(opted.adopt_unowned);
    }
}
