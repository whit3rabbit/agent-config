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

fn package_lock(project: &Project) -> serde_json::Value {
    serde_json::from_str(&project.read("agent.config.lock.json")).unwrap()
}

fn write_mcp_manifest(dir: &Path, name: &str, command: &str) {
    std::fs::write(
        dir.join("agent.config.toml"),
        format!("[package]\nname = \"toolkit\"\n[[mcp]]\nname = \"{name}\"\ncommand = [\"{command}\"]\n"),
    ).unwrap();
}

#[test]
fn git_ref_and_dry_run_json_replay_the_selected_source() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(source.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-qb", "main"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    write_mcp_manifest(source.path(), "server", "main");
    git(&["add", "."]);
    git(&["commit", "-qm", "main"]);
    git(&["checkout", "-qb", "pinned"]);
    write_mcp_manifest(source.path(), "server", "pinned");
    git(&["commit", "-qam", "pinned"]);
    git(&["checkout", "-q", "main"]);

    let path = source.path().to_string_lossy().replace('\\', "/");
    let slash = if path.starts_with('/') { "" } else { "/" };
    let url = format!("file://{slash}{path}");
    let args = [
        "add", &url, "--ref", "pinned", "-a", "claude", "-y", "--json",
    ];
    let mut dry_run = args.to_vec();
    dry_run.push("--dry-run");
    let (code, out) = project.run(&dry_run);
    assert_eq!(code, 0, "{out}");
    let result: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(result["outcomes"][0]["status"], "would_change");
    assert!(project.path().read_dir().unwrap().next().is_none());
    let (code, out) = project.run(&args);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        package_lock(&project)["packages"]["toolkit"]["git_ref"],
        "pinned"
    );
    assert_eq!(project.run(&["check", "toolkit"]).0, 0);
    git(&["checkout", "-q", "pinned"]);
    write_mcp_manifest(source.path(), "server", "pinned-v2");
    git(&["commit", "-qam", "pinned v2"]);
    git(&["checkout", "-q", "main"]);
    assert_eq!(project.run(&["update", "toolkit", "-y"]).0, 0);
    assert!(project.read(".mcp.json").contains("pinned-v2"));
}

