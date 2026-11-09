# The `agent.config.toml` manifest

`agentconfig` installs package content into harnesses. The manifest declares
what to install. The CLI resolves targets and drives the `agent-config`
library. This page is the reference for the manifest format.

- A package is any folder or repo with `agent.config.toml` at its root.
- `[package].name` must be lowercase kebab-case. It becomes the ownership
  ledger owner tag for every surface the package installs.
- Unknown top-level sections are refused, because a typo'd section would
  otherwise silently install nothing. One invalid entry inside a section is
  reported as a warning and skipped, and the rest of the section still
  applies.
- Relative paths in entries resolve against the manifest's directory.
- `agents` (allow-list) and `skip` (deny-list) on any entry intersect with
  `--agent` flags and registry capability. With no filter at all, every
  capable harness is targeted.

## `[package]`

```toml
[package]
name = "acme-toolkit"      # required, kebab-case, ledger owner tag
version = "0.1.0"          # optional
description = "..."        # optional
```

## `[[hooks]]`

Maps to `HookSpec` and `Integration::install`. A hook entry requires `tag`
and `command`.

```toml
[[hooks]]
tag = "guard-secrets"                # required, the hook owner namespace on disk
command = ["scripts/guard.sh", "--strict"]   # argv array; or a plain string for raw shell
event = "PreToolUse"                 # optional, see events below
matcher = "Bash"                     # optional, see matchers below
timeout = 30                         # optional seconds
status_message = "Guarding secrets"  # optional progress text
rules = "be safe"                    # optional inline rules markdown
rules_file = "rules/guard.md"        # or a file path; mutually exclusive with rules
agents = ["claude", "codex"]         # optional allow-list
skip = ["cursor"]                    # optional deny-list
```

### Events

Canonical spellings map to each tool's own event names by the library:
`PreToolUse` (default), `PostToolUse`, `Notification`, `UserPromptSubmit`,
`Stop`, `SubagentStop`, `PreCompact`, `SessionStart`, `SessionEnd`,
`PermissionRequest`, `PermissionDenied`, `ConfigChange`, `FileChanged`,
`InstructionsLoaded`. Anything else passes through verbatim as a custom event
(`Event::Custom`). Prompt-only tools (Roo, Kilo Code, OpenClaw, Hermes, and
the rest of the prompt-rules-only column) accept only `PreToolUse` and
require `rules` or `rules_file`.

### Matchers

| Manifest value              | Library value      | Notes                                  |
| --------------------------- | ------------------ | -------------------------------------- |
| `"*"` or omitted            | `Matcher::All`     | Match every tool call                  |
| `"bash"` / `"shell"`        | `Matcher::Bash`    | Case-insensitive                       |
| `["Edit", "Write"]`         | `Matcher::AnyOf`   | Array form                             |
| `{ exact = "Edit" }`        | `Matcher::Exact`   | Table form                             |
| `{ regex = "Write|Edit" }`  | `Matcher::Regex`   | Passed through to the harness          |
| `"Write|Edit"`              | `Matcher::Regex`   | Regex metacharacters are auto-detected |
| `"Edit"`                    | `Matcher::Exact`   | Plain tool name                        |

## `[[instructions]]`

Maps to `InstructionSpec`. Instructions are persistent named markdown:
prompt rules for harnesses without a hook surface. Requires `name` and one
of `body` or `file`.

```toml
[[instructions]]
name = "code-style"
file = "STYLE.md"       # or body = "inline markdown"
placement = "auto"      # auto | inline | referenced | standalone
agents = ["roo"]        # optional
skip = []               # optional
```

`placement` picks the library's placement mode:

| Value         | Library value       | Behavior                                                       |
| ------------- | ------------------- | -------------------------------------------------------------- |
| `auto`        | registry-resolved   | First placement the target harness supports, from `registry::instruction_placements` |
| `inline`      | `InlineBlock`       | Tagged fence inside the harness's memory file                  |
| `referenced`  | `ReferencedFile`    | Standalone file plus a managed `@include` in the memory file   |
| `standalone`  | `StandaloneFile`    | File only, in the harness's rules dir                          |

## `[[mcp]]`

Maps to `McpSpec`. Requires `name` and a transport.

```toml
[[mcp]]
name = "context7"
command = ["npx", "-y", "@upstash/context7-mcp"]   # stdio (default): argv array
env = { API_KEY = "${CONTEXT7_API_KEY}" }          # stdio only
```

```toml
[[mcp]]
name = "remote"
type = "http"                    # or "sse"
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ${TOKEN}" }
```

- `type` defaults to `stdio` and accepts `http`, `sse`. Each transport
  rejects the other's fields.
