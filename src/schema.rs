//! Live JSON manifest of every registered agent's file layout, surface
//! coverage, and marker conventions.
//!
//! [`build`] walks [`crate::registry::all`] and the per-surface
//! `*_capable` lists, runs probe specs through each agent's `plan_install_*`
//! method, and renders the resulting paths as literal strings keyed by agent
//! id, surface, and scope. Paths under the user's home are rendered with a
//! leading `~/`; project-local paths are rendered with a leading `<project>/`.
//!
//! The output is intentionally machine-readable so external tooling can
//! discover what this crate would touch without depending on the Rust API.
//! `examples/gen_schema.rs` renders [`build`] to `schema/agents.json`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::error::AgentConfigError;
use crate::integration::{InstructionSurface, Integration, McpSurface, SkillSurface};
use crate::paths;
use crate::plan::{PlannedChange, RefusalReason};
use crate::registry::{all, instruction_capable, mcp_capable, skill_capable};
use crate::scope::{Scope, ScopeKind};
use crate::spec::{
    Event, HookSpec, InstructionPlacement, InstructionSpec, Matcher, McpSpec, SkillSpec,
};

/// Sentinel project root used when probing Local-scope plans. Chosen so it
/// will not clash with any real path, and so prefix substitution is
/// unambiguous on output.
const PROJECT_ROOT_SENTINEL: &str = "/__AGENT_CONFIG_PROJECT_ROOT__";

/// Placeholder rendered into the final JSON in place of the user's home dir.
const HOME_PLACEHOLDER: &str = "~";

/// Placeholder rendered into the final JSON in place of the local project root.
const PROJECT_PLACEHOLDER: &str = "<project>";

/// Probe spec names. These leak into rendered paths for skill and instruction
/// surfaces, so they read as placeholders rather than realistic identifiers.
const SKILL_NAME_PROBE: &str = "placeholder";
const INSTRUCTION_NAME_PROBE: &str = "PLACEHOLDER";
const MCP_NAME_PROBE: &str = "placeholder";
const HOOK_TAG_PROBE: &str = "placeholder";
const OWNER_TAG_PROBE: &str = "placeholder";

/// Build the full agent schema as a JSON value.
///
/// The shape is documented at the crate root and stabilised by the golden
/// fixture at `schema/agents.json`. Use the
/// [`crate::registry`] module if a programmatic Rust API is preferable.
///
/// **Platform note.** Cline and Roo's MCP global paths flow through
/// `paths::config_dir()`, which differs across macOS, Linux, and Windows.
/// The committed `schema/agents.json` is the Linux view; regenerate on
/// Linux for canonical output. The companion test only byte-compares on
/// Linux for the same reason.
pub fn build() -> Value {
    let mut root = Map::new();
    root.insert(
        "_warning".into(),
        json!(
            "AUTO-GENERATED. Do not edit by hand. \
             Regenerate on Linux via `cargo run --example gen_schema` \
             or `AGENT_SCHEMA_UPDATE=1 cargo test --test schema_golden`. \
             A few VS Code globalStorage paths are OS-specific; the canonical schema is the Linux view."
        ),
    );
    root.insert("crate_version".into(), json!(env!("CARGO_PKG_VERSION")));
    root.insert("placeholders".into(), placeholders_block());
    root.insert("marker_conventions".into(), marker_conventions_block());
    root.insert("agents".into(), Value::Array(agents_array()));
    Value::Object(root)
}

fn placeholders_block() -> Value {
    json!({
        "home": HOME_PLACEHOLDER,
        "project_root": PROJECT_PLACEHOLDER,
        "skill_name": SKILL_NAME_PROBE,
        "instruction_name": INSTRUCTION_NAME_PROBE,
        "mcp_name": MCP_NAME_PROBE,
        "hook_tag": HOOK_TAG_PROBE,
        "owner_tag": OWNER_TAG_PROBE,
    })
}

fn marker_conventions_block() -> Value {
    json!({
        "json_tag_field": "_agent_config_tag",
        "markdown_fence": {
            "begin": "<!-- BEGIN AGENT-CONFIG:<NAME> -->",
            "end": "<!-- END AGENT-CONFIG:<NAME> -->",
        },
        "instruction_markdown_fence": {
            "begin": "<!-- BEGIN AGENT-CONFIG-INSTR:<NAME> -->",
            "end": "<!-- END AGENT-CONFIG-INSTR:<NAME> -->",
        },
        "ledger_files": {
            "mcp": ".agent-config-mcp.json",
            "skill": ".agent-config-skills.json",
            "instruction": ".agent-config-instructions.json",
        },
        "backup_suffix": ".bak",
    })
}

