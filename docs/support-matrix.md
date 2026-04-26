# Support matrix

This is the release-facing support matrix for `agent-config` v0.1. It lists
the files and directories this crate writes, not every path a harness may read.
Per-agent detail remains in [`docs/agents/`](agents/README.md).

Support levels:

- `StableDocumented` means the path and config shape are described in public
  upstream documentation, with the source recorded in the per-agent doc.
- `Observed` means the path is implemented, tested, and documented in this
  repository, but the per-agent doc does not yet carry a source citation that
  should be treated as an upstream contract.
- `Experimental` means the shape is intentionally best-effort and should not be
  extended without fresh upstream review.

Current policy: registered integrations stay at `Observed` until their
per-agent docs record upstream source links and a verification date. This keeps
the support promise honest: tests prove our behavior, but they do not prove that
a fast-moving harness will keep the same file contract.

Last repository contract review: 2026-04-26.

| Agent | ID | Hooks and prompt writes | MCP writes | Skill writes | Support | Source review | Notes |
| ----- | -- | ----------------------- | ---------- | ------------ | ------- | ------------- | ----- |
| Claude Code | `claude` | Hooks: `~/.claude/settings.json`, `<root>/.claude/settings.json`<br>Prompt: `~/.claude/CLAUDE.md`, `<root>/CLAUDE.md` | `~/.claude.json`, `<root>/.mcp.json` | `~/.claude/skills/<name>/`, `<root>/.claude/skills/<name>/` | Observed | 2026-04-26 repo review | User MCP writes use Claude's home-level project map. |
| Cursor | `cursor` | Hooks: `~/.cursor/hooks.json`, `<root>/.cursor/hooks.json` | `~/.cursor/mcp.json`, `<root>/.cursor/mcp.json` | `~/.cursor/skills/<name>/`, `<root>/.cursor/skills/<name>/` | Observed | 2026-04-26 repo review | No prompt-rules surface is registered. |
| Gemini CLI | `gemini` | Hooks: `~/.gemini/settings.json`, `<root>/.gemini/settings.json`<br>Prompt: `~/.gemini/GEMINI.md`, `<root>/GEMINI.md` | `~/.gemini/settings.json`, `<root>/.gemini/settings.json` | `~/.gemini/skills/<name>/`, `<root>/.gemini/skills/<name>/` | Observed | 2026-04-26 repo review | Hooks and MCP share `settings.json`. |
| OpenClaw | `openclaw` | Prompt: `<root>/AGENTS.md` | `~/.openclaw/openclaw.json` | `~/.openclaw/skills/<name>/`, `<root>/.agents/skills/<name>/` | Observed | 2026-04-26 repo review | Native hook/plugin install remains deferred. |
| Hermes Agent | `hermes` | Prompt: `<root>/.hermes.md` | `~/.hermes/config.yaml` | `~/.hermes/skills/agent-config/<name>/` | Observed | 2026-04-26 repo review | MCP and skills are global-only. |
| Codex CLI | `codex` | Hooks: `$CODEX_HOME/hooks.json`, `<root>/.codex/hooks.json`<br>Prompt: `$CODEX_HOME/AGENTS.md`, `<root>/AGENTS.md` | `$CODEX_HOME/config.toml`, `<root>/.codex/config.toml` | `~/.agents/skills/<name>/`, `<root>/.agents/skills/<name>/` | Observed | 2026-04-26 repo review | `$CODEX_HOME` defaults to `~/.codex`. |
| GitHub Copilot | `copilot` | Hooks: `<root>/.github/hooks/<tag>-rewrite.json`<br>Prompt: `<root>/.github/copilot-instructions.md` | `~/.copilot/mcp-config.json`, `<root>/.mcp.json` | `~/.copilot/skills/<name>/`, `<root>/.github/skills/<name>/` | Observed | 2026-04-26 repo review | Hook and prompt writes are project-local only. |
| OpenCode | `opencode` | Hooks: `~/.config/opencode/plugins/<tag>.ts`, `<root>/.opencode/plugins/<tag>.ts` | `~/.config/opencode/opencode.json`, `<root>/opencode.json` | `~/.config/opencode/skills/<name>/`, `<root>/.opencode/skills/<name>/` | Observed | 2026-04-26 repo review | MCP accepts JSONC input and rewrites strict JSON. |
| Cline | `cline` | Hooks: `<root>/.clinerules/hooks/<event>`<br>Prompt: `<root>/.clinerules/<tag>.md` | `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json` | `~/.cline/skills/<name>/`, `<root>/.cline/skills/<name>/` | Observed | 2026-04-26 repo review | Hook and prompt writes are project-local; MCP is global-only. |
| Roo Code | `roo` | Prompt: `<root>/.roo/rules/<tag>.md` | `~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`, `<root>/.roo/mcp.json` | - | Observed | 2026-04-26 repo review | No skill surface is registered. |
| Windsurf | `windsurf` | Hooks: `<root>/.windsurf/hooks.json`<br>Prompt: `<root>/.windsurf/rules/<tag>.md` | `~/.codeium/windsurf/mcp_config.json`, `<root>/.windsurf/mcp_config.json` | `~/.codeium/windsurf/skills/<name>/`, `<root>/.windsurf/skills/<name>/` | Observed | 2026-04-26 repo review | Hook and prompt writes are project-local. |
| Kilo Code | `kilocode` | Prompt: `<root>/.kilocode/rules/<tag>.md` | `~/.config/kilo/kilo.jsonc`, `<root>/kilo.jsonc` or existing `<root>/.kilo/kilo.jsonc` | `~/.kilo/skills/<name>/`, `<root>/.kilo/skills/<name>/` | Observed | 2026-04-26 repo review | MCP accepts JSONC. |
| Google Antigravity | `antigravity` | Prompt: `<root>/.agent/rules/<tag>.md` | `~/.gemini/antigravity/mcp_config.json`, `<root>/.agent/mcp_config.json` | `~/.gemini/antigravity/skills/<name>/`, `<root>/.agent/skills/<name>/` | Observed | 2026-04-26 repo review | Prompt writes are project-local. |
| Amp | `amp` | Prompt: `~/.amp/AGENTS.md`, `<root>/AGENTS.md` | `~/.amp/settings.json`, `<root>/.amp/settings.json` | `~/.amp/skills/<name>/`, `<root>/.amp/skills/<name>/` | Observed | 2026-04-26 repo review | No hook surface is registered. |
| CodeBuddy CLI | `codebuddy` | Hooks: `~/.codebuddy/settings.json`, `<root>/.codebuddy/settings.json`<br>Prompt: `~/.codebuddy/CLAUDE.md`, `<root>/CLAUDE.md` | - | `~/.codebuddy/skills/<name>/`, `<root>/.codebuddy/skills/<name>/` | Observed | 2026-04-26 repo review | No MCP surface is registered. |
| Forge | `forge` | Prompt: `~/.forge/AGENTS.md`, `<root>/AGENTS.md` | `~/.forge/.mcp.json`, `<root>/.mcp.json` | `~/.forge/skills/<name>/`, `<root>/.forge/skills/<name>/` | Observed | 2026-04-26 repo review | No hook surface is registered. |
| iFlow CLI | `iflow` | Hooks: `~/.iflow/settings.json`, `<root>/.iflow/settings.json` | `~/.iflow/settings.json`, `<root>/.iflow/settings.json` | - | Observed | 2026-04-26 repo review | Hooks and MCP share `settings.json`. |
| JetBrains Junie | `junie` | Prompt: `<root>/.junie/AGENTS.md` | `~/.junie/mcp/mcp.json`, `<root>/.junie/mcp/mcp.json` | - | Observed | 2026-04-26 repo review | Prompt writes are project-local. |
| Qoder CLI | `qodercli` | Prompt: `~/.qoder/AGENTS.md`, `<root>/AGENTS.md` | `~/.qoder.json`, `<root>/.mcp.json` | - | Observed | 2026-04-26 repo review | No skill surface is registered. |
| Qwen Code | `qwen` | Prompt: `~/.qwen/QWEN.md`, `<root>/QWEN.md` | `~/.qwen/settings.json`, `<root>/.qwen/settings.json` | `~/.qwen/skills/<name>/`, `<root>/.qwen/skills/<name>/` | Observed | 2026-04-26 repo review | No hook surface is registered. |
| Tabnine CLI | `tabnine` | Hooks: `~/.tabnine/agent/settings.json`, `<root>/.tabnine/agent/settings.json` | `~/.tabnine/agent/settings.json`, `<root>/.tabnine/agent/settings.json` | - | Observed | 2026-04-26 repo review | Hooks and MCP share `settings.json`. |
| Trae | `trae` | Prompt: `<root>/.trae/project_rules.md` | - | `~/.trae/skills/<name>/`, `<root>/.trae/skills/<name>/` | Observed | 2026-04-26 repo review | Prompt writes are project-local. |

Ownership sidecars:

- MCP ledgers live next to the target config as `.agent-config-mcp.json`.
- Skill ledgers live at each skills root as `.agent-config-skills.json`.
- Directory hook ledgers, where needed, live next to hooks as
  `.agent-config-hooks.json`.

Before promoting any row to `StableDocumented`, update the matching
`docs/agents/<id>.md` with the upstream source URL, the checked date, and any
version caveats.
