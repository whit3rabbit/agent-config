<!--
Doc template for `docs/agents/<id>.md`. Replace placeholders, then delete the
sections (and their `<!-- DELETE IF NOT SUPPORTED -->` markers) that don't
apply to your harness. See `docs/agents/claude.md` (full surfaces),
`docs/agents/qwen.md` (prompt + MCP + skills, no hooks), and
`docs/agents/junie.md` (local-only prompt + dual-scope MCP) for fully
filled-in examples.

Always end with a trailing line: `Accessed: YYYY-MM-DD.` so future readers
can judge whether the upstream contract may have moved.
-->

# <Display Name>

ID: `<id>` — `ai_hooker::by_id("<id>")`

## Surfaces

| Surface | Scope            | Notes                                  |
| ------- | ---------------- | -------------------------------------- |
| Hooks   | Global + Local   | JSON envelope under `hooks.<event>`    |
| Prompt  | Global + Local   | `<id>` rules markdown                  |
| MCP     | Global + Local   | `mcpServers` JSON object map           |
| Skills  | Global + Local   | `SKILL.md` directories                 |

Every implemented surface exposes `status`, `validate`, `plan_install`,
`plan_uninstall`, `install`, and `uninstall` (plus the `mcp_*` / `skill_*`
variants for those surfaces). The plan methods are side-effect-free.

<!-- DELETE THIS WHOLE SECTION IF YOUR HARNESS HAS NO HOOK SURFACE -->
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

Ownership is recorded in a sidecar `<config-dir>/.ai-hooker-mcp.json` ledger
(schema v2: includes a SHA-256 content hash for drift detection). Multiple
consumers coexist; `uninstall_mcp` returns `HookerError::NotOwnedByCaller`
on owner mismatch or hand-installed entries.

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

Ownership lives in `<skills_root>/.ai-hooker-skills.json`.

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

Accessed: YYYY-MM-DD.
