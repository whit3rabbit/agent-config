# Windsurf

ID: `windsurf` — `agent_config::by_id("windsurf")`

(Windsurf was developed by Codeium and has transitioned into Devin Desktop under Cognition. Cascade is the legacy local agent in Devin Desktop, and Devin Local is the successor agent. Devin Desktop retains native backwards-compatible support for `.windsurf/rules/`, `.windsurf/hooks.json`, `.windsurf/mcp_config.json`, `.windsurf/skills/`, and `~/.codeium/windsurf/*`.)

## Hooks

### User scope (`Scope::Global`)

Not supported in v0.1. Windsurf has user-level hook locations that vary by
client family (e.g. `~/.codeium/windsurf/hooks.json` in Windsurf IDE / Devin Desktop and `~/.codeium/hooks.json` in the JetBrains plugin), so this crate only writes the project-local hook file. Calling with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.windsurf/hooks.json` |
| Format | JSON |
| Mechanism | Pre/post response hooks via tagged arrays |

### Hook types

**Pre-hooks (blocking):** Receive event JSON on stdin and can exit with code 2 to block actions.

- `pre_user_prompt`
- `pre_read_code`
- `pre_write_code`
- `pre_run_command` (default mapping for `Event::PreToolUse`)
- `pre_mcp_tool_use`

**Post-hooks (non-blocking):**

- `post_cascade_response` (default mapping for `Event::PostToolUse`; includes `rules_applied` field)
- `post_cascade_response_with_transcript`
- `post_user_prompt`
- `post_read_code`
- `post_write_code`
- `post_run_command`
- `post_mcp_tool_use`
- `post_setup_worktree`

### Configuration

Hooks are stored as an array of objects under each event key in `.windsurf/hooks.json`. Each consumer entry includes `"bash"` with the rendered command and `"_agent_config_tag"` identifying the caller for idempotent updates and clean uninstalls:

```json
{
  "pre_run_command": [
    {
      "bash": "myapp hook windsurf",
      "_agent_config_tag": "my-tag"
    }
  ],
  "post_cascade_response": [
    {
      "bash": "myapp log windsurf",
      "_agent_config_tag": "my-tag"
    }
  ]
}
```

## Prompt instructions

### Memory file support

- **Supported format**: Both own rules (`.windsurf/rules/`) and `AGENTS.md`.
- **Primary rules directory**: `<root>/.windsurf/rules/<name>.md` (and `~/.codeium/windsurf/memories/global_rules.md`).
- **`AGENTS.md` support**: Native; Devin Desktop and Windsurf Cascade automatically discover and parse `<root>/AGENTS.md`.


### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.windsurf/rules/<tag>.md` |
| Mechanism | One markdown file per consumer |
| Format | Plain markdown (frontmatter `trigger: always_on | glob | model_decision | manual` and `globs:` are optional; max 12,000 characters per file) |

Uninstall removes the file and prunes empty parent directories.

> Windsurf also reads the legacy single-file `<root>/.windsurfrules` and
> project-root `AGENTS.md`. The directory form is preferred.

### User scope (`Scope::Global`)

Not supported in v0.1. Windsurf's global rules live at
`~/.codeium/windsurf/memories/global_rules.md` or `~/.codeium/windsurf/global_rules.md` (a single file, no
frontmatter). Calling with `Scope::Global` returns
`AgentConfigError::UnsupportedScope`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Windsurf already has a per-tag
rules directory; each instruction is one file in that directory, with no host
include needed.

| | |
| --- | --- |
| Instruction file | `<root>/.windsurf/rules/<name>.md` |
| Mechanism | One file per instruction — no host file needed |
| Ledger | `<root>/.windsurf/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.codeium/windsurf/mcp_config.json` |
| Ledger | `~/.codeium/windsurf/.agent-config-mcp.json` |
| Format | JSON |
| Marketplace | Accessible via MCPs icon in Cascade panel |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.windsurf/mcp_config.json` |
| Ledger | `.windsurf/.agent-config-mcp.json` |
| Format | JSON |

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["@example/server"],
      "env": {
        "API_KEY": "${API_KEY}"
      }
    }
  }
}
```

Supports stdio, Streamable HTTP, and SSE transports. Variable interpolation via
`${env:VAR_NAME}` and `${file:/path/to/file}`. Maximum 100 tools across all MCPs.

## Skills

| Scope | Path | Ledger |
| --- | --- | --- |
| User | `~/.codeium/windsurf/skills/<name>/` | `~/.codeium/windsurf/skills/.agent-config-skills.json` |
| Project | `.windsurf/skills/<name>/` | `.windsurf/skills/.agent-config-skills.json` |

Each skill is a directory containing `SKILL.md` (with YAML frontmatter `name:` and `description:`). Windsurf also discovers `.agents/skills`; `agent-config` writes the native
Windsurf path for this integration.

## References

- Legacy Cascade Agent Reference: <https://docs.devin.ai/desktop/cascade>
- Cascade Memories & Rules: <https://docs.devin.ai/windsurf/plugins/cascade/memories>
- Cascade MCP Configuration: <https://docs.devin.ai/windsurf/plugins/cascade/mcp>
- Devin Desktop Customizations (Rules, Skills, MCP, Hooks): <https://docs.devin.ai/desktop/devin-local/customizations>
- Devin CLI & Desktop Hooks Overview: <https://docs.devin.ai/cli/extensibility/hooks/overview>
- Devin Desktop Transition & FAQ: <https://docs.devin.ai/desktop/devin-desktop-faq>
