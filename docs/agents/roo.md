# Roo Code

ID: `roo` — `ai_hooker::by_id("roo")`

Roo Code is a Cline fork; despite shared lineage, it uses a distinct rules
directory and is treated as a separate integration.

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Primary file | `.roo/rules/` (directory of markdown files) |
| Mode-specific | `.roo/rules-{modeSlug}/` (e.g., `.roo/rules-code/`) |
| Fallback file | `.roorules` (single-file mode, legacy) |
| Mechanism | One markdown file per consumer in `.roo/rules/` |
| Format | Plain markdown or text |

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Path | `~/.roo/rules/` |
| Format | Markdown or text files |

Roo Code loads rules from workspace rules first, then global rules (workspace
takes precedence in case of conflicts).

### AGENTS.md support

**As of February 2026**, Roo Code fully supports `AGENTS.md` (open standard).

| | |
| --- | --- |
| Primary | `<root>/AGENTS.md` or `<root>/AGENT.md` |
| Variant | `<root>/AGENTS.local.md` (personal overrides, auto-gitignored) |
| Format | Standard Markdown |
| Enable/disable | `"roo-cline.useAgentRules": false` in settings |

## MCP servers — Not supported

Roo Code does not support MCP servers.

## Skills — Not supported

Roo Code does not support skills.

## References

- <https://docs.roocode.com/features/custom-instructions>
- <https://docs.roocode.com/update-notes/v3.47.0>
