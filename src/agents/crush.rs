//! Charm Crush integration.
//!
//! Crush stores hooks and MCP servers in a single JSONC `crush.json` file. The
//! same file also carries provider settings, LSP config, permissions, and
//! tools — we touch only the `hooks` and `mcp` keys.
//!
//! Surfaces:
//!
//! 1. **Hooks**: tagged entries in `hooks.<EventName>` arrays. Currently
//!    upstream only fires `PreToolUse`; we still pass through `PostToolUse`
//!    and `Custom(_)` since Crush ignores unrecognised events without
//!    erroring (per upstream `docs/hooks/README.md`). The hook entry shape
//!    is flatter than Claude Code's — `{matcher, command, timeout}` with no
//!    nested `hooks: [{type:"command", ...}]` array.
//! 2. **Prompt rules** (optional): tagged HTML-comment fence in `AGENTS.md`,
//!    Crush's documented memory file (or its `initialize_as` alternative —
//!    we always write the default name).
//! 3. **MCP servers**: object map under the `mcp` key (note: not
//!    `mcpServers`), each entry carrying a required `type` discriminant
//!    (`stdio` | `http` | `sse`) plus the per-transport fields.
//! 4. **Skills**: SKILL.md folders under `.crush/skills/` (Local) or
//!    `<crush_home>/skills/` (Global). Crush also picks up
//!    `~/.config/agents/skills/` via its `skills_paths` config; we
//!    intentionally write to the Crush-namespaced root to avoid colliding
//!    with cross-host skill sets.
//! 5. **Instructions**: `InlineBlock` inside `AGENTS.md` (same host as the
//!    rules surface).
//!
//! Note: Crush's `HookConfig` JSON schema declares
//! `additionalProperties: false`. Crush is documented as Claude Code-
//! compatible and Go's `encoding/json` ignores unknown fields by default, so
//! the inline `_agent_config_tag` marker is preserved at runtime even though
//! a schema-aware editor may flag it. If upstream ever switches to strict
//! decoding we'll need to migrate to a sidecar `.agent-config-hooks.json`
//! ledger; defer that until necessary.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::agents::planning as agent_planning;
use crate::error::AgentConfigError;
use crate::integration::{
    InstallReport, InstructionSurface, Integration, McpSurface, SkillSurface, UninstallReport,
};
use crate::paths;
use crate::plan::{has_refusal, InstallPlan, PlanTarget, UninstallPlan};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{Event, HookSpec, InstructionSpec, Matcher, McpSpec, SkillSpec};
use crate::status::StatusReport;
use crate::util::{
    file_lock, fs_atomic, instructions_dir, json_patch, mcp_json_map, md_block, ownership,
    planning, safe_fs, skills_dir,
};

const HOOKS_KEY: &str = "hooks";
const MCP_KEY: &str = "mcp";
const MCP_PATH: &[&str] = &[MCP_KEY];

/// Charm Crush installer.
#[derive(Debug, Clone, Copy, Default)]
pub struct CrushAgent {
    _private: (),
}

impl CrushAgent {
    /// Construct an instance. Stateless.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// `crush_home/crush.json` (Global) or `<root>/crush.json` (Local). Crush
    /// also recognises `.crush.json` at the project root; we standardise on
    /// the unhidden filename to match the auto-init that Crush ships with.
    fn config_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::crush_home()?.join("crush.json"),
            Scope::Local(p) => p.join("crush.json"),
        })
    }

    fn rules_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::crush_home()?.join("AGENTS.md"),
            Scope::Local(p) => p.join("AGENTS.md"),
        })
    }

    fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::crush_home()?.join("skills"),
            Scope::Local(p) => p.join(".crush").join("skills"),
        })
    }

    fn instruction_config_dir(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
        Ok(match scope {
            Scope::Global => paths::crush_home()?,
            Scope::Local(p) => p.join(".crush"),
        })
    }
}

