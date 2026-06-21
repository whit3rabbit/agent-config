# Support matrix

This is the release-facing support matrix for `agent-config` v0.4. It lists
the files and directories this crate writes, not every path a harness may read.
Per-agent detail remains in [`docs/agents/`](agents/README.md). Source review
details live in [`path-contract-audit.md`](path-contract-audit.md).

Support levels:

- `StableDocumented` means the path and config shape are described in public
  upstream documentation, with the source recorded in the path-contract audit.
- `Observed` means the path is implemented, tested, and documented in this
  repository, but the upstream source does not yet expose every implemented
  path and shape as a stable contract.
- `Experimental` means the shape is intentionally best-effort and should not be
  extended without fresh upstream review.

Current policy: registered integrations stay at `Observed` until
[`path-contract-audit.md`](path-contract-audit.md) records exact upstream source
links and a verification date. This keeps the support promise honest: tests
prove our behavior, but they do not prove that a fast-moving harness will keep
the same file contract.

Last repository contract review: 2026-06-21.
Last upstream path-contract audit: 2026-06-21.

| Agent | ID | Hooks and prompt writes | MCP writes | Skill writes | Instruction writes | Support | Source review | Notes |
| ----- | -- | ----------------------- | ---------- | ------------ | ------------------ | ------- | ------------- | ----- |
| Claude Code | `claude` | Hooks: `~/.claude/settings.json`, `<root>/.claude/settings.json`<br>Prompt: `~/.claude/CLAUDE.md`, `<root>/CLAUDE.md` | `~/.claude.json`, `<root>/.mcp.json` | `~/.claude/skills/<name>/`, `<root>/.claude/skills/<name>/` | `~/.claude/<name>.md` + `@<name>.md` in `~/.claude/CLAUDE.md`; `<root>/.claude/instructions/<name>.md` + `@.claude/instructions/<name>.md` in `<root>/CLAUDE.md` (ReferencedFile) | StableDocumented | 2026-04-26 upstream audit | User MCP writes use Claude's home-level project map. |
| Cursor | `cursor` | Hooks: `~/.cursor/hooks.json`, `<root>/.cursor/hooks.json` | `~/.cursor/mcp.json`, `<root>/.cursor/mcp.json` | `~/.cursor/skills/<name>/`, `<root>/.cursor/skills/<name>/` | - | Observed | 2026-04-26 upstream audit | MCP and rules are documented; hook/skill pages were not fully text-extractable in audit. |
| Gemini CLI | `gemini` | Hooks: `~/.gemini/settings.json`, `<root>/.gemini/settings.json`<br>Prompt: `~/.gemini/GEMINI.md`, `<root>/GEMINI.md` | `~/.gemini/settings.json`, `<root>/.gemini/settings.json` | `~/.gemini/skills/<name>/`, `<root>/.gemini/skills/<name>/` | `~/.gemini/GEMINI.md`, `<root>/GEMINI.md` (InlineBlock) | StableDocumented (legacy) | 2026-06-21 upstream audit | **Legacy / conditional.** Gemini CLI stopped serving requests for free, AI Pro, and AI Ultra tiers on 2026-06-18; enterprise and paid API-key users remain supported. Paths kept for those tiers; new consumer-tier work belongs on `antigravitycli`. |
| OpenClaw | `openclaw` | Prompt: `<root>/AGENTS.md` | `~/.openclaw/openclaw.json` | `~/.openclaw/skills/<name>/`, `<root>/.agents/skills/<name>/` | `<root>/AGENTS.md` (InlineBlock); ledger at `<root>/.agents/.agent-config-instructions.json` | Observed | 2026-04-26 upstream audit | Native hook/plugin install remains deferred; exact MCP file path was not exposed by fetched docs. |
| Hermes Agent | `hermes` | Prompt: `<root>/.hermes.md` | `~/.hermes/config.yaml` | `~/.hermes/skills/agent-config/<name>/` | `<root>/.hermes.md` (InlineBlock); ledger at `<root>/.hermes/.agent-config-instructions.json` | StableDocumented | 2026-04-26 upstream audit | MCP and skills are global-only. |
| Codex CLI | `codex` | Hooks: `$CODEX_HOME/hooks.json`, `<root>/.codex/hooks.json`<br>Prompt: `$CODEX_HOME/AGENTS.md`, `<root>/AGENTS.md` | `$CODEX_HOME/config.toml`, `<root>/.codex/config.toml` | `~/.agents/skills/<name>/`, `<root>/.agents/skills/<name>/` | `$CODEX_HOME/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock; Codex has no `@import`) | StableDocumented | 2026-05-29 upstream audit | `$CODEX_HOME` defaults to `~/.codex`. MCP supports stdio and streamable HTTP; SSE is refused. |
| GitHub Copilot | `copilot` | Hooks: `<root>/.github/hooks/<tag>-rewrite.json`<br>Prompt: `<root>/.github/copilot-instructions.md` | `~/.copilot/mcp-config.json`, `<root>/.mcp.json` | `~/.copilot/skills/<name>/`, `<root>/.github/skills/<name>/` | `<root>/.github/copilot-instructions.md` (InlineBlock; Local-only) | StableDocumented | 2026-04-26 upstream audit | Hook and prompt writes are project-local only. |
| OpenCode | `opencode` | Hooks: `~/.config/opencode/plugins/<tag>.ts`, `<root>/.opencode/plugins/<tag>.ts`<br>Prompt: `~/.config/opencode/AGENTS.md`, `<root>/AGENTS.md` | `~/.config/opencode/opencode.json`, `<root>/opencode.json` | `~/.config/opencode/skills/<name>/`, `<root>/.opencode/skills/<name>/` | `~/.config/opencode/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock) | StableDocumented | 2026-05-29 upstream audit | MCP accepts JSONC input and rewrites strict JSON. |
| Cline | `cline` | Hooks: `<root>/.clinerules/hooks/<event>`<br>Prompt: `<root>/.clinerules/<tag>.md` | `~/.cline/data/settings/cline_mcp_settings.json` (env override via `CLINE_DATA_DIR` / `CLINE_DIR`) | `~/.cline/skills/<name>/`, `<root>/.cline/skills/<name>/` | `<root>/.clinerules/<name>.md` (StandaloneFile) | StableDocumented | 2026-06-21 upstream audit | Targets the standalone Cline CLI (`~/.cline/data/settings/`), now upstream-documented, not the VS Code extension globalStorage path. Hook and prompt writes are project-local. |
| Roo Code | `roo` | Prompt: `<root>/.roo/rules/<tag>.md` | `~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`, `<root>/.roo/mcp.json` | - | `<root>/.roo/rules/<name>.md` (StandaloneFile) | Observed (retired) | 2026-06-21 upstream audit | **Retired / EOL.** All Roo Code products (extension, Cloud, Router) shut down 2026-05-15. Entry kept for compatibility and migration; no further path work planned. |
| Windsurf | `windsurf` | Hooks: `<root>/.windsurf/hooks.json`<br>Prompt: `<root>/.windsurf/rules/<tag>.md` | `~/.codeium/windsurf/mcp_config.json`, `<root>/.windsurf/mcp_config.json` | `~/.codeium/windsurf/skills/<name>/`, `<root>/.windsurf/skills/<name>/` | `<root>/.windsurf/rules/<name>.md` (StandaloneFile) | StableDocumented | 2026-04-26 upstream audit | Hook and prompt writes are project-local. |
| Kilo Code | `kilocode` | Prompt: `<root>/.kilo/rules/<tag>.md` | `~/.config/kilo/kilo.jsonc`, `<root>/kilo.jsonc` or existing `<root>/.kilo/kilo.jsonc` | `~/.kilo/skills/<name>/`, `<root>/.kilo/skills/<name>/` | `<root>/.kilo/rules/<name>.md` (StandaloneFile) | StableDocumented | 2026-06-21 upstream audit | MCP accepts JSONC. Rules now write the current `.kilo/rules` layout; legacy `.kilocode/rules` still works upstream but is no longer written. |
| Google Antigravity | `antigravity` | Hooks: `~/.gemini/config/hooks.json` (Global), `<root>/.agents/hooks.json` (Local)<br>Prompt: `<root>/.agents/rules/<tag>.md`<br>Legacy status/uninstall: `<root>/.agent/rules/<tag>.md` | `~/.gemini/config/mcp_config.json`, `<root>/.agents/mcp_config.json` (with fallback to legacy `<root>/.agent/mcp_config.json`) | `~/.gemini/antigravity/skills/<name>/`, `<root>/.agents/skills/<name>/`<br>Legacy status/uninstall: `<root>/.agent/skills/<name>/` | `<root>/.agents/rules/<name>.md` (StandaloneFile)<br>Legacy status/uninstall: `<root>/.agent/rules/<name>.md` | Observed | 2026-05-30 upstream audit | `.agents` rules, skills, hooks, and MCP updated for Antigravity 2.0. Global MCP uses `~/.gemini/config/mcp_config.json`. |
| Antigravity CLI | `antigravitycli` | Hooks: `~/.gemini/antigravity-cli/hooks.json` (Global), `<root>/.agents/hooks.json` (Local)<br>Prompt: `~/.gemini/GEMINI.md`, `<root>/AGENTS.md` | `~/.gemini/antigravity-cli/mcp_config.json`, `<root>/.agents/mcp_config.json` | `~/.gemini/antigravity-cli/skills/<name>/`, `<root>/.agents/skills/<name>/` | `~/.gemini/GEMINI.md`, `<root>/AGENTS.md` (InlineBlock) | Observed | 2026-05-30 upstream audit | Added full support for global and local hooks and skills in addition to prompt rules and MCP. |
| Amp | `amp` | Prompt: `~/.config/AGENTS.md`, `<root>/AGENTS.md` | `~/.config/amp/settings.json`, `<root>/.amp/settings.json` | `~/.config/amp/skills/<name>/`, `<root>/.agents/skills/<name>/` | `~/.config/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock); local ledger at `<root>/.amp/.agent-config-instructions.json` | StableDocumented | 2026-06-21 upstream audit | No hook surface is registered. Global paths use Amp's documented `~/.config/amp/` (settings, skills) and `~/.config/AGENTS.md` (global rules); native project skills use `<root>/.agents/skills`. |
| CodeBuddy CLI | `codebuddy` | Hooks: `~/.codebuddy/settings.json`, `<root>/.codebuddy/settings.json`<br>Prompt: `~/.codebuddy/CLAUDE.md`, `<root>/CLAUDE.md` | - | `~/.codebuddy/skills/<name>/`, `<root>/.codebuddy/skills/<name>/` | `~/.codebuddy/CLAUDE.md`, `<root>/CLAUDE.md` (InlineBlock) | Observed | 2026-04-26 upstream audit | Settings and hooks are documented; prompt and skill paths need stronger upstream source. |
| Charm Crush | `crush` | Hooks: `<crush_home>/crush.json`, `<root>/crush.json`<br>Prompt: `<crush_home>/AGENTS.md`, `<root>/AGENTS.md` | `<crush_home>/crush.json`, `<root>/crush.json` (under `mcp.<name>`, JSONC) | `<crush_home>/skills/<name>/`, `<root>/.crush/skills/<name>/` | `<crush_home>/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock) | StableDocumented | 2026-05-30 upstream audit | `crush_home` honors `$CRUSH_GLOBAL_CONFIG`, else `$XDG_CONFIG_HOME/crush`. Hooks currently fire only `PreToolUse` upstream. |
| Forge | `forge` | Prompt: `~/.forge/AGENTS.md`, `<root>/AGENTS.md` | `~/.forge/.mcp.json`, `<root>/.mcp.json` | `~/.forge/skills/<name>/`, `<root>/.forge/skills/<name>/` | `~/.forge/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock) | Observed | 2026-04-26 upstream audit | No hook surface is registered; fetched docs did not expose every skill path. |
| iFlow CLI | `iflow` | Hooks: `~/.iflow/settings.json`, `<root>/.iflow/settings.json` | `~/.iflow/settings.json`, `<root>/.iflow/settings.json` | - | - | StableDocumented | 2026-04-26 upstream audit | Hooks and MCP share `settings.json`. |
| JetBrains Junie | `junie` | Prompt: `<root>/.junie/AGENTS.md` | `~/.junie/mcp/mcp.json`, `<root>/.junie/mcp/mcp.json` | - | `<root>/.junie/AGENTS.md` (InlineBlock; Local-only) | StableDocumented | 2026-06-21 upstream audit | Prompt writes are project-local. Re-verified: `.junie/AGENTS.md` is the current default guideline file (`.junie/guidelines.md` is the legacy format). |
| Pi | `pi` | Prompt: `~/.pi/agent/AGENTS.md`, `<root>/AGENTS.md` | `~/.pi/agent/mcp.json`, `<root>/.pi/mcp.json` | `~/.pi/agent/skills/<name>/`, `<root>/.pi/skills/<name>/` | `~/.pi/agent/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock) | Observed | 2026-04-28 upstream audit | Pi has no config-file hook surface; HookSpec installs require a `rules` body. MCP file shape matches `pi-mcp-adapter`. |
| Qoder CLI | `qodercli` | Prompt: `~/.qoder/AGENTS.md`, `<root>/AGENTS.md` | `~/.qoder.json`, `<root>/.mcp.json` | - | `~/.qoder/AGENTS.md`, `<root>/AGENTS.md` (InlineBlock) | Observed | 2026-06-21 upstream audit | No skill surface is registered. Open re-audit item: official docs expose neither `~/.qoder.json` nor a clear global MCP file; AGENTS.md memory layout is confirmed. |
| Qwen Code | `qwen` | Prompt: `~/.qwen/QWEN.md`, `<root>/QWEN.md` | `~/.qwen/settings.json`, `<root>/.qwen/settings.json` | `~/.qwen/skills/<name>/`, `<root>/.qwen/skills/<name>/` | `~/.qwen/QWEN.md`, `<root>/QWEN.md` (InlineBlock) | StableDocumented | 2026-04-26 upstream audit | No hook surface is registered. |
| Tabnine CLI | `tabnine` | Hooks: `~/.tabnine/agent/settings.json`, `<root>/.tabnine/agent/settings.json` | `~/.tabnine/agent/settings.json`, `<root>/.tabnine/agent/settings.json` | - | - | StableDocumented | 2026-04-26 upstream audit | Hooks and MCP share `settings.json`. |
| Trae | `trae` | Prompt: `<root>/.trae/project_rules.md` | - | `~/.trae/skills/<name>/`, `<root>/.trae/skills/<name>/` | `<root>/.trae/project_rules.md` (InlineBlock; Local-only) | Observed | 2026-04-26 upstream audit | Prompt writes are project-local; fetched official docs did not expose exact file paths. |

Ownership sidecars:

- MCP ledgers live next to the target config as `.agent-config-mcp.json`.
- Skill ledgers live at each skills root as `.agent-config-skills.json`.
- Instruction ledgers live at the agent's instruction config dir as
  `.agent-config-instructions.json` (per-agent path documented in each
  per-agent doc).
- Directory hook ledgers, where needed, live next to hooks as
  `.agent-config-hooks.json`.

Before promoting any row to `StableDocumented`, update
[`path-contract-audit.md`](path-contract-audit.md) with the upstream source URL,
the checked date, and any version caveats.