fn agents_array() -> Vec<Value> {
    let integrations = all();
    let mcp_agents = mcp_capable();
    let skill_agents = skill_capable();
    let instruction_agents = instruction_capable();

    let mut out = Vec::with_capacity(integrations.len());
    for hook_agent in &integrations {
        let id = hook_agent.id();
        let mut entry = Map::new();
        entry.insert("id".into(), json!(id));
        entry.insert("display_name".into(), json!(hook_agent.display_name()));
        entry.insert(
            "supported_scopes".into(),
            scope_list(hook_agent.supported_scopes()),
        );

        let mut surfaces = Map::new();
        if let Some(value) = hook_surface(hook_agent.as_ref()) {
            surfaces.insert("hook".into(), value);
        }
        if let Some(mcp) = mcp_agents.iter().find(|a| a.id() == id) {
            if let Some(value) = mcp_surface(mcp.as_ref()) {
                surfaces.insert("mcp".into(), value);
            }
        }
        if let Some(skill) = skill_agents.iter().find(|a| a.id() == id) {
            if let Some(value) = skill_surface(skill.as_ref()) {
                surfaces.insert("skill".into(), value);
            }
        }
        if let Some(instr) = instruction_agents.iter().find(|a| a.id() == id) {
            if let Some(value) = instruction_surface(instr.as_ref()) {
                surfaces.insert("instruction".into(), value);
            }
        }
        entry.insert("surfaces".into(), Value::Object(surfaces));
        out.push(Value::Object(entry));
    }
    out
}

fn scope_list(scopes: &[ScopeKind]) -> Value {
    let mut v = Vec::new();
    for s in scopes {
        v.push(match s {
            ScopeKind::Global => json!("global"),
            ScopeKind::Local => json!("local"),
        });
    }
    Value::Array(v)
}

fn hook_surface(agent: &dyn Integration) -> Option<Value> {
    let scopes = agent.supported_scopes();
    if scopes.is_empty() {
        return None;
    }
    let mut by_scope = Map::new();
    for kind in scopes {
        let scope = scope_for(*kind);
        let spec = HookSpec::builder(HOOK_TAG_PROBE)
            .command_program("noop", [] as [&str; 0])
            .matcher(Matcher::Bash)
            .event(Event::PreToolUse)
            .rules("placeholder")
            .build();
        let plan_result = agent.plan_install(&scope, &spec);
        by_scope.insert(
            scope_key(*kind).into(),
            changes_to_value(&scope, plan_result),
        );
    }
    Some(json!({
        "supported_scopes": scope_list(scopes),
        "scopes": by_scope,
    }))
}

fn mcp_surface(agent: &dyn McpSurface) -> Option<Value> {
    let scopes = agent.supported_mcp_scopes();
    if scopes.is_empty() {
        return None;
    }
    let mut by_scope = Map::new();
    for kind in scopes {
        let scope = scope_for(*kind);
        // No env to avoid the inline-secret refusal in Local scope.
        let spec = McpSpec::builder(MCP_NAME_PROBE)
            .owner(OWNER_TAG_PROBE)
            .stdio("noop", [] as [&str; 0])
            .build();
        let plan_result = agent.plan_install_mcp(&scope, &spec);
        by_scope.insert(
            scope_key(*kind).into(),
            changes_to_value(&scope, plan_result),
        );
    }
    Some(json!({
        "supported_scopes": scope_list(scopes),
        "scopes": by_scope,
    }))
}

fn skill_surface(agent: &dyn SkillSurface) -> Option<Value> {
    let scopes = agent.supported_skill_scopes();
    if scopes.is_empty() {
        return None;
    }
    let mut by_scope = Map::new();
    for kind in scopes {
        let scope = scope_for(*kind);
        let spec = SkillSpec::builder(SKILL_NAME_PROBE)
            .owner(OWNER_TAG_PROBE)
            .description("placeholder skill for schema generation")
            .body("placeholder")
            .build();
        let plan_result = agent.plan_install_skill(&scope, &spec);
        by_scope.insert(
            scope_key(*kind).into(),
            changes_to_value(&scope, plan_result),
        );
    }
    Some(json!({
        "supported_scopes": scope_list(scopes),
        "scopes": by_scope,
    }))
}

