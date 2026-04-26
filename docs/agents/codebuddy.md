# CodeBuddy CLI

ID: `codebuddy` — `agent_config::by_id("codebuddy")`

Tencent CodeBuddy mirrors the Claude Code config envelope.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | Global + Local | Claude-shape JSON envelope (9 events)  |
| Prompt  | Global + Local | Fenced block in `CLAUDE.md`            |
| MCP     | -              | Not part of CodeBuddy's documented surface |
| Skills  | Global + Local | `SKILL.md` directories                 |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.codebuddy/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.codebuddy/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codebuddy/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.codebuddy/settings.json.bak` (first patch only) |

CodeBuddy also reads `<root>/.codebuddy/settings.local.json` (gitignored)
ahead of the project file. This crate writes the project-shared file.

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "myapp hook codebuddy" }],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

| `Event::*`     | CodeBuddy string |
| -------------- | ---------------- |
| `PreToolUse`   | `PreToolUse`     |
| `PostToolUse`  | `PostToolUse`    |
| `Custom(s)`    | `s`              |

CodeBuddy supports `Notification`, `UserPromptSubmit`, `Stop`, `SubagentStop`,
`PreCompact`, `SessionStart`, `SessionEnd` via `Event::Custom`.

### Matcher mapping

| `Matcher::*`        | CodeBuddy string |
| ------------------- | ---------------- |
| `All`               | `*`              |
| `Bash`              | `Bash`           |
| `Exact(s)`          | `s`              |
| `AnyOf([a, b])`     | `a\|b`           |
| `Regex(s)`          | `s` (verbatim)   |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.codebuddy/CLAUDE.md` |
| Project scope file | `<root>/CLAUDE.md` |
| Format | Tagged HTML-comment fence |

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.codebuddy/skills/<name>/` |
| Project | `<root>/.codebuddy/skills/<name>/` |

## References

- <https://www.codebuddy.ai/docs/cli/>
- <https://www.codebuddy.ai/docs/cli/settings>
- <https://www.codebuddy.ai/docs/cli/hooks>

Accessed: 2026-04-26.
