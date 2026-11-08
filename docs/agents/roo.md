# Roo Code

ID: `roo` — `agent_config::by_id("roo")`

> **Retired / EOL.** All Roo Code products (the VS Code extension, Roo Code
> Cloud, and Roo Code Router) shut down on **2026-05-15** (announced
> 2026-04-21). The team pivoted to a separate cloud agent. This integration is
> kept for compatibility and migration off Roo; no further path work is planned.
> See <https://docs.roocode.com/sunset>.

Roo Code is a Cline fork; despite shared lineage, it uses a distinct rules
directory and is treated as a separate integration.

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Memory file support

- **Supported format**: Both own rules (`.roo/rules/`) and `AGENTS.md`.
- **Primary rules directory**: `<root>/.roo/rules/<name>.md` (and mode-specific `.roo/rules-{mode}/`).
- **`AGENTS.md` support**: Native; Roo Code discovers project-root `AGENTS.md` in addition to its rules directory.


### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Primary file | `.roo/rules/` (directory of markdown files) |
| Mode-specific | `.roo/rules-{modeSlug}/` (e.g., `.roo/rules-code/`) |
| Fallback file | `.roorules` (single-file mode, legacy) |
| Mechanism | One markdown file per consumer in `.roo/rules/` |
| Format | Plain markdown or text |

### User scope (`Scope::Global`)

Not supported in v0.1. Roo Code reads global rules from `~/.roo/rules/` and
mode-specific `~/.roo/rules-{modeSlug}/` (`%USERPROFILE%\.roo\rules\` on Windows),
but this crate only writes project-local rules (`Scope::Local`).

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
| File | `<vscode-globalStorage>/rooveterinaryinc.roo-cline/settings/mcp_settings.json`<br>- macOS: `~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`<br>- Linux: `~/.config/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`<br>- Windows: `%APPDATA%\Code\User\globalStorage\rooveterinaryinc.roo-cline\settings\mcp_settings.json` |
| Format | JSON |
| Key | `mcpServers` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.roo/mcp.json` |
| Format | JSON |
| Key | `mcpServers` |

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["/path/to/server.js"],
      "env": {
        "API_KEY": "secret"
      }
    }
  }
}
```

## Skills — Not registered

While Roo Code v3.47.0+ added official skills support (reading `<root>/.roo/skills/<name>/SKILL.md`
or `<root>/.agents/skills/<name>/SKILL.md` locally, and `~/.roo/skills/<name>/SKILL.md` or
`~/.agents/skills/<name>/SKILL.md` globally), Roo Code is intentionally not registered
for `SkillSurface` in `agent-config`. Because the product was sunset on May 15, 2026,
this crate maintains existing integration surfaces (rules, standalone instructions, and MCP)
for migration and compatibility, but did not add new surface implementations.

## References

- <https://docs.roocode.com/sunset> (archive: <https://roocodeinc.github.io/Roo-Code/sunset/>)
- <https://docs.roocode.com/features/custom-instructions> (archive: <https://roocodeinc.github.io/Roo-Code/features/custom-instructions/>)
- <https://docs.roocode.com/features/mcp/using-mcp-in-roo> (archive: <https://roocodeinc.github.io/Roo-Code/features/mcp/using-mcp-in-roo/>)
- <https://docs.roocode.com/features/mcp/overview> (archive: <https://roocodeinc.github.io/Roo-Code/features/mcp/overview/>)
- <https://docs.roocode.com/features/skills> (archive: <https://roocodeinc.github.io/Roo-Code/features/skills/>)
- <https://docs.roocode.com/update-notes/v3.47.0> (archive: <https://roocodeinc.github.io/Roo-Code/update-notes/v3.47.0/>)
- <https://github.com/RooCodeInc/Roo-Code>

Accessed: 2026-11-08.
