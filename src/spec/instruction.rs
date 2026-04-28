//! Caller-supplied description of a standalone instruction file to install.

use crate::error::AgentConfigError;

use super::validate::{validate_identifier, IdentifierKind};

/// How an instruction file should be placed relative to the agent's config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InstructionPlacement {
    /// Inject content as a managed markdown block inside a shared file
    /// (reuses `md_block::upsert`/`md_block::remove` with the instruction
    /// name as the tag).
    InlineBlock,

    /// Write a standalone file and add a managed include reference line
    /// (e.g. `@MYAPP.md` in `~/.claude/CLAUDE.md`). The reference itself
    /// is managed as a fenced markdown block so it can be removed cleanly.
    ReferencedFile,

    /// Write a standalone file only, no include reference.
    /// For agents with rules directories where the file's presence alone
    /// causes the agent to load it (e.g. `.roo/rules/MYAPP.md`).
    StandaloneFile,
}

/// Caller-supplied description of a standalone instruction file to install.
///
/// Instructions differ from hook rules: they are named, standalone files
/// that persist across sessions (like `~/.claude/MYAPP.md`). They may be
/// referenced via include directives from the agent's memory file, or
/// placed directly in a rules directory.
///
/// Build via [`InstructionSpec::builder`].
#[derive(Debug, Clone)]
pub struct InstructionSpec {
    /// Instruction name. Used as the filename stem (e.g. `MYAPP` becomes
    /// `MYAPP.md`). Must be ASCII alnum / `_` / `-`, non-empty.
    ///
    /// **Naming caveat.** For [`InstructionPlacement::ReferencedFile`] and
    /// [`InstructionPlacement::InlineBlock`], `name` is also reused as the
    /// fence tag in the host markdown file
    /// (`<!-- BEGIN AGENT-CONFIG:<name> --> ... <!-- END AGENT-CONFIG:<name> -->`).
    /// If a consumer installs a hook with `tag = "T"` *and* an instruction with
    /// `name = "T"` into the same memory file (e.g. both in
    /// `~/.claude/CLAUDE.md`), the second upsert silently replaces the first.
    /// Pick a name that does not collide with any of your hook tags. When in
    /// doubt, prefix the name (e.g. `instr-myapp`) or use
    /// [`InstructionPlacement::StandaloneFile`].
    pub name: String,

    /// The consumer of this library that owns the instruction.
    /// Recorded in the sidecar ownership ledger; refusal to remove an
    /// instruction owned by another consumer matches the same
    /// [`AgentConfigError::NotOwnedByCaller`] model used for MCP and skills.
    pub owner_tag: String,

    /// How this instruction should be placed for the target agent.
    pub placement: InstructionPlacement,

    /// Markdown body of the instruction file.
    pub body: String,

    /// When true, install adopts an instruction file (or include block) that
    /// exists on disk but has no recorded owner instead of refusing. Use
    /// after a crash between file write and ledger record. Default `false`.
    pub adopt_unowned: bool,
}

impl InstructionSpec {
    /// Begin building an instruction spec with the given name.
    pub fn builder(name: impl Into<String>) -> InstructionSpecBuilder {
        InstructionSpecBuilder {
            name: name.into(),
            owner_tag: None,
            placement: InstructionPlacement::ReferencedFile,
            body: String::new(),
            adopt_unowned: false,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), AgentConfigError> {
        Self::validate_name(&self.name)?;
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)?;
        if self.body.trim().is_empty() {
            return Err(AgentConfigError::MissingSpecField {
                id: "<instruction>",
                field: "body",
            });
        }
        Ok(())
    }

    pub(crate) fn validate_name(name: &str) -> Result<(), AgentConfigError> {
        validate_identifier(name, IdentifierKind::InstructionName)
    }
}

/// Builder for [`InstructionSpec`].
#[derive(Debug, Clone)]
pub struct InstructionSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    placement: InstructionPlacement,
    body: String,
    adopt_unowned: bool,
}

impl InstructionSpecBuilder {
    /// Set the owner tag. Required.
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Adopt an instruction file (or include block) that exists on disk but
    /// has no recorded owner. See [`InstructionSpec::adopt_unowned`].
    pub fn adopt_unowned(mut self, adopt: bool) -> Self {
        self.adopt_unowned = adopt;
        self
    }

    /// Set the placement mode. Defaults to `ReferencedFile`.
    pub fn placement(mut self, p: InstructionPlacement) -> Self {
        self.placement = p;
        self
    }

    /// Set the markdown body. Required, must not be empty/whitespace.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Consume the builder and return an [`InstructionSpec`].
    ///
    /// # Panics
    ///
    /// Panics if required fields are missing or validation fails.
    pub fn build(self) -> InstructionSpec {
        self.try_build()
            .expect("InstructionSpec missing required field")
    }

    /// Consume the builder and return an [`InstructionSpec`], or an error
    /// if required fields are missing or validation fails.
    ///
    /// # Errors
    ///
    /// - [`AgentConfigError::MissingSpecField`] with `field = "owner"` when
    ///   [`InstructionSpecBuilder::owner`] was never called, or
    ///   `field = "body"` when the body is empty/whitespace-only.
    /// - [`AgentConfigError::InvalidTag`] when `name` or `owner_tag` contain
    ///   characters outside ASCII alnum / `_` / `-`.
    pub fn try_build(self) -> Result<InstructionSpec, AgentConfigError> {
        let owner_tag = self.owner_tag.ok_or(AgentConfigError::MissingSpecField {
            id: "<instruction>",
            field: "owner",
        })?;
        let spec = InstructionSpec {
            name: self.name,
            owner_tag,
            placement: self.placement,
            body: self.body,
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
    fn invalid_name_error_mentions_instruction_not_mcp() {
        let err = InstructionSpec::builder("bad name")
            .owner("owner")
            .body("body")
            .try_build()
            .expect_err("space in name must be rejected");
        let AgentConfigError::InvalidTag { reason, .. } = err else {
            panic!("expected InvalidTag, got {err:?}");
        };
        assert!(
            reason.contains("instruction name"),
            "error reason should reference instruction name, got: {reason}"
        );
        assert!(
            !reason.contains("MCP"),
            "error reason must not mention MCP for an instruction failure: {reason}"
        );
    }

    #[test]
    fn empty_name_error_mentions_instruction() {
        let err = InstructionSpec::builder("")
            .owner("owner")
            .body("body")
            .try_build()
            .expect_err("empty name must be rejected");
        let AgentConfigError::InvalidTag { reason, .. } = err else {
            panic!("expected InvalidTag, got {err:?}");
        };
        assert!(
            reason.contains("instruction name"),
            "empty-name reason should reference instruction name, got: {reason}"
        );
    }
}
