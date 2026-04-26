//! Integration-level concurrency coverage for shared configs and ledgers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;

use ai_hooker::{Event, HookSpec, HookerError, Matcher, McpSpec, Scope, SkillSpec};
use serde_json::Value;

fn hook_spec(tag: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command("echo concurrency")
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules(format!("Rules for {tag}."))
        .build()
}

fn mcp_spec(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .env("AI_HOOKER_TEST", name)
        .build()
}

fn skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description(format!("Use during concurrency test for {name}."))
        .body(format!("## Goal\nInstall {name}.\n"))
        .build()
}

fn run_two<A, B, FA, FB>(a: FA, b: FB) -> (A, B)
where
    A: Send + 'static,
    B: Send + 'static,
    FA: FnOnce() -> A + Send + 'static,
    FB: FnOnce() -> B + Send + 'static,
{
    let barrier = Arc::new(Barrier::new(3));
    let a_barrier = Arc::clone(&barrier);
    let b_barrier = Arc::clone(&barrier);

    let a_thread = thread::spawn(move || {
        a_barrier.wait();
        a()
    });
    let b_thread = thread::spawn(move || {
        b_barrier.wait();
        b()
    });

    barrier.wait();

    let a_result = a_thread.join().expect("first worker panicked");
    let b_result = b_thread.join().expect("second worker panicked");
    (a_result, b_result)
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert!(
        !bytes.is_empty(),
        "{} should not be truncated",
        path.display()
    );
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn read_ledger(path: &Path) -> Value {
    let ledger = read_json(path);
    assert_eq!(ledger["version"], 2);
    assert!(
        ledger["entries"].is_object(),
        "ledger entries should be an object"
    );
    ledger
}

fn hook_tag_count(settings: &Value, tag: &str) -> usize {
    settings["hooks"]["PreToolUse"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["_ai_hooker_tag"] == tag)
        .count()
}

fn markdown_block_count(path: &Path, tag: &str) -> usize {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.matches(&format!("<!-- BEGIN AI-HOOKER:{tag} -->"))
        .count()
}

fn assert_no_duplicate_hook(settings: &Value, tag: &str) {
    assert_eq!(hook_tag_count(settings, tag), 1, "duplicate hook tag {tag}");
}

fn assert_at_most_one_backup(path: &Path) {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".bak");
    let bak = PathBuf::from(bak);
    assert!(
        !bak.exists() || bak.is_file(),
        "backup path should be a single file: {}",
        bak.display()
    );
}

fn assert_ok<T>(result: Result<T, HookerError>) -> T {
    result.unwrap_or_else(|e| panic!("thread returned error: {e}"))
}

fn is_owner_mismatch<T>(result: &Result<T, HookerError>) -> bool {
    matches!(result, Err(HookerError::NotOwnedByCaller { .. }))
}

