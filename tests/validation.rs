//! Public-API acceptance tests for drift validation.

use ai_hooker::{
    by_id, mcp_by_id, skill_by_id, DriftIssue, Event, HookSpec, McpSpec, Scope, SkillSpec,
};

fn mcp_spec(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .build()
}

fn skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("A test skill for validation checks.")
        .body("## Goal\nDo the thing.\n")
        .build()
}

fn hook_spec(tag: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command("echo ok")
        .event(Event::PreToolUse)
        .build()
}

#[test]
fn mcp_validation_reports_ledger_only_when_config_missing() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    agent
        .install_mcp(&scope, &mcp_spec("smoketest-server", "myapp"))
        .unwrap();

    std::fs::remove_file(dir.path().join(".mcp.json")).unwrap();

    let report = agent.validate_mcp(&scope, "smoketest-server").unwrap();
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::LedgerOnly { owner: Some(owner), .. } if owner == "myapp")),
        "expected LedgerOnly, got {:?}",
        report.issues
    );
}

#[test]
fn mcp_validation_reports_config_only_when_ledger_missing() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(
        dir.path().join(".mcp.json"),
        r#"{ "mcpServers": { "user-thing": { "command": "user-cmd" } } }"#,
    )
    .unwrap();

    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "user-thing")
        .unwrap();
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::ConfigOnly { .. })),
        "expected ConfigOnly, got {:?}",
        report.issues
    );
}

#[test]
fn mcp_validation_reports_owner_mismatch_when_owner_requested() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    agent
        .install_mcp(&scope, &mcp_spec("smoketest-server", "app-a"))
        .unwrap();

    let report = agent
        .validate_mcp_for_owner(&scope, "smoketest-server", Some("app-b"))
        .unwrap();
    assert!(!report.ok);
    assert!(
        report.issues.iter().any(|issue| {
            matches!(
                issue,
                DriftIssue::OwnerMismatch { expected, actual: Some(actual), .. }
                    if expected == "app-b" && actual == "app-a"
            )
        }),
        "expected OwnerMismatch, got {:?}",
        report.issues
    );

    let ownerless = agent.validate_mcp(&scope, "smoketest-server").unwrap();
    assert!(
        ownerless.ok,
        "ownerless consistency validation should pass, got {:?}",
        ownerless.issues
    );
}

#[test]
fn mcp_validation_reports_malformed_config() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(dir.path().join(".mcp.json"), b"{not valid json").unwrap();

    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "smoketest-server")
        .unwrap();
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::MalformedConfig { .. })),
        "expected MalformedConfig, got {:?}",
        report.issues
    );
}

#[test]
fn mcp_validation_reports_malformed_ledger_without_modifying_it() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(
        dir.path().join(".mcp.json"),
        r#"{ "mcpServers": { "smoketest-server": { "command": "npx" } } }"#,
    )
    .unwrap();
    let ledger = dir.path().join(".ai-hooker-mcp.json");
    std::fs::write(&ledger, b"{not valid ledger").unwrap();
    let before = std::fs::read(&ledger).unwrap();

    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "smoketest-server")
        .unwrap();
    let after = std::fs::read(&ledger).unwrap();

    assert_eq!(before, after, "validation must not repair ledgers");
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::MalformedLedger { .. })),
        "expected MalformedLedger, got {:?}",
        report.issues
    );
}

#[test]
fn mcp_validation_reports_malformed_ledger_shape() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(
        dir.path().join(".mcp.json"),
        r#"{ "mcpServers": { "smoketest-server": { "command": "npx" } } }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".ai-hooker-mcp.json"),
        r#"{ "version": 1, "entries": [] }"#,
    )
    .unwrap();

    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "smoketest-server")
        .unwrap();

    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::MalformedLedger { .. })),
        "expected MalformedLedger, got {:?}",
        report.issues
    );
}

#[test]
fn skill_validation_reports_empty_directory_and_missing_skill_md() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::create_dir_all(dir.path().join(".claude/skills/empty-skill")).unwrap();

    let report = skill_by_id("claude")
        .unwrap()
        .validate_skill(&scope, "empty-skill")
        .unwrap();
    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::SkillMissingSkillMd { .. })),
        "expected SkillMissingSkillMd, got {:?}",
        report.issues
    );
}

