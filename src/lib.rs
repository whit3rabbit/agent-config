//! `ai-hooker` installs hooks and prompt-level integrations into AI coding harnesses.
//!
//! The library knows where each harness keeps its hook configuration and what shape
//! that configuration takes. Callers supply a [`HookSpec`] describing what command
//! to run, which event to attach to, and any prompt content to inject. The library
//! handles atomic writes, backups, and idempotent edits.
//!
//! # Quick start
//!
//! ```no_run
//! use ai_hooker::{by_id, HookSpec, Matcher, Event, Scope};
//!
//! let spec = HookSpec::builder("myapp")
//!     .command("myapp hook claude")
//!     .matcher(Matcher::Bash)
//!     .event(Event::PreToolUse)
//!     .build();
//!
//! let claude = by_id("claude").expect("claude integration registered");
//! claude.install(&Scope::Global, &spec).unwrap();
//! ```
//!
//! # Safety guarantees
//!
//! - Atomic writes (write-to-temp + rename).
//! - First-touch `.bak` backups of any pre-existing file we modify.
//! - Idempotent installs: repeating `install` with the same `tag` yields the same state.
//! - Reversible: `uninstall` removes only the tagged content.

#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub mod error;
pub mod integration;
pub mod paths;
pub mod registry;
pub mod scope;
pub mod spec;
pub mod util;

pub mod agents;

pub use error::HookerError;
pub use integration::{
    InstallReport, Integration, McpSurface, MigrationReport, SkillSurface, UninstallReport,
};
pub use registry::{all, by_id, mcp_by_id, mcp_capable, skill_by_id, skill_capable};
pub use scope::{Scope, ScopeKind};
pub use spec::{
    Event, HookSpec, HookSpecBuilder, Matcher, McpSpec, McpSpecBuilder, McpTransport, RulesBlock,
    ScriptTemplate, SkillAsset, SkillFrontmatter, SkillSpec, SkillSpecBuilder,
};

/// Result alias used throughout the crate's public API.
pub type Result<T> = std::result::Result<T, HookerError>;
