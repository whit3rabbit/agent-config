//! Install an Agent Skill into the harness's skills directory.
//!
//! Demonstrates:
//!
//! - `SkillSpec::builder` with `description`, `body`, and an executable asset
//! - That the install creates `<skills_root>/<name>/SKILL.md` plus assets
//!   under `scripts/`
//! - Ownership ledger lives at `<skills_root>/.agent-config-skills.json`
//!
//! Run: `cargo run --example skill_install`

use std::path::PathBuf;

use agent_config::{skill_by_id, Result, Scope, SkillAsset, SkillSpec};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());

    let spec = SkillSpec::builder("repo-context")
        .owner("myapp")
        .description("Use when the user asks about the layout of this repository.")
        .allowed_tools(["Read", "Bash"])
        .body(concat!(
            "## Goal\n",
            "Answer questions about the project's directory structure.\n\n",
            "## Instructions\n",
            "Run `scripts/tree.sh` to print a depth-2 tree before answering.\n",
        ))
        .asset(SkillAsset {
            relative_path: PathBuf::from("scripts/tree.sh"),
            bytes: b"#!/bin/sh\nfind . -maxdepth 2 -type d | sort\n".to_vec(),
            executable: true,
        })
        .try_build()?;

    let claude = skill_by_id("claude").expect("claude supports skills");

    let install = claude.install_skill(&scope, &spec)?;
    println!("installed skill repo-context");
    println!("  created: {:?}", install.created);
    println!("  patched: {:?}", install.patched);

    // The skill manifest is at <project>/.claude/skills/repo-context/SKILL.md.
    let skill_md = project
        .path()
        .join(".claude")
        .join("skills")
        .join("repo-context")
        .join("SKILL.md");
    assert!(skill_md.exists(), "SKILL.md should exist at {skill_md:?}");
    println!("  manifest: {}", skill_md.display());

    // Reinstall with identical content is a no-op.
    let again = claude.install_skill(&scope, &spec)?;
    assert!(again.already_installed);
    println!("  reinstall was idempotent");

    // Uninstall is keyed on (skill name, owner tag).
    let removed = claude.uninstall_skill(&scope, "repo-context", "myapp")?;
    println!(
        "uninstalled (removed={:?}, patched={:?})",
        removed.removed, removed.patched
    );
    assert!(!skill_md.exists());

    Ok(())
}
