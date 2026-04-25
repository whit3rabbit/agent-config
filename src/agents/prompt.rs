//! Generic implementation for harnesses that only read project-local rules
//! markdown files and have no other surface (no hooks, no MCP, no skills).
//!
//! Currently serves Roo Code and Kilo Code. Cline, Windsurf, and Antigravity
//! used to share this struct but have since been promoted to dedicated
//! agents because they expose additional surfaces (hooks, MCP, skills).

use std::path::Path;

use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, UninstallReport};
use crate::scope::{Scope, ScopeKind};
use crate::spec::HookSpec;
use crate::util::rules_dir;

/// Reusable project-local rules-file integration.
pub struct PromptAgent {
    id: &'static str,
    display_name: &'static str,
    /// Path relative to the project root, e.g. `".roo/rules"` or
    /// `".kilocode/rules"`.
    rules_dir: &'static str,
}

impl PromptAgent {
    /// Roo Code: `./.roo/rules/<tag>.md`.
    pub const fn roo() -> Self {
        Self {
            id: "roo",
            display_name: "Roo Code",
            rules_dir: ".roo/rules",
        }
    }

    /// Kilo Code: `./.kilocode/rules/<tag>.md`.
    pub const fn kilocode() -> Self {
        Self {
            id: "kilocode",
            display_name: "Kilo Code",
            rules_dir: ".kilocode/rules",
        }
    }

    fn require_local<'a>(&self, scope: &'a Scope) -> Result<&'a Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: self.id,
                scope: ScopeKind::Global,
            }),
        }
    }
}

impl Integration for PromptAgent {
    fn id(&self) -> &'static str {
        self.id
    }

    fn display_name(&self) -> &'static str {
        self.display_name
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        let root = self.require_local(scope)?;
        rules_dir::is_installed(root, self.rules_dir, tag)
    }

    fn install(&self, spec_scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let root = self.require_local(spec_scope)?;
        let rules = spec.rules.as_ref().ok_or(HookerError::MissingSpecField {
            id: self.id,
            field: "rules",
        })?;
        rules_dir::install(root, self.rules_dir, &spec.tag, &rules.content)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let root = self.require_local(scope)?;
        rules_dir::uninstall(root, self.rules_dir, tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn spec(tag: &str, rules: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command("noop")
            .rules(rules)
            .build()
    }

    #[test]
    fn roo_writes_to_dot_roo_rules() {
        let dir = tempdir().unwrap();
        let agent = PromptAgent::roo();
        agent
            .install(
                &Scope::Local(dir.path().to_path_buf()),
                &spec("alpha", "rule body"),
            )
            .unwrap();
        let p = dir.path().join(".roo/rules/alpha.md");
        assert!(p.exists());
        assert_eq!(fs::read_to_string(&p).unwrap(), "rule body\n");
    }

    #[test]
    fn kilocode_writes_to_dot_kilocode_rules() {
        let dir = tempdir().unwrap();
        let agent = PromptAgent::kilocode();
        agent
            .install(
                &Scope::Local(dir.path().to_path_buf()),
                &spec("alpha", "x"),
            )
            .unwrap();
        assert!(dir.path().join(".kilocode/rules/alpha.md").exists());
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempdir().unwrap();
        let agent = PromptAgent::roo();
        let scope = Scope::Local(dir.path().to_path_buf());
        let s = spec("alpha", "body");
        let r1 = agent.install(&scope, &s).unwrap();
        let r2 = agent.install(&scope, &s).unwrap();
        assert!(!r1.already_installed);
        assert!(r2.already_installed);
    }

    #[test]
    fn uninstall_round_trip() {
        let dir = tempdir().unwrap();
        let agent = PromptAgent::kilocode();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &spec("alpha", "x")).unwrap();
        let report = agent.uninstall(&scope, "alpha").unwrap();
        assert_eq!(report.removed.len(), 1);
        assert!(!dir.path().join(".kilocode").exists());
    }

    #[test]
    fn rejects_global_scope() {
        let agent = PromptAgent::roo();
        let err = agent.is_installed(&Scope::Global, "a").unwrap_err();
        assert!(matches!(err, HookerError::UnsupportedScope { .. }));
    }

    #[test]
    fn install_requires_rules_field() {
        let dir = tempdir().unwrap();
        let agent = PromptAgent::roo();
        let scope = Scope::Local(dir.path().to_path_buf());
        let no_rules = HookSpec::builder("alpha").command("noop").build();
        let err = agent.install(&scope, &no_rules).unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { field: "rules", .. }));
    }
}
