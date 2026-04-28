# Windsurf

ID: `windsurf` — `agent_config::by_id("windsurf")`

(Windsurf is Codeium's AI editor; the agent is called Cascade.)

## Hooks

### User scope (`Scope::Global`)

Not supported in v0.1. Windsurf has user-level hook locations that vary by
client family, so this crate only writes the project-local hook file.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.windsurf/hooks.json` |
| Format | JSON |
| Mechanism | Pre/post response hooks |

### Hook types

**Pre-hooks (blocking):** Can exit with code 2 to block actions.

- `pre_user_prompt`
- `pre_read_code`
- `pre_write_code`
- `pre_run_command`
- `pre_mcp_tool_use`

**Post-hooks (non-blocking):**

- `post_cascade_response` (includes `rules_applied` field as of Feb 2026)
- Other post_* variants

### Configuration

```json
{
  "pre_run_command": {
    "bash": "myapp hook windsurf",
    "powershell": "myapp.exe hook windsurf"
  },
  "post_cascade_response": {
    "bash": "myapp log windsurf"
  }
}
```

## Prompt instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.windsurf/rules/<tag>.md` |
| Mechanism | One markdown file per consumer |
| Format | Plain markdown (frontmatter `trigger: always_on \| glob \| model_decision \| manual` and `globs:` are optional) |

Uninstall removes the file and prunes empty parent directories.

> Windsurf also reads the legacy single-file `<root>/.windsurfrules` and
> project-root `AGENTS.md`. The directory form is preferred.

### User scope (`Scope::Global`)

Not supported in v0.1. Windsurf's global rules live at
`~/.codeium/windsurf/memories/global_rules.md` (a single file, no
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
| Format | JSON |
| Marketplace | Accessible via MCPs icon in Cascade panel |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.windsurf/mcp_config.json` |
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

| Scope | Path |
| --- | --- |
| User | `~/.codeium/windsurf/skills/<name>/` |
| Project | `.windsurf/skills/<name>/` |

Windsurf also discovers `.agents/skills`; `agent-config` writes the native
Windsurf path for this integration.

## References

- <https://docs.windsurf.com/windsurf/cascade/memories>
- <https://docs.windsurf.com/windsurf/cascade/hooks>
- <https://docs.windsurf.com/windsurf/cascade/mcp>
- <https://docs.windsurf.com/windsurf/cascade/skills>
