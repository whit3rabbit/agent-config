//! Structured diagnostics and the CLI error type.
//!
//! Mirrors the `[CODE] why` + `fix:` rendering style used by the `skills`
//! CLI in this workspace, prefixed `AGENTCONFIG_` so the two tools never
//! produce colliding codes.

use std::fmt;

/// One actionable diagnostic: a stable code, why it fired, and how to fix it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// Stable uppercase code, e.g. `AGENTCONFIG_NO_MANIFEST`.
    pub code: &'static str,
    /// One-line explanation.
    pub why: String,
    /// Optional suggested fix.
    pub fix: Option<String>,
}

impl Diag {
    /// Build a diagnostic without a fix hint.
    pub fn new(code: &'static str, why: impl Into<String>) -> Self {
        Self {
            code,
            why: why.into(),
            fix: None,
        }
    }

    /// Build a diagnostic with a fix hint.
    pub fn with_fix(code: &'static str, why: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code,
            why: why.into(),
            fix: Some(fix.into()),
        }
    }

    /// Attach a fix hint to an existing diagnostic.
    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.why)?;
        if let Some(fix) = &self.fix {
            write!(f, "\n  fix: {fix}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Diag {}

/// Result alias for CLI operations.
pub type CliResult<T> = Result<T, Diag>;

/// Well-known diagnostic codes.
pub mod codes {
    /// A manifest or other required file could not be read from disk.
    pub const IO: &str = "AGENTCONFIG_IO";
    /// The manifest is not valid TOML or has a structural problem.
    pub const INVALID_MANIFEST: &str = "AGENTCONFIG_INVALID_MANIFEST";
    /// One manifest entry is invalid; the rest of the manifest still applies.
    pub const INVALID_ENTRY: &str = "AGENTCONFIG_INVALID_ENTRY";
    /// No manifest and no fallback layout was found in the source.
    pub const NO_MANIFEST: &str = "AGENTCONFIG_NO_MANIFEST";
    /// The source string could not be parsed.
    pub const SOURCE_INVALID: &str = "AGENTCONFIG_SOURCE_INVALID";
    /// A git operation failed.
    pub const GIT_FAILED: &str = "AGENTCONFIG_GIT_FAILED";
    /// An `--agent` id is not in the registry.
    pub const UNKNOWN_AGENT: &str = "AGENTCONFIG_UNKNOWN_AGENT";
    /// An `--only` surface name is not recognized.
    pub const UNKNOWN_SURFACE: &str = "AGENTCONFIG_UNKNOWN_SURFACE";
    /// The lockfile is missing, unreadable, or malformed.
    pub const LOCK_INVALID: &str = "AGENTCONFIG_LOCK_INVALID";
    /// A referenced package is not recorded in the lockfile.
    pub const NOT_IN_LOCKFILE: &str = "AGENTCONFIG_NOT_IN_LOCKFILE";
    /// The `agentconfig init` scaffold target already exists.
    pub const INIT_EXISTS: &str = "AGENTCONFIG_INIT_EXISTS";
    /// The install produced no applicable targets.
    pub const NO_TARGETS: &str = "AGENTCONFIG_NO_TARGETS";
    /// A hook entry opted into raw shell syntax.
    pub const SHELL_UNCHECKED: &str = "AGENTCONFIG_SHELL_UNCHECKED";
    /// A declared agent id is unknown or lacks the surface for an entry.
    pub const TARGET_WARNING: &str = "AGENTCONFIG_TARGET_WARNING";
    /// A file referenced by the manifest is too large to ingest.
    pub const FILE_TOO_LARGE: &str = "AGENTCONFIG_FILE_TOO_LARGE";
    /// A symlink was found where the manifest expects a regular file/dir.
    pub const SYMLINK_SKIPPED: &str = "AGENTCONFIG_SYMLINK_SKIPPED";
}
