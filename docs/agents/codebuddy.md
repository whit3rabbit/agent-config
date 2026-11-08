# CodeBuddy CLI

ID: `codebuddy` — `agent_config::by_id("codebuddy")`

Tencent CodeBuddy (CodeBuddy Code CLI, `@tencent-ai/codebuddy-code`) mirrors the Claude Code config envelope.

## Surfaces

| Surface      | Scope          | Notes                                                                     |
| ------------ | -------------- | ------------------------------------------------------------------------- |
| Hooks        | Global + Local | Claude-shape JSON envelope (supports 7 primary lifecycle events + custom) |
| Prompt       | Global + Local | Fenced block in `CLAUDE.md`                                               |
| MCP          | -              | Upstream supports MCP via `.mcp.json`; `agent-config` does not implement MCP |
| Skills       | Global + Local | `SKILL.md` directories                                                    |
| Instructions | Global + Local | `InstructionPlacement::InlineBlock` in `CLAUDE.md`                         |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.codebuddy/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.codebuddy/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codebuddy/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.codebuddy/settings.json.bak` (first patch only) |

CodeBuddy also reads `<root>/.codebuddy/settings.local.json` (gitignored)
ahead of the project file for local uncommitted preferences. This crate writes
the project-shared file.

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "myapp hook codebuddy" }],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

| `Event::*`     | CodeBuddy string |
| -------------- | ---------------- |
| `PreToolUse`   | `PreToolUse`     |
| `PostToolUse`  | `PostToolUse`    |
| `Custom(s)`    | `s`              |

Official documentation highlights 7 primary lifecycle events: `SessionStart`,
`SessionEnd`, `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `Stop`, and
`PreCompact` (and supports `SubagentStop`, `PostToolUseFailure`, `Notification`,
and others across its full 27+ event family). `agent-config` maps `PreToolUse`
and `PostToolUse` directly, and additional lifecycle events via `Event::Custom`.

### Matcher mapping

| `Matcher::*`        | CodeBuddy string |
| ------------------- | ---------------- |
| `All`               | `*`              |
| `Bash`              | `Bash`           |
| `Exact(s)`          | `s`              |
| `AnyOf([a, b])`     | `a\|b`           |
| `Regex(s)`          | `s` (verbatim)   |

## Prompt instructions

### Memory file support

- **Supported format**: Both / all three (`CODEBUDDY.md`, `AGENTS.md`, and `CLAUDE.md`).
- **Primary memory files**: Upstream natively designates `CODEBUDDY.md` and `.codebuddy/rules/*.md`. `agent-config` targets `CLAUDE.md` (`~/.codebuddy/CLAUDE.md` and `<root>/CLAUDE.md`) following the Claude Code mirror envelope.
- **`AGENTS.md` support**: Native; CodeBuddy automatically falls back to `AGENTS.md` when native files are absent.


| | |
| --- | --- |
| User scope file | `~/.codebuddy/CLAUDE.md` |
| Project scope file | `<root>/CLAUDE.md` |
| Format | Tagged HTML-comment fence |

> [!NOTE]
> Upstream CodeBuddy Code natively designates `CODEBUDDY.md` as its primary memory
> file (`~/.codebuddy/CODEBUDDY.md` for global user memory, and `./CODEBUDDY.md` or
> `./.codebuddy/CODEBUDDY.md` for project memory, with automatic fallback to
> `AGENTS.md`). Upstream also supports modular rules in `.codebuddy/rules/*.md` and
> `@path/to/import` inclusion syntax.
> 
> The current `agent-config` implementation targets `CLAUDE.md` (`~/.codebuddy/CLAUDE.md`
> globally and `<root>/CLAUDE.md` locally) following Claude Code compatibility
> conventions.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because CodeBuddy CLI's memory file does
not expose a documented `@import` syntax in its Claude envelope; the body is
injected as a tagged HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.codebuddy/CLAUDE.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `~/.codebuddy/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/CLAUDE.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.codebuddy/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.codebuddy/skills/<name>/` |
| Project | `<root>/.codebuddy/skills/<name>/` |

Each skill directory contains a required `SKILL.md` (with YAML frontmatter `name`
and `description`) along with optional bundled resources (`scripts/`, `references/`,
and `assets/`), adhering to standard agent skill directory conventions.

## Model Context Protocol (MCP)

Upstream CodeBuddy Code CLI supports MCP server configuration using JSONC files
with the standard `mcpServers` object map:
- User scope: `~/.codebuddy/.mcp.json` (recommended) or `~/.codebuddy/mcp.json`
- Project scope: `<root>/.mcp.json` (recommended) or `<root>/mcp.json`

Currently, `agent-config` does not implement `McpSurface` for CodeBuddy.

## References

- [Tencent Cloud CodeBuddy Code Overview](https://intl.cloud.tencent.com/document/product/1316/84414)
- [CLI Command Reference](https://intl.cloud.tencent.com/document/product/1316/84467)
- [Settings Configuration Reference](https://intl.cloud.tencent.com/document/product/1316/84425)
- [Hook Feature Documentation](https://intl.cloud.tencent.com/document/product/1316/84413)
- [Hooks Configuration Reference](https://intl.cloud.tencent.com/document/product/1316/84468)
- [Memory Documentation](https://intl.cloud.tencent.com/document/product/1316/84427)
- [Skills Documentation](https://intl.cloud.tencent.com/document/product/1316/84412)
- [Directory Structure of `.codebuddy`](https://intl.cloud.tencent.com/document/product/1316/84431)
- [MCP Documentation](https://intl.cloud.tencent.com/document/product/1316/84438)
- [CodeBuddy Code Repository (CNB)](https://cnb.cool/codebuddy/codebuddy-code)
- [CodeBuddy Code on npm (`@tencent-ai/codebuddy-code`)](https://www.npmjs.com/package/@tencent-ai/codebuddy-code)

Accessed: 2026-11-08.
