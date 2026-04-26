#![allow(unused_must_use)]

//! Golden config-shape coverage for hooks, MCP, and skills.
//!
//! Fixtures live under `tests/golden/<surface>/<agent>/<scenario>.golden`.
//! Update them deliberately with:
//!
//! ```text
//! AGENT_CONFIG_UPDATE_GOLDENS=1 cargo test --test golden
//! ```

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use agent_config::{
    all, by_id, mcp_by_id, mcp_capable, skill_by_id, skill_capable, Event, HookSpec, Matcher,
    McpSpec, Scope, ScopeKind, SkillAsset, SkillSpec,
};
use pretty_assertions::assert_eq;
use serde_json::{Map, Value};
use tempfile::TempDir;

const OWNER: &str = "golden-owner";
const OTHER_OWNER: &str = "other-owner";
const TARGET_MCP: &str = "golden-server";
const SIBLING_MCP: &str = "sibling-server";
const TARGET_HOOK: &str = "golden-hook";
const SIBLING_HOOK: &str = "sibling-hook";
const TARGET_SKILL: &str = "golden-skill";
const SIBLING_SKILL: &str = "sibling-skill";

#[test]
fn golden_snapshots_match() {
    for agent in mcp_capable() {
        let id = agent.id();
        drop(agent);
        run_mcp_goldens(id);
    }

    for agent in all() {
        let id = agent.id();
        drop(agent);
        run_hook_goldens(id);
    }

    for agent in skill_capable() {
        let id = agent.id();
        drop(agent);
        run_skill_goldens(id);
    }
}

fn run_mcp_goldens(agent_id: &str) {
    mcp_case(agent_id, "empty_config_install", |agent, scope, _env| {
        let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
        format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
    });

    mcp_case(
        agent_id,
        "existing_unrelated_config_install",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(SIBLING_MCP, OTHER_OWNER))
                .unwrap();
            remove_mcp_ledger(agent, scope, SIBLING_MCP, OTHER_OWNER);
            let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
        },
    );

    mcp_case(
        agent_id,
        "existing_user_installed_same_name_no_ledger",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER))
                .unwrap();
            remove_mcp_ledger(agent, scope, TARGET_MCP, OWNER);
            let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
        },
    );

    mcp_case(
        agent_id,
        "existing_owned_same_name_reinstall",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER))
                .unwrap();
            let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
        },
    );

    mcp_case(
        agent_id,
        "existing_other_owner_same_name",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OTHER_OWNER))
                .unwrap();
            let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
        },
    );

    mcp_case(agent_id, "http_transport", |agent, scope, _env| {
        let install = agent.install_mcp(scope, &mcp_http(TARGET_MCP, OWNER));
        format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
    });

    mcp_case(agent_id, "sse_transport", |agent, scope, _env| {
        let install = agent.install_mcp(scope, &mcp_sse(TARGET_MCP, OWNER));
        format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
    });

    mcp_case(
        agent_id,
        "stdio_transport_with_env",
        |agent, scope, _env| {
            let install = agent.install_mcp(scope, &mcp_stdio_env(TARGET_MCP, OWNER));
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install)
        },
    );

    mcp_case(
        agent_id,
        "uninstall_only_managed_entry",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER))
                .unwrap();
            let uninstall = agent.uninstall_mcp(scope, TARGET_MCP, OWNER);
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "uninstall", uninstall)
        },
    );

    mcp_case(
        agent_id,
        "uninstall_preserves_siblings",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER))
                .unwrap();
            agent
                .install_mcp(scope, &mcp_stdio(SIBLING_MCP, OWNER))
                .unwrap();
            let uninstall = agent.uninstall_mcp(scope, TARGET_MCP, OWNER);
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "uninstall", uninstall)
        },
    );

    mcp_case(
        agent_id,
        "uninstall_final_entry_prunes",
        |agent, scope, _env| {
            agent
                .install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER))
                .unwrap();
            agent.uninstall_mcp(scope, TARGET_MCP, OWNER).unwrap();
            let uninstall = agent.uninstall_mcp(scope, TARGET_MCP, OWNER);
            format_mcp_result(agent, scope, TARGET_MCP, OWNER, "uninstall", uninstall)
        },
    );

    mcp_case(
        agent_id,
        "invalid_config_no_rewrite",
        |agent, scope, _env| {
            let status = agent.mcp_status(scope, TARGET_MCP, OWNER).unwrap();
            let config = status
                .config_path
                .expect("MCP status should expose config path");
            if let Some(parent) = config.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let invalid = invalid_config_for(&config);
            fs::write(&config, invalid.as_bytes()).unwrap();
            let install = agent.install_mcp(scope, &mcp_stdio(TARGET_MCP, OWNER));
            let preserved = fs::read_to_string(&config).unwrap() == invalid;
            let mut out = format_mcp_result(agent, scope, TARGET_MCP, OWNER, "install", install);
            out.push_str(&format!("invalid_preserved: {preserved}\n"));
            out
        },
    );
}