#[test]
fn skill_validation_reports_directory_without_skill_md() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let skill_dir = dir.path().join(".claude/skills/no-manifest");
    std::fs::create_dir_all(skill_dir.join("assets")).unwrap();
    std::fs::write(skill_dir.join("assets/readme.txt"), b"asset").unwrap();

    let report = skill_by_id("claude")
        .unwrap()
        .validate_skill(&scope, "no-manifest")
        .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::SkillMissingSkillMd { .. })),
        "expected SkillMissingSkillMd, got {:?}",
        report.issues
    );
}

#[cfg(unix)]
#[test]
fn skill_validation_reports_symlink_escape() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = skill_by_id("claude").unwrap();
    agent
        .install_skill(&scope, &skill_spec("escape-skill", "myapp"))
        .unwrap();
    let link = dir
        .path()
        .join(".claude/skills/escape-skill/assets/outside");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    symlink(outside.path(), &link).unwrap();

    let report = agent.validate_skill(&scope, "escape-skill").unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::SkillAssetEscapesRoot { .. })),
        "expected SkillAssetEscapesRoot, got {:?}",
        report.issues
    );
}

#[cfg(unix)]
#[test]
fn hook_validation_reports_non_executable_script() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = by_id("cline").unwrap();
    agent.install(&scope, &hook_spec("script-owner")).unwrap();

    let script = dir.path().join(".clinerules/hooks/PreToolUse");
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o644);
    std::fs::set_permissions(&script, permissions).unwrap();

    let report = agent.validate(&scope, "script-owner").unwrap();
    assert!(
        report.issues.iter().any(|issue| {
            matches!(
                issue,
                DriftIssue::UnexpectedDirectoryShape { reason, .. }
                    if reason.contains("not executable")
            )
        }),
        "expected non-executable hook issue, got {:?}",
        report.issues
    );
}

#[test]
fn hook_validation_reports_malformed_markdown_fence() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(
        dir.path().join("AGENTS.md"),
        "<!-- BEGIN AI-HOOKER:broken -->\nmissing end\n",
    )
    .unwrap();

    let report = by_id("openclaw")
        .unwrap()
        .validate(&scope, "broken")
        .unwrap();
    assert!(
        report
            .issues
            .iter()
            .any(|issue| matches!(issue, DriftIssue::MalformedConfig { .. })),
        "expected MalformedConfig for malformed fence, got {:?}",
        report.issues
    );
}

#[test]
fn validation_does_not_create_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "missing-server")
        .unwrap();

    assert!(
        report.ok,
        "expected clean absent state, got {:?}",
        report.issues
    );
    assert!(!dir.path().join(".mcp.json").exists());
    assert!(!dir.path().join(".ai-hooker-mcp.json").exists());
}

#[test]
fn validation_accepts_valid_empty_ledger() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::write(
        dir.path().join(".ai-hooker-mcp.json"),
        r#"{ "version": 1, "entries": {} }"#,
    )
    .unwrap();

    let report = mcp_by_id("claude")
        .unwrap()
        .validate_mcp(&scope, "missing-server")
        .unwrap();
    assert!(
        report.ok,
        "expected valid empty ledger, got {:?}",
        report.issues
    );
}

#[test]
fn validation_issue_order_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    std::fs::create_dir_all(dir.path().join(".claude/skills/order-skill")).unwrap();
    let agent = skill_by_id("claude").unwrap();

    let first = agent.validate_skill(&scope, "order-skill").unwrap().issues;
    let second = agent.validate_skill(&scope, "order-skill").unwrap().issues;

    assert_eq!(first, second);
    assert!(
        matches!(first.first(), Some(DriftIssue::ConfigOnly { .. })),
        "expected ConfigOnly first, got {first:?}"
    );
    assert!(
        matches!(first.get(1), Some(DriftIssue::SkillMissingSkillMd { .. })),
        "expected SkillMissingSkillMd second, got {first:?}"
    );
}
