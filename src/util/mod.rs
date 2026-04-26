//! Internal helpers shared by every integration.
//!
//! These modules are the safety-critical core of the crate. Bugs here corrupt
//! user files; agent-specific code only composes these primitives.

pub(crate) mod file_lock;
pub(crate) mod fs_atomic;
pub(crate) mod json5_patch;
pub(crate) mod json_patch;
#[allow(dead_code)]
pub(crate) mod mcp_json_array;
pub(crate) mod mcp_json_map;
pub(crate) mod mcp_json_object;
pub(crate) mod md_block;
pub(crate) mod ownership;
pub(crate) mod planning;
pub(crate) mod rules_dir;
pub(crate) mod safe_fs;
pub(crate) mod skills_dir;
pub(crate) mod toml_patch;
pub(crate) mod yaml_mcp_map;
