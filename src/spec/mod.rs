//! Caller-supplied description of a hook (or MCP server, or skill) to install.

mod hook;
mod mcp;
mod skill;
mod validate;

pub use hook::{Event, HookSpec, HookSpecBuilder, Matcher, RulesBlock, ScriptTemplate};
pub use mcp::{McpSpec, McpSpecBuilder, McpTransport};
pub use skill::{SkillAsset, SkillFrontmatter, SkillSpec, SkillSpecBuilder};
