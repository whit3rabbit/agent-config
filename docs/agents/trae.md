# Trae

ID: `trae` — `agent_config::by_id("trae")`

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Trae's documented surface  |
| Prompt  | Local          | Fenced block in `.trae/project_rules.md` |
| MCP     | -              | MCP path not documented at install time |
| Skills  | Global + Local | `SKILL.md` directories                 |

## Prompt instructions

| | |
| --- | --- |
| Project scope file | `<root>/.trae/project_rules.md` |
| Format | Tagged HTML-comment fence |

Global prompt install is unsupported. Trae also reads `.trae/user_rules.md`;
this crate writes the project-scoped file only.

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
| User | `~/.trae/skills/<name>/` |
| Project | `<root>/.trae/skills/<name>/` |

Each skill is a directory with `SKILL.md` plus optional `scripts/`,
`examples/`, `resources/`. Trae's CLI sometimes installs skills via
`npx skills add <repo>`; this crate writes the documented directory layout.

## References

- <https://docs.trae.ai/ide/skills>
- <https://docs.trae.ai/ide/rules>
- <https://github.com/bytedance/trae-agent>

Accessed: 2026-04-26.
