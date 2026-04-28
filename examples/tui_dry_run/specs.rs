//! Canned specs and per-tab metadata for the TUI dry-run example.
//!
//! Each tab demonstrates one library surface with one canonical spec. The
//! specs are constructed in source so a reader of this file can correlate
//! the builder calls with the planned changes the right pane shows.

use agent_config::{
    Event, HookSpec, InstructionPlacement, InstructionSpec, Matcher, McpSpec, SkillSpec,
};

/// Owner tag recorded in every sidecar ledger this example writes to.
/// Single value across all surfaces so the example is recognizable in
/// `.agent-config-*.json` files.
pub const OWNER: &str = "example-tui";

/// One library surface per tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Skills,
    Mcp,
    Hooks,
    Instructions,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Skills, Tab::Mcp, Tab::Hooks, Tab::Instructions];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Skills => "SKILLS",
            Tab::Mcp => "MCP",
            Tab::Hooks => "HOOKS",
            Tab::Instructions => "INSTRUCTIONS",
        }
    }

    /// Static snippet of builder code shown in the right-pane "Spec" view,
    /// chosen to mirror the runtime spec exactly.
    pub fn spec_snippet(self) -> &'static str {
        match self {
            Tab::Skills => SKILL_SNIPPET,
            Tab::Mcp => MCP_SNIPPET,
            Tab::Hooks => HOOK_SNIPPET,
            Tab::Instructions => INSTRUCTION_SNIPPET,
        }
    }

    pub fn surface_caption(self) -> &'static str {
        match self {
            Tab::Skills => "skill_capable() | SkillSurface::plan_install_skill",
            Tab::Mcp => "mcp_capable() | McpSurface::plan_install_mcp",
            Tab::Hooks => "hook list (matrix) | Integration::plan_install",
            Tab::Instructions => {
                "instruction_capable() | InstructionSurface::plan_install_instruction"
            }
        }
    }
}

/// Hook-capable agents, mirroring rows in `docs/support-matrix.md` where the
/// Hooks column is `done`. Maintained by hand because the library has no
/// `hook_capable()` filter and `registry::all()` includes prompt-only
/// registrants (Roo, Kilocode) that would reject a `HookSpec` lacking
/// `rules`. If the matrix changes, update this list.
pub const HOOK_AGENTS: &[&str] = &[
    "claude",
    "cursor",
    "gemini",
    "codex",
    "copilot",
    "opencode",
    "cline",
    "windsurf",
    "codebuddy",
    "iflow",
    "tabnine",
];

const HOOK_SNIPPET: &str = "\
HookSpec::builder(\"example-hook\")
    .command_program(\"echo\", [\"tool used\"])
    .matcher(Matcher::Bash)
    .event(Event::PostToolUse)
    .try_build()?";

const MCP_SNIPPET: &str = "\
McpSpec::builder(\"weather\")
    .owner(\"example-tui\")
    .stdio(\"mcp-weather\", [] as [&str; 0])
    .try_build()?";

const SKILL_SNIPPET: &str = "\
SkillSpec::builder(\"sample-skill\")
    .owner(\"example-tui\")
    .description(\"Use when the user asks to demonstrate ...\")
    .body(/* see examples/assets/sample-skill/skill.md */)
    .try_build()?";

// Placement for the INSTRUCTIONS tab is per-agent (see
// `instruction_placement_for`). The static snippet shown when no agent
// is highlighted shows the generic shape; the live preview substitutes
// the real placement variant.
const INSTRUCTION_SNIPPET: &str = "\
InstructionSpec::builder(\"example-instruction\")
    .owner(\"example-tui\")
    .placement(/* per-agent: see instruction_placement_for() */)
    .body(\"Always document why, not what.\\n\")
    .try_build()?";

/// Build the canonical hook spec used by the HOOKS tab.
pub fn hook_spec() -> HookSpec {
    HookSpec::builder("example-hook")
        .command_program("echo", ["tool used"])
        .matcher(Matcher::Bash)
        .event(Event::PostToolUse)
        .try_build()
        .expect("canonical hook spec is well-formed")
}

/// Build the canonical MCP spec used by the MCP tab.
pub fn mcp_spec() -> McpSpec {
    McpSpec::builder("weather")
        .owner(OWNER)
        .stdio("mcp-weather", [] as [&str; 0])
        .try_build()
        .expect("canonical mcp spec is well-formed")
}

/// Build the canonical skill spec used by the SKILLS tab.
///
/// The body is hardcoded here. The on-disk artifact at
/// `examples/assets/sample-skill/skill.md` shows what the same content
/// looks like as a frontmatter+body file once installed; readers can grep
/// for it without running the example.
pub fn skill_spec() -> SkillSpec {
    SkillSpec::builder("sample-skill")
        .owner(OWNER)
        .description("Use when the user asks to demonstrate a minimal Agent Skill end-to-end.")
        .body(concat!(
            "## Goal\n",
            "Show what an installed skill looks like on disk.\n\n",
            "## Instructions\n",
            "Read the frontmatter for the activation description, then treat\n",
            "the body as plain markdown instructions.\n",
        ))
        .try_build()
        .expect("canonical skill spec is well-formed")
}

/// Pick the right `InstructionPlacement` for a given agent.
///
/// Mirrors `docs/support-matrix.md`. Hardcoded because the library does
/// not surface "preferred placement" through `InstructionSurface`. Update
/// alongside `registry::instruction_capable()`.
pub fn instruction_placement_for(agent_id: &str) -> InstructionPlacement {
    match agent_id {
        // ReferencedFile: harness has a documented `@import` syntax.
        "claude" => InstructionPlacement::ReferencedFile,
        // StandaloneFile: harness's memory model is a per-file rules dir.
        "cline" | "roo" | "kilocode" | "windsurf" | "antigravity" => {
            InstructionPlacement::StandaloneFile
        }
        // InlineBlock: harness has a single shared memory file.
        _ => InstructionPlacement::InlineBlock,
    }
}

/// Build the canonical instruction spec for `agent_id`. The placement
/// switches per agent so each row demos the right shape (Claude →
/// ReferencedFile, Antigravity → StandaloneFile, Codex → InlineBlock,
/// etc.). Without this routing, agents that need `StandaloneFile` would
/// reject an `InlineBlock` spec at plan time.
pub fn instruction_spec_for(agent_id: &str) -> InstructionSpec {
    InstructionSpec::builder("example-instruction")
        .owner(OWNER)
        .placement(instruction_placement_for(agent_id))
        .body("Always document why, not what.\n")
        .try_build()
        .expect("canonical instruction spec is well-formed")
}

/// Render a per-agent variant of `INSTRUCTION_SNIPPET` showing the real
/// placement variant chosen by `instruction_placement_for`.
pub fn instruction_spec_snippet_for(agent_id: &str) -> String {
    let variant = match instruction_placement_for(agent_id) {
        InstructionPlacement::ReferencedFile => "ReferencedFile",
        InstructionPlacement::StandaloneFile => "StandaloneFile",
        InstructionPlacement::InlineBlock => "InlineBlock",
        _ => "InlineBlock",
    };
    format!(
        "InstructionSpec::builder(\"example-instruction\")\n    \
         .owner(\"example-tui\")\n    \
         .placement(InstructionPlacement::{variant})\n    \
         .body(\"Always document why, not what.\\n\")\n    \
         .try_build()?"
    )
}
