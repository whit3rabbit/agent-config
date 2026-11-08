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

### Memory file support

- **Supported format**: Both `AGENTS.md` (primary) and `CLAUDE.md` / `AGENT.md` (fallback).
- **Primary memory file**: `~/.config/AGENTS.md` (global) and `<root>/AGENTS.md` (local).
- **`AGENTS.md` support**: Native; Amp searches upward from the working directory for `AGENTS.md` and falls back to `CLAUDE.md` or `AGENT.md`.


| | |
| --- | --- |
| User scope file | `~/.config/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Amp automatically discovers and reads `AGENTS.md` files from the working directory
upward to `$HOME`, as well as `$HOME/.config/amp/AGENTS.md` and `$HOME/.config/AGENTS.md`.
Amp falls back to `AGENT.md` (singular) or `CLAUDE.md` when `AGENTS.md` is absent.
This crate writes the canonical `AGENTS.md` (`~/.config/AGENTS.md` in global scope,
`<root>/AGENTS.md` in local project scope). Set `HookSpec::rules` to inject a
`RulesBlock`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

> [!NOTE]
> Amp supports dynamic context inclusion within `AGENTS.md` using `@-mention`
> syntax (e.g. `@doc/style.md` or `@~/path`, resolved relative to the file or
> `$HOME`). This crate uses `InlineBlock` to embed managed, fenced instruction
> blocks directly.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.config/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `~/.config/amp/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.amp/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.config/amp/settings.json` |
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

> [!NOTE]
> Amp's CLI settings schema (`cli-settings.schema.json`) defines `amp.mcpServers`
> (alongside `amp.mcpPermissions`), whereas this crate manages the standard
> `"mcpServers"` object via `mcp_json_object`.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.config/agents/skills/<name>/` |
| Project | `<root>/.agents/skills/<name>/` |
| Legacy user drain | `~/.config/amp/skills/<name>/` (status/uninstall only) |

Each skill is a directory with `SKILL.md` plus optional `scripts/`,
`references/`, `assets/`.

Amp evaluates skill directories in the following precedence order:
1. `~/.config/agents/skills/` (global machine-level)
2. `~/.agents/skills/`
3. `~/.config/amp/skills/` (legacy global)
4. `.agents/skills/` in the project and searched parent directories
5. `.claude/skills/` in those directories

Amp moved global skills from `~/.config/amp/skills/` to the shared
`~/.config/agents/skills/` standard; `~/.config/amp/` still hosts
`settings.json` and the instruction ledger. Skills installed at the legacy
location by older releases are still detected and removed during status and
uninstall operations.

## References

- Amp Documentation: <https://ampcode.com/docs>
- Amp Settings: <https://ampcode.com/docs/cli/settings>
- Amp Settings JSON Schema: <https://ampcode.com/cli-settings.schema.json>
- Amp `AGENTS.md` Guidance: <https://ampcode.com/docs/customize/agents-md>
- Amp MCP Documentation: <https://ampcode.com/docs/customize/mcp>
- Amp Skills Documentation: <https://ampcode.com/docs/customize/skills>
- Amp Global Plugins & Skills: <https://ampcode.com/docs/customize/global-plugins-and-skills>
- Amp GitHub Organization: <https://github.com/ampcode>

Accessed: 2026-11-08.