fn run_hook_goldens(agent_id: &str) {
    hook_case(agent_id, "empty_rules_install", |agent, scope, _env| {
        let install = agent.install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."));
        format_hook_result(agent, scope, TARGET_HOOK, "install", install)
    });

    hook_case(agent_id, "existing_same_reinstall", |agent, scope, _env| {
        agent
            .install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."))
            .unwrap();
        let install = agent.install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."));
        format_hook_result(agent, scope, TARGET_HOOK, "install", install)
    });

    hook_case(
        agent_id,
        "different_consumer_preserved",
        |agent, scope, _env| {
            agent
                .install(scope, &hook_spec(SIBLING_HOOK, "Use sibling hook rules."))
                .unwrap();
            let install = agent.install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."));
            format_hook_result(agent, scope, TARGET_HOOK, "install", install)
        },
    );

    hook_case(
        agent_id,
        "uninstall_one_preserves_sibling",
        |agent, scope, _env| {
            agent
                .install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."))
                .unwrap();
            agent
                .install(scope, &hook_spec(SIBLING_HOOK, "Use sibling hook rules."))
                .unwrap();
            let uninstall = agent.uninstall(scope, TARGET_HOOK);
            format_hook_result(agent, scope, TARGET_HOOK, "uninstall", uninstall)
        },
    );

    hook_case(
        agent_id,
        "uninstall_final_block_or_file",
        |agent, scope, _env| {
            agent
                .install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."))
                .unwrap();
            let uninstall = agent.uninstall(scope, TARGET_HOOK);
            format_hook_result(agent, scope, TARGET_HOOK, "uninstall", uninstall)
        },
    );

    if prose_path(agent_id, Path::new("[ROOT]")).is_some() {
        hook_case(
            agent_id,
            "existing_prose_install_fenced_block",
            |agent, scope, env| {
                let path = prose_path(agent_id, &env.project).unwrap();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&path, "Existing project guidance.\n").unwrap();
                let install =
                    agent.install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."));
                format_hook_result(agent, scope, TARGET_HOOK, "install", install)
            },
        );

        hook_case(
            agent_id,
            "uninstall_fenced_block_preserves_prose",
            |agent, scope, env| {
                let path = prose_path(agent_id, &env.project).unwrap();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&path, "Existing project guidance.\n").unwrap();
                agent
                    .install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."))
                    .unwrap();
                let uninstall = agent.uninstall(scope, TARGET_HOOK);
                format_hook_result(agent, scope, TARGET_HOOK, "uninstall", uninstall)
            },
        );
    }

    if directory_rules_agent(agent_id) {
        hook_case(
            agent_id,
            "directory_rules_one_file_per_consumer",
            |agent, scope, _env| {
                agent
                    .install(scope, &hook_spec(SIBLING_HOOK, "Use sibling hook rules."))
                    .unwrap();
                let install =
                    agent.install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."));
                format_hook_result(agent, scope, TARGET_HOOK, "install", install)
            },
        );

        hook_case(
            agent_id,
            "directory_uninstall_prunes_only_when_safe",
            |agent, scope, _env| {
                agent
                    .install(scope, &hook_spec(TARGET_HOOK, "Use golden hook rules."))
                    .unwrap();
                agent
                    .install(scope, &hook_spec(SIBLING_HOOK, "Use sibling hook rules."))
                    .unwrap();
                agent.uninstall(scope, TARGET_HOOK).unwrap();
                let uninstall = agent.uninstall(scope, SIBLING_HOOK);
                format_hook_result(agent, scope, SIBLING_HOOK, "uninstall", uninstall)
            },
        );
    }
}

