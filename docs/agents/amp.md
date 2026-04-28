# Amp

ID: `amp` — `agent_config::by_id("amp")`

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Amp's documented surface   |
| Prompt  | Global + Local | Fenced block in `AGENTS.md`            |
| MCP     | Global + Local | `mcpServers` JSON map in `settings.json` |
| Skills  | Global + Local | `SKILL.md` directories                 |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.amp/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Amp falls back to `CLAUDE.md` when `AGENTS.md` is absent. This crate writes
the canonical `AGENTS.md`. Set `HookSpec::rules` to inject a `RulesBlock`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Amp's memory file does not expose
a documented `@import` syntax; the body is injected as a tagged HTML-comment
fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.amp/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `~/.amp/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `<root>/.amp/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.amp/settings.json` |
| Local | `<root>/.amp/settings.json` |

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

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.amp/skills/<name>/` |
| Project | `<root>/.amp/skills/<name>/` |

Each skill is a directory with `SKILL.md` plus optional `scripts/`,
`references/`, `assets/`. Amp also supports bundling an `mcp.json` alongside
`SKILL.md`; this crate writes the standard layout only.

## References

- <https://ampcode.com/manual>
- <https://github.com/sourcegraph/amp>

Accessed: 2026-04-26.
