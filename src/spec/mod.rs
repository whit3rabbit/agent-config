//! Caller-supplied description of a hook (or MCP server, or skill) to install.

mod hook;
mod instruction;
mod mcp;
mod skill;
mod validate;

pub use hook::{
    Event, HookCommand, HookSpec, HookSpecBuilder, Matcher, RulesBlock, ScriptTemplate,
};
pub use instruction::{InstructionPlacement, InstructionSpec, InstructionSpecBuilder};
pub use mcp::{McpSpec, McpSpecBuilder, McpTransport, SecretPolicy};
pub use skill::{SkillAsset, SkillFrontmatter, SkillSpec, SkillSpecBuilder};
