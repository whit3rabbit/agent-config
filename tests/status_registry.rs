//! Public-API acceptance tests for `Integration::status`,
//! `McpSurface::mcp_status`, and `SkillSurface::skill_status`.
//!
//! Mirrors the surface-specific `tests/registry.rs`, `tests/mcp_registry.rs`,
//! and `tests/skill_registry.rs` smoke tests, but exercises the richer
//! [`InstallStatus`] reporting introduced by the status checklist work.

use ai_hooker::status::{
    DriftIssue, InstallStatus, PathStatus, PlanTarget, StatusReport, StatusWarning,
};
use ai_hooker::{
    by_id, mcp_by_id, skill_by_id, Event, HookSpec, Matcher, McpSpec, Scope, SkillSpec,
};

const LOCAL_HOOK_AGENTS: &[&str] = &[
    "claude", "cursor", "gemini", "codex", "copilot", "opencode", "cline", "windsurf",
];

const LOCAL_MCP_AGENTS: &[&str] = &[
    "claude",
    "cursor",
    "gemini",
    "codex",
    "copilot",
    "opencode",
    "windsurf",
    "kilocode",
    "antigravity",
    "roo",
];

const LOCAL_SKILL_AGENTS: &[&str] = &[
    "claude",
    "cursor",
    "gemini",
    "codex",
    "copilot",
    "opencode",
    "windsurf",
    "kilocode",
    "antigravity",
    "openclaw",
    "cline",
];

fn hook_spec(tag: &str) -> HookSpec {
    HookSpec::builder(tag)
        .command("smoketest")
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Use smoketest.")
        .build()
}

fn mcp_spec(name: &str, owner: &str) -> McpSpec {
    McpSpec::builder(name)
        .owner(owner)
        .stdio("npx", ["-y", "@example/server"])
        .build()
}

fn skill_spec(name: &str, owner: &str) -> SkillSpec {
    SkillSpec::builder(name)
        .owner(owner)
        .description("A test skill for status checks.")
        .body("## Goal\nDo the thing.\n")
        .build()
}

// ----- Hook status -----

#[test]
fn hook_status_absent_when_nothing_installed() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    for &id in LOCAL_HOOK_AGENTS {
        let agent = by_id(id).expect(id);
        let report = agent.status(&scope, "smoketest").expect(id);
        assert!(
            matches!(report.status, InstallStatus::Absent),
            "{id} expected Absent before install, got {:?}",
            report.status
        );
        assert_eq!(
            report.target,
            PlanTarget::Hook {
                tag: "smoketest".into()
            }
        );
    }
}

#[test]
fn hook_status_owned_after_install() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = hook_spec("smoketest");
    for &id in LOCAL_HOOK_AGENTS {
        let agent = by_id(id).expect(id);
        agent.install(&scope, &spec).expect(id);
        let report = agent.status(&scope, "smoketest").expect(id);
        assert!(
            matches!(report.status, InstallStatus::InstalledOwned { ref owner } if owner == "smoketest"),
            "{id} expected InstalledOwned, got {:?}",
            report.status
        );
        // is_installed compatibility wrapper still returns true.
        assert!(agent.is_installed(&scope, "smoketest").expect(id), "{id}");
    }
}

// ----- MCP status -----

#[test]
fn mcp_status_absent_when_nothing_installed() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    for &id in LOCAL_MCP_AGENTS {
        let agent = mcp_by_id(id).expect(id);
        let report = agent
            .mcp_status(&scope, "smoketest-server", "myapp")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::Absent),
            "{id} expected Absent, got {:?}",
            report.status
        );
        assert_eq!(
            report.target,
            PlanTarget::Mcp {
                name: "smoketest-server".into()
            }
        );
        assert!(
            report.config_path.is_some(),
            "{id} should report config_path"
        );
        assert!(
            report.ledger_path.is_some(),
            "{id} should report ledger_path"
        );
    }
}

#[test]
fn mcp_status_owned_when_caller_is_owner() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = mcp_spec("smoketest-server", "myapp");
    for &id in LOCAL_MCP_AGENTS {
        let agent = mcp_by_id(id).expect(id);
        agent.install_mcp(&scope, &spec).expect(id);
        let report = agent
            .mcp_status(&scope, "smoketest-server", "myapp")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::InstalledOwned { ref owner } if owner == "myapp"),
            "{id} expected InstalledOwned(myapp), got {:?}",
            report.status
        );
        agent
            .uninstall_mcp(&scope, "smoketest-server", "myapp")
            .expect(id);
    }
}