fn run_skill_goldens(agent_id: &str) {
    skill_case(agent_id, "minimal_skill_md", |agent, scope, _env| {
        let install = agent.install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER));
        format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
    });

    skill_case(
        agent_id,
        "skill_md_with_allowed_tools",
        |agent, scope, _env| {
            let install = agent.install_skill(scope, &skill_allowed_tools(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "skill_with_scripts_references_assets",
        |agent, scope, _env| {
            let install = agent.install_skill(scope, &skill_with_assets(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "executable_script_mode_unix",
        |agent, scope, _env| {
            let install = agent.install_skill(scope, &skill_with_assets(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "reinstall_identical_skill",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER))
                .unwrap();
            let install = agent.install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "update_owned_skill_content",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER))
                .unwrap();
            let install = agent.install_skill(scope, &skill_updated(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "refuse_other_owner_skill",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OTHER_OWNER))
                .unwrap();
            let install = agent.install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "refuse_user_installed_skill_directory",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER))
                .unwrap();
            remove_skill_ledger(agent, scope, TARGET_SKILL, OWNER);
            let install = agent.install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );

    skill_case(
        agent_id,
        "uninstall_removes_skill_and_ledger_entry",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER))
                .unwrap();
            agent
                .install_skill(scope, &skill_minimal(SIBLING_SKILL, OWNER))
                .unwrap();
            let uninstall = agent.uninstall_skill(scope, TARGET_SKILL, OWNER);
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "uninstall", uninstall)
        },
    );

    skill_case(
        agent_id,
        "uninstall_final_skill_removes_empty_ledger",
        |agent, scope, _env| {
            agent
                .install_skill(scope, &skill_minimal(TARGET_SKILL, OWNER))
                .unwrap();
            let uninstall = agent.uninstall_skill(scope, TARGET_SKILL, OWNER);
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "uninstall", uninstall)
        },
    );

    skill_case(
        agent_id,
        "asset_path_escape_rejected",
        |agent, scope, _env| {
            let install = agent.install_skill(scope, &skill_escape(TARGET_SKILL, OWNER));
            format_skill_result(agent, scope, TARGET_SKILL, OWNER, "install", install)
        },
    );
}

fn mcp_case(
    agent_id: &str,
    scenario: &str,
    run: impl FnOnce(&dyn agent_config::McpSurface, &Scope, &CaseEnv) -> String,
) {
    run_case("mcp", agent_id, scenario, |env| {
        let agent = mcp_by_id(agent_id).expect(agent_id);
        let scope = if agent.supported_mcp_scopes().contains(&ScopeKind::Local) {
            Scope::Local(env.project.clone())
        } else {
            Scope::Global
        };
        run(agent.as_ref(), &scope, env)
    });
}

fn hook_case(
    agent_id: &str,
    scenario: &str,
    run: impl FnOnce(&dyn agent_config::Integration, &Scope, &CaseEnv) -> String,
) {
    run_case("hooks", agent_id, scenario, |env| {
        let agent = by_id(agent_id).expect(agent_id);
        let scope = if agent.supported_scopes().contains(&ScopeKind::Local) {
            Scope::Local(env.project.clone())
        } else {
            Scope::Global
        };
        run(agent.as_ref(), &scope, env)
    });
}

fn skill_case(
    agent_id: &str,
    scenario: &str,
    run: impl FnOnce(&dyn agent_config::SkillSurface, &Scope, &CaseEnv) -> String,
) {
    run_case("skills", agent_id, scenario, |env| {
        let agent = skill_by_id(agent_id).expect(agent_id);
        let scope = if agent.supported_skill_scopes().contains(&ScopeKind::Local) {
            Scope::Local(env.project.clone())
        } else {
            Scope::Global
        };
        run(agent.as_ref(), &scope, env)
    });
}

