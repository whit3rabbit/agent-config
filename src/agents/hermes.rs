//! Hermes Agent integration.
//!
//! Implemented surfaces:
//!
//! 1. **Prompt rules**: project-local fenced blocks in `.hermes.md`.
//! 2. **Skills**: global category-scoped folders under
//!    `~/.hermes/skills/ai-hooker/<name>`.
//! 3. **MCP servers**: global YAML config at `~/.hermes/config.yaml`, under
//!    `mcp_servers.<name>`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::agents::planning as agent_planning;
use crate::error::HookerError;
use crate::integration::{InstallReport, Integration, McpSurface, SkillSurface, UninstallReport};
use crate::paths;
use crate::plan::{InstallPlan, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{HookSpec, McpSpec, McpTransport, SkillSpec};
use crate::status::{
    InstallStatus as StatusInstallStatus, PathStatus, PlanTarget as StatusPlanTarget, StatusReport,
};
use crate::util::{fs_atomic, md_block, ownership, skills_dir, yaml_mcp_map};

const SKILL_CATEGORY: &str = "ai-hooker";

/// Hermes Agent file-backed installer.
pub struct HermesAgent;

impl HermesAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self
    }

    fn require_local(scope: &Scope) -> Result<&Path, HookerError> {
        match scope {
            Scope::Local(p) => Ok(p),
            Scope::Global => Err(HookerError::UnsupportedScope {
                id: "hermes",
                scope: ScopeKind::Global,
            }),
        }
    }

    fn prompt_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        Ok(Self::require_local(scope)?.join(".hermes.md"))
    }

    fn hermes_home_from_home(home: &Path) -> PathBuf {
        home.join(".hermes")
    }

    fn mcp_config_path_from_home(home: &Path) -> PathBuf {
        Self::hermes_home_from_home(home).join("config.yaml")
    }

    fn mcp_path(scope: &Scope) -> Result<PathBuf, HookerError> {
        match scope {
            Scope::Global => Ok(Self::mcp_config_path_from_home(&paths::home_dir()?)),
            Scope::Local(_) => Err(HookerError::UnsupportedScope {
                id: "hermes",
                scope: ScopeKind::Local,
            }),
        }
    }

    fn skills_root_from_home(home: &Path) -> PathBuf {
        Self::hermes_home_from_home(home)
            .join("skills")
            .join(SKILL_CATEGORY)
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, HookerError> {
        match scope {
            Scope::Global => Ok(Self::skills_root_from_home(&paths::home_dir()?)),
            Scope::Local(_) => Err(HookerError::UnsupportedScope {
                id: "hermes",
                scope: ScopeKind::Local,
            }),
        }
    }

    fn install_mcp_config(
        config_path: &Path,
        spec: &McpSpec,
    ) -> Result<InstallReport, HookerError> {
        let ledger = ownership::mcp_ledger_for(config_path);
        yaml_mcp_map::install(
            config_path,
            &ledger,
            spec,
            &["mcp_servers"],
            hermes_mcp_value,
        )
    }

    fn uninstall_mcp_config(
        config_path: &Path,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let ledger = ownership::mcp_ledger_for(config_path);
        yaml_mcp_map::uninstall(
            config_path,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            &["mcp_servers"],
        )
    }
}

impl Default for HermesAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl Integration for HermesAgent {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn display_name(&self) -> &'static str {
        "Hermes Agent"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::prompt_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&path)?;
        let block_present = md_block::contains(&host, tag);
        let exists = path.exists();
        Ok(StatusReport {
            target: StatusPlanTarget::Hook {
                tag: tag.to_string(),
            },
            status: if block_present {
                StatusInstallStatus::InstalledOwned {
                    owner: tag.to_string(),
                }
            } else {
                StatusInstallStatus::Absent
            },
            config_path: Some(path.clone()),
            ledger_path: None,
            files: vec![if exists {
                PathStatus::Exists { path }
            } else {
                PathStatus::Missing { path }
            }],
            warnings: Vec::new(),
        })
    }

    fn plan_install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallPlan, HookerError> {
        agent_planning::markdown_install(
            Integration::id(self),
            scope,
            spec,
            Self::prompt_path(scope),
            true,
        )
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, HookerError> {
        agent_planning::markdown_uninstall(
            Integration::id(self),
            scope,
            tag,
            Self::prompt_path(scope),
        )
    }

    fn is_installed(&self, scope: &Scope, tag: &str) -> Result<bool, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::prompt_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&path)?;
        Ok(md_block::contains(&host, tag))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, HookerError> {
        HookSpec::validate_tag(&spec.tag)?;
        let rules = spec.rules.as_ref().ok_or(HookerError::MissingSpecField {
            id: "hermes",
            field: "rules",
        })?;
        let path = Self::prompt_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&path)?;
        let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
        let outcome = fs_atomic::write_atomic(&path, new_host.as_bytes(), true)?;

        let mut report = InstallReport::default();
        if outcome.no_change {
            report.already_installed = true;
        } else if outcome.existed {
            report.patched.push(outcome.path.clone());
        } else {
            report.created.push(outcome.path.clone());
        }
        if let Some(b) = outcome.backup {
            report.backed_up.push(b);
        }
        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, HookerError> {
        HookSpec::validate_tag(tag)?;
        let path = Self::prompt_path(scope)?;
        let host = fs_atomic::read_to_string_or_empty(&path)?;
        let (stripped, removed) = md_block::remove(&host, tag);

        let mut report = UninstallReport::default();
        if !removed {
            report.not_installed = true;
            return Ok(report);
        }

        if stripped.trim().is_empty() {
            if fs_atomic::restore_backup(&path)? {
                report.restored.push(path.clone());
            } else {
                fs_atomic::remove_if_exists(&path)?;
                report.removed.push(path.clone());
            }
        } else {
            fs_atomic::write_atomic(&path, stripped.as_bytes(), false)?;
            report.patched.push(path.clone());
        }
        Ok(report)
    }
}

