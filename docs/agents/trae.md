# Trae

ID: `trae` — `ai_hooker::by_id("trae")`

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
