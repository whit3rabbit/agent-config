//! `ai-hooker` installs hooks, prompt rules, MCP servers, and skills into AI coding harnesses.
//!
//! The library knows where each harness keeps its configuration and what shape
//! that configuration takes. Callers supply a [`HookSpec`], [`McpSpec`], or
//! [`SkillSpec`]. The library handles atomic writes, backups, ownership ledgers,
//! and idempotent edits.
//!
//! # Quick start
//!
//! ```no_run
//! use ai_hooker::{by_id, HookSpec, Matcher, Event, Scope};
//!
//! let spec = HookSpec::builder("myapp")
//!     .command_program("myapp", ["hook", "claude"])
//!     .matcher(Matcher::Bash)
//!     .event(Event::PreToolUse)
//!     .build();
//!
//! let claude = by_id("claude").expect("claude integration registered");
//! claude.install(&Scope::Global, &spec).unwrap();
//! ```
//!
//! # MCP servers
//!
//! ```no_run
//! use ai_hooker::{mcp_by_id, McpSpec, Scope};
//!
//! let spec = McpSpec::builder("github")
//!     .owner("myapp")
//!     .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
//!     .build();
//!
//! let codex = mcp_by_id("codex").expect("codex MCP support registered");
//! codex.install_mcp(&Scope::Global, &spec).unwrap();
//! ```
//!
//! # Skills
//!
//! ```no_run
//! use ai_hooker::{skill_by_id, Scope, SkillSpec};
//!
//! let spec = SkillSpec::builder("my-skill")
//!     .owner("myapp")
//!     .description("Use when my app needs custom repository context.")
//!     .body("# My Skill\n\nFollow the local project conventions.")
//!     .build();
//!
//! let claude = skill_by_id("claude").expect("claude skill support registered");
//! claude.install_skill(&Scope::Global, &spec).unwrap();
//! ```
//!
//! # Discovery and uninstall
//!
//! ```no_run
//! use ai_hooker::{all, by_id, Scope};
//!
//! for integration in all() {
//!     if integration.supported_scopes().contains(&Scope::Global.kind())
//!         && integration.is_installed(&Scope::Global, "myapp").unwrap_or(false)
//!     {
//!         println!("{} has myapp installed", integration.display_name());
//!     }
//! }
//!
//! let claude = by_id("claude").expect("claude integration registered");
//! claude.uninstall(&Scope::Global, "myapp").unwrap();
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
pub mod plan;
pub mod registry;
pub mod scope;
pub mod spec;
pub mod status;
pub mod validation;

mod agents;
mod util;

pub use error::HookerError;
pub use integration::{
    InstallReport, Integration, McpSurface, MigrationReport, SkillSurface, UninstallReport,
};
pub use plan::{
    InstallPlan, InstallStatus, PlanTarget, PlanWarning, PlannedChange, RefusalReason,
    UninstallPlan,
};
pub use registry::{all, by_id, mcp_by_id, mcp_capable, skill_by_id, skill_capable};
pub use scope::{Scope, ScopeKind};
pub use spec::{
    Event, HookCommand, HookSpec, HookSpecBuilder, Matcher, McpSpec, McpSpecBuilder, McpTransport,
    RulesBlock, ScriptTemplate, SecretPolicy, SkillAsset, SkillFrontmatter, SkillSpec,
    SkillSpecBuilder,
};
pub use status::{
    DriftIssue, InstallStatus as StatusInstallStatus, PathStatus, PlanTarget as StatusPlanTarget,
    StatusReport, StatusWarning,
};
pub use validation::{SuggestedAction, ValidationReport};

/// Result alias used throughout the crate's public API.
pub type Result<T> = std::result::Result<T, HookerError>;
