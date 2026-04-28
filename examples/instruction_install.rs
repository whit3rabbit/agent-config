//! Install standalone instructions across the three placement modes.
//!
//! Demonstrates:
//!
//! - `ReferencedFile` (Claude): writes `<scope>/.claude/instructions/MYAPP.md`
//!   plus an `@.claude/instructions/MYAPP.md` include in `<root>/CLAUDE.md`
//! - `InlineBlock` (Codex): injects the body as a fenced markdown block
//!   inside `<root>/AGENTS.md`, no separate file
//! - `StandaloneFile` (Cline): writes `<root>/.clinerules/MYAPP.md` only,
//!   no host edit
//!
//! Each placement is right for one class of harness. The instruction surface
//! picks the path layout the agent expects; the consumer just supplies the
//! body and the placement.
//!
//! Run: `cargo run --example instruction_install`

use agent_config::{instruction_by_id, InstructionPlacement, InstructionSpec, Result, Scope};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());

    let body = "# MyApp\n\nProject-specific guidance loaded into the agent every session.\n";

    // 1. Claude uses ReferencedFile by default. The instruction file lives
    //    next to CLAUDE.md and is included via a managed @import.
    let claude_spec = InstructionSpec::builder("MYAPP")
        .owner("myapp")
        .placement(InstructionPlacement::ReferencedFile)
        .body(body)
        .try_build()?;
    let claude = instruction_by_id("claude").expect("claude instructions");
    let report = claude.install_instruction(&scope, &claude_spec)?;
    println!("claude (ReferencedFile)");
    println!("  created: {:?}", report.created);
    println!("  patched: {:?}", report.patched);
    let claude_md = project.path().join("CLAUDE.md");
    let instr_md = project
        .path()
        .join(".claude")
        .join("instructions")
        .join("MYAPP.md");
    assert!(claude_md.exists() && instr_md.exists());

    // 2. Codex uses InlineBlock. The body is fenced inside AGENTS.md so the
    //    library can find it again at uninstall time.
    let codex_spec = InstructionSpec::builder("MYAPP")
        .owner("myapp")
        .placement(InstructionPlacement::InlineBlock)
        .body(body)
        .try_build()?;
    let codex = instruction_by_id("codex").expect("codex instructions");
    let report = codex.install_instruction(&scope, &codex_spec)?;
    println!("\ncodex (InlineBlock)");
    println!("  created: {:?}", report.created);
    println!("  patched: {:?}", report.patched);
    let agents_md = project.path().join("AGENTS.md");
    let agents_text = std::fs::read_to_string(&agents_md).expect("AGENTS.md exists");
    assert!(
        agents_text.contains("BEGIN AGENT-CONFIG:MYAPP"),
        "expected fenced block in AGENTS.md"
    );

    // 3. Cline uses StandaloneFile. The agent loads any markdown file dropped
    //    into its rules directory, so no host edit is needed.
    let cline_spec = InstructionSpec::builder("MYAPP")
        .owner("myapp")
        .placement(InstructionPlacement::StandaloneFile)
        .body(body)
        .try_build()?;
    let cline = instruction_by_id("cline").expect("cline instructions");
    let report = cline.install_instruction(&scope, &cline_spec)?;
    println!("\ncline (StandaloneFile)");
    println!("  created: {:?}", report.created);
    println!("  patched: {:?}", report.patched);
    let cline_rule = project.path().join(".clinerules").join("MYAPP.md");
    assert!(cline_rule.exists(), "expected {cline_rule:?}");

    // Tear all three down. Uninstall is keyed on (name, owner_tag) for every
    // placement.
    let _ = claude.uninstall_instruction(&scope, "MYAPP", "myapp")?;
    let _ = codex.uninstall_instruction(&scope, "MYAPP", "myapp")?;
    let _ = cline.uninstall_instruction(&scope, "MYAPP", "myapp")?;
    println!("\nall three placements uninstalled cleanly");
    Ok(())
}
