# Kilo Code

ID: `kilocode` — `agent_config::by_id("kilocode")`

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Memory file support

- **Supported format**: Both own rules (`.kilo/rules/`) and `AGENTS.md`.
- **Primary rules directory**: `<root>/.kilo/rules/<name>.md`.
- **`AGENTS.md` support**: Native; Kilo Code explicitly documents support for `<root>/AGENTS.md` and global `~/.config/kilo/AGENTS.md`.


### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.kilo/rules/<tag>.md` |
| Mechanism | One markdown file per consumer |
| Format | Plain markdown |

`.kilo/rules` is Kilo's current rules layout (referenced from `kilo.jsonc`); the
legacy `.kilocode/rules` directory still works upstream but is no longer written
by this crate. Kilo Code can also read root-level `AGENTS.md`, but this crate
writes only the per-consumer rules directory above.

### User scope (`Scope::Global`)

Not supported in v0.1. Upstream Kilo Code configures global instructions via the
`instructions` key in `~/.config/kilo/kilo.jsonc` or via `~/.config/kilo/AGENTS.md`.
Calling with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Kilo Code already has a per-tag
rules directory; each instruction is one file in that directory, with no host
include needed.

| | |
| --- | --- |
| Instruction file | `<root>/.kilo/rules/<name>.md` |
| Mechanism | One file per instruction — no host file needed |
| Ledger | `<root>/.kilo/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.config/kilo/kilo.jsonc` |
| Format | JSONC |
| Key | `mcp` (object keyed by server name) |
| Ledger | `~/.config/kilo/.agent-config-mcp.json` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/kilo.jsonc`, or existing `<root>/.kilo/kilo.jsonc` |
| Format | JSONC |
| Key | `mcp` (object keyed by server name) |
| Ledger | `<root>/.agent-config-mcp.json` or `<root>/.kilo/.agent-config-mcp.json` |

### Configuration

```jsonc
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["node", "/path/to/server.js"],
      "environment": {
        "API_KEY": "${API_KEY}"
      }
    }
  }
}
```

## Skills

| Scope | Path | Ledger |
| --- | --- | --- |
| User | `~/.kilo/skills/<name>/` | `~/.kilo/skills/.agent-config-skills.json` |
| Project | `.agents/skills/<name>/` | `.agents/skills/.agent-config-skills.json` |
| Project legacy drain | `.kilo/skills/<name>/` (status/uninstall only) | `.kilo/skills/.agent-config-skills.json` |

Current Kilo builds load project skills from the standard `.agents/skills/`
directory first, so `agent-config` writes that for Local installs. `.kilo/skills/`
still loads but is legacy; installs there from older releases are still
detected and removed. The global directory stays `~/.kilo/skills/`.

## References

- <https://kilo.ai/docs/getting-started/settings>
- <https://kilo.ai/docs/customize/agents-md>
- <https://kilo.ai/docs/customize/custom-rules>
- <https://kilo.ai/docs/customize/custom-instructions>
- <https://kilo.ai/docs/automate/mcp/using-in-kilo-code>
- <https://kilo.ai/docs/customize/skills>

Accessed: 2026-11-08.
