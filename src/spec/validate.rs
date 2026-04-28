//! Shared identifier validation used by every spec builder in this module.

use crate::error::AgentConfigError;

/// Which identifier we are validating; controls the wording of
/// [`AgentConfigError::InvalidTag::reason`].
#[derive(Copy, Clone)]
pub(super) enum IdentifierKind {
    Tag,
    OwnerTag,
    McpName,
    SkillName,
    InstructionName,
}

/// Shared identifier validator. Hook tags, owner tags, MCP names, and
/// instruction names allow ASCII alphanumerics, `_`, and `-`; skill names
/// follow the stricter Agent Skills kebab-case contract.
pub(super) fn validate_identifier(
    value: &str,
    kind: IdentifierKind,
) -> Result<(), AgentConfigError> {
    let (empty, illegal) = match kind {
        IdentifierKind::Tag => (
            "tag must not be empty",
            "tag may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::OwnerTag => (
            "owner_tag must not be empty",
            "owner_tag may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::McpName => (
            "MCP server name must not be empty",
            "MCP server name may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::SkillName => (
            "skill name must not be empty",
            "skill name must be lowercase ASCII letters/digits with single '-' separators",
        ),
        IdentifierKind::InstructionName => (
            "instruction name must not be empty",
            "instruction name may only contain ASCII letters, digits, '_' and '-'",
        ),
    };
    if value.is_empty() {
        return Err(AgentConfigError::InvalidTag {
            tag: value.into(),
            reason: empty,
        });
    }
    if matches!(kind, IdentifierKind::SkillName) {
        return validate_skill_name(value);
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok {
        return Err(AgentConfigError::InvalidTag {
            tag: value.into(),
            reason: illegal,
        });
    }
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<(), AgentConfigError> {
    const ILLEGAL: &str =
        "skill name must be lowercase ASCII letters/digits with single '-' separators";
    if name.len() > 64 {
        return Err(AgentConfigError::InvalidTag {
            tag: name.into(),
            reason: "skill name must be 64 characters or fewer",
        });
    }
    let mut prev_hyphen = false;
    for (i, c) in name.chars().enumerate() {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if !ok {
            return Err(AgentConfigError::InvalidTag {
                tag: name.into(),
                reason: ILLEGAL,
            });
        }
        if c == '-' {
            if i == 0 || prev_hyphen {
                return Err(AgentConfigError::InvalidTag {
                    tag: name.into(),
                    reason: ILLEGAL,
                });
            }
            prev_hyphen = true;
        } else {
            prev_hyphen = false;
        }
    }
    if prev_hyphen {
        return Err(AgentConfigError::InvalidTag {
            tag: name.into(),
            reason: ILLEGAL,
        });
    }
    Ok(())
}
