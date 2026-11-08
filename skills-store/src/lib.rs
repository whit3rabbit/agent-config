//! Canonical Agent Skills store shared by AI coding agents.
//!
//! This crate is a Rust implementation of the on-disk engine behind the
//! [`vercel-labs/skills`](https://github.com/vercel-labs/skills) CLI: skills
//! are installed once into a canonical directory (`.agents/skills` in the
//! project, `~/.agents/skills` in the home directory) and every agent that
//! does not read that location natively receives a symlink from its own
//! skills directory. Two lock files track installs:
//!
//! - the global lock (`$XDG_STATE_HOME/skills/.skill-lock.json`, else
//!   `~/.agents/.skill-lock.json`, schema version 3), and
//! - the project lock (`skills-lock.json` in the project root, schema
//!   version 1, intended to be committed).
//!
//! The layout, lock schemas, and name sanitization are byte-compatible with
//! the upstream `skills` CLI v1.7.6, so skills installed (or removed) through
//! either tool are visible to the other.
//!
//! The crate is deliberately network-free: callers fetch sources themselves
//! (git clone, HTTPS download, well-known discovery) and hand the material to
//! [`installer`] as a directory on disk or an in-memory file set. The
//! `skills` CLI binary in this repository wires the two together.
//!
//! # Example
//!
//! ```no_run
//! use skills_store::{self, Scope};
//! use std::path::Path;
//!
//! // Install the skill directory ./my-skill into every detected agent.
//! let agents = skills_store::detect_installed_agents(Path::new("."));
//! let mut warnings = Vec::new();
//! for agent in agents {
//!     skills_store::install_skill_dir(
//!         Path::new("./my-skill"),
//!         agent,
//!         &skills_store::InstallOptions { global: Scope::Project, ..Default::default() },
//!         &mut warnings,
//!     );
//! }
//! ```

#![forbid(unsafe_code)]

pub mod error;
pub mod frontmatter;
pub mod installer;
pub mod locks;
pub mod paths;
pub mod registry;
pub mod sanitize;
pub mod skills;

pub use error::SkillsError;
pub use installer::{
    default_install_mode, get_agent_base_dir, get_canonical_path, get_canonical_skills_dir,
    get_install_path, install_for_agent, install_skill_dir, install_skill_files,
    install_skill_snapshot, list_installed_skills, sanitize_name, InstallMode, InstallOptions,
    InstallOutcome, InstalledSkill, RemoveOptions, RemoveOutcome, Scope,
};
pub use locks::{
    compute_folder_hash, read_local_lock, read_skill_lock, write_local_lock, write_skill_lock,
    LocalSkillLock, LocalSkillLockEntry, SkillLock, SkillLockEntry,
};
pub use registry::{
    agent_by_id, agent_project_skill_dirs, agents, detect_installed_agents, AgentConfig,
    DetectContext,
};
pub use skills::{discover_skills, filter_skills, parse_skill_md, DiscoverOptions, Skill};

/// Result alias used across the crate.
pub type Result<T> = std::result::Result<T, SkillsError>;
