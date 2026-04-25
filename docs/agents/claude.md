# Claude Code

ID: `claude` — `ai_hooker::by_id("claude")`

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.claude/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.claude/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.claude/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.claude/settings.json.bak` (first patch only) |

> Claude Code also reads `<root>/.claude/settings.local.json` (gitignored) and
> obeys precedence Managed > CLI > local > project > user. v0.1 writes the
> *project-shared* file. A `settings_target` knob may land later.

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "myapp hook claude" }
        ],
        "_ai_hooker_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

| `Event::*`     | Claude string |
| -------------- | ------------- |
| `PreToolUse`   | `PreToolUse`  |
| `PostToolUse`  | `PostToolUse` |
| `Custom(s)`    | `s`           |

Claude supports many additional events (`SessionStart`, `UserPromptSubmit`,
`Stop`, `SubagentStart`, etc.). Use `Event::Custom` to attach to those.

### Matcher mapping

| `Matcher::*`        | Claude string |
| ------------------- | ------------- |
| `All`               | `*`           |
| `Bash`              | `Bash`        |
| `Exact(s)`          | `s`           |
| `AnyOf([a, b])`     | `a\|b`        |
| `Regex(s)`          | `s` (verbatim; Claude treats non-`[A-Za-z0-9_\|]` as JS regex) |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.claude/CLAUDE.md` |
| Project scope file | `<root>/CLAUDE.md` |
| Format | Tagged HTML-comment fence |

Set `HookSpec::rules` to inject a `RulesBlock`. Repeated installs replace the
fenced span in place.

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.claude.json` |
| Format | JSON |
| Mechanism | Server config under the current project's entry for local/user scoped servers |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.mcp.json` |
| Format | JSON |
| Mechanism | Server config (version-controlled) |

**Important:** Do not use `settings.json` for MCP servers. This integration
writes user/local MCP to `~/.claude.json` and project-shared MCP to
`<root>/.mcp.json`.

### Example

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

### Installation

Use the Claude Code CLI to manage MCP servers:

```bash
claude mcp add <server-name>
claude mcp remove <server-name>
claude mcp list
```

## Skills

### Path

| | |
| --- | --- |
| User scope | `~/.claude/skills/<name>/` |
| Project scope | `.claude/skills/<name>/` |

### Format

Skills are directory-scoped. Each skill contains:

```
my-skill/
├── SKILL.md              (required: frontmatter + markdown body)
├── scripts/              (optional: python, bash, node scripts)
├── references/           (optional: documentation, templates)
└── assets/               (optional: images, static files)
```

`SKILL.md` frontmatter example:

```markdown
---
name: my-skill
description: Clear, specific trigger phrase for skill activation
---

## Goal
Describe what the skill does.

## Instructions
Step-by-step guidance.

## Examples
Usage examples.

## Constraints
Limitations or edge cases.
```

## References

- <https://code.claude.com/docs/en/hooks>
- <https://code.claude.com/docs/en/settings>
- <https://code.claude.com/docs/en/mcp>
- <https://code.claude.com/docs/en/memory>