impl McpSurface for HermesAgent {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global]
    }

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, HookerError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence = yaml_mcp_map::config_presence(&cfg, &["mcp_servers"], name)?;
        let recorded = ownership::owner_of(&ledger, name)?;
        Ok(StatusReport::for_mcp(
            name,
            cfg,
            ledger,
            presence,
            expected_owner,
            recorded,
        ))
    }

    fn plan_install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallPlan, HookerError> {
        agent_planning::mcp_yaml_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::mcp_path(scope),
            &["mcp_servers"],
            hermes_mcp_value,
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::mcp_yaml_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::mcp_path(scope),
            &["mcp_servers"],
        )
    }

    fn is_mcp_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::mcp_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        yaml_mcp_map::is_installed(&ledger, name)
    }

    fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, HookerError> {
        spec.validate()?;
        let cfg = Self::mcp_path(scope)?;
        Self::install_mcp_config(&cfg, spec)
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::mcp_path(scope)?;
        Self::uninstall_mcp_config(&cfg, name, owner_tag)
    }
}

impl SkillSurface for HermesAgent {
    fn id(&self) -> &'static str {
        "hermes"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global]
    }

    fn skill_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, HookerError> {
        SkillSpec::validate_name(name)?;
        let root = Self::skills_root(scope)?;
        let (dir, manifest, ledger) = skills_dir::paths_for_status(&root, name);
        let recorded = ownership::owner_of(&ledger, name)?;
        Ok(StatusReport::for_skill(
            name,
            dir,
            manifest,
            ledger,
            expected_owner,
            recorded,
        ))
    }

    fn plan_install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallPlan, HookerError> {
        agent_planning::skill_install(
            SkillSurface::id(self),
            scope,
            spec,
            Self::skills_root(scope),
        )
    }

    fn plan_uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, HookerError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn is_skill_installed(&self, scope: &Scope, name: &str) -> Result<bool, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::is_installed(&root, name)
    }

    fn install_skill(&self, scope: &Scope, spec: &SkillSpec) -> Result<InstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, HookerError> {
        let root = Self::skills_root(scope)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

fn hermes_mcp_value(spec: &McpSpec) -> Value {
    let mut obj = Map::new();
    match &spec.transport {
        McpTransport::Stdio { command, args, env } => {
            obj.insert("command".into(), Value::String(command.clone()));
            obj.insert(
                "args".into(),
                Value::Array(args.iter().cloned().map(Value::String).collect()),
            );
            if !env.is_empty() {
                obj.insert("env".into(), string_map_value(env));
            }
        }
        McpTransport::Http { url, headers } | McpTransport::Sse { url, headers } => {
            obj.insert("url".into(), Value::String(url.clone()));
            if !headers.is_empty() {
                obj.insert("headers".into(), string_map_value(headers));
            }
        }
    }
    Value::Object(obj)
}

fn string_map_value(map: &BTreeMap<String, String>) -> Value {
    let mut obj = Map::new();
    for (k, v) in map {
        obj.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn rules_spec(tag: &str, rules: &str) -> HookSpec {
        HookSpec::builder(tag).command("noop").rules(rules).build()
    }

    fn skill(name: &str, owner: &str) -> SkillSpec {
        SkillSpec::builder(name)
            .owner(owner)
            .description("A test Hermes skill.")
            .body("## Goal\nDo the thing.\n")
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .env("FOO", "bar")
            .build()
    }

    #[test]
    fn install_rules_writes_dot_hermes_md_block() {
        let dir = tempdir().unwrap();
        let agent = HermesAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());

        agent
            .install(&scope, &rules_spec("alpha", "Use Hermes project rules."))
            .unwrap();

        let body = std::fs::read_to_string(dir.path().join(".hermes.md")).unwrap();
        assert!(body.contains("BEGIN AI-HOOKER:alpha"));
        assert!(body.contains("Use Hermes project rules."));
        assert!(agent.is_installed(&scope, "alpha").unwrap());
    }

    #[test]
    fn rules_install_is_idempotent() {
        let dir = tempdir().unwrap();
        let agent = HermesAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = rules_spec("alpha", "rules");

        agent.install(&scope, &spec).unwrap();
        let second = agent.install(&scope, &spec).unwrap();
        assert!(second.already_installed);
    }

    #[test]
    fn uninstall_rules_round_trip() {
        let dir = tempdir().unwrap();
        let agent = HermesAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());

        agent
            .install(&scope, &rules_spec("alpha", "rules"))
            .unwrap();
        let report = agent.uninstall(&scope, "alpha").unwrap();
        assert!(!report.removed.is_empty());
        assert!(!dir.path().join(".hermes.md").exists());
    }

    #[test]
    fn global_skill_root_uses_ai_hooker_category() {
        let home = PathBuf::from("/tmp/home");
        assert_eq!(
            HermesAgent::skills_root_from_home(&home),
            PathBuf::from("/tmp/home/.hermes/skills/ai-hooker")
        );
    }

    #[test]
    fn install_skill_under_category_helper_round_trip() {
        let dir = tempdir().unwrap();
        let root = HermesAgent::skills_root_from_home(dir.path());
        skills_dir::install(&root, &skill("alpha-skill", "myapp")).unwrap();

        assert!(dir
            .path()
            .join(".hermes/skills/ai-hooker/alpha-skill/SKILL.md")
            .exists());
        let second = skills_dir::install(&root, &skill("alpha-skill", "myapp")).unwrap();
        assert!(second.already_installed);
        skills_dir::uninstall(&root, "alpha-skill", "myapp").unwrap();
        assert!(!dir
            .path()
            .join(".hermes/skills/ai-hooker/alpha-skill")
            .exists());
    }

    #[test]
    fn local_skill_scope_is_rejected() {
        let dir = tempdir().unwrap();
        let agent = HermesAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let err = agent
            .install_skill(&scope, &skill("alpha-skill", "myapp"))
            .unwrap_err();
        assert!(matches!(
            err,
            HookerError::UnsupportedScope {
                scope: ScopeKind::Local,
                ..
            }
        ));
    }

    #[test]
    fn install_mcp_preserves_unrelated_yaml_keys() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");
        std::fs::write(
            &cfg,
            "model: anthropic/claude\nterminal:\n  backend: local\n",
        )
        .unwrap();

        HermesAgent::install_mcp_config(&cfg, &mcp_spec("github", "myapp")).unwrap();

        let parsed: Value = yaml_serde::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(parsed["model"], json!("anthropic/claude"));
        assert_eq!(parsed["terminal"]["backend"], json!("local"));
        assert_eq!(parsed["mcp_servers"]["github"]["command"], json!("npx"));
        assert_eq!(parsed["mcp_servers"]["github"]["env"]["FOO"], json!("bar"));
    }

    #[test]
    fn install_mcp_is_idempotent() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");
        let spec = mcp_spec("github", "myapp");

        HermesAgent::install_mcp_config(&cfg, &spec).unwrap();
        let second = HermesAgent::install_mcp_config(&cfg, &spec).unwrap();
        assert!(second.already_installed);
    }

    #[test]
    fn uninstall_mcp_round_trip() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");

        HermesAgent::install_mcp_config(&cfg, &mcp_spec("github", "myapp")).unwrap();
        let report = HermesAgent::uninstall_mcp_config(&cfg, "github", "myapp").unwrap();
        assert!(!report.removed.is_empty());
        assert!(!cfg.exists());
    }

    #[test]
    fn uninstall_mcp_owner_mismatch_is_refused() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.yaml");

        HermesAgent::install_mcp_config(&cfg, &mcp_spec("github", "app-a")).unwrap();
        let err = HermesAgent::uninstall_mcp_config(&cfg, "github", "app-b").unwrap_err();
        assert!(matches!(err, HookerError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn local_mcp_scope_is_rejected() {
        let dir = tempdir().unwrap();
        let agent = HermesAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let err = agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap_err();
        assert!(matches!(
            err,
            HookerError::UnsupportedScope {
                scope: ScopeKind::Local,
                ..
            }
        ));
    }

    #[test]
    fn remote_mcp_mapping_uses_url_and_headers() {
        let spec = McpSpec::builder("docs")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Authorization", "Bearer token")
            .build();

        let value = hermes_mcp_value(&spec);
        assert_eq!(value["url"], json!("https://example.com/mcp"));
        assert_eq!(value["headers"]["Authorization"], json!("Bearer token"));
        assert!(value.get("transport").is_none());
    }

    #[test]
    fn mcp_config_path_from_home_uses_hermes_home() {
        let home = PathBuf::from("/tmp/home");
        assert_eq!(
            HermesAgent::mcp_config_path_from_home(&home),
            PathBuf::from("/tmp/home/.hermes/config.yaml")
        );
    }
}
