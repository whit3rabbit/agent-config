//! Where a hook should be installed: globally for the user, or scoped to a
//! single project directory.

use std::path::{Path, PathBuf};

/// Install location for a hook.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Scope {
    /// User-global: writes to the harness's per-user config dir
    /// (e.g., `~/.claude/`, `~/.cursor/`).
    Global,

    /// Project-local: writes to a specific project directory
    /// (e.g., `./CLAUDE.md`, `./.clinerules`).
    Local(PathBuf),
}

impl Scope {
    /// Returns the [`ScopeKind`] discriminant.
    pub fn kind(&self) -> ScopeKind {
        match self {
            Scope::Global => ScopeKind::Global,
            Scope::Local(_) => ScopeKind::Local,
        }
    }

    /// Returns the local project root for [`Scope::Local`].
    pub fn local_root(&self) -> Option<&Path> {
        match self {
            Scope::Local(p) => Some(p),
            Scope::Global => None,
        }
    }

    /// Verify that `path` is contained within the local project root.
    ///
    /// For [`Scope::Global`], this is a no-op (global writes go to the user's
    /// home/config directories, which are not project-scoped). For
    /// [`Scope::Local`], canonicalizes both the resolved path's parent and
    /// the project root, then checks the parent starts with the root.
    ///
    /// Returns [`crate::HookerError::PathResolution`] if the path escapes the root.
    /// Returns `Ok(())` if the parent directory does not yet exist (a new
    /// file being created under the project root is safe).
    pub fn ensure_contained(&self, path: &Path) -> Result<(), crate::error::HookerError> {
        match self {
            Scope::Global => Ok(()),
            Scope::Local(root) => crate::util::fs_atomic::ensure_contained(path, root),
        }
    }
}

/// Discriminant of [`Scope`] without payload, used in the
/// [`Integration::supported_scopes`](crate::integration::Integration::supported_scopes)
/// trait method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScopeKind {
    /// User-global scope.
    Global,
    /// Project-local scope.
    Local,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_accessors() {
        assert_eq!(Scope::Global.kind(), ScopeKind::Global);
        assert_eq!(Scope::Local(PathBuf::from("/tmp")).kind(), ScopeKind::Local);
        assert!(Scope::Global.local_root().is_none());
        assert_eq!(
            Scope::Local(PathBuf::from("/project")).local_root(),
            Some(Path::new("/project"))
        );
        assert_ne!(ScopeKind::Global, ScopeKind::Local);
    }
}
