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
    /// (e.g. `@RTK.md` in `~/.claude/CLAUDE.md`). The reference itself
    /// is managed as a fenced markdown block so it can be removed cleanly.
    ReferencedFile,

    /// Write a standalone file only, no include reference.
    /// For agents with rules directories where the file's presence alone
    /// causes the agent to load it (e.g. `.roo/rules/RTK.md`).
    StandaloneFile,
}

/// Caller-supplied description of a standalone instruction file to install.
///
/// Instructions differ from hook rules: they are named, standalone files
/// that persist across sessions (like `~/.claude/RTK.md`). They may be
/// referenced via include directives from the agent's memory file, or
/// placed directly in a rules directory.
///
/// Build via [`InstructionSpec::builder`].
#[derive(Debug, Clone)]
pub struct InstructionSpec {
    /// Instruction name. Used as the filename stem (e.g. `RTK` becomes
    /// `RTK.md`). Must be ASCII alnum / `_` / `-`, non-empty.
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
}

impl InstructionSpec {
    /// Begin building an instruction spec with the given name.
    pub fn builder(name: impl Into<String>) -> InstructionSpecBuilder {
        InstructionSpecBuilder {
            name: name.into(),
            owner_tag: None,
            placement: InstructionPlacement::ReferencedFile,
            body: String::new(),
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
        validate_identifier(name, IdentifierKind::McpName)
    }
}

/// Builder for [`InstructionSpec`].
#[derive(Debug, Clone)]
pub struct InstructionSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    placement: InstructionPlacement,
    body: String,
}

impl InstructionSpecBuilder {
    /// Set the owner tag. Required.
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
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
        };
        spec.validate()?;
        Ok(spec)
    }
}