fn run_case(surface: &str, agent_id: &str, scenario: &str, run: impl FnOnce(&CaseEnv) -> String) {
    let env = CaseEnv::new();
    let _guard = EnvGuard::apply(&env);
    let normalizer = Normalizer::new(&env);

    let details = run(&env);
    let mut actual = String::new();
    actual.push_str(&format!(
        "surface: {surface}\nagent: {agent_id}\nscenario: {scenario}\n\n"
    ));
    actual.push_str(&normalizer.normalize(&details));
    actual.push('\n');
    actual.push_str(&capture_tree("ROOT", &env.project, &normalizer));
    actual.push('\n');
    actual.push_str(&capture_tree("HOME", &env.home, &normalizer));

    assert_golden(surface, agent_id, scenario, &actual);
}

fn format_mcp_result<T: std::fmt::Debug>(
    agent: &dyn agent_config::McpSurface,
    scope: &Scope,
    name: &str,
    owner: &str,
    operation: &str,
    result: Result<T, agent_config::AgentConfigError>,
) -> String {
    let mut out = format!("{operation}_result:\n{:#?}\n", result);
    out.push_str(&format!(
        "status:\n{:#?}\n",
        agent.mcp_status(scope, name, owner)
    ));
    out.push_str(&format!(
        "validation:\n{:#?}\n",
        agent.validate_mcp_for_owner(scope, name, Some(owner))
    ));
    out
}

fn format_hook_result<T: std::fmt::Debug>(
    agent: &dyn agent_config::Integration,
    scope: &Scope,
    tag: &str,
    operation: &str,
    result: Result<T, agent_config::AgentConfigError>,
) -> String {
    let mut out = format!("{operation}_result:\n{:#?}\n", result);
    out.push_str(&format!("status:\n{:#?}\n", agent.status(scope, tag)));
    out.push_str(&format!("validation:\n{:#?}\n", agent.validate(scope, tag)));
    out
}

fn format_skill_result<T: std::fmt::Debug>(
    agent: &dyn agent_config::SkillSurface,
    scope: &Scope,
    name: &str,
    owner: &str,
    operation: &str,
    result: Result<T, agent_config::AgentConfigError>,
) -> String {
    let mut out = format!("{operation}_result:\n{:#?}\n", result);
    out.push_str(&format!(
        "status:\n{:#?}\n",
        agent.skill_status(scope, name, owner)
    ));
    out.push_str(&format!(
        "validation:\n{:#?}\n",
        agent.validate_skill_for_owner(scope, name, Some(owner))
    ));
    out
}

fn mcp_stdio(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .build()
}

fn mcp_stdio_env(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .env("API_TOKEN", "golden-token")
        .env("CACHE_DIR", "/tmp/golden-cache")
        // This fixture preserves raw env serialization shape; policy tests cover default refusal.
        .allow_local_inline_secrets()
        .build()
}

fn mcp_http(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .http("https://example.test/mcp")
        .header("Authorization", "Bearer golden-token")
        .build()
}

fn mcp_sse(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .sse("https://example.test/sse")
        .header("X-Golden", "true")
        .build()
}

fn hook_spec(tag: &str, rules: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command_program("golden", ["hook"])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules(rules)
        .build()
}

fn skill_minimal(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during golden snapshot tests.")
        .body("## Goal\nPreserve the config shape.\n")
        .build()
}

fn skill_allowed_tools(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during golden snapshot tests with tools.")
        .allowed_tools(["Bash", "Read", "Write"])
        .body("## Goal\nPreserve allowed tools frontmatter.\n")
        .build()
}

