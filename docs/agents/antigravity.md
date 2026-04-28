# Google Antigravity

ID: `antigravity` — `agent_config::by_id("antigravity")`

(Google's agent-first IDE, Gemini-backed.)

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.agent/rules/<tag>.md` |
| Mechanism | One markdown file per consumer |
| Format | Markdown (frontmatter `trigger: always_on \| model_decision` is optional) |

> Note the directory is **`.agent/`** (singular), not `.agents/`. Sibling
> dirs `.agent/skills/` and `.agent/workflows/` exist for those surfaces.
>
> Antigravity also reads project-root `GEMINI.md` (highest priority) and
> `AGENTS.md` (cross-tool, since v1.20.3).

### User scope (`Scope::Global`)

Not supported in v0.1. Antigravity's user rules are configured via the
editor settings UI rather than a documented file path. Calling with
`Scope::Global` returns `AgentConfigError::UnsupportedScope`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Antigravity already has a
per-tag rules directory; each instruction is one file in that directory, with
no host include needed.

| | |
| --- | --- |
| Instruction file | `<root>/.agent/rules/<name>.md` |
| Mechanism | One file per instruction — no host file needed |
| Ledger | `<root>/.agent/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## Skills

### Path

| | |
| --- | --- |
| Workspace | `<root>/.agent/skills/<name>/` |
| Global | `~/.gemini/antigravity/skills/<name>/` |

### Format

Skills are directory-scoped. Each skill must contain:

```
my-skill/
├── SKILL.md              (required: frontmatter + body)
├── scripts/              (optional: Python, Bash, Node scripts)
├── references/           (optional: documentation, templates)
└── assets/               (optional: static assets)
```

### SKILL.md format

```markdown
---
name: git-commit-formatter
description: Executes automated formatting and generates semantic commit messages
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

**Key fields:**

- `description` (required): Specific trigger phrase for semantic relevance. This
  determines when Antigravity activates the skill.
- `name` (optional): Lowercase with hyphens.

### Automatic activation

Skills are loaded and automatically activated based on semantic description matching
against the current task.

## Workflows — TODO

Antigravity has a dedicated `<root>/.agent/workflows/` directory. Not yet
wired up. See [`CLAUDE.md`](../../CLAUDE.md).

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.gemini/antigravity/mcp_config.json` |
| Format | JSON |
| Key | `mcpServers` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.agent/mcp_config.json` |
| Format | JSON |
| Key | `mcpServers` |

## References

- <https://antigravity.codes/blog/user-rules>
- <https://antigravity.google/docs/skills>
- <https://codelabs.developers.google.com/getting-started-with-antigravity-skills>
- <https://codelabs.developers.google.com/getting-started-google-antigravity>
- <https://www.devopness.com/docsmcp/antigravity/>