#[test]
fn same_hook_tag_identical_content_is_idempotent_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = hook_spec("same-hook");
    let spec_b = spec_a.clone();

    let (a, b) = run_two(
        move || {
            ai_hooker::by_id("claude")
                .unwrap()
                .install(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::by_id("claude")
                .unwrap()
                .install(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let settings_path = dir.path().join(".claude/settings.json");
    let settings = read_json(&settings_path);
    assert_no_duplicate_hook(&settings, "same-hook");
    assert_eq!(
        markdown_block_count(&dir.path().join("CLAUDE.md"), "same-hook"),
        1
    );
    assert_at_most_one_backup(&settings_path);
    assert_at_most_one_backup(&dir.path().join("CLAUDE.md"));
}

#[test]
fn different_hook_tags_share_markdown_file_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = hook_spec("hook-alpha");
    let spec_b = hook_spec("hook-beta");

    let (a, b) = run_two(
        move || {
            ai_hooker::by_id("claude")
                .unwrap()
                .install(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::by_id("claude")
                .unwrap()
                .install(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let settings = read_json(&dir.path().join(".claude/settings.json"));
    assert_no_duplicate_hook(&settings, "hook-alpha");
    assert_no_duplicate_hook(&settings, "hook-beta");
    let memory = dir.path().join("CLAUDE.md");
    assert_eq!(markdown_block_count(&memory, "hook-alpha"), 1);
    assert_eq!(markdown_block_count(&memory, "hook-beta"), 1);
    assert_at_most_one_backup(&memory);
}

#[test]
fn different_mcp_names_share_config_and_ledger_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = mcp_spec("mcp-alpha", "owner-a");
    let spec_b = mcp_spec("mcp-beta", "owner-b");

    let (a, b) = run_two(
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let cfg_path = dir.path().join(".mcp.json");
    let cfg = read_json(&cfg_path);
    assert_eq!(cfg["mcpServers"]["mcp-alpha"]["command"], "npx");
    assert_eq!(cfg["mcpServers"]["mcp-beta"]["command"], "npx");
    let ledger = read_ledger(&dir.path().join(".ai-hooker-mcp.json"));
    assert_eq!(ledger["entries"]["mcp-alpha"]["owner"], "owner-a");
    assert_eq!(ledger["entries"]["mcp-beta"]["owner"], "owner-b");
    assert_at_most_one_backup(&cfg_path);
}

#[test]
fn same_mcp_name_same_owner_same_content_is_idempotent_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = mcp_spec("shared-mcp", "same-owner");
    let spec_b = spec_a.clone();

    let (a, b) = run_two(
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let cfg = read_json(&dir.path().join(".mcp.json"));
    let servers = cfg["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
    assert!(servers.contains_key("shared-mcp"));
    let ledger = read_ledger(&dir.path().join(".ai-hooker-mcp.json"));
    let entries = ledger["entries"].as_object().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries["shared-mcp"]["owner"], "same-owner");
}

#[test]
fn same_mcp_name_different_owners_returns_controlled_error_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = mcp_spec("contended-mcp", "owner-a");
    let spec_b = mcp_spec("contended-mcp", "owner-b");

    let (a, b) = run_two(
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_b, &spec_b)
        },
    );

    assert_eq!(a.is_ok() as usize + b.is_ok() as usize, 1);
    assert_eq!(
        is_owner_mismatch(&a) as usize + is_owner_mismatch(&b) as usize,
        1
    );
    let cfg = read_json(&dir.path().join(".mcp.json"));
    let servers = cfg["mcpServers"].as_object().unwrap();
    assert_eq!(servers.len(), 1);
    assert!(servers.contains_key("contended-mcp"));
    let ledger = read_ledger(&dir.path().join(".ai-hooker-mcp.json"));
    let owner = ledger["entries"]["contended-mcp"]["owner"]
        .as_str()
        .unwrap();
    assert!(matches!(owner, "owner-a" | "owner-b"));
}

#[test]
fn install_and_uninstall_same_mcp_name_leave_valid_winner_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope = Scope::Local(root.clone());
    let spec = mcp_spec("flapping-mcp", "owner-a");
    ai_hooker::mcp_by_id("claude")
        .unwrap()
        .install_mcp(&scope, &spec)
        .unwrap();

    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_b = spec.clone();
    let (install, uninstall) = run_two(
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_a, &spec_b)
        },
        move || {
            ai_hooker::mcp_by_id("claude").unwrap().uninstall_mcp(
                &scope_b,
                "flapping-mcp",
                "owner-a",
            )
        },
    );

    assert_ok(install);
    assert_ok(uninstall);
    let cfg_path = dir.path().join(".mcp.json");
    let ledger_path = dir.path().join(".ai-hooker-mcp.json");
    match (cfg_path.exists(), ledger_path.exists()) {
        (true, true) => {
            let cfg = read_json(&cfg_path);
            assert!(cfg["mcpServers"]["flapping-mcp"].is_object());
            let ledger = read_ledger(&ledger_path);
            assert_eq!(ledger["entries"]["flapping-mcp"]["owner"], "owner-a");
        }
        (false, false) => {}
        other => panic!("config and ledger should agree, got {other:?}"),
    }
}

