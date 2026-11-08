# Codex CLI

ID: `codex` — `agent_config::by_id("codex")`

Codex CLI is OpenAI's terminal-based AI coding agent. Global-scope paths resolve
under `$CODEX_HOME` when set, falling back to `~/.codex`.

## Surfaces

| Surface | Scope | Notes |
| ------- | ----- | ----- |
| Hooks | Global + Local | JSON patch in `hooks.json` (or inline TOML table in `config.toml`) |
| Prompt | Global + Local | Fenced block in `AGENTS.md` |
| MCP | Global + Local | `[mcp_servers.<name>]` table in `config.toml` |
| Skills | Global + Local | `SKILL.md` directories under `.agents/skills/<name>/` |
| Instructions | Global + Local | Tagged inline block in `AGENTS.md` (`InstructionPlacement::InlineBlock`) |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `$CODEX_HOME/hooks.json` (default `~/.codex/hooks.json`) |
| Mechanism | JSON patch |
| Backup | `<file>.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codex/hooks.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.codex/hooks.json.bak` (first patch only) |

> Project-scope hooks only load when the project is trusted by Codex.
> Codex supports hooks defined both in a standalone `hooks.json` file and as
> inline `[hooks]` tables in `config.toml`. By default, this crate writes
> `hooks.json`. Setting `spec.options.codex_inline_toml(true)` targets
> `config.toml` inline tables instead.

### Format

#### Standalone `hooks.json` (default)

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "myapp hook codex",
            "timeout": 600,
            "statusMessage": "Running myapp hook..."
          }
        ],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

#### Inline `config.toml` (`codex_inline_toml = true`)

```toml
[hooks.myapp]
event = "PreToolUse"
matcher = "Bash"
command = "myapp hook codex"
timeout = 600
statusMessage = "Running myapp hook..."
```

### Event mapping

Codex uses PascalCase event names (like Claude).

| `Event::*`          | Codex string        | Matcher Support |
| ------------------- | ------------------- | --------------- |
| `PreToolUse`        | `PreToolUse`        | Tool matchers (`Bash`, `Exact`, `AnyOf`, `Regex`, `*`) |
| `PostToolUse`       | `PostToolUse`       | Tool matchers (`Bash`, `Exact`, `AnyOf`, `Regex`, `*`) |
| `PermissionRequest` | `PermissionRequest` | Tool matchers (`Bash`, `Exact`, `AnyOf`, `Regex`, `*`) |
| `PreCompact`        | `PreCompact`        | Lifecycle (`Matcher::All` / `*` only) |
| `SessionStart`      | `SessionStart`      | Lifecycle (`Matcher::All` / `*` only) |
| `SubagentStop`      | `SubagentStop`      | Lifecycle (`Matcher::All` / `*` only) |
| `UserPromptSubmit`  | `UserPromptSubmit`  | Lifecycle (`Matcher::All` / `*` only) |
| `Stop`              | `Stop`              | Lifecycle (`Matcher::All` / `*` only) |
| `Custom(s)`         | `s`                 | Passed through verbatim |

### Matcher mapping

| `Matcher::*`        | Codex string |
| ------------------- | ------------ |
| `All`               | `*`          |
| `Bash`              | `Bash`       |
| `Exact(s)`          | `s`          |
| `AnyOf([a, b])`     | `a\|b`       |
| `Regex(s)`          | `s`          |

## Prompt instructions

### Memory file support

- **Supported format**: `AGENTS.md`.
- **Primary memory file**: `$CODEX_HOME/AGENTS.md` (default `~/.codex/AGENTS.md`, global) and `<root>/AGENTS.md` (local).
- **`AGENTS.md` support**: Native; Codex created and natively standardizes on `AGENTS.md` without a separate proprietary memory file.


| | |
| --- | --- |
| User scope file | `$CODEX_HOME/AGENTS.md` (default `~/.codex/AGENTS.md`) |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |

