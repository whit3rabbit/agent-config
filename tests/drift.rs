#![allow(unused_must_use)]

//! Public-API regression tests for drift enforcement on uninstall.
//!
//! These tests exercise the registered integration's `install_mcp` /
//! `uninstall_mcp` and `install_skill` / `uninstall_skill` surfaces directly,
//! proving that ledger-recorded content hashes are checked against the
//! current on-disk state and that drift is refused with
//! `AgentConfigError::ConfigDrifted`.

use std::fs;
use std::path::PathBuf;

use agent_config::{mcp_by_id, skill_by_id, AgentConfigError, McpSpec, Scope, SkillSpec};

fn local_scope(dir: &tempfile::TempDir) -> Scope {
    Scope::Local(dir.path().to_path_buf())
}

fn stdio_spec(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .build()
}

fn basic_skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("Use during the drift regression tests.")
        .body("# Body\nDrift coverage.\n")
        .build()
}

fn read_mcp_json(path: &PathBuf) -> serde_json::Value {
    let bytes = fs::read(path).expect("read .mcp.json");
    serde_json::from_slice(&bytes).expect("parse .mcp.json")
}

#[test]
fn public_api_mcp_uninstall_refuses_when_entry_drifted() {
    let dir = tempfile::tempdir().unwrap();
    let scope = local_scope(&dir);
    let claude = mcp_by_id("claude").expect("claude MCP support registered");
    let cfg = dir.path().join(".mcp.json");

    claude
        .install_mcp(&scope, &stdio_spec("alpha", "appA"))
        .expect("install_mcp succeeds");
    assert!(cfg.exists(), ".mcp.json must exist after install");

    // User edits the entry's command field outside our control.
    let mut v = read_mcp_json(&cfg);
    v["mcpServers"]["alpha"]["command"] = serde_json::json!("uvx");
    let mutated = serde_json::to_vec_pretty(&v).unwrap();
    fs::write(&cfg, &mutated).unwrap();

    let pre_meta = fs::metadata(&cfg).unwrap();
    let pre_modified = pre_meta.modified().unwrap();
    let pre_bytes = fs::read(&cfg).unwrap();

    let err = claude
        .uninstall_mcp(&scope, "alpha", "appA")
        .expect_err("uninstall must refuse on drift");
    assert!(
        matches!(err, AgentConfigError::ConfigDrifted { .. }),
        "expected ConfigDrifted, got {err:?}",
    );

    // The refused uninstall must not have touched the file.
    assert!(cfg.exists(), "config must still exist after refusal");
    let post_bytes = fs::read(&cfg).unwrap();
    assert_eq!(
        pre_bytes, post_bytes,
        "config bytes must be unchanged after refused uninstall",
    );
    let post_meta = fs::metadata(&cfg).unwrap();
    assert_eq!(
        pre_modified,
        post_meta.modified().unwrap(),
        "config mtime must be unchanged after refused uninstall",
    );

    // The drifted edit is still present.
    let v = read_mcp_json(&cfg);
    assert_eq!(
        v["mcpServers"]["alpha"]["command"],
        serde_json::json!("uvx")
    );
}

#[test]
fn public_api_skill_uninstall_refuses_when_skill_md_drifted() {
    let dir = tempfile::tempdir().unwrap();
    let scope = local_scope(&dir);
    let claude = skill_by_id("claude").expect("claude skill support registered");

    claude
        .install_skill(&scope, &basic_skill_spec("alpha-skill", "appA"))
        .expect("install_skill succeeds");

    let skill_md = dir.path().join(".claude/skills/alpha-skill/SKILL.md");
    assert!(skill_md.exists(), "SKILL.md must exist after install");

    // User appends to SKILL.md outside our control.
    let mut s = fs::read_to_string(&skill_md).unwrap();
    s.push_str("\n<!-- user note added by hand -->\n");
    fs::write(&skill_md, &s).unwrap();

    let pre_bytes = fs::read(&skill_md).unwrap();

    let err = claude
        .uninstall_skill(&scope, "alpha-skill", "appA")
        .expect_err("uninstall_skill must refuse on drift");
    assert!(
        matches!(err, AgentConfigError::ConfigDrifted { .. }),
        "expected ConfigDrifted, got {err:?}",
    );

    // The drifted SKILL.md must still exist with its user edits intact.
    assert!(skill_md.exists(), "drifted SKILL.md must not be deleted");
    let post_bytes = fs::read(&skill_md).unwrap();
    assert_eq!(
        pre_bytes, post_bytes,
        "SKILL.md bytes must be unchanged after refused uninstall",
    );
}

#[test]
fn public_api_mcp_sibling_install_does_not_trip_drift_on_first_uninstall() {
    // Per-entry hashing means installing a sibling (different name) into the
    // same shared config must NOT cause the first entry's uninstall to fail
    // with ConfigDrifted. This would have failed under whole-file hashing.
    let dir = tempfile::tempdir().unwrap();
    let scope = local_scope(&dir);
    let claude = mcp_by_id("claude").expect("claude MCP support registered");
    let cfg = dir.path().join(".mcp.json");

    claude
        .install_mcp(&scope, &stdio_spec("alpha", "appA"))
        .expect("install alpha");
    claude
        .install_mcp(&scope, &stdio_spec("beta", "appA"))
        .expect("install beta into same shared config");

    // The shared config now contains both entries; whole-file hash recorded
    // for "alpha" at install time is stale, but per-entry hashing should
    // still let the alpha uninstall succeed.
    claude
        .uninstall_mcp(&scope, "alpha", "appA")
        .expect("uninstall alpha must succeed despite sibling beta");

    let v = read_mcp_json(&cfg);
    assert!(
        v["mcpServers"].get("alpha").is_none(),
        "alpha entry must be removed",
    );
    assert_eq!(
        v["mcpServers"]["beta"]["command"],
        serde_json::json!("npx"),
        "sibling beta entry must be preserved",
    );
}
