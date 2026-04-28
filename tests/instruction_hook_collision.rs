//! Cross-surface regression: a hook with `tag = "T"` and an instruction with
//! `name = "T"` installed into the same harness must not overwrite each
//! other. Pre-rename, both wrote to the same `AGENT-CONFIG:T` fence and the
//! second install silently replaced the first.

use std::fs;

use agent_config::{
    by_id, instruction_by_id, Event, HookSpec, InstructionPlacement, InstructionSpec, Matcher,
    Scope,
};

#[test]
fn hook_tag_and_instruction_name_coexist_in_same_memory_file() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    // Hook with rules — Claude writes both a hook entry to settings.json and
    // a fenced rules block to CLAUDE.md, keyed on tag "shared".
    let hook_spec = HookSpec::builder("shared")
        .command_program("shared", ["hook"])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Hook rules content for the shared tag.")
        .build();

    let claude = by_id("claude").expect("claude integration");
    let _ = claude.install(&scope, &hook_spec).expect("install hook");

    // Instruction with the SAME name. ReferencedFile placement also writes
    // a fenced block into CLAUDE.md (an `@<NAME>.md` include). Pre-rename,
    // this would have overwritten the hook block.
    let instr_spec = InstructionSpec::builder("shared")
        .owner("instr-owner")
        .placement(InstructionPlacement::ReferencedFile)
        .body("# Shared instruction body.\n")
        .try_build()
        .expect("valid instruction spec");

    let instr_surface = instruction_by_id("claude").expect("claude instructions");
    let _ = instr_surface
        .install_instruction(&scope, &instr_spec)
        .expect("install instruction");

    let claude_md = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();

    assert!(
        claude_md.contains("<!-- BEGIN AGENT-CONFIG:shared -->"),
        "hook fence missing after instruction install: {claude_md}"
    );
    assert!(
        claude_md.contains("<!-- BEGIN AGENT-CONFIG-INSTR:shared -->"),
        "instruction fence missing: {claude_md}"
    );
    assert!(
        claude_md.contains("Hook rules content for the shared tag."),
        "hook body lost: {claude_md}"
    );
    assert!(
        claude_md.contains("@.claude/instructions/shared.md"),
        "instruction include line missing: {claude_md}"
    );
}

#[test]
fn uninstalling_instruction_does_not_disturb_hook_block_with_same_tag() {
    let dir = tempfile::tempdir().unwrap();
    let scope = Scope::Local(dir.path().to_path_buf());

    let hook_spec = HookSpec::builder("shared")
        .command_program("shared", ["hook"])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Hook rules survive instruction uninstall.")
        .build();

    let claude = by_id("claude").expect("claude integration");
    let _ = claude.install(&scope, &hook_spec).unwrap();

    let instr_spec = InstructionSpec::builder("shared")
        .owner("instr-owner")
        .placement(InstructionPlacement::ReferencedFile)
        .body("# Shared instruction body.\n")
        .try_build()
        .unwrap();

    let instr_surface = instruction_by_id("claude").unwrap();
    let _ = instr_surface
        .install_instruction(&scope, &instr_spec)
        .unwrap();
    let _ = instr_surface
        .uninstall_instruction(&scope, "shared", "instr-owner")
        .unwrap();

    let claude_md = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();

    assert!(
        claude_md.contains("<!-- BEGIN AGENT-CONFIG:shared -->"),
        "hook fence destroyed by instruction uninstall: {claude_md}"
    );
    assert!(
        claude_md.contains("Hook rules survive instruction uninstall."),
        "hook body destroyed by instruction uninstall: {claude_md}"
    );
    assert!(
        !claude_md.contains("<!-- BEGIN AGENT-CONFIG-INSTR:shared -->"),
        "instruction fence not removed: {claude_md}"
    );
    assert!(
        !claude_md.contains("@.claude/instructions/shared.md"),
        "instruction include line not removed: {claude_md}"
    );
}
