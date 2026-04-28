# Roo Code

ID: `roo` — `agent_config::by_id("roo")`

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

Not supported in v0.1. Roo Code can read global rules, but this crate only
writes project-local rules.

### AGENTS.md support

**As of February 2026**, Roo Code fully supports `AGENTS.md` (open standard).

| | |
| --- | --- |
| Primary | `<root>/AGENTS.md` or `<root>/AGENT.md` |
| Variant | `<root>/AGENTS.local.md` (personal overrides, auto-gitignored) |
| Format | Standard Markdown |
| Enable/disable | `"roo-cline.useAgentRules": false` in settings |

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Roo Code already has a per-tag
rules directory; each instruction is one file in that directory, with no host
include needed.

| | |
| --- | --- |
| Instruction file | `<root>/.roo/rules/<name>.md` |
| Mechanism | One file per instruction — no host file needed |
| Ledger | `<root>/.roo/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json` on macOS |
| Format | JSON |
| Key | `mcpServers` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.roo/mcp.json` |
| Format | JSON |
| Key | `mcpServers` |

## Skills — Not registered

No official Roo Code skill path has been identified, so Roo is intentionally
left out of `SkillSurface`. Roo's own docs also describe a product sunset on
May 15, 2026, which makes new surface work lower priority than registered,
actively documented skill harnesses.

## References

- <https://docs.roocode.com/features/custom-instructions>
- <https://docs.roocode.com/features/mcp/using-mcp-in-roo>
- <https://docs.roocode.com/update-notes/v3.47.0>