fn skill_with_assets(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during golden snapshot tests with assets.")
        .body("## Goal\nPreserve the full skill directory.\n")
        .asset(SkillAsset {
            relative_path: PathBuf::from("scripts/run.sh"),
            bytes: b"#!/bin/sh\necho golden\n".to_vec(),
            executable: true,
        })
        .asset(SkillAsset {
            relative_path: PathBuf::from("references/notes.md"),
            bytes: b"# Notes\nKeep this reference.\n".to_vec(),
            executable: false,
        })
        .asset(SkillAsset {
            relative_path: PathBuf::from("assets/sample.txt"),
            bytes: b"asset payload\n".to_vec(),
            executable: false,
        })
        .build()
}

fn skill_updated(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during golden snapshot tests after an update.")
        .body("## Goal\nUpdated content must patch in place.\n")
        .build()
}

fn skill_escape(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during golden snapshot escape tests.")
        .body("## Goal\nReject escaping paths.\n")
        .asset(SkillAsset {
            relative_path: PathBuf::from("../escape.txt"),
            bytes: b"escape\n".to_vec(),
            executable: false,
        })
        .build()
}

fn remove_mcp_ledger(agent: &dyn agent_config::McpSurface, scope: &Scope, name: &str, owner: &str) {
    if let Some(path) = agent.mcp_status(scope, name, owner).unwrap().ledger_path {
        let _ = fs::remove_file(path);
    }
}

fn remove_skill_ledger(
    agent: &dyn agent_config::SkillSurface,
    scope: &Scope,
    name: &str,
    owner: &str,
) {
    if let Some(path) = agent.skill_status(scope, name, owner).unwrap().ledger_path {
        let _ = fs::remove_file(path);
    }
}

fn invalid_config_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("toml") => "[mcp_servers\nbroken = true\n".into(),
        Some("yaml") | Some("yml") => "mcp_servers: [unterminated\n".into(),
        _ => "{ invalid json\n".into(),
    }
}

fn prose_path(agent_id: &str, root: &Path) -> Option<PathBuf> {
    Some(match agent_id {
        "claude" => root.join("CLAUDE.md"),
        "gemini" => root.join("GEMINI.md"),
        "codex" | "openclaw" => root.join("AGENTS.md"),
        "copilot" => root.join(".github").join("copilot-instructions.md"),
        "hermes" => root.join(".hermes.md"),
        _ => return None,
    })
}

fn directory_rules_agent(agent_id: &str) -> bool {
    matches!(
        agent_id,
        "cline" | "roo" | "windsurf" | "kilocode" | "antigravity"
    )
}

fn capture_tree(label: &str, root: &Path, normalizer: &Normalizer) -> String {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    collect_entries(root, root, &mut dirs, &mut files);
    dirs.sort();
    files.sort();

    let mut out = format!("{label} tree:\n");
    if dirs.is_empty() && files.is_empty() {
        out.push_str("<empty>\n");
        return out;
    }

    if !dirs.is_empty() {
        out.push_str("directories:\n");
        for dir in dirs {
            out.push_str(&format!("- {}\n", slash_path(&dir)));
        }
    }

    if !files.is_empty() {
        out.push_str("files:\n");
        for file in files {
            let absolute = root.join(&file);
            out.push_str(&format!("- {}\n", slash_path(&file)));
            if executable(&absolute) {
                out.push_str("  mode: 755\n");
            }
            let bytes = fs::read(&absolute).unwrap();
            match String::from_utf8(bytes) {
                Ok(text) => {
                    let text = canonical_file_text(&absolute, &text);
                    out.push_str("  content:\n");
                    out.push_str("```text\n");
                    out.push_str(&normalizer.normalize(&text));
                    if !text.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("```\n");
                }
                Err(err) => {
                    out.push_str(&format!("  bytes: {:?}\n", err.into_bytes()));
                }
            }
        }
    }
    out
}

fn collect_entries(root: &Path, current: &Path, dirs: &mut Vec<PathBuf>, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().contains(".agent-config.lock") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap().to_path_buf();
        if path.is_dir() {
            dirs.push(rel);
            collect_entries(root, &path, dirs, files);
        } else {
            files.push(rel);
        }
    }
}