#[test]
fn uninstall_one_owner_while_other_owner_installs_different_mcp_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope = Scope::Local(root.clone());
    ai_hooker::mcp_by_id("claude")
        .unwrap()
        .install_mcp(&scope, &mcp_spec("old-mcp", "owner-a"))
        .unwrap();

    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let new_spec = mcp_spec("new-mcp", "owner-b");
    let (uninstall, install) = run_two(
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .uninstall_mcp(&scope_a, "old-mcp", "owner-a")
        },
        move || {
            ai_hooker::mcp_by_id("claude")
                .unwrap()
                .install_mcp(&scope_b, &new_spec)
        },
    );

    assert_ok(uninstall);
    assert_ok(install);
    let cfg = read_json(&dir.path().join(".mcp.json"));
    assert!(cfg["mcpServers"]["old-mcp"].is_null());
    assert!(cfg["mcpServers"]["new-mcp"].is_object());
    let ledger = read_ledger(&dir.path().join(".ai-hooker-mcp.json"));
    assert!(ledger["entries"]["old-mcp"].is_null());
    assert_eq!(ledger["entries"]["new-mcp"]["owner"], "owner-b");
}

#[test]
fn different_skills_share_root_and_ledger_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = skill_spec("skill-alpha", "owner-a");
    let spec_b = skill_spec("skill-beta", "owner-b");

    let (a, b) = run_two(
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let skills_root = dir.path().join(".claude/skills");
    assert!(skills_root.join("skill-alpha/SKILL.md").is_file());
    assert!(skills_root.join("skill-beta/SKILL.md").is_file());
    let ledger = read_ledger(&skills_root.join(".ai-hooker-skills.json"));
    assert_eq!(ledger["entries"]["skill-alpha"]["owner"], "owner-a");
    assert_eq!(ledger["entries"]["skill-beta"]["owner"], "owner-b");
}

#[test]
fn same_skill_same_owner_is_idempotent_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root.clone());
    let spec_a = skill_spec("shared-skill", "same-owner");
    let spec_b = spec_a.clone();

    let (a, b) = run_two(
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_b, &spec_b)
        },
    );

    assert_ok(a);
    assert_ok(b);
    let skills_root = dir.path().join(".claude/skills");
    assert!(skills_root.join("shared-skill/SKILL.md").is_file());
    let ledger = read_ledger(&skills_root.join(".ai-hooker-skills.json"));
    let entries = ledger["entries"].as_object().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries["shared-skill"]["owner"], "same-owner");
}

#[test]
fn same_skill_different_owners_returns_controlled_error_under_race() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let scope_a = Scope::Local(root.clone());
    let scope_b = Scope::Local(root);
    let spec_a = skill_spec("contended-skill", "owner-a");
    let spec_b = skill_spec("contended-skill", "owner-b");

    let (a, b) = run_two(
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_a, &spec_a)
        },
        move || {
            ai_hooker::skill_by_id("claude")
                .unwrap()
                .install_skill(&scope_b, &spec_b)
        },
    );

    assert_eq!(a.is_ok() as usize + b.is_ok() as usize, 1);
    assert_eq!(
        is_owner_mismatch(&a) as usize + is_owner_mismatch(&b) as usize,
        1
    );
    let skills_root = dir.path().join(".claude/skills");
    assert!(skills_root.join("contended-skill/SKILL.md").is_file());
    let ledger = read_ledger(&skills_root.join(".ai-hooker-skills.json"));
    let owner = ledger["entries"]["contended-skill"]["owner"]
        .as_str()
        .unwrap();
    assert!(matches!(owner, "owner-a" | "owner-b"));
}