fn instruction_surface(agent: &dyn InstructionSurface) -> Option<Value> {
    let scopes = agent.supported_instruction_scopes();
    if scopes.is_empty() {
        return None;
    }
    let placement = instruction_placement_for(agent.id());
    let mut by_scope = Map::new();
    for kind in scopes {
        let scope = scope_for(*kind);
        let spec = InstructionSpec::builder(INSTRUCTION_NAME_PROBE)
            .owner(OWNER_TAG_PROBE)
            .placement(placement)
            .body("placeholder")
            .build();
        let plan_result = agent.plan_install_instruction(&scope, &spec);
        by_scope.insert(
            scope_key(*kind).into(),
            changes_to_value(&scope, plan_result),
        );
    }
    Some(json!({
        "supported_scopes": scope_list(scopes),
        "placement": placement_label(placement),
        "scopes": by_scope,
    }))
}

fn instruction_placement_for(id: &str) -> InstructionPlacement {
    match id {
        "claude" => InstructionPlacement::ReferencedFile,
        "cline" | "roo" | "kilocode" | "windsurf" | "antigravity" => {
            InstructionPlacement::StandaloneFile
        }
        _ => InstructionPlacement::InlineBlock,
    }
}

fn placement_label(p: InstructionPlacement) -> &'static str {
    match p {
        InstructionPlacement::InlineBlock => "inline_block",
        InstructionPlacement::ReferencedFile => "referenced_file",
        InstructionPlacement::StandaloneFile => "standalone_file",
    }
}

fn scope_for(kind: ScopeKind) -> Scope {
    match kind {
        ScopeKind::Global => Scope::Global,
        ScopeKind::Local => Scope::Local(PathBuf::from(PROJECT_ROOT_SENTINEL)),
    }
}

fn scope_key(kind: ScopeKind) -> &'static str {
    match kind {
        ScopeKind::Global => "global",
        ScopeKind::Local => "local",
    }
}

/// Turn one plan into the per-scope JSON block. On error (e.g. unsupported
/// scope, missing `$HOME`), we record the error string instead of dropping the
/// scope so consumers can see why a surface is unavailable.
fn changes_to_value(
    scope: &Scope,
    plan: Result<crate::plan::InstallPlan, AgentConfigError>,
) -> Value {
    let plan = match plan {
        Ok(p) => p,
        Err(e) => {
            return json!({
                "error": e.to_string(),
                "config_files": [],
                "directories": [],
                "ledger_files": [],
                "refusals": [],
            });
        }
    };

    // Use BTreeSet for deterministic, deduplicated output.
    let mut config_files: BTreeSet<String> = BTreeSet::new();
    let mut directories: BTreeSet<String> = BTreeSet::new();
    let mut ledger_files: BTreeSet<String> = BTreeSet::new();
    let mut refusals: Vec<Value> = Vec::new();

    let render = |p: &Path| -> Option<String> { render_path(scope, p).ok() };

    for change in &plan.changes {
        match change {
            PlannedChange::CreateFile { path } | PlannedChange::PatchFile { path } => {
                if let Some(s) = render(path) {
                    config_files.insert(s);
                }
            }
            PlannedChange::CreateDir { path } => {
                if let Some(s) = render(path) {
                    directories.insert(s);
                }
            }
            PlannedChange::WriteLedger { path, .. } => {
                if let Some(s) = render(path) {
                    ledger_files.insert(s);
                }
            }
            PlannedChange::Refuse { reason, path } => {
                refusals.push(json!({
                    "reason": refusal_label(*reason),
                    "path": path.as_ref().and_then(|p| render(p)),
                }));
            }
            // Other variants (RemoveFile, RestoreBackup, CreateBackup,
            // RemoveDir, RemoveLedgerEntry, SetPermissions, NoOp) only show
            // up in uninstall/repair plans or for paths we already captured
            // via Create/Patch. They are not load-bearing for a layout
            // manifest, so skip.
            _ => {}
        }
    }

    json!({
        "config_files": config_files.into_iter().collect::<Vec<_>>(),
        "directories": directories.into_iter().collect::<Vec<_>>(),
        "ledger_files": ledger_files.into_iter().collect::<Vec<_>>(),
        "refusals": refusals,
    })
}