fn canonical_file_text(path: &Path, text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return text;
    };
    if matches!(name, ".agent-config-mcp.json" | ".agent-config-skills.json") {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            return serde_json::to_string_pretty(&sort_json(value)).unwrap() + "\n";
        }
    }
    text
}

fn sort_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json).collect()),
        Value::Object(map) => {
            let mut pairs: Vec<_> = map.into_iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut sorted = Map::new();
            for (key, value) in pairs {
                sorted.insert(key, sort_json(value));
            }
            Value::Object(sorted)
        }
        other => other,
    }
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn assert_golden(surface: &str, agent_id: &str, scenario: &str, actual: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(surface)
        .join(agent_id)
        .join(format!("{scenario}.golden"));

    if std::env::var_os("AGENT_CONFIG_UPDATE_GOLDENS").as_deref() == Some("1".as_ref()) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing or unreadable golden fixture {}: {e}. Run AGENT_CONFIG_UPDATE_GOLDENS=1 cargo test --test golden to refresh deliberately.",
            path.display()
        )
    });
    assert_eq!(
        expected,
        actual,
        "golden fixture drifted: {}",
        path.display()
    );
}

struct CaseEnv {
    _tmp: TempDir,
    project: PathBuf,
    home: PathBuf,
    xdg: PathBuf,
    codex_home: PathBuf,
}

impl CaseEnv {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("project");
        let home = tmp.path().join("home");
        let xdg = home.join("Library").join("Application Support");
        let codex_home = home.join(".codex");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&home).unwrap();
        Self {
            _tmp: tmp,
            project,
            home,
            xdg,
            codex_home,
        }
    }
}

struct EnvGuard {
    vars: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn apply(env: &CaseEnv) -> Self {
        let keys = [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "CODEX_HOME",
        ];
        let guard = Self {
            vars: keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        };
        std::env::set_var("HOME", &env.home);
        std::env::set_var("USERPROFILE", &env.home);
        std::env::set_var("APPDATA", &env.xdg);
        std::env::set_var("LOCALAPPDATA", env.home.join("AppData").join("Local"));
        std::env::set_var("XDG_CONFIG_HOME", &env.xdg);
        std::env::set_var("CODEX_HOME", &env.codex_home);
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.vars.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

struct Normalizer {
    replacements: Vec<(String, &'static str)>,
}

impl Normalizer {
    fn new(env: &CaseEnv) -> Self {
        let mut replacements = Vec::new();
        push_path_replacements(&mut replacements, &env.project, "[ROOT]");
        push_path_replacements(&mut replacements, &env.home, "[HOME]");
        push_path_replacements(
            &mut replacements,
            &env.xdg,
            "[HOME]/Library/Application Support",
        );
        push_path_replacements(&mut replacements, &env.codex_home, "[CODEX_HOME]");
        replacements.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        Self { replacements }
    }

    fn normalize(&self, input: &str) -> String {
        let mut out = input.replace("\r\n", "\n");
        for (from, to) in &self.replacements {
            out = out.replace(from, to);
        }
        out
    }
}

fn push_path_replacements(
    replacements: &mut Vec<(String, &'static str)>,
    path: &Path,
    to: &'static str,
) {
    let raw = path.to_string_lossy().into_owned();
    replacements.push((raw.replace('\\', "\\\\"), to));
    replacements.push((raw.replace('\\', "/"), to));
    replacements.push((raw, to));
}

#[test]
fn normalizer_redacts_debug_escaped_windows_paths() {
    let tmp = TempDir::new().unwrap();
    let env = CaseEnv {
        _tmp: tmp,
        project: PathBuf::from(r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmp123\project"),
        home: PathBuf::from(r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmp123\home"),
        xdg: PathBuf::from(
            r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmp123\home\Library\Application Support",
        ),
        codex_home: PathBuf::from(r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmp123\home\.codex"),
    };

    let normalizer = Normalizer::new(&env);
    let input = r#""C:\\Users\\RUNNER~1\\AppData\\Local\\Temp\\.tmp123\\project\\.mcp.json""#;

    assert_eq!(normalizer.normalize(input), r#""[ROOT]\\.mcp.json""#);
}