impl Integration for CrushAgent {
    fn id(&self) -> &'static str {
        "crush"
    }

    fn display_name(&self) -> &'static str {
        "Charm Crush"
    }

    fn supported_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn status(&self, scope: &Scope, tag: &str) -> Result<StatusReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let p = Self::config_path(scope)?;
        let presence = json_patch::tagged_hook_presence_jsonc(&p, &[HOOKS_KEY], tag)?;
        Ok(StatusReport::for_tagged_hook(tag, p, presence))
    }

    fn plan_install(
        &self,
        scope: &Scope,
        spec: &HookSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: spec.tag.clone(),
        };
        let p = Self::config_path(scope)?;
        let mut changes = Vec::new();

        let event_key = event_to_string(&spec.event);
        let entry = build_hook_entry(spec);
        planning::plan_tagged_json_upsert(
            &mut changes,
            &p,
            &[HOOKS_KEY, event_key.as_str()],
            &spec.tag,
            entry,
            |_| {},
        )?;
        if has_refusal(&changes) {
            return Ok(InstallPlan::from_changes(target, changes));
        }

        if let Some(rules) = &spec.rules {
            let rules_file = Self::rules_path(scope)?;
            planning::plan_markdown_upsert(&mut changes, &rules_file, &spec.tag, &rules.content)?;
        }

        Ok(InstallPlan::from_changes(target, changes))
    }

    fn plan_uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallPlan, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let target = PlanTarget::Hook {
            integration_id: Integration::id(self),
            scope: scope.clone(),
            tag: tag.to_string(),
        };
        let mut changes = Vec::new();
        let p = Self::config_path(scope)?;
        planning::plan_tagged_json_remove_under(
            &mut changes,
            &p,
            &[HOOKS_KEY],
            tag,
            planning::json_object_empty,
            true,
        )?;
        if has_refusal(&changes) {
            return Ok(UninstallPlan::from_changes(target, changes));
        }

        let rules_file = Self::rules_path(scope)?;
        planning::plan_markdown_remove(&mut changes, &rules_file, tag)?;

        Ok(UninstallPlan::from_changes(target, changes))
    }

    fn install(&self, scope: &Scope, spec: &HookSpec) -> Result<InstallReport, AgentConfigError> {
        HookSpec::validate_tag(&spec.tag)?;
        let mut report = InstallReport::default();

        let p = Self::config_path(scope)?;
        scope.ensure_contained(&p)?;
        file_lock::with_lock(&p, || {
            let mut root = mcp_json_map::read_jsonc_or_empty(&p)?;

            let event_key = event_to_string(&spec.event);
            let entry = build_hook_entry(spec);

            let changed = json_patch::upsert_tagged_array_entry(
                &mut root,
                &[HOOKS_KEY, &event_key],
                &spec.tag,
                entry,
            )?;

            if changed {
                let bytes = json_patch::to_pretty(&root);
                let outcome = safe_fs::write(scope, &p, &bytes, true)?;
                if outcome.existed {
                    report.patched.push(outcome.path.clone());
                } else {
                    report.created.push(outcome.path.clone());
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
            } else {
                report.already_installed = true;
            }
            Ok::<(), AgentConfigError>(())
        })?;

        if let Some(rules) = &spec.rules {
            let rules_file = Self::rules_path(scope)?;
            scope.ensure_contained(&rules_file)?;
            file_lock::with_lock(&rules_file, || {
                let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
                let new_host = md_block::upsert(&host, &spec.tag, &rules.content);
                let outcome = safe_fs::write(scope, &rules_file, new_host.as_bytes(), true)?;
                if !outcome.no_change {
                    if outcome.existed {
                        report.patched.push(outcome.path.clone());
                    } else {
                        report.created.push(outcome.path.clone());
                    }
                    report.already_installed = false;
                }
                if let Some(b) = outcome.backup {
                    report.backed_up.push(b);
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        Ok(report)
    }

    fn uninstall(&self, scope: &Scope, tag: &str) -> Result<UninstallReport, AgentConfigError> {
        HookSpec::validate_tag(tag)?;
        let mut report = UninstallReport::default();

        let p = Self::config_path(scope)?;
        scope.ensure_contained(&p)?;
        if p.exists() {
            file_lock::with_lock(&p, || {
                let mut root = mcp_json_map::read_jsonc_or_empty(&p)?;
                let changed = json_patch::remove_tagged_array_entries_under(
                    &mut root,
                    &[HOOKS_KEY],
                    tag,
                )?;
                if changed {
                    let empty = root.as_object().map(|o| o.is_empty()).unwrap_or(true);
                    let bytes = json_patch::to_pretty(&root);
                    if empty && safe_fs::restore_backup_if_matches(scope, &p, &bytes)? {
                        report.restored.push(p.clone());
                    } else if empty {
                        safe_fs::remove_file(scope, &p)?;
                        report.removed.push(p.clone());
                    } else {
                        safe_fs::write(scope, &p, &bytes, false)?;
                        report.patched.push(p.clone());
                    }
                }
                Ok::<(), AgentConfigError>(())
            })?;
        }

        let rules_file = Self::rules_path(scope)?;
        scope.ensure_contained(&rules_file)?;
        file_lock::with_lock(&rules_file, || {
            let host = fs_atomic::read_to_string_or_empty(&rules_file)?;
            let (stripped, removed) = md_block::remove(&host, tag);
            if removed {
                if stripped.trim().is_empty() {
                    if safe_fs::restore_backup_if_matches(scope, &rules_file, stripped.as_bytes())?
                    {
                        report.restored.push(rules_file.clone());
                    } else {
                        safe_fs::remove_file(scope, &rules_file)?;
                        report.removed.push(rules_file.clone());
                    }
                } else {
                    safe_fs::write(scope, &rules_file, stripped.as_bytes(), false)?;
                    report.patched.push(rules_file.clone());
                }
            }
            Ok::<(), AgentConfigError>(())
        })?;

        if report.removed.is_empty() && report.patched.is_empty() && report.restored.is_empty() {
            report.not_installed = true;
        }
        Ok(report)
    }
}

impl McpSurface for CrushAgent {
    fn id(&self) -> &'static str {
        "crush"
    }

    fn supported_mcp_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn mcp_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        let cfg = Self::config_path(scope)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        let presence =
            mcp_json_map::config_presence(&cfg, MCP_PATH, name, mcp_json_map::ConfigFormat::Jsonc)?;
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

    fn plan_install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        agent_planning::mcp_json_map_install(
            McpSurface::id(self),
            scope,
            spec,
            Self::config_path(scope),
            MCP_PATH,
            mcp_json_map::vscode_servers_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn plan_uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::mcp_json_map_uninstall(
            McpSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::config_path(scope),
            MCP_PATH,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn install_mcp(
        &self,
        scope: &Scope,
        spec: &McpSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let cfg = Self::config_path(scope)?;
        spec.validate_local_secret_policy(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::install(
            &cfg,
            &ledger,
            spec,
            MCP_PATH,
            mcp_json_map::vscode_servers_value,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }

    fn uninstall_mcp(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        McpSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let cfg = Self::config_path(scope)?;
        scope.ensure_contained(&cfg)?;
        let ledger = ownership::mcp_ledger_for(&cfg);
        mcp_json_map::uninstall(
            &cfg,
            &ledger,
            name,
            owner_tag,
            "mcp server",
            MCP_PATH,
            mcp_json_map::ConfigFormat::Jsonc,
        )
    }
}

impl SkillSurface for CrushAgent {
    fn id(&self) -> &'static str {
        "crush"
    }

    fn supported_skill_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn skill_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
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
    ) -> Result<InstallPlan, AgentConfigError> {
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
    ) -> Result<UninstallPlan, AgentConfigError> {
        agent_planning::skill_uninstall(
            SkillSurface::id(self),
            scope,
            name,
            owner_tag,
            Self::skills_root(scope),
        )
    }

    fn install_skill(
        &self,
        scope: &Scope,
        spec: &SkillSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        spec.validate()?;
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::install(&root, spec)
    }

    fn uninstall_skill(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        SkillSpec::validate_name(name)?;
        HookSpec::validate_tag(owner_tag)?;
        let root = Self::skills_root(scope)?;
        scope.ensure_contained(&root)?;
        skills_dir::uninstall(&root, name, owner_tag)
    }
}

impl CrushAgent {
    fn inline_layout(
        &self,
        scope: &Scope,
    ) -> Result<instructions_dir::InlineLayout, AgentConfigError> {
        Ok(instructions_dir::InlineLayout {
            config_dir: Self::instruction_config_dir(scope)?,
            host_file: Self::rules_path(scope)?,
        })
    }
}

impl InstructionSurface for CrushAgent {
    fn id(&self) -> &'static str {
        "crush"
    }

    fn supported_instruction_scopes(&self) -> &'static [ScopeKind] {
        &[ScopeKind::Global, ScopeKind::Local]
    }

    fn instruction_status(
        &self,
        scope: &Scope,
        name: &str,
        expected_owner: &str,
    ) -> Result<StatusReport, AgentConfigError> {
        instructions_dir::inline_status(self.inline_layout(scope)?, name, expected_owner)
    }

    fn plan_install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_install(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            spec,
        )
    }

    fn plan_uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallPlan, AgentConfigError> {
        instructions_dir::inline_plan_uninstall(
            InstructionSurface::id(self),
            scope,
            self.inline_layout(scope),
            name,
            owner_tag,
        )
    }

    fn install_instruction(
        &self,
        scope: &Scope,
        spec: &InstructionSpec,
    ) -> Result<InstallReport, AgentConfigError> {
        instructions_dir::inline_install(scope, self.inline_layout(scope)?, spec)
    }

    fn uninstall_instruction(
        &self,
        scope: &Scope,
        name: &str,
        owner_tag: &str,
    ) -> Result<UninstallReport, AgentConfigError> {
        instructions_dir::inline_uninstall(scope, self.inline_layout(scope)?, name, owner_tag)
    }
}

/// Build one Crush hook entry. Crush's `HookConfig` is flatter than Claude's
/// envelope: `{matcher?, command, timeout, _agent_config_tag}` with no nested
/// `hooks: [{type:"command", ...}]` array. The matcher field is omitted for
/// `Matcher::All` so the file is minimal — Crush docs explicitly say "empty
/// matcher matches all tools".
fn build_hook_entry(spec: &HookSpec) -> Value {
    let mut obj = Map::new();
    if let Some(m) = matcher_to_crush(&spec.matcher) {
        obj.insert("matcher".into(), Value::String(m));
    }
    obj.insert(
        "command".into(),
        Value::String(spec.command.render_shell()),
    );
    obj.insert("timeout".into(), json!(30));
    Value::Object(obj)
}

/// Map our generic matcher to the regex Crush expects against lowercase tool
/// names (`bash`, `edit`, `write`, `multiedit`, `view`, `ls`, `grep`, `glob`,
/// `mcp_<server>_<tool>`). Returns `None` for `Matcher::All` so the field is
/// omitted; Crush treats omission as "match all tools".
fn matcher_to_crush(m: &Matcher) -> Option<String> {
    match m {
        Matcher::All => None,
        Matcher::Bash => Some("^bash$".to_string()),
        Matcher::Exact(s) => Some(format!("^{}$", regex_escape(&s.to_ascii_lowercase()))),
        Matcher::AnyOf(items) => {
            let inner = items
                .iter()
                .map(|s| regex_escape(&s.to_ascii_lowercase()))
                .collect::<Vec<_>>()
                .join("|");
            Some(format!("^({inner})$"))
        }
        Matcher::Regex(s) => Some(s.clone()),
    }
}

/// Escape RE2 metacharacters so a literal tool name like `mcp_github_create`
/// or `foo.bar` matches itself when wrapped with `^...$`.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '|'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Crush event names are case-insensitive (per docs both `PreToolUse` and
/// `pre_tool_use` resolve to the same event); we emit the canonical PascalCase
/// spelling so the on-disk file is greppable.
fn event_to_string(e: &Event) -> String {
    match e {
        Event::PreToolUse => "PreToolUse".into(),
        Event::PostToolUse => "PostToolUse".into(),
        Event::Custom(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::Path;
    use tempfile::tempdir;

    fn local_spec(tag: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("./hook.sh", [] as [&str; 0])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .build()
    }

    fn rules_local_spec(tag: &str, rules: &str) -> HookSpec {
        HookSpec::builder(tag)
            .command_program("./hook.sh", [] as [&str; 0])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .rules(rules)
            .build()
    }

    fn mcp_spec(name: &str, owner: &str) -> McpSpec {
        McpSpec::builder(name)
            .owner(owner)
            .stdio("npx", ["-y", "@example/server"])
            .build()
    }

    fn read_jsonc(p: &Path) -> Value {
        // Tests read with the same parser the install path uses so we don't
        // regress on JSONC compatibility.
        mcp_json_map::read_jsonc_or_empty(p).unwrap()
    }

    #[test]
    fn install_writes_flat_hook_entry() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();

        let v = read_jsonc(&dir.path().join("crush.json"));
        let entry = &v["hooks"]["PreToolUse"][0];
        assert_eq!(entry["matcher"], json!("^bash$"));
        assert!(entry.get("command").and_then(Value::as_str).is_some());
        assert_eq!(entry["timeout"], json!(30));
        assert_eq!(entry["_agent_config_tag"], json!("alpha"));
        // No nested {type:"command"} array — Crush hooks are flatter than Claude.
        assert!(entry.get("hooks").is_none());
        assert!(entry.get("type").is_none());
    }

    #[test]
    fn install_omits_matcher_for_all() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("./hook.sh", [] as [&str; 0])
            .matcher(Matcher::All)
            .event(Event::PreToolUse)
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_jsonc(&dir.path().join("crush.json"));
        let entry = &v["hooks"]["PreToolUse"][0];
        assert!(entry.get("matcher").is_none());
    }

    #[test]
    fn install_idempotent() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");
        agent.install(&scope, &spec).unwrap();
        let r2 = agent.install(&scope, &spec).unwrap();
        assert!(r2.already_installed);
    }

    #[test]
    fn install_round_trip_removes_file() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent.uninstall(&scope, "alpha").unwrap();
        assert!(!dir.path().join("crush.json").exists());
    }

    #[test]
    fn install_accepts_jsonc_with_comments() {
        // Pre-existing user `// comments` and trailing commas in crush.json
        // must not break our patch path. Verifies we read via JSONC, not strict
        // JSON, even though we rewrite as strict JSON.
        let dir = tempdir().unwrap();
        let p = dir.path().join("crush.json");
        std::fs::write(
            &p,
            br#"{
  // Crush settings written by hand.
  "options": { "context_paths": ["AGENTS.md"], },
}
"#,
        )
        .unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        let v = read_jsonc(&p);
        // User block survived (we preserved options.context_paths).
        assert_eq!(v["options"]["context_paths"][0], json!("AGENTS.md"));
        // Our hook landed.
        assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], json!("^bash$"));
    }

    #[test]
    fn matcher_anyof_lowercases_and_escapes() {
        // Matcher::AnyOf(["Edit", "Write", "MultiEdit"]) → "^(edit|write|multiedit)$"
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = HookSpec::builder("alpha")
            .command_program("./hook.sh", [] as [&str; 0])
            .matcher(Matcher::AnyOf(vec![
                "Edit".into(),
                "Write".into(),
                "MultiEdit".into(),
            ]))
            .event(Event::PreToolUse)
            .build();
        agent.install(&scope, &spec).unwrap();
        let v = read_jsonc(&dir.path().join("crush.json"));
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["matcher"],
            json!("^(edit|write|multiedit)$")
        );
    }

    #[test]
    fn install_writes_rules_into_agents_md() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install(&scope, &rules_local_spec("alpha", "Use Crush."))
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(body.contains("Use Crush."));
        assert!(body.contains("BEGIN AGENT-CONFIG:alpha"));
    }

    #[test]
    fn install_mcp_writes_mcp_key_with_type_field() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_jsonc(&dir.path().join("crush.json"));
        // Crush uses `mcp` key, not `mcpServers`, and stdio entries carry the
        // required `type` discriminant.
        assert_eq!(v["mcp"]["github"]["type"], json!("stdio"));
        assert_eq!(v["mcp"]["github"]["command"], json!("npx"));
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn install_mcp_http_carries_url_and_type() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .build();
        agent.install_mcp(&scope, &spec).unwrap();
        let v = read_jsonc(&dir.path().join("crush.json"));
        assert_eq!(v["mcp"]["remote"]["type"], json!("http"));
        assert_eq!(v["mcp"]["remote"]["url"], json!("https://example.com/mcp"));
    }

    #[test]
    fn hook_and_mcp_share_crush_json() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent.install(&scope, &local_spec("alpha")).unwrap();
        agent
            .install_mcp(&scope, &mcp_spec("github", "myapp"))
            .unwrap();
        let v = read_jsonc(&dir.path().join("crush.json"));
        assert!(v["hooks"]["PreToolUse"].is_array());
        assert_eq!(v["mcp"]["github"]["command"], json!("npx"));
    }

    #[test]
    fn uninstall_mcp_other_owner_refused() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        agent
            .install_mcp(&scope, &mcp_spec("github", "appA"))
            .unwrap();
        let err = agent.uninstall_mcp(&scope, "github", "appB").unwrap_err();
        assert!(matches!(err, AgentConfigError::NotOwnedByCaller { .. }));
    }

    #[test]
    fn install_skill_writes_dot_crush_skills_dir() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = SkillSpec::builder("my-skill")
            .owner("myapp")
            .description("test")
            .body("Body.")
            .build();
        agent.install_skill(&scope, &spec).unwrap();
        assert!(dir
            .path()
            .join(".crush/skills/my-skill/SKILL.md")
            .exists());
    }

    #[test]
    fn plan_install_then_install_matches() {
        let dir = tempdir().unwrap();
        let agent = CrushAgent::new();
        let scope = Scope::Local(dir.path().to_path_buf());
        let spec = local_spec("alpha");

        let plan = agent.plan_install(&scope, &spec).unwrap();
        assert!(!dir.path().join("crush.json").exists());
        assert!(!plan.changes.is_empty());

        agent.install(&scope, &spec).unwrap();
        assert!(dir.path().join("crush.json").exists());
    }
}
