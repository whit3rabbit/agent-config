# GitHub Copilot

ID: `copilot` — `ai_hooker::by_id("copilot")`

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

## MCP servers — Not supported

Copilot does not have documented MCP server support. MCP integration is
handled through hook mechanisms when available.

## Skills — Not supported

Copilot does not have a dedicated skills system.

## References

- <https://docs.github.com/en/copilot/reference/hooks-configuration>
- <https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks>
- <https://code.visualstudio.com/docs/copilot/customization/hooks>
