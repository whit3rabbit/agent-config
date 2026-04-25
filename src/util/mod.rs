//! Internal helpers shared by every integration.
//!
//! These modules are the safety-critical core of the crate. Bugs here corrupt
//! user files; agent-specific code only composes these primitives.

pub(crate) mod fs_atomic;
pub(crate) mod json_patch;
pub(crate) mod mcp_json_array;
pub(crate) mod mcp_json_object;
pub(crate) mod md_block;
pub(crate) mod ownership;
pub(crate) mod rules_dir;
pub(crate) mod skills_dir;
pub(crate) mod toml_patch;