#[test]
fn mcp_status_other_owner_when_caller_differs() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = mcp_spec("smoketest-server", "appA");
    for &id in LOCAL_MCP_AGENTS {
        let agent = mcp_by_id(id).expect(id);
        agent.install_mcp(&scope, &spec).expect(id);
        let report = agent
            .mcp_status(&scope, "smoketest-server", "appB")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::InstalledOtherOwner { ref owner } if owner == "appA"),
            "{id} expected InstalledOtherOwner(appA), got {:?}",
            report.status
        );
        agent
            .uninstall_mcp(&scope, "smoketest-server", "appA")
            .expect(id);
    }
}

#[test]
fn mcp_status_present_unowned_when_hand_installed() {
    // Sanity: pick claude (uses standard mcpServers JSON object).
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    // Hand-installed MCP: file present, no ledger entry.
    let cfg = dir.path().join(".mcp.json");
    std::fs::write(
        &cfg,
        r#"{ "mcpServers": { "user-thing": { "command": "user-cmd" } } }"#,
    )
    .unwrap();

    let agent = mcp_by_id("claude").unwrap();
    let report = agent.mcp_status(&scope, "user-thing", "myapp").unwrap();
    assert!(
        matches!(report.status, InstallStatus::PresentUnowned),
        "expected PresentUnowned, got {:?}",
        report.status
    );
}

#[test]
fn mcp_status_ledger_only_when_config_missing() {
    // Install, then delete the config file behind the library's back. The
    // ledger still claims the entry; status should report LedgerOnly.
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = mcp_spec("smoketest-server", "myapp");

    let agent = mcp_by_id("claude").unwrap();
    agent.install_mcp(&scope, &spec).unwrap();

    let cfg = dir.path().join(".mcp.json");
    std::fs::remove_file(&cfg).unwrap();

    let report = agent
        .mcp_status(&scope, "smoketest-server", "myapp")
        .unwrap();
    assert!(
        matches!(report.status, InstallStatus::LedgerOnly { ref owner } if owner == "myapp"),
        "expected LedgerOnly(myapp), got {:?}",
        report.status
    );
}

#[test]
fn mcp_status_drifted_on_invalid_config() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let cfg = dir.path().join(".mcp.json");
    std::fs::write(&cfg, b"{not valid json").unwrap();

    let agent = mcp_by_id("claude").unwrap();
    // The probe must NOT panic / propagate a parse error.
    let report = agent.mcp_status(&scope, "anything", "myapp").unwrap();
    let issues = match &report.status {
        InstallStatus::Drifted { issues } => issues,
        other => panic!("expected Drifted, got {other:?}"),
    };
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, DriftIssue::InvalidConfig { .. })),
        "expected InvalidConfig drift, got {issues:?}"
    );
    // The malformed file should also surface as PathStatus::Invalid.
    assert!(
        report
            .files
            .iter()
            .any(|f| matches!(f, PathStatus::Invalid { .. })),
        "expected an Invalid PathStatus entry"
    );
}

// ----- Skill status -----

#[test]
fn skill_status_absent_when_nothing_installed() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    for &id in LOCAL_SKILL_AGENTS {
        let agent = skill_by_id(id).expect(id);
        let report = agent
            .skill_status(&scope, "smoketest-skill", "myapp")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::Absent),
            "{id} expected Absent, got {:?}",
            report.status
        );
        assert_eq!(
            report.target,
            PlanTarget::Skill {
                name: "smoketest-skill".into()
            }
        );
    }
}

#[test]
fn skill_status_owned_after_install() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = skill_spec("smoketest-skill", "myapp");
    for &id in LOCAL_SKILL_AGENTS {
        let agent = skill_by_id(id).expect(id);
        agent.install_skill(&scope, &spec).expect(id);
        let report = agent
            .skill_status(&scope, "smoketest-skill", "myapp")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::InstalledOwned { ref owner } if owner == "myapp"),
            "{id} expected InstalledOwned(myapp), got {:?}",
            report.status
        );
        agent
            .uninstall_skill(&scope, "smoketest-skill", "myapp")
            .expect(id);
    }
}

