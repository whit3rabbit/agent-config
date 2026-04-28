# GitHub Copilot

ID: `copilot` — `agent_config::by_id("copilot")`

## Hooks

### User scope (`Scope::Global`)

Not supported. Copilot's CLI loads hook configs from cwd; the cloud agent
loads them from the repo's default branch under `.github/hooks/`. There is
no documented per-user hook directory.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.github/hooks/<tag>-rewrite.json` |
| Mechanism | One file per consumer (no shared JSON document) |
| Backup | `<file>.bak` (only if the file already existed before install) |

> Because each consumer gets its own file, multiple CLIs coexist by default;
> there is no shared array to dedupe inside.

### Format

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "type": "command",
        "bash": "myapp hook copilot",
        "matcher": "Shell"
      }
    ]
  }
}
```

> Copilot uses `bash` (or `powershell`) as the command field, **not**
> `command`. VS Code's agent hook reader maps these to `osx`+`linux`/`windows`
> internally.

### Event mapping

Copilot uses lowerCamelCase events (Claude is PascalCase).

| `Event::*`     | Copilot string |
| -------------- | -------------- |
| `PreToolUse`   | `preToolUse`   |
| `PostToolUse`  | `postToolUse`  |
| `Custom(s)`    | `s`            |

Other events: `sessionStart`, `sessionEnd`, `userPromptSubmitted`, `errorOccurred`.

### Matcher mapping

| `Matcher::*`        | Copilot string |
| ------------------- | -------------- |
| `All`               | `*`            |
| `Bash`              | `Shell`        |
| `Exact(s)`          | `s`            |
| `AnyOf([a, b])`     | `a\|b`         |
| `Regex(s)`          | `s`            |

### Windows-only PowerShell variant — TODO

For pure Windows targeting, Copilot accepts `powershell` instead of `bash`.
v0.1 always writes `bash`. Add `ScriptTemplate::PowerShell` if/when needed.

## Prompt instructions

| | |
| --- | --- |
| Project scope file | `<root>/.github/copilot-instructions.md` |
| Format | Tagged HTML-comment fence |

Copilot also reads `<root>/.github/instructions/*.instructions.md` (path-scoped
via frontmatter) and project-root `AGENTS.md`. Use the Codex integration to
write `AGENTS.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because GitHub
Copilot's memory file does not expose a documented `@import` syntax; the body
is injected as a tagged HTML-comment fenced block in the existing memory
file.

| | |
| --- | --- |
| Host file | `<root>/.github/copilot-instructions.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.github/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.copilot/mcp-config.json` |
| Format | JSON |
| Key | `mcpServers` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.mcp.json` |
| Format | JSON |
| Key | `mcpServers` |

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "@example/server"]
    }
  }
}
```

VS Code Copilot can also read `<root>/.vscode/mcp.json` with a `servers`
object. `agent-config` targets the Copilot CLI/cloud-agent contract here.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.copilot/skills/<name>/` |
| Project | `.github/skills/<name>/` |

Each skill is a directory containing `SKILL.md` with required `name` and
`description` frontmatter. Copilot also supports `allowed-tools`,
`user-invocable`, and `disable-model-invocation` frontmatter, but `agent-config`
only renders the shared fields exposed by `SkillSpec`.

## References

- <https://docs.github.com/en/copilot/reference/hooks-configuration>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks>
- <https://code.visualstudio.com/docs/copilot/customization/hooks>
- <https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference>
