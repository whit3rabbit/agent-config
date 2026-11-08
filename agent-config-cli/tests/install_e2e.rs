//! End-to-end tests: run the compiled `agentconfig` binary against sample
//! packages in isolated temp projects. Each test uses its own tempdir as the
//! binary's working directory, so project-scope lockfiles and harness
//! writes never touch the host or each other.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_agentconfig")
}

fn fixture() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample-pkg")
}

struct Project(tempfile::TempDir);

impl Project {
    fn new() -> Self {
        Self(tempfile::tempdir().expect("tempdir"))
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let output = Command::new(bin())
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("spawn agentconfig");
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        (output.status.code().unwrap_or(-1), text)
    }

    fn exists(&self, rel: &str) -> bool {
        self.path().join(rel).exists()
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path().join(rel)).unwrap_or_default()
    }
}

#[test]
fn add_installs_all_surfaces_and_records_lockfile() {
    let project = Project::new();
    let (code, out) = project.run(&[
        "add",
        fixture(),
        "--agent",
        "claude,codex,roo",
        "--yes",
        "--json",
    ]);
    assert_eq!(code, 0, "add failed:\n{out}");

    let skill_md = project.read(".claude/skills/acme-notes/SKILL.md");
    assert!(
        skill_md.contains("Acme notes"),
        "SKILL.md wrong: {skill_md:?}"
    );
    assert!(
        project.exists(".claude/skills/acme-notes/references/extra.md"),
        "asset not copied"
    );

    let settings = project.read(".claude/settings.json");
    assert!(
        settings.contains("acme-guard"),
        "hook missing: {settings:?}"
    );

    let claude_md = project.read("CLAUDE.md");
    assert!(
        claude_md.contains("destructive shell commands"),
        "rules fence missing: {claude_md:?}"
    );

    let mcp = project.read(".mcp.json");
    assert!(mcp.contains("acme-mcp"), "mcp wrong: {mcp:?}");
    // MCP payloads carry no inline tag; ownership lives in the sidecar ledger.
    let ledger = project.read(".agent-config-mcp.json");
    assert!(
        ledger.contains("acme-mcp") && ledger.contains("acme-toolkit"),
        "mcp ledger wrong: {ledger:?}"
    );

    let roo_rules = project.read(".roo/rules/acme-style.md");
    assert!(
        roo_rules.contains("One logical change per commit"),
        "roo instruction wrong: {roo_rules:?}"
    );

    let lock = project.read("agent.config.lock.json");
    for needle in [
        "acme-toolkit",
        "acme-guard",
        "acme-mcp",
        "acme-notes",
        "acme-style",
    ] {
        assert!(lock.contains(needle), "lockfile missing {needle}: {lock:?}");
    }

    let value: serde_json::Value = serde_json::from_str(out.trim()).expect("stdout not JSON");
    assert_eq!(value["package"], "acme-toolkit");
    assert!(value["outcomes"].as_array().expect("outcomes").len() >= 8);
}

#[test]
fn add_is_idempotent() {
    let project = Project::new();
    let (first, out) = project.run(&["add", fixture(), "--agent", "claude", "--yes"]);
    assert_eq!(first, 0, "first add failed:\n{out}");
    let (second, out) = project.run(&["add", fixture(), "--agent", "claude", "--yes"]);
    assert_eq!(second, 0, "second add failed:\n{out}");
    assert!(
        out.contains("already installed") || out.contains("up to date"),
        "second add should be a no-op:\n{out}"
    );
}

#[test]
fn add_dry_run_writes_nothing() {
    let project = Project::new();
    let (code, out) = project.run(&[
        "add",
        fixture(),
        "--agent",
        "claude,codex,roo",
        "--yes",
        "--dry-run",
    ]);
    assert_eq!(code, 0, "dry run failed:\n{out}");
    assert!(
        !project.exists("agent.config.lock.json"),
        "dry run wrote lockfile"
    );
    assert!(!project.exists(".claude"), "dry run touched .claude");
    assert!(!project.exists(".roo"), "dry run touched .roo");
    assert!(!project.exists(".mcp.json"), "dry run touched .mcp.json");
}

#[test]
fn add_then_remove_round_trips() {
    let project = Project::new();
    let (code, out) = project.run(&["add", fixture(), "--agent", "claude,roo", "--yes"]);
    assert_eq!(code, 0, "add failed:\n{out}");
    assert!(project.exists(".claude/skills/acme-notes/SKILL.md"));

    let (code, out) = project.run(&["remove", "acme-toolkit", "--yes"]);
    assert_eq!(code, 0, "remove failed:\n{out}");

    assert!(
        !project.exists(".claude/skills/acme-notes/SKILL.md"),
        "skill left"
    );
    assert!(
        !project.exists(".roo/rules/acme-style.md"),
        "instruction left"
    );
    let claude_md = project.read("CLAUDE.md");
    assert!(
        !claude_md.contains("destructive shell commands"),
        "fence left: {claude_md:?}"
    );
    let mcp = project.read(".mcp.json");
    assert!(!mcp.contains("acme-mcp"), "mcp left: {mcp:?}");
    let lock = project.read("agent.config.lock.json");
    assert!(!lock.contains("acme-toolkit"), "lock entry left: {lock:?}");
}

