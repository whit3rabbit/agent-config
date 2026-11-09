//! Offline CLI tests. Each process uses a disposable project and home.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    source: PathBuf,
    state: PathBuf,
    scratch: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let make_dir = |name: &str| {
            let path = root.path().join(name);
            fs::create_dir(&path).unwrap();
            path
        };
        let home = make_dir("home");
        let project = make_dir("project");
        let source = make_dir("source");
        let state = make_dir("state");
        let scratch = make_dir("scratch");
        Self {
            _root: root,
            home,
            project,
            source,
            state,
            scratch,
        }
    }

    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_skillz"))
            .args(args)
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("CLAUDE_CONFIG_DIR", self.home.join(".claude"))
            .env("XDG_STATE_HOME", &self.state)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("TMPDIR", &self.scratch)
            .env("TMP", &self.scratch)
            .env("TEMP", &self.scratch)
            .env("CI", "true")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home.join("gitconfig"))
            .output()
            .unwrap()
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(&self.source)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn branch_fixture(&self) -> String {
        self.git(&["init", "--quiet", "-b", "main"]);
        self.git(&["config", "user.name", "Fixture"]);
        self.git(&["config", "user.email", "fixture@example.invalid"]);
        self.git(&["config", "commit.gpgsign", "false"]);
        self.write_skill("", "root-skill", "main");
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "main"]);
        self.git(&["checkout", "-qb", "pinned"]);
        self.write_skill("", "root-skill", "pinned v1");
        self.write_skill("skills/custom-folder", "nested-skill", "pinned v1");
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", "pinned"]);
        self.git(&["checkout", "-q", "main"]);
        let path = self.source.to_string_lossy().replace('\\', "/");
        let slash = if path.starts_with('/') { "" } else { "/" };
        format!("file://{slash}{path}#pinned")
    }

    fn write_skill(&self, directory: &str, name: &str, body: &str) {
        let dir = self.source.join(directory);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill.\n---\n{body}\n"),
        )
        .unwrap();
    }

    fn advance_pinned(&self) {
        self.git(&["checkout", "-q", "pinned"]);
        self.write_skill("", "root-skill", "pinned v2");
        self.write_skill("skills/custom-folder", "nested-skill", "pinned v2");
        self.git(&["commit", "-qam", "advance pinned"]);
        self.git(&["checkout", "-q", "main"]);
    }
}

fn ok(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn cloned_skills_restore_and_update_in_the_recorded_scope() {
    let fixture = Fixture::new();
    let source = fixture.branch_fixture();
    let add = [
        "add",
        &source,
        "--full-depth",
        "--copy",
        "-a",
        "claude-code",
        "-y",
        "--json",
    ];
    let output = ok(fixture.cli(&add));
    let results: Value = serde_json::from_slice(&output.stdout).expect("only JSON on stdout");
    assert_eq!(results.as_array().unwrap().len(), 2);
    assert!(fixture.scratch.read_dir().unwrap().next().is_none());
    let project_lock = fixture.project.join("skills-lock.json");
    let lock = json(&project_lock);
    let root = &lock["skills"]["root-skill"];
    assert_eq!(root["skillPath"], "SKILL.md");
    assert_eq!(root["ref"], "pinned");
    assert_eq!(root["placements"][0]["agent"], "claude-code");
    assert_eq!(root["placements"][0]["mode"], "copy");
    assert_eq!(
        lock["skills"]["nested-skill"]["skillPath"],
        "skills/custom-folder/SKILL.md"
    );
    let project_skills = fixture.project.join(".claude/skills");
    let project_root = project_skills.join("root-skill/SKILL.md");
    assert!(fs::read_to_string(&project_root)
        .unwrap()
        .contains("pinned v1"));

    fs::remove_dir_all(&project_skills).unwrap();
    ok(fixture.cli(&["experimental_install", "--frozen-lockfile"]));
    assert!(project_skills.join("nested-skill/SKILL.md").is_file());
    assert!(!project_skills.join("root-skill").is_symlink());

    let mut global_add = add.to_vec();
    global_add.push("-g");
    ok(fixture.cli(&global_add));
    let global_root = fixture.home.join(".claude/skills/root-skill/SKILL.md");
    let global_lock = fixture.state.join("skills/.skill-lock.json");
    assert!(global_root.is_file(), "global lock: {}", json(&global_lock));
    fixture.advance_pinned();
    assert!(!fixture.cli(&["check", "-g", "-y"]).status.success());

    ok(fixture.cli(&["update", "-g", "-y"]));
    assert!(fs::read_to_string(&global_root)
        .unwrap()
        .contains("pinned v2"));
    assert!(fs::read_to_string(&project_root)
        .unwrap()
        .contains("pinned v1"));
    ok(fixture.cli(&["check", "-g", "-y"]));

    fs::remove_dir_all(&project_skills).unwrap();
    let output = fixture.cli(&["experimental_install", "--frozen-lockfile"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("content hash changed"));
    assert!(!project_skills.exists());
    assert_eq!(json(&project_lock), lock);
    ok(fixture.cli(&["update", "-p", "-y"]));
    assert!(fs::read_to_string(&project_root)
        .unwrap()
        .contains("pinned v2"));
    ok(fixture.cli(&["check", "-p", "-y"]));
}

#[test]
fn add_returns_failure_when_the_target_cannot_be_written() {
    let fixture = Fixture::new();
    fixture.write_skill("", "root-skill", "content");
    fs::write(fixture.project.join(".claude"), "file blocks the target").unwrap();
    let source = fixture.source.to_str().unwrap();
    let args = ["add", source, "--copy", "-a", "claude-code", "-y"];
    assert!(!fixture.cli(&args).status.success());
    let mut json_args = args.to_vec();
    json_args.push("--json");
    let output = fixture.cli(&json_args);
    assert!(!output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json[0]["status"], "failed");
    assert!(!fixture.project.join("skills-lock.json").exists());
}

#[test]
fn update_keeps_the_old_fingerprint_when_a_recorded_target_fails() {
    let fixture = Fixture::new();
    let source = fixture.branch_fixture();
    ok(fixture.cli(&["add", &source, "--copy", "-a", "claude-code", "-y"]));
    let lock = fixture.project.join("skills-lock.json");
    let before = json(&lock);
    fixture.advance_pinned();
    let claude = fixture.project.join(".claude");
    fs::remove_dir_all(&claude).unwrap();
    fs::write(&claude, "file blocks the target").unwrap();
    assert!(!fixture.cli(&["update", "-p", "-y"]).status.success());
    assert_eq!(json(&lock), before);
    assert!(fixture.scratch.read_dir().unwrap().next().is_none());
    fs::remove_file(claude).unwrap();
    ok(fixture.cli(&["update", "-p", "-y"]));
    assert_ne!(
        json(&lock)["skills"]["root-skill"]["computedHash"],
        before["skills"]["root-skill"]["computedHash"]
    );
}
