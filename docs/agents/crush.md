# Charm Crush

ID: `crush` — `agent_config::by_id("crush")`

**Status:** registered for hooks (currently only `PreToolUse` fires upstream),
prompt rules in `AGENTS.md`, MCP servers, skills, and instructions.

## Surfaces

| Surface      | Scope          | Notes                                                   |
| ------------ | -------------- | ------------------------------------------------------- |
| Hooks        | Global + Local | JSONC `crush.json`; flat entry `{matcher, command, timeout}` |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`                             |
| MCP          | Global + Local | JSONC `crush.json` under the `mcp` key (not `mcpServers`) |
| Skills       | Global + Local | SKILL.md folders                                        |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`                            |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `<crush_home>/crush.json` |
| Mechanism | JSONC patch (rewrites as strict JSON) |
| Backup | `<crush_home>/crush.json.bak` (first patch only) |

`<crush_home>` honors `$CRUSH_GLOBAL_CONFIG`; otherwise it resolves to
`$XDG_CONFIG_HOME/crush` on Unix or `%APPDATA%\crush` on Windows.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/crush.json` |
| Mechanism | JSONC patch |
| Backup | `<root>/crush.json.bak` (first patch only) |

Crush also accepts `.crush.json` at the project root; this crate writes the
unhidden filename to match the auto-init Crush ships with.

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^bash$",
        "command": "myapp hook crush",
        "timeout": 30,
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

The `matcher` field is omitted when `Matcher::All` is used (Crush docs:
"empty means match all tools"). Hook entries are flatter than Claude Code's:
no nested `hooks: [{type:"command", command}]` array.

### Event mapping

| `Event::*`     | Crush string  |
| -------------- | ------------- |
| `PreToolUse`   | `PreToolUse`  |
| `PostToolUse`  | `PostToolUse` |
| `Custom(s)`    | `s`           |

Crush event names are case-insensitive (`PreToolUse`, `pre_tool_use`, and
`PRETOOLUSE` all resolve to the same event). This crate emits the canonical
PascalCase spelling. Upstream currently only fires `PreToolUse`; other
entries are written through and ignored until support lands.

### Matcher mapping

Crush matches the matcher as an RE2 regex against the lowercase tool name
(`bash`, `edit`, `write`, `multiedit`, `view`, `ls`, `grep`, `glob`,
`mcp_<server>_<tool>`):

| `Matcher::*`        | Crush string                          |
| ------------------- | ------------------------------------- |
| `All`               | (omitted — empty matches all tools)   |
| `Bash`              | `^bash$`                              |
| `Exact(s)`          | `^<lowercased(s) escaped>$`           |
| `AnyOf([a, b])`     | `^(<a>\|<b>)$` (lowercased + escaped) |
| `Regex(s)`          | `s` (verbatim)                        |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `<crush_home>/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Crush's `initialize_as` option lets users rename this file (e.g. to
`CRUSH.md`); this crate always writes the documented default `AGENTS.md`.

## Instructions

Inline-block placement, sharing the same host file as the prompt rules
surface.

| | |
| --- | --- |
| User scope ledger | `<crush_home>/.agent-config-instructions.json` |
| Project scope ledger | `<root>/.crush/.agent-config-instructions.json` |
| Host file | Same as prompt (`AGENTS.md`) |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope  | File                       |
| ------ | -------------------------- |
| Global | `<crush_home>/crush.json`  |
| Local  | `<root>/crush.json`        |

Crush keys server entries under `"mcp"` (not the `"mcpServers"` map most
other harnesses use) and requires a per-entry `"type"` discriminant.

```json
{
  "mcp": {
    "github": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@example/server"]
    },
    "remote": {
      "type": "http",
      "url": "https://example.com/mcp"
    }
  }
}
```

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json`
ledger that lives next to `crush.json`.

## Skills

| Scope  | Path                            |
| ------ | ------------------------------- |
| Global | `<crush_home>/skills/<name>/`   |
| Local  | `<root>/.crush/skills/<name>/`  |

Each skill is a folder with `SKILL.md` plus optional `scripts/`,
`references/`, `assets/`. Crush also picks up
`~/.config/agents/skills/` and a few cross-host directories via its
`skills_paths` config; this crate writes only into the Crush-namespaced root
to avoid colliding with cross-host skill sets.

## References

- <https://github.com/charmbracelet/crush>
- <https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md>
- <https://charm.land/crush.json>

Accessed: 2026-04-28.
