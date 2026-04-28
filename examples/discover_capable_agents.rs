//! Print the registry surfaces and which scope kinds each agent accepts.
//!
//! Demonstrates:
//!
//! - `all()`, `mcp_capable()`, `skill_capable()`, `instruction_capable()`
//!   for runtime discovery
//! - `supported_scopes()` (per surface) so callers can gate Global vs Local
//!
//! This program does not write to disk. It is the discovery half of any
//! installer that lets users choose a target by id.
//!
//! Run: `cargo run --example discover_capable_agents`

use agent_config::{all, instruction_capable, mcp_capable, skill_capable, ScopeKind};

fn main() {
    // `all()` returns every harness that implements the `Integration` trait.
    // That covers both real hook surfaces (Claude, Codex, etc.) and prompt-
    // only agents (Roo, Trae, OpenClaw) whose Integration::install only
    // writes a markdown rules file. Use `supported_scopes` plus per-agent
    // docs to distinguish hooks-with-commands from prompt-only.
    println!("Registered integrations (Integration trait):");
    for agent in all() {
        let scopes = agent.supported_scopes();
        println!(
            "  {:<14} {:<24} scopes={}",
            agent.id(),
            agent.display_name(),
            format_scopes(scopes)
        );
    }

    println!("\nMCP-capable:");
    for mcp in mcp_capable() {
        println!(
            "  {:<14} scopes={}",
            mcp.id(),
            format_scopes(mcp.supported_mcp_scopes())
        );
    }

    println!("\nSkill-capable:");
    for skill in skill_capable() {
        println!(
            "  {:<14} scopes={}",
            skill.id(),
            format_scopes(skill.supported_skill_scopes())
        );
    }

    println!("\nInstruction-capable:");
    for instr in instruction_capable() {
        println!(
            "  {:<14} scopes={}",
            instr.id(),
            format_scopes(instr.supported_instruction_scopes())
        );
    }
}

fn format_scopes(scopes: &[ScopeKind]) -> String {
    let mut parts = Vec::new();
    if scopes.contains(&ScopeKind::Global) {
        parts.push("Global");
    }
    if scopes.contains(&ScopeKind::Local) {
        parts.push("Local");
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(" + ")
    }
}