> Codex walks from git root down to cwd, reading `AGENTS.override.md` first,
> then `AGENTS.md`, in each directory. There is **no `@import` directive**
> (an open feature request as of April 2026), so we inject the rules body
> inline rather than emitting an `@<name>.md`-style include.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Codex CLI's memory file does not
expose a documented `@import` syntax; the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `$CODEX_HOME/AGENTS.md` (default `~/.codex/AGENTS.md`) |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `$CODEX_HOME/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.codex/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`) |
| Format | TOML |
| Table | `[mcp_servers.<server-name>]` |
| Ledger | `$CODEX_HOME/.agent-config-mcp.json` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codex/config.toml` |
| Format | TOML |
| Table | `[mcp_servers.<server-name>]` |
| Ledger | `<root>/.codex/.agent-config-mcp.json` |

> Project MCP config only loads when the project is trusted by Codex.

### Configuration

#### Standard IO (stdio)

```toml
[mcp_servers.my_server]
command = "node"
args = ["/path/to/server.js"]

[mcp_servers.my_server.env]
API_KEY = "${API_KEY}"
```

#### HTTP

```toml
[mcp_servers.remote_server]
url = "https://mcp.example.com"
http_headers = { Authorization = "Bearer secret-token" }
```

Supports both stdio (`command`/`args`/`env`) and http (`url`/`http_headers`) transports.
Codex also supports `bearer_token_env_var` in native configurations, while this crate
serializes explicit headers under `http_headers`. Server-sent events (SSE) is not a
supported Codex MCP transport and is refused with `RefusalReason::UnsupportedTransport`.

## Skills

| Scope | Path | Ledger |
| ----- | ---- | ------ |
| User | `~/.agents/skills/<name>/` | `~/.agents/skills/.agent-config-skills.json` |
| Project | `.agents/skills/<name>/` | `<root>/.agents/skills/.agent-config-skills.json` |

Codex treats `.agents/skills` as its native repository skill location and cross-agent
user skill directory. Codex CLI also discovers personal user skills in `~/.codex/skills/`
(and `$CODEX_HOME/skills`), while `agent-config` targets the `.agents/skills` standard.
Each skill is a folder containing a `SKILL.md` file with `name` and `description`
YAML frontmatter plus optional scripts, examples, or references.

## References

### Official Documentation (Canonical)

- Hooks Guide: <https://learn.chatgpt.com/docs/hooks>
- AGENTS.md Custom Instructions: <https://learn.chatgpt.com/docs/agent-configuration/agents-md>
- Configuration File (`config.toml`): <https://learn.chatgpt.com/docs/config-file/config-basic>
- Model Context Protocol (MCP) in CLI: <https://learn.chatgpt.com/docs/extend/mcp?surface=cli>
- Agent Skills Guide: <https://learn.chatgpt.com/docs/build-skills>
- Codex CLI Reference: <https://learn.chatgpt.com/docs/codex/cli>
- OpenAI Documentation Hub: <https://learn.chatgpt.com/docs>

### Repositories and Standards

- OpenAI Codex Repository: <https://github.com/openai/codex>
- AGENTS.md Standard Specification: <https://agents.md>

### Historical Aliases

The original `developers.openai.com` URLs now issue HTTP 308 permanent redirects to the canonical pages above:
- `https://developers.openai.com/codex/hooks` → `https://learn.chatgpt.com/docs/hooks`
- `https://developers.openai.com/codex/guides/agents-md` → `https://learn.chatgpt.com/docs/agent-configuration/agents-md`
- `https://developers.openai.com/codex/config-basic` → `https://learn.chatgpt.com/docs/config-file/config-basic`
- `https://developers.openai.com/codex/mcp` → `https://learn.chatgpt.com/docs/extend/mcp?surface=cli`
- `https://developers.openai.com/codex/skills` → `https://learn.chatgpt.com/docs/build-skills`
