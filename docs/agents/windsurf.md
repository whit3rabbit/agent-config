# Windsurf

ID: `windsurf` — `ai_hooker::by_id("windsurf")`

(Windsurf is Codeium's AI editor; the agent is called Cascade.)

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.codeium/windsurf/hooks.json` (VS Code) or `~/.codeium/hooks.json` (JetBrains) |
| Format | JSON |
| Mechanism | Pre/post response hooks with blocking capability |

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
`HookerError::UnsupportedScope`.

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
        "API_KEY": "secret"
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

Windsurf also discovers `.agents/skills`; `ai-hooker` writes the native
Windsurf path for this integration.

## References

- <https://docs.windsurf.com/windsurf/cascade/memories>
- <https://docs.windsurf.com/windsurf/cascade/hooks>
- <https://docs.windsurf.com/windsurf/cascade/mcp>
- <https://docs.windsurf.com/windsurf/cascade/skills>