#[test]
fn remove_of_unknown_package_is_an_error() {
    let project = Project::new();
    let (code, out) = project.run(&["remove", "nope", "--yes"]);
    assert_ne!(code, 0);
    assert!(out.contains("AGENTCONFIG_NOT_IN_LOCKFILE"), "{out}");
}

#[test]
fn instruction_file_mode_installs_one_rule() {
    let project = Project::new();
    let src = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        src.path().join("README.md"),
        "# Manifest-less repo\n\nBe excellent to each other.\n",
    )
    .unwrap();

    let (code, out) = project.run(&[
        "add",
        src.path().to_str().unwrap(),
        "--instruction-file",
        "README.md",
        "--agent",
        "roo",
        "--yes",
    ]);
    assert_eq!(code, 0, "instruction-file add failed:\n{out}");

    // The package name is the sanitized source dir name; assert on content
    // instead of replicating the sanitizer.
    let rules_dir = project.path().join(".roo/rules");
    let found = std::fs::read_dir(&rules_dir)
        .expect("roo rules dir")
        .filter_map(|e| e.ok())
        .any(|e| {
            std::fs::read_to_string(e.path())
                .map(|t| t.contains("Be excellent to each other."))
                .unwrap_or(false)
        });
    assert!(found, "no instruction in {rules_dir:?} carries the body");
}

#[test]
fn foreign_layout_plugin_json_is_recognized() {
    let project = Project::new();
    let src = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        src.path().join("plugin.json"),
        r#"{ "name": "foreign-tools" }"#,
    )
    .unwrap();
    std::fs::write(
        src.path().join("mcp.json"),
        r#"{ "mcpServers": { "foreign-mcp": { "command": "npx", "args": ["-y", "@f/m"] } } }"#,
    )
    .unwrap();
    std::fs::create_dir_all(src.path().join("skills/foreign-skill")).unwrap();
    std::fs::write(
        src.path().join("skills/foreign-skill/SKILL.md"),
        "---\nname: foreign-skill\ndescription: Use for foreign layouts.\n---\n\n# Foreign\n",
    )
    .unwrap();

    let (code, out) = project.run(&[
        "add",
        src.path().to_str().unwrap(),
        "--agent",
        "claude",
        "--yes",
    ]);
    assert_eq!(code, 0, "foreign add failed:\n{out}");

    let skill_md = project.read(".claude/skills/foreign-skill/SKILL.md");
    assert!(
        skill_md.contains("foreign-skill"),
        "foreign skill missing: {skill_md:?}"
    );
    let mcp = project.read(".mcp.json");
    assert!(mcp.contains("foreign-mcp"), "foreign mcp missing: {mcp:?}");
}

#[test]
fn check_fails_after_upstream_content_changes() {
    let project = Project::new();
    let src = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        src.path().join("agent.config.toml"),
        "[package]\nname = \"mutable-pkg\"\n\n[[instructions]]\nname = \"note\"\nbody = \"first\"\n",
    )
    .unwrap();

    let (code, out) = project.run(&[
        "add",
        src.path().to_str().unwrap(),
        "--agent",
        "roo",
        "--yes",
    ]);
    assert_eq!(code, 0, "add failed:\n{out}");

    let (code, out) = project.run(&["check"]);
    assert_eq!(code, 0, "check should pass when unchanged:\n{out}");

    std::fs::write(
        src.path().join("agent.config.toml"),
        "[package]\nname = \"mutable-pkg\"\n\n[[instructions]]\nname = \"note\"\nbody = \"CHANGED\"\n",
    )
    .unwrap();

    let (code, out) = project.run(&["check"]);
    assert_eq!(code, 1, "check should fail after upstream change:\n{out}");
    assert!(out.contains("updates available"), "{out}");

    let (code, out) = project.run(&["update", "--yes"]);
    assert_eq!(code, 0, "update failed:\n{out}");
    let rules = project.read(".roo/rules/note.md");
    assert!(
        rules.contains("CHANGED"),
        "update did not apply new body: {rules:?}"
    );

    let (code, _) = project.run(&["check"]);
    assert_eq!(code, 0, "check should pass after update");
}

#[test]
fn init_scaffolds_a_manifest() {
    let project = Project::new();
    let (code, out) = project.run(&["init", "my-pkg"]);
    assert_eq!(code, 0, "init failed:\n{out}");
    let manifest = project.read("agent.config.toml");
    assert!(manifest.contains("name = \"my-pkg\""), "{manifest:?}");

    let (code, out) = project.run(&["init", "my-pkg"]);
    assert_ne!(code, 0);
    assert!(out.contains("AGENTCONFIG_INIT_EXISTS"), "{out}");
}

#[test]
fn unknown_agent_flag_is_rejected() {
    let project = Project::new();
    let (code, out) = project.run(&["add", fixture(), "--agent", "not-an-agent", "--yes"]);
    assert_ne!(code, 0);
    assert!(out.contains("AGENTCONFIG_UNKNOWN_AGENT"), "{out}");
}

#[test]
fn list_shows_installed_package() {
    let project = Project::new();
    project.run(&["add", fixture(), "--agent", "claude,roo", "--yes"]);
    let (code, out) = project.run(&["list"]);
    assert_eq!(code, 0, "list failed:\n{out}");
    assert!(out.contains("acme-toolkit"), "{out}");
    assert!(out.contains("acme-notes"), "{out}");
}