fn refusal_label(reason: RefusalReason) -> &'static str {
    match reason {
        RefusalReason::OwnerMismatch => "owner_mismatch",
        RefusalReason::UserInstalledEntry => "user_installed_entry",
        RefusalReason::InvalidConfig => "invalid_config",
        RefusalReason::BackupAlreadyExists => "backup_already_exists",
        RefusalReason::UnsupportedScope => "unsupported_scope",
        RefusalReason::MissingRequiredSpecField => "missing_required_spec_field",
        RefusalReason::InlineSecretInLocalScope => "inline_secret_in_local_scope",
        RefusalReason::UnsupportedPlatform => "unsupported_platform",
    }
}

/// Render a planner-produced path with the home or project-root prefix
/// replaced by the corresponding placeholder string. Returns the raw path on
/// platforms where the home dir is unresolvable (so the schema still has data
/// to look at).
fn render_path(scope: &Scope, p: &Path) -> Result<String, AgentConfigError> {
    let s = p.to_string_lossy().to_string();

    if let Scope::Local(_) = scope {
        if let Some(rest) = s.strip_prefix(PROJECT_ROOT_SENTINEL) {
            // strip leading separator if any so the placeholder stays clean
            let trimmed = rest.trim_start_matches('/');
            return Ok(if trimmed.is_empty() {
                PROJECT_PLACEHOLDER.to_string()
            } else {
                format!("{PROJECT_PLACEHOLDER}/{trimmed}")
            });
        }
    }

    if let Ok(home) = paths::home_dir() {
        let home_s = home.to_string_lossy().to_string();
        if let Some(rest) = s.strip_prefix(&home_s) {
            let trimmed = rest.trim_start_matches('/');
            return Ok(if trimmed.is_empty() {
                HOME_PLACEHOLDER.to_string()
            } else {
                format!("{HOME_PLACEHOLDER}/{trimmed}")
            });
        }
    }

    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_emits_warning_and_agents() {
        let v = build();
        assert!(v.get("_warning").and_then(|w| w.as_str()).is_some());
        assert!(v
            .get("crate_version")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .is_some());
        let agents = v.get("agents").and_then(|a| a.as_array()).unwrap();
        assert!(!agents.is_empty(), "schema must have at least one agent");
    }

    #[test]
    fn every_registered_id_present() {
        let v = build();
        let agents = v.get("agents").and_then(|a| a.as_array()).unwrap();
        let ids: Vec<&str> = agents
            .iter()
            .filter_map(|a| a.get("id").and_then(|s| s.as_str()))
            .collect();
        for integ in all() {
            assert!(
                ids.contains(&integ.id()),
                "missing agent {} in schema",
                integ.id()
            );
        }
    }

    #[test]
    fn render_path_substitutes_project_root() {
        let scope = Scope::Local(PathBuf::from(PROJECT_ROOT_SENTINEL));
        let p = PathBuf::from(format!("{PROJECT_ROOT_SENTINEL}/.claude/settings.json"));
        let s = render_path(&scope, &p).unwrap();
        assert_eq!(s, "<project>/.claude/settings.json");
    }

    #[test]
    fn claude_local_hook_path_is_settings_json() {
        // Schema is the canonical Linux view (see `build` doc); on Windows the
        // PathBuf separator is `\` and the assertion against forward-slash
        // path strings is meaningless. Match the convention in
        // `tests/schema_golden.rs` and skip the path-shape check off-Linux.
        if !cfg!(target_os = "linux") {
            return;
        }
        let v = build();
        let agents = v.get("agents").and_then(|a| a.as_array()).unwrap();
        let claude = agents
            .iter()
            .find(|a| a.get("id").and_then(|s| s.as_str()) == Some("claude"))
            .expect("claude in schema");
        let local = claude
            .pointer("/surfaces/hook/scopes/local/config_files")
            .and_then(|v| v.as_array())
            .expect("claude hook local config files");
        let strs: Vec<&str> = local.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            strs.iter().any(|s| s.contains(".claude/settings.json")),
            "expected .claude/settings.json in {strs:?}"
        );
    }
}
