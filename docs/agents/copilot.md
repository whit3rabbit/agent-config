# GitHub Copilot

ID: `copilot` — `agent_config::by_id("copilot")`

## Hooks

### User scope (`Scope::Global`)

Not supported by this integration. Copilot CLI supports user-level hooks in
`~/.copilot/hooks/*.json` (or `$COPILOT_HOME/hooks/*.json`), but `agent-config`
currently targets repository-scoped hooks (`Scope::Local`) under `.github/hooks/`,
which are supported across both Copilot CLI and Copilot in the cloud.

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

Other events: `sessionStart`, `sessionEnd`, `userPromptSubmitted`, `postToolUseFailure`, `permissionRequest`, `errorOccurred`.

### Matcher mapping

| `Matcher::*`        | Copilot string |
| ------------------- | -------------- |
| `All`               | `*`            |
| `Bash`              | `Shell`        |
| `Exact(s)`          | `s`            |
| `AnyOf([a, b])`     | `a|b`         |
| `Regex(s)`          | `s`            |

### Windows-only PowerShell variant — TODO

For pure Windows targeting, Copilot accepts `powershell` instead of `bash`.
v0.1 always writes `bash`. Add `ScriptTemplate::PowerShell` if/when needed.

## Prompt instructions

### Memory file support

- **Supported format**: Both own file (`.github/copilot-instructions.md`) and `AGENTS.md`.
- **Primary memory file**: `~/.copilot/copilot-instructions.md` (user) and `.github/copilot-instructions.md` (repository).
- **`AGENTS.md` support**: Supported; Copilot CLI and GitHub.com coding agents also recognize repository-level `AGENTS.md`.


| | |
| --- | --- |
| Project scope file | `<root>/.github/copilot-instructions.md` |
| Format | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |

Copilot also reads `<root>/.github/instructions/*.instructions.md` (path-scoped
via frontmatter `applyTo`) and project-root `AGENTS.md`. Use the Codex integration to
write `AGENTS.md`. At user scope, Copilot CLI also supports `~/.copilot/copilot-instructions.md`
and `~/.copilot/instructions/**/*.instructions.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because GitHub
Copilot's memory file does not expose a documented `@import` syntax across all
surfaces (Copilot CLI supports `@<relative-path>` file references, but
`agent-config` embeds an inline fenced block into `.github/copilot-instructions.md`
for universal compatibility across CLI, IDE, and cloud agent).

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
| Ledger | `~/.copilot/.agent-config-mcp.json` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.mcp.json` |
| Format | JSON |
| Key | `mcpServers` |
| Ledger | `<root>/.agent-config-mcp.json` |

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
object. `agent-config` targets the Copilot CLI/cloud-agent `.mcp.json` contract
with `mcpServers`. Copilot CLI also reads shared project configuration from
`<root>/.github/mcp.json` when present.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.copilot/skills/<name>/` |
| Project | `.github/skills/<name>/` |
| Ledger (User) | `~/.copilot/skills/.agent-config-skills.json` |
| Ledger (Project) | `.github/skills/.agent-config-skills.json` |

Each skill is a directory containing `SKILL.md` with required `name` and
`description` frontmatter. Copilot CLI also discovers skills in `.claude/skills`
and `.agents/skills`, but `agent-config` writes to `.github/skills/<name>/` for
project scope and `~/.copilot/skills/<name>/` for user scope. Copilot also supports
`allowed-tools`, `license`, `user-invocable`, and `disable-model-invocation` frontmatter,
but `agent-config` only renders the shared fields exposed by `SkillSpec`.

## References

### Official GitHub Copilot Documentation
- <https://docs.github.com/en/copilot/reference/hooks-reference>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-custom-instructions>
- <https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/add-custom-instructions/add-repository-instructions>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills>
- <https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference>
- <https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference>

### VS Code Agent Customization Documentation
- <https://code.visualstudio.com/docs/agent-customization/overview>
- <https://code.visualstudio.com/docs/agent-customization/hooks>
- <https://code.visualstudio.com/docs/agent-customization/custom-instructions>
- <https://code.visualstudio.com/docs/agent-customization/mcp-servers>