- The library's secret policy applies: env or header values that look like
  literal secrets in project-local scope are refused. Placeholder values
  (`${VAR}`) pass through and resolve where the harness launches the server.
- Transport support varies a lot between tools (`http`/`sse` are narrow), so
  entries can carry `agents` filters when a server needs one specific tool.

## `[[skills]]`

Maps to `SkillSpec`. One entry per skill: a `SKILL.md` folder or an inline
definition.

```toml
[[skills]]
path = "skills/pdf-tools"        # folder with SKILL.md; contents become assets
```

```toml
[[skills]]
name = "inline-skill"
description = "Use when the inline skill applies."
body = "# Inline skill
Follow it."
```

- Folder form: `name` (default: the directory name) and `description` are
  required in the SKILL.md frontmatter, parsed as YAML. Recognized
  frontmatter fields flow into the library's `SkillFrontmatter`:
  `when_to_use`, `argument-hint`, `arguments`, `disable-model-invocation`,
  `user-invocable`, `allowed-tools`, `disallowed-tools`, `model`, `effort`
  (low/medium/high/xhigh/max), `context` (fork), `agent`, `paths`, `shell`
  (bash/powershell).
- Every regular file under the skill folder (except `SKILL.md` itself)
  becomes an asset, preserving relative paths. Executable bits are preserved
  on Unix. Symlinks are skipped with a warning. Files are capped at 8 MiB,
  matching the library's read cap.
- The library's ledger model applies: the skill installs into each target's
  own skills directory (`.claude/skills/...` for Claude, `.agents/skills/`
  for Codex, and so on) and records `.agent-config-skills.json` next to it.
  This is a different model from the `skillz` CLI's canonical-store-plus-symlinks
  layout, and the two do not track each other's installs.

## Sources and lockfiles

`agentconfig add <source>` accepts:

| Form                        | Example                          |
| --------------------------- | -------------------------------- |
| GitHub shorthand            | `owner/repo`, `owner/repo/subpath` |
| Pinned shorthand            | `owner/repo@v1.2.3`              |
| `github:` alias             | `github:owner/repo`              |
| Git URLs                    | `https://...`, `git@host:...`, any `.git` URL |
| Local directories           | `./pkg`, `/abs/path`, `~/pkg`    |

Git sources clone shallow into a tempdir. The lockfile records the effective
ref and subpath so `check` and `update` can fetch the same source again.
`remove` works from the lockfile and the installed files.

- Project lockfile: `agent.config.lock.json` in the project directory.
- Global lockfile: `agentconfig/lock.json` under the user config directory.

`check` exits 1 when package content changes or a previous install is
incomplete. `update` replays the saved filters. It keeps tracking older
components when an upstream package drops or renames them, so a later
`remove` can clean them up. If a harness refuses removal, the row stays in
the lockfile. Use `add --agent` to re-add an agent after a filtered removal.

## Fallback layouts

When the source has no `agent.config.toml`, `agentconfig add` imports skills
and supported MCP entries. It reads `plugin.json` for the package name,
`mcp.json` for servers, and `skills/<name>/SKILL.md` for skills. A Claude
plugin with `.claude-plugin/plugin.json` can also supply its default
`.mcp.json`. [Agent Plugins specification](https://agent-plugins.org/specification),
[Claude Code plugin manifest reference](https://code.claude.com/docs/en/plugins-reference)

The importer supports stdio, Streamable HTTP, and SSE where the target
harness supports the transport. Use a PATH executable or an absolute host
executable for stdio. Entries that need a plugin-relative executable,
`cwd`, or plugin runtime variables are skipped with a diagnostic. Valid
servers and skills in the same package can still install.

Use the native manifest for hooks and instructions, or pass
`--instruction-file <path>` for one Markdown instruction. Package file
references resolve within the source directory. Each file is capped at
8 MiB.

## Diagnostics

Errors print `[AGENTCONFIG_CODE] why` plus a `fix:` line, in the same shape
as the `skillz` CLI's diagnostics. Codes include `AGENTCONFIG_NO_MANIFEST`,
`AGENTCONFIG_INVALID_MANIFEST`, `AGENTCONFIG_INVALID_ENTRY`,
`AGENTCONFIG_UNKNOWN_AGENT`, `AGENTCONFIG_SOURCE_INVALID`,
`AGENTCONFIG_GIT_FAILED`, `AGENTCONFIG_NOT_IN_LOCKFILE`, and
`AGENTCONFIG_INIT_EXISTS`. Warnings (unknown agents in `agents`/`skip`
lists, unsupported scopes, skipped symlinks) print to stderr and do not fail
the run unless everything is skipped.
