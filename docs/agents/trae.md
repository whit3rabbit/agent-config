# Trae

ID: `trae` — `agent_config::by_id("trae")`

## Surfaces

| Surface      | Scope          | Notes                                                                                       |
| ------------ | -------------- | ------------------------------------------------------------------------------------------- |
| Hooks        | -              | Upstream supports lifecycle hooks via `hooks.json`; `agent-config` does not implement hooks |
| Prompt       | Local          | Fenced block in `.trae/project_rules.md`                                                    |
| MCP          | -              | Upstream supports MCP via `mcp.json` (`mcpServers`); `agent-config` does not implement MCP  |
| Skills       | Global + Local | `SKILL.md` directories                                                                      |
| Instructions | Local          | `InlineBlock` in `.trae/project_rules.md`                                                   |

## Prompt instructions

### Memory file support

- **Supported format**: Both own rules (`.trae/project_rules.md`, `.trae/rules/*.md`) and `AGENTS.md` / `CLAUDE.md`.
- **Primary memory file**: `<root>/.trae/project_rules.md` (written by this crate) and `~/.trae/user_rules` (global).
- **`AGENTS.md` support**: Native; Trae automatically checks for project-level `AGENTS.md` and `CLAUDE.md`.


| | |
| --- | --- |
| Project scope file | `<root>/.trae/project_rules.md` |
| Format | Tagged HTML-comment fence |

Global prompt install is unsupported. Upstream Trae documents global user rules at
`~/.trae/user_rules` (or `%userprofile%/.trae/user_rules` on Windows), project rules in
the `.trae/rules/` directory, and project-root `AGENTS.md` / `CLAUDE.md`. This crate writes
the project-scoped `.trae/project_rules.md` file.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because Trae's
memory file does not expose a documented `@import` syntax; the body is
injected as a tagged HTML-comment fenced block in the existing memory file.

| | |
| --- | --- |
| Host file | `<root>/.trae/project_rules.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.trae/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.trae/skills/<name>/` (`%userprofile%/.trae/skills/<name>/` on Windows) |
| Project | `<root>/.trae/skills/<name>/` |

Each skill is a directory with `SKILL.md` plus optional `scripts/`,
`examples/`, `resources/`. Trae also recognizes the open `.agents/skills/`
convention directory. Trae's CLI sometimes installs skills via
`npx skills add <repo>`; this crate writes the documented directory layout.

## References

- <https://docs.trae.ai/ide/skills>
- <https://docs.trae.ai/ide/rules>
- <https://docs.trae.ai/ide/automate-actions-with-hooks>
- <https://docs.trae.ai/ide/hook-configuration-reference>
- <https://docs.trae.ai/ide/model-context-protocol>
- <https://docs.trae.ai/ide/add-mcp-servers>
- <https://github.com/bytedance/trae-agent>

Accessed: 2026-11-08.
