# Google Antigravity

ID: `antigravity` — `agent_config::by_id("antigravity")`

Google's Antigravity desktop app / IDE. Antigravity CLI is separate:
[`antigravitycli`](antigravitycli.md).

## Hooks

Hook commands are configured inside `hooks.json` under top-level consumer tags.

| Scope | File |
| --- | --- |
| Global | `~/.gemini/config/hooks.json` |
| Local | `<root>/.agents/hooks.json` |

Matches event names like `PreToolUse` and `PostToolUse`. Matcher translates `Matcher::Bash` to `"run_command"`.

If the hook specification includes `rules`, prompt rules are written to `<root>/.agents/rules/<tag>.md` (Local scope only; global prompt rules are unsupported).

## Prompt Instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.agents/rules/<tag>.md` |
| Legacy fallback | `<root>/.agent/rules/<tag>.md` for status and uninstall |
| Mechanism | One markdown file per consumer |
| Format | Markdown |

New installs write `.agents/rules`. Existing `.agent/rules` content is not
migrated or removed unless the caller explicitly uninstalls the matching tag.

### User scope (`Scope::Global`)

Not supported for this integration. Antigravity global rules live in
`~/.gemini/GEMINI.md`, but this crate keeps that file with Gemini CLI and
Antigravity CLI rather than mixing it into the app/IDE integration. Calling
with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Antigravity has a per-file
rules directory; no host include is needed.

| | |
| --- | --- |
| Instruction file | `<root>/.agents/rules/<name>.md` |
| Legacy fallback | `<root>/.agent/rules/<name>.md` for status and uninstall |
| Ledger | `<root>/.agents/.agent-config-instructions.json` |
| Legacy ledger fallback | `<root>/.agent/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## Skills

### Path

| | |
| --- | --- |
| Workspace | `<root>/.agents/skills/<name>/` |
| Workspace legacy fallback | `<root>/.agent/skills/<name>/` for status and uninstall |
| Global | `~/.gemini/antigravity/skills/<name>/` |

### Format

Skills are directory-scoped. Each skill contains a required `SKILL.md` file
plus optional supporting files.

```text
my-skill/
├── SKILL.md
├── scripts/
├── examples/
└── resources/
```

### SKILL.md format

```markdown
---
name: git-commit-formatter
description: Executes automated formatting and generates semantic commit messages
---

## Goal
Describe what the skill does.
```

`description` is required. `name` is optional and defaults to the folder name
when omitted by the host.

## Workflows

Not implemented. Antigravity documents workflows as markdown files, but this
crate has no workflow surface yet.

## MCP Servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.gemini/config/mcp_config.json` |
| Format | JSON |
| Key | `mcpServers` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.agents/mcp_config.json` |
| Legacy fallback | `<root>/.agent/mcp_config.json` |
| Format | JSON |
| Key | `mcpServers` |
| Support level | Observed |

Local app MCP is updated to `.agents/mcp_config.json`, with legacy fallback support to `.agent/mcp_config.json` for status, planning, and uninstall.

## References

- <https://antigravity.google/docs/hooks>
- <https://antigravity.google/docs/rules-workflows>
- <https://antigravity.google/docs/skills>
- <https://antigravity.google/docs/mcp>
- <https://antigravity.google/assets/docs/antigravity-2-0/rules-workflows.md>
- <https://antigravity.google/assets/docs/editor/ide-skills.md>
- <https://antigravity.google/assets/docs/editor/ide-mcp.md>
