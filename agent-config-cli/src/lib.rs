//! `agentconfig` — install hooks, prompt rules, MCP servers, skills, and
//! instructions from an `agent.config.toml` manifest into AI coding
//! harnesses, on top of the [`agent-config`] library.
//!
//! The crate is split so the engine is testable without a TTY:
//!
//! - [`manifest`] parses `agent.config.toml`
//! - [`mapping`] turns manifest entries into library specs
//! - [`engine`] resolves targets and drives plan / install / uninstall /
//!   status
//! - [`source`] parses and fetches sources (local paths, git URLs,
//!   `owner/repo` shorthand)
//! - [`lockfile`] records installed packages
//! - [`cmd`] holds the thin command-line layer
//!
//! The engine never writes harness files directly; every mutation goes
//! through the library so ledgers, backups, idempotency, and symlink safety
//! behave exactly as they do for library consumers.

pub mod cmd;
pub mod diag;
pub mod engine;
pub mod lockfile;
pub mod manifest;
pub mod mapping;
pub mod source;
