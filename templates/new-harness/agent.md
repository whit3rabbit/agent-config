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

ID: `<id>` — `agent_config::by_id("<id>")`

## Surfaces

| Surface       | Scope            | Notes                                          |
| ------------- | ---------------- | ---------------------------------------------- |
| Hooks         | Global + Local   | JSON envelope under `hooks.<event>`            |
| Prompt        | Global + Local   | `<id>` rules markdown                          |
| MCP           | Global + Local   | `mcpServers` JSON object map                   |
| Skills        | Global + Local   | `SKILL.md` directories                         |
| Instructions  | Global + Local   | `InstructionPlacement::<picked>` (see below)   |

Every implemented surface exposes `status`, `validate`, `plan_install`,
`plan_uninstall`, `install`, and `uninstall` (plus the `mcp_*`, `skill_*`, or
`instruction_*` variants for those surfaces). The plan methods are
side-effect-free.

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
        "_agent_config_tag": "myapp"
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

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger
(schema v2: includes a SHA-256 content hash for drift detection). Multiple
consumers coexist; `uninstall_mcp` returns `AgentConfigError::NotOwnedByCaller`
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

Ownership lives in `<skills_root>/.agent-config-skills.json`.

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

<!-- DELETE THIS WHOLE SECTION IF YOUR HARNESS HAS NO PROMPT/RULES SURFACE -->
## Instructions

Standalone instruction files installed via `InstructionSurface`. Pick exactly
one default placement based on the harness's memory model — the surface
supports all three modes per [`InstructionPlacement`](../../src/spec/instruction.rs):

- **`ReferencedFile`** — write `<config_dir>/<NAME>.md` and add `@<NAME>.md`
  inside a managed fenced block in the harness's memory file. Pick this if
  the harness documents an `@import` syntax (Claude does; most others don't).
- **`InlineBlock`** — inject the body as a tagged HTML-comment fenced block
  inside the harness's existing memory file. The natural default for
  single-memory-file harnesses without import support.
- **`StandaloneFile`** — write `<rules-dir>/<NAME>.md` only, no host edit.
  The natural default for harnesses whose memory model is a per-tag rules
  directory (e.g. `.clinerules/`, `.roo/rules/`, `.kilocode/rules/`).

### User scope (`Scope::Global`)  <!-- DELETE if your harness is local-only -->

| | |
| --- | --- |
| Host file | `~/.<id>/<MEMORY>.md` |
| Mechanism | Tagged HTML-comment fence `AGENT-CONFIG-INSTR:<name>` (or `@<name>.md` include wrapped in that fence if `ReferencedFile`) |
| Ledger | `~/.<id>/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::<picked>` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file or instruction file | `<root>/<MEMORY>.md` (InlineBlock / ReferencedFile) **or** `<root>/.<id>/rules/<name>.md` (StandaloneFile) |
| Mechanism | Tagged HTML-comment fence `AGENT-CONFIG-INSTR:<name>`, `@`-include wrapped in that fence, or per-file rule |
| Ledger | `<root>/.<id>/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::<picked>` |

## References

- <https://docs.example.com/<id>/hooks>
- <https://docs.example.com/<id>/mcp>
- <https://docs.example.com/<id>/skills>

Accessed: YYYY-MM-DD.
