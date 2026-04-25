<!--
Doc template for `docs/agents/<id>.md`. Replace placeholders, then delete the
sections (and their `<!-- DELETE IF NOT SUPPORTED -->` markers) that don't
apply to your harness. See `docs/agents/claude.md` and `docs/agents/codex.md`
for fully filled-in examples.
-->

# <Display Name>

ID: `<id>` — `ai_hooker::by_id("<id>")`

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.<id>/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.<id>/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.<id>/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.<id>/settings.json.bak` (first patch only) |

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "myapp hook <id>" }
        ],
        "_ai_hooker_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

| `Event::*`     | <Display Name> string |
| -------------- | --------------------- |
| `PreToolUse`   | `PreToolUse`          |
| `PostToolUse`  | `PostToolUse`         |
| `Custom(s)`    | `s`                   |

### Matcher mapping

| `Matcher::*`        | <Display Name> string |
| ------------------- | --------------------- |
| `All`               | `*`                   |
| `Bash`              | `Bash`                |
| `Exact(s)`          | `s`                   |
| `AnyOf([a, b])`     | `a\|b`                |
| `Regex(s)`          | `s` (verbatim)        |

<!-- DELETE THIS WHOLE SECTION IF YOUR HARNESS HAS NO RULES/MEMORY FILE -->
## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.<id>/RULES.md` |
| Project scope file | `<root>/RULES.md` |
| Format | Tagged HTML-comment fence |

Set `HookSpec::rules` to inject a `RulesBlock`. Repeated installs replace the
fenced span in place.

<!-- DELETE THIS WHOLE SECTION IF YOUR HARNESS HAS NO FILE-BACKED MCP CONTRACT -->
## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.<id>/mcp.json` |
| Format | JSON |
| Mechanism | `mcpServers` object map |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.<id>/mcp.json` |
| Format | JSON |
| Mechanism | `mcpServers` object map |

### Example

```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["/path/to/server.js"],
      "env": { "API_KEY": "secret" }
    }
  }
}
```

<!-- DELETE THIS WHOLE SECTION IF YOUR HARNESS HAS NO SKILLS CONCEPT -->
## Skills

### Path

| | |
| --- | --- |
| User scope | `~/.<id>/skills/<name>/` |
| Project scope | `<root>/.<id>/skills/<name>/` |

### Format

```
my-skill/
├── SKILL.md              (required: frontmatter + markdown body)
├── scripts/              (optional)
├── references/           (optional)
└── assets/               (optional)
```

`SKILL.md` frontmatter:

```markdown
---
name: my-skill
description: When to activate this skill.
---

## Goal
...
```

## References

- <https://docs.example.com/<id>/hooks>
- <https://docs.example.com/<id>/mcp>
- <https://docs.example.com/<id>/skills>