#[test]
fn add_tracks_successes_only_and_remove_retains_refused_rows() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    write_mcp_manifest(source.path(), "server", "package-cmd");
    let file = project.path().join(".mcp.json");
    let manual = r#"{"mcpServers":{"server":{"command":"manual","args":[]}}}"#;
    std::fs::write(&file, manual).unwrap();
    let source = source.path().to_str().unwrap();
    assert_ne!(project.run(&["add", source, "-a", "claude", "-y"]).0, 0);
    assert!(!project.exists("agent.config.lock.json"));
    let (code, out) = project.run(&["add", source, "-a", "claude,codex", "-y"]);
    assert_ne!(code, 0, "{out}");
    let pkg = &package_lock(&project)["packages"]["toolkit"];
    assert_eq!(pkg["pending"], true);
    assert_eq!(pkg["items"].as_array().unwrap().len(), 1);
    assert_eq!(pkg["items"][0]["agent"], "codex");

    std::fs::remove_file(&file).unwrap();
    let (code, out) = project.run(&["update", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(project.run(&["check", "toolkit"]).0, 0);
    std::fs::write(&file, manual).unwrap();
    let (code, out) = project.run(&["remove", "toolkit", "-y"]);
    assert_ne!(code, 0, "{out}");
    let pkg = &package_lock(&project)["packages"]["toolkit"];
    assert_eq!(pkg["items"].as_array().unwrap().len(), 1);
    assert_eq!(pkg["items"][0]["agent"], "claude");
    assert_eq!(project.read(".mcp.json"), manual);
}

#[test]
fn obsolete_rows_survive_update_and_can_be_removed() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    write_mcp_manifest(source.path(), "old-server", "package-cmd");
    let (code, out) = project.run(&["add", source.path().to_str().unwrap(), "-a", "claude", "-y"]);
    assert_eq!(code, 0, "{out}");
    write_mcp_manifest(source.path(), "new-server", "package-cmd");
    let (code, out) = project.run(&["update", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    let pkg = &package_lock(&project)["packages"]["toolkit"];
    assert_eq!(pkg["items"].as_array().unwrap().len(), 2);
    let (code, out) = project.run(&["remove", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert!(!project.read(".mcp.json").contains("-server"));
}

#[test]
fn filtered_removal_keeps_the_shared_skill_and_excludes_the_agent_on_update() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    let skill = source.path().join("skills/tool");
    std::fs::create_dir_all(&skill).unwrap();
    let write_skill = |body| {
        std::fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: tool\ndescription: Test skill.\n---\n{body}\n"),
        )
        .unwrap();
    };
    write_skill("first");
    std::fs::write(
        source.path().join("agent.config.toml"),
        "[package]\nname = \"toolkit\"\n[[skills]]\npath = \"skills/tool\"\n",
    )
    .unwrap();
    let (code, out) = project.run(&[
        "add",
        source.path().to_str().unwrap(),
        "-a",
        "cline,codex",
        "-y",
    ]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = project.run(&["remove", "toolkit", "-a", "cline", "-y"]);
    assert_eq!(code, 0, "{out}");
    let installed = ".agents/skills/tool/SKILL.md";
    assert!(project.read(installed).contains("first"));
    write_skill("second");
    let (code, out) = project.run(&["update", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert!(project.read(installed).contains("second"));
    let pkg = &package_lock(&project)["packages"]["toolkit"];
    assert_eq!(pkg["items"].as_array().unwrap().len(), 1);
    assert_eq!(pkg["items"][0]["agent"], "codex");
    assert_eq!(pkg["excluded_agents"][0], "cline");
}

#[test]
fn claude_plugin_skips_runtime_dependent_and_malformed_mcp_entries() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"toolkit"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join(".mcp.json"),
        r#"{
        "valid": {"command": "npx", "args": ["-y", "tool"]},
        "relative": {"command": "./bin/server"},
        "runtime": {"command": "node", "args": ["${PLUGIN_ROOT}/server.js"]},
        "cwd": {"command": "node", "cwd": "./servers"},
        "invalid": {"command": "node", "args": [123]}
    }"#,
    )
    .unwrap();
    let (code, out) = project.run(&["add", source.path().to_str().unwrap(), "-a", "claude", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("requires a plugin runtime"), "{out}");
    assert!(out.contains("expected an array of strings"), "{out}");
    let mcp: serde_json::Value = serde_json::from_str(&project.read(".mcp.json")).unwrap();
    assert_eq!(mcp["mcpServers"].as_object().unwrap().len(), 1);
    assert_eq!(mcp["mcpServers"]["valid"]["command"], "npx");
}

#[test]
fn manifest_escapes_and_oversized_files_leave_valid_siblings_installable() {
    let project = Project::new();
    let fixture = tempfile::tempdir().unwrap();
    let source = fixture.path().join("package");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(fixture.path().join("outside.md"), "outside").unwrap();
    let oversized = std::fs::File::create(source.join("oversized.md")).unwrap();
    oversized.set_len(8 * 1024 * 1024 + 1).unwrap();
    let manifest = "[package]\nname = \"toolkit\"\n[[instructions]]\nname = \"escape\"\nfile = \"../outside.md\"\n[[instructions]]\nname = \"oversized\"\nfile = \"oversized.md\"\n[[instructions]]\nname = \"valid\"\nbody = \"safe\"\n";
    #[cfg(unix)]
    let manifest = {
        std::os::unix::fs::symlink(fixture.path().join("outside.md"), source.join("link.md"))
            .unwrap();
        format!("{manifest}[[instructions]]\nname = \"symlink\"\nfile = \"link.md\"\n")
    };
    std::fs::write(source.join("agent.config.toml"), manifest).unwrap();
    let (code, out) = project.run(&["add", source.to_str().unwrap(), "-a", "roo", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("must be relative"), "{out}");
    assert!(out.contains("8 MiB limit"), "{out}");
    #[cfg(unix)]
    assert!(out.contains("escapes the source directory"), "{out}");
    assert_eq!(
        package_lock(&project)["packages"]["toolkit"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn update_keeps_independent_agent_and_surface_filters() {
    let project = Project::new();
    let source = tempfile::tempdir().unwrap();
    let source = source.path();
    let write = |body| {
        std::fs::write(source.join("agent.config.toml"), format!(
        "[package]\nname = \"toolkit\"\n[[mcp]]\nname = \"tool\"\ncommand = [\"npx\"]\n[[instructions]]\nname = \"note\"\nbody = \"{body}\"\n"
    )).unwrap()
    };
    write("first");
    let source_arg = source.to_str().unwrap();
    let (code, out) = project.run(&["add", source_arg, "-a", "claude", "--only", "mcp", "-y"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = project.run(&[
        "add",
        source_arg,
        "-a",
        "codex",
        "--only",
        "instructions",
        "-y",
    ]);
    assert_eq!(code, 0, "{out}");
    write("second");
    let (code, out) = project.run(&["update", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert!(project.read("AGENTS.md").contains("second"));
    assert!(project.read(".mcp.json").contains("tool"));
    assert!(!project.exists("CLAUDE.md"));
    assert!(!project.exists(".codex/config.toml"));
    assert_eq!(
        package_lock(&project)["packages"]["toolkit"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    std::fs::write(
        source.join("agent.config.toml"),
        "[package]\nname = \"toolkit\"\n",
    )
    .unwrap();
    let (code, out) = project.run(&["update", "toolkit", "-y"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(project.run(&["check", "toolkit"]).0, 0);
    assert_eq!(
        package_lock(&project)["packages"]["toolkit"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn concurrent_adds_preserve_both_package_records() {
    let project = Project::new();
    let sources = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    for (index, dir) in sources.iter().enumerate() {
        std::fs::write(dir.path().join("agent.config.toml"), format!(
            "[package]\nname = \"toolkit-{index}\"\n[[mcp]]\nname = \"server-{index}\"\ncommand = [\"npx\"]\n"
        )).unwrap();
    }
    std::thread::scope(|scope| {
        let handles = sources
            .iter()
            .map(|source| {
                let project = &project;
                scope.spawn(move || {
                    project.run(&["add", source.path().to_str().unwrap(), "-a", "claude", "-y"])
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            let (code, out) = handle.join().unwrap();
            assert_eq!(code, 0, "{out}");
        }
    });
    assert_eq!(
        package_lock(&project)["packages"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    assert!(project.read(".mcp.json").contains("server-0"));
    assert!(project.read(".mcp.json").contains("server-1"));
    assert!(!project.exists("agent.config.lock.json.mutation.lock"));
}
