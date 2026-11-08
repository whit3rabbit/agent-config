# Qoder CLI

ID: `qodercli` — `agent_config::by_id("qodercli")`

## Surfaces

| Surface      | Scope          | Notes                                                                                             |
| ------------ | -------------- | ------------------------------------------------------------------------------------------------- |
| Hooks        | -              | Upstream Qoder CLI supports lifecycle hooks in `settings.json`; `agent-config` only supports prompt rules |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`                                                                       |
| MCP          | Global + Local | `mcpServers` JSON map; per-scope files                                                            |
| Skills       | -              | Upstream Qoder CLI supports `SKILL.md` folders; not implemented in `agent-config`                 |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`                                                                      |

## Prompt instructions

### Memory file support

- **Supported format**: Both `AGENTS.md` and modular topic rules (`.qoder/rules/`).
- **Primary memory file**: `~/.qoder/AGENTS.md` (global) and `<root>/AGENTS.md` (local).
- **`AGENTS.md` support**: Native; Qoder CLI designates `AGENTS.md` as its primary static memory file.


| | |
| --- | --- |
| User scope file | `~/.qoder/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |

Qoder CLI reconstructs context on every session. Static memory is read from
`~/.qoder/AGENTS.md` for global user-level preferences and `<root>/AGENTS.md`
for project-level conventions. Qoder also supports project-local
`<root>/AGENTS.local.md` and rule files in `<root>/.qoder/rules/**/*.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Qoder CLI's memory file does not
expose a documented `@import` syntax; the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.qoder/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `~/.qoder/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.qoder/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope  | File                                                           | Ledger                                |
| ------ | -------------------------------------------------------------- | ------------------------------------- |
| Global | `~/.qoder.json` (flat file in `$HOME`, not under `~/.qoder/`) | `~/.agent-config-mcp.json`            |
| Local  | `<root>/.mcp.json`                                             | `<root>/.agent-config-mcp.json`       |

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@example/server"]
    }
  }
}
```

Ownership is recorded in a sidecar `.agent-config-mcp.json` ledger.
Qoder CLI's `qoder mcp add/remove/list` commands manage this same JSON
`mcpServers` structure. Upstream Qoder documentation documents user-level
MCP servers under the `mcpServers` object in `~/.qoder/settings.json`, and
project-level MCP servers in `<root>/.mcp.json` (or `.qoder/settings.json`);
this crate targets `~/.qoder.json` for global scope and `<root>/.mcp.json` for
local scope.

## Skills (Upstream only)

Upstream Qoder CLI supports custom skills placed in `~/.qoder/skills/<name>/SKILL.md`
(user-level) and `<root>/.qoder/skills/<name>/SKILL.md` (project-level). Skills
are not currently implemented in `agent-config` for `qodercli`.

## Hooks (Upstream only)

Upstream Qoder CLI supports command hooks defined in JSON configuration files
(`~/.qoder/settings.json` or `<root>/.qoder/settings.json`) under the top-level
`"hooks"` key (e.g. `PreToolUse`, `PostToolUse`). Hooks are not currently
implemented in `agent-config` for `qodercli`.

## References

### Official Documentation
- <https://qoder.com/cli>
- <https://docs.qoder.com/cli/overview>
- <https://docs.qoder.com/cli/quickstart>
- <https://docs.qoder.com/cli/settings>
- <https://docs.qoder.com/cli/settings-reference>
- <https://docs.qoder.com/cli/config-scope>
- <https://docs.qoder.com/cli/memory>
- <https://docs.qoder.com/cli/how-memory-works>
- <https://docs.qoder.com/cli/mcp-servers>
- <https://docs.qoder.com/cli/mcp-reference>
- <https://docs.qoder.com/cli/Skills>
- <https://docs.qoder.com/cli/hooks>
- <https://docs.qoder.com/cli/hooks-reference>
- <https://docs.qoder.com/cli/cli-reference>

Accessed: 2026-11-08.