#[test]
fn skill_status_other_owner_when_caller_differs() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = skill_spec("smoketest-skill", "appA");
    for &id in LOCAL_SKILL_AGENTS {
        let agent = skill_by_id(id).expect(id);
        agent.install_skill(&scope, &spec).expect(id);
        let report = agent
            .skill_status(&scope, "smoketest-skill", "appB")
            .expect(id);
        assert!(
            matches!(report.status, InstallStatus::InstalledOtherOwner { ref owner } if owner == "appA"),
            "{id} expected InstalledOtherOwner(appA), got {:?}",
            report.status
        );
        agent
            .uninstall_skill(&scope, "smoketest-skill", "appA")
            .expect(id);
    }
}

#[test]
fn skill_status_drifted_when_manifest_missing() {
    // Pick claude (skills_root = <root>/.claude/skills).
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let spec = skill_spec("smoketest-skill", "myapp");
    let agent = skill_by_id("claude").unwrap();
    agent.install_skill(&scope, &spec).unwrap();

    let manifest = dir.path().join(".claude/skills/smoketest-skill/SKILL.md");
    std::fs::remove_file(&manifest).unwrap();

    let report = agent
        .skill_status(&scope, "smoketest-skill", "myapp")
        .unwrap();
    let issues = match &report.status {
        InstallStatus::Drifted { issues } => issues,
        other => panic!("expected Drifted, got {other:?}"),
    };
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, DriftIssue::SkillIncomplete { .. })),
        "expected SkillIncomplete drift, got {issues:?}"
    );
}

// ----- BackupExists warning -----

#[test]
fn status_emits_backup_warning_when_bak_present() {
    // Pick claude. Install, uninstall, then leave a stray .bak around to
    // simulate a stale rollback artifact. Status should be Absent + warn.
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    let spec = mcp_spec("smoketest-server", "myapp");

    // Pre-create the config file so install patches an existing file (which
    // creates the .bak).
    let cfg = dir.path().join(".mcp.json");
    std::fs::write(&cfg, b"{}\n").unwrap();
    agent.install_mcp(&scope, &spec).unwrap();
    assert!(
        cfg.with_extension("json.bak").exists(),
        "expected .bak after first patch"
    );

    agent
        .uninstall_mcp(&scope, "smoketest-server", "myapp")
        .unwrap();

    // Now: ledger gone, config gone, but .bak remains. Recreate the .bak so
    // we have a stable assertion (uninstall might have restored it).
    let bak = cfg.with_extension("json.bak");
    if !bak.exists() {
        std::fs::write(&bak, b"{}\n").unwrap();
    }

    let report = agent
        .mcp_status(&scope, "smoketest-server", "myapp")
        .unwrap();
    assert!(matches!(report.status, InstallStatus::Absent));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, StatusWarning::BackupExists { .. })),
        "expected BackupExists warning, got {:?}",
        report.warnings
    );
}

// ----- StatusReport plumbing sanity -----

#[test]
fn status_report_includes_files_and_paths() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    let report = agent
        .mcp_status(&scope, "smoketest-server", "myapp")
        .unwrap();
    assert!(!report.files.is_empty(), "files vec should be populated");
    let cfg = report.config_path.as_ref().unwrap();
    assert!(
        report.files.iter().any(|f| match f {
            PathStatus::Missing { path } | PathStatus::Exists { path } => path == cfg,
            _ => false,
        }),
        "files vec should include config_path"
    );
}

// ----- Compatibility wrapper preserves "any owner" semantics -----

#[test]
fn is_mcp_installed_returns_true_for_any_owner() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let agent = mcp_by_id("claude").unwrap();
    agent
        .install_mcp(&scope, &mcp_spec("smoketest-server", "appA"))
        .unwrap();
    assert!(agent.is_mcp_installed(&scope, "smoketest-server").unwrap());
}

#[test]
fn invalid_config_does_not_panic() {
    // Repeated specifically to satisfy the spec line "Invalid config returns
    // typed invalid status instead of panic." We rely on Status not unwrapping
    // a parse error.
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());
    let cfg = dir.path().join(".mcp.json");
    std::fs::write(&cfg, b"{").unwrap();
    let report: StatusReport = mcp_by_id("claude")
        .unwrap()
        .mcp_status(&scope, "anything", "myapp")
        .unwrap();
    assert!(matches!(report.status, InstallStatus::Drifted { .. }));
}
