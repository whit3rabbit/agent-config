//! Public error type for the crate.

use std::path::PathBuf;

use thiserror::Error;

/// All failures returned by `ai-hooker`'s public API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HookerError {
    /// Filesystem I/O failed. Wraps [`std::io::Error`] with the path that caused it.
    #[error("io error at {path}: {source}")]
    Io {
        /// The path that triggered the error.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A target file existed but contained invalid JSON.
    #[error("invalid JSON in {path}: {source}")]
    JsonInvalid {
        /// The path of the malformed file.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },

    /// Could not resolve a platform path (e.g., home directory missing).
    #[error("could not resolve path: {0}")]
    PathResolution(String),

    /// The integration does not support the requested scope.
    #[error("integration {id} does not support scope {scope:?}")]
    UnsupportedScope {
        /// Integration id.
        id: &'static str,
        /// The rejected scope kind.
        scope: crate::scope::ScopeKind,
    },

    /// The caller-supplied [`HookSpec`](crate::HookSpec) is missing a field this
    /// integration requires (e.g., gemini needs `script`, prompt-only agents
    /// need `rules`).
    #[error("integration {id} requires field `{field}` in HookSpec")]
    MissingSpecField {
        /// Integration id.
        id: &'static str,
        /// The missing field name.
        field: &'static str,
    },

    /// The hook tag is invalid (empty or contains illegal characters).
    #[error("invalid tag {tag:?}: {reason}")]
    InvalidTag {
        /// The offending tag.
        tag: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A would-be backup file already exists at `<path>.bak`.
    #[error("backup already exists at {0}")]
    BackupExists(PathBuf),

    /// Could not acquire a filesystem lock before the timeout elapsed.
    #[error(
        "timed out waiting for lock at {path}; if no ai-hooker process is running, this lock may be stale and can be deleted"
    )]
    LockTimeout {
        /// The lock file path that remained held.
        path: PathBuf,
    },

    /// A target file existed but contained invalid TOML (Codex `config.toml`).
    #[error("invalid TOML in {path}: {source}")]
    TomlInvalid {
        /// The path of the malformed file.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: toml_edit::TomlError,
    },

    /// Caller tried to uninstall an MCP server or skill that is owned by a
    /// different consumer (or by a hand-edit not recorded in the sidecar
    /// ledger). Refused to avoid clobbering work this caller did not install.
    #[error(
        "{kind} {name:?} is not owned by caller {expected:?} \
        (actual_owner = {actual:?}); refusing to remove"
    )]
    NotOwnedByCaller {
        /// What kind of resource is in dispute (e.g. `"mcp server"`, `"skill"`).
        kind: &'static str,
        /// The disputed name.
        name: String,
        /// The expected owner (the caller's tag).
        expected: String,
        /// The actual recorded owner, if any.
        actual: Option<String>,
    },

    /// Anything else, with context.
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl HookerError {
    /// Helper to wrap an I/O error with its path.
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Helper to wrap a JSON parse error with its path.
    pub(crate) fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::JsonInvalid {
            path: path.into(),
            source,
        }
    }

    /// Helper to wrap a TOML parse error with its path.
    pub(crate) fn toml(path: impl Into<PathBuf>, source: toml_edit::TomlError) -> Self {
        Self::TomlInvalid {
            path: path.into(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn io_helper_format() {
        let err = HookerError::io(
            "/some/path",
            std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        );
        let msg = format!("{err}");
        assert!(msg.contains("/some/path"), "message: {msg}");
        assert!(msg.contains("gone"), "message: {msg}");
    }

    #[test]
    fn json_helper_format() {
        let parse_err = serde_json::from_str::<serde_json::Value>("{bad").unwrap_err();
        let err = HookerError::json("/bad.json", parse_err);
        let msg = format!("{err}");
        assert!(msg.contains("/bad.json"), "message: {msg}");
        assert!(msg.contains("invalid JSON"), "message: {msg}");
    }

    #[test]
    fn from_anyhow() {
        let err = HookerError::from(anyhow::anyhow!("something broke"));
        assert!(matches!(err, HookerError::Other(_)));
        assert_eq!(format!("{err}"), "something broke");
    }

    #[test]
    fn display_format_for_each_variant() {
        let io = HookerError::Io {
            path: PathBuf::from("/a"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        };
        assert!(format!("{io}").contains("/a"));

        let json = HookerError::JsonInvalid {
            path: PathBuf::from("/b.json"),
            source: serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
        };
        assert!(format!("{json}").contains("/b.json"));

        let path = HookerError::PathResolution("no home".into());
        assert!(format!("{path}").contains("no home"));

        let unsupported = HookerError::UnsupportedScope {
            id: "test",
            scope: crate::scope::ScopeKind::Global,
        };
        assert!(format!("{unsupported}").contains("test"));

        let missing = HookerError::MissingSpecField {
            id: "agent",
            field: "command",
        };
        assert!(format!("{missing}").contains("command"));

        let tag = HookerError::InvalidTag {
            tag: "bad!".into(),
            reason: "chars",
        };
        assert!(format!("{tag}").contains("bad!"));

        let backup = HookerError::BackupExists(PathBuf::from("/c.bak"));
        assert!(format!("{backup}").contains("/c.bak"));

        let lock = HookerError::LockTimeout {
            path: PathBuf::from("/c.lock"),
        };
        assert!(format!("{lock}").contains("/c.lock"));

        let toml = HookerError::TomlInvalid {
            path: PathBuf::from("/d.toml"),
            source: toml_edit::DocumentMut::from_str("=bad")
                .expect_err("malformed TOML to parse-fail"),
        };
        let toml_msg = format!("{toml}");
        assert!(toml_msg.contains("/d.toml"));
        assert!(toml_msg.contains("invalid TOML"));

        let owned = HookerError::NotOwnedByCaller {
            kind: "mcp server",
            name: "github".into(),
            expected: "myapp".into(),
            actual: Some("otherapp".into()),
        };
        let owned_msg = format!("{owned}");
        assert!(owned_msg.contains("github"));
        assert!(owned_msg.contains("myapp"));
        assert!(owned_msg.contains("otherapp"));

        let other = HookerError::Other(anyhow::anyhow!("misc"));
        assert_eq!(format!("{other}"), "misc");
    }
}
