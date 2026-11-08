# JetBrains Junie

ID: `junie` — `agent_config::by_id("junie")`

**Status:** registered for project-local prompt rules, standalone instructions
(project-local inline block), and MCP (Global + Local). Lifecycle hook support
is in Early Access (EAP) upstream in Junie CLI, but not yet registered in this
crate. Agent skills are documented upstream, but not yet registered in this
crate's `SkillSurface`.

## Surfaces

| Surface      | Scope          | Notes                                                              |
| ------------ | -------------- | ------------------------------------------------------------------ |
| Hooks        | -              | Early access upstream in Junie CLI; not yet registered in crate    |
| Prompt       | Local          | Fenced block in `<root>/.junie/AGENTS.md`                          |
| MCP          | Global + Local | `mcpServers` JSON map at `mcp/mcp.json`                            |
| Skills       | -              | Documented upstream (`.junie/skills/`), but not registered in crate|
| Instructions | Local          | Inline fenced block in `<root>/.junie/AGENTS.md`                   |

## Prompt instructions

### Memory file support

- **Supported format**: `AGENTS.md` (both `.junie/AGENTS.md` and root `AGENTS.md`).
- **Primary memory file**: `<root>/.junie/AGENTS.md` (written by this crate) and `~/.junie/AGENTS.md` (global).
- **`AGENTS.md` support**: Native; Junie guidelines precedence checks `.junie/AGENTS.md` first, then `<root>/AGENTS.md`.


| | |
| --- | --- |
| Project scope file | `<root>/.junie/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Global prompt install is unsupported by this crate. Junie CLI discovers project
guidelines in the following order: `<root>/.junie/AGENTS.md`, `<root>/AGENTS.md`
(combined with `.junie/playbook.md` and `.junie/rules/*.md`), or legacy
`.junie/guidelines.md`. Junie CLI also supports global guidelines at
`~/.junie/AGENTS.md` (`%USERPROFILE%\.junie\AGENTS.md` on Windows) and custom
filenames via the `JUNIE_GUIDELINES_FILENAME` environment variable,
`--guidelines-filename` CLI flag, or `guidelines-location` in `config.json`.
This crate writes the documented default `<root>/.junie/AGENTS.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because JetBrains
Junie's memory file does not expose a documented `@import` syntax; the body
is injected as a tagged HTML-comment fenced block in the existing memory
file.

| | |
| --- | --- |
| Host file | `<root>/.junie/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.junie/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.junie/mcp/mcp.json` |
| Local | `<root>/.junie/mcp/mcp.json` |

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

## Hooks — Not registered

Junie CLI introduced lifecycle hooks in its Early Access Program (EAP)
under `~/.junie/config.json` (`SessionStart`, `UserPromptSubmit`, `PreToolUse`,
`PostToolUse`, `Stop`, `StopFailure`, `SessionEnd`, and `PermissionRequest`).
Hook registration in this crate is deferred until general availability.

## Skills — Not registered

Junie CLI and the Junie IDE plugin support the open Agent Skills specification
at project scope (`<root>/.junie/skills/<name>/`), user scope
(`~/.junie/skills/<name>/`, `%USERPROFILE%\.junie\skills\<name>\` on Windows),
and cross-agent `.agents/skills/` directories. Junie is not yet registered in
this crate's `SkillSurface`.

## References

- <https://junie.jetbrains.com/docs/guidelines-and-memory.html> (Guidelines and AGENTS.md discovery)
- <https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html> (Junie CLI MCP configuration)
- <https://junie.jetbrains.com/docs/junie-plugin-mcp-settings.html> (Junie IDE Plugin MCP settings)
- <https://junie.jetbrains.com/docs/junie-cli-configuration.html> (Junie CLI configuration files)
- <https://junie.jetbrains.com/docs/environment-variables.html> (Junie CLI environment variables)
- <https://junie.jetbrains.com/docs/agent-skills.html> (Junie Agent Skills specification)
- <https://junie.jetbrains.com/docs/junie-cli-hooks.html> (Junie CLI lifecycle hooks in EAP)

Accessed: 2026-11-08.
