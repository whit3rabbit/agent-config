# Claude Code

ID: `claude` — `agent_config::by_id("claude")`

Global-scope paths below resolve under `~/.claude`; when the host sets
`CLAUDE_CONFIG_DIR` (Claude Code's documented config-directory override),
every `~/.claude/...` path is rooted at that directory instead. Note that
`~/.claude.json` (user-scoped MCP configuration) is always located in the user
home directory (`$HOME` or `%USERPROFILE%`).

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
          {
            "type": "command",
            "command": "myapp",
            "args": ["hook", "claude"]
          }
        ],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

`HookCommand::Program` renders as Claude's exec form (`command` plus `args`).
`HookCommand::ShellUnchecked` preserves the legacy shell-string form with only
`command`.

### Event mapping

| `Event::*`     | Claude string |
| -------------- | ------------- |
| `PreToolUse`   | `PreToolUse`  |
| `PostToolUse`  | `PostToolUse` |
| `Notification` | `Notification` |
| `UserPromptSubmit` | `UserPromptSubmit` |
| `Stop`         | `Stop`        |
| `SubagentStop` | `SubagentStop` |
| `PreCompact`   | `PreCompact`  |
| `SessionStart` | `SessionStart` |
| `SessionEnd`   | `SessionEnd`  |
| `PermissionRequest` | `PermissionRequest` |
| `PermissionDenied`  | `PermissionDenied`  |
| `ConfigChange` | `ConfigChange` |
| `FileChanged`  | `FileChanged` |
| `InstructionsLoaded` | `InstructionsLoaded` |
| `Custom(s)`    | `s`           |

Claude supports additional hook events (e.g. `WorktreeCreate`,
`WorktreeRemove`, `Setup`, `Teardown`, `SubagentStart`, `MessageDisplay`).
Use `Event::Custom` to attach to those or any newly introduced event.

### Matcher mapping

| `Matcher::*`        | Claude string |
| ------------------- | ------------- |
| `All`               | `*`           |
| `Bash`              | `Bash`        |
| `Exact(s)`          | `s`           |
| `AnyOf([a, b])`     | `a\|b`        |
| `Regex(s)`          | `s` (verbatim; Claude treats non-`[A-Za-z0-9_\|]` as JS regex) |

## Prompt instructions

### Memory file support

- **Supported format**: Own memory file (`CLAUDE.md`).
- **Primary memory file**: `~/.claude/CLAUDE.md` (global), `<root>/CLAUDE.md` and `<root>/CLAUDE.local.md` (local), plus `.claude/rules/*.md`.
- **`AGENTS.md` support**: Claude Code does not natively load `AGENTS.md` by default; projects can import `AGENTS.md` by adding `@AGENTS.md` inside `CLAUDE.md`.


| | |
| --- | --- |
| User scope file | `~/.claude/CLAUDE.md` |
| Project scope file | `<root>/CLAUDE.md` |
| Format | Tagged HTML-comment fence |

Set `HookSpec::rules` to inject a `RulesBlock`. Repeated installs replace the
fenced span in place.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Claude is
the only registered agent that uses `InstructionPlacement::ReferencedFile` by
default — it writes a separate `<name>.md` and adds an `@<name>.md` include
to `CLAUDE.md`. Other placements (`InlineBlock`, `StandaloneFile`) work too if
the consumer overrides `placement`.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Instruction file | `~/.claude/<name>.md` |
| Host include in | `~/.claude/CLAUDE.md` |
| Reference syntax | `@<name>.md` (inside a managed `<!-- BEGIN AGENT-CONFIG-INSTR:<name> --> ... <!-- END AGENT-CONFIG-INSTR:<name> -->` fence) |
| Ledger | `~/.claude/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::ReferencedFile` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Instruction file | `<root>/.claude/instructions/<name>.md` |
| Host include in | `<root>/CLAUDE.md` |
| Reference syntax | `@.claude/instructions/<name>.md` |
| Ledger | `<root>/.claude/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::ReferencedFile` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.claude.json` |
| Format | JSON |
| Mechanism | Top-level `mcpServers` object (user scope, applies across all projects) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.mcp.json` |
| Format | JSON |
| Mechanism | Top-level `mcpServers` object (project scope, version-controlled) |

**Important:** Do not use `settings.json` for MCP servers. This integration
writes user-scoped MCP servers to `~/.claude.json` and project-shared MCP to
`<root>/.mcp.json`.

### Example

```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["/path/to/server.js"],
      "env": {
        "API_KEY": "${API_KEY}"
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

If a folder under a Claude skills directory contains
`.claude-plugin/plugin.json`, Claude treats it as a skills-directory plugin
instead of a plain skill. This crate installs plain skill directories; plugin
lifecycle scaffolding remains out of scope.

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
when_to_use: Use for repository-specific review workflows
argument-hint: issue-id
arguments:
  - issue
disable-model-invocation: true
user-invocable: true
allowed-tools:
  - Read
  - Bash
disallowed-tools:
  - Write
model: inherit
effort: high
context: fork
agent: reviewer
paths:
  - src/**
shell: bash
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

The builder exposes typed setters on `SkillFrontmatter` for Claude's skill
frontmatter: `when_to_use`, `argument_hint`, `arguments`,
`disable_model_invocation`, `user_invocable`, `allowed_tools`,
`disallowed_tools`, `model`, `effort`, `context`, `agent`, `paths`, and
`shell`. Claude Code also recognizes `background`, `hooks`, `metadata`,
`license`, and `compatibility`. `description` is required by this crate for
cross-harness safety, even though Claude can infer it in some cases.

## References

- Hooks:
  - Hooks reference: <https://code.claude.com/docs/en/hooks>
  - Hooks guide: <https://code.claude.com/docs/en/hooks-guide>
- Settings and configuration directory:
  - Settings files and precedence: <https://code.claude.com/docs/en/settings>
  - Settings reference: <https://code.claude.com/docs/en/settings-reference>
  - `.claude` directory reference: <https://code.claude.com/docs/en/claude-directory>
  - Environment variables reference: <https://code.claude.com/docs/en/env-vars>
- MCP:
  - Model Context Protocol (MCP): <https://code.claude.com/docs/en/mcp>
- Memory and instructions:
  - How Claude remembers your project: <https://code.claude.com/docs/en/memory>
- Skills and plugins:
  - Skills reference: <https://code.claude.com/docs/en/skills>
  - Plugin manifest reference: <https://code.claude.com/docs/en/plugins/manifest-reference> (legacy alias: <https://code.claude.com/docs/en/plugins-reference>)
