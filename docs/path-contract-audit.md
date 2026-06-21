# Harness path contract audit

This audit records how each registered harness path contract was checked. The
base pass was 2026-05-29; a targeted re-verification pass on 2026-06-21 covered
lifecycle changes and the path-family rows flagged in external review (see
"2026-06-21 verification pass" below). The release-facing path list remains in
[`support-matrix.md`](support-matrix.md); this file explains the source status
behind those rows.

## Status meanings

- `StableDocumented`: upstream docs directly document the implemented path or
  directory and the relevant config shape.
- `Observed`: the crate implements and tests the path, but the fetched upstream
  sources did not expose every implemented path as a stable public contract.
- `Experimental`: the shape is intentionally best-effort and must be reviewed
  before widening support.

## Audit scope

Implementation paths were compared against `src/agents/*.rs`, `src/paths.rs`,
and the per-agent docs in `docs/agents/`. Upstream checks used public vendor
docs or official project repositories where available. If a source documented a
feature but not this crate's exact file location, the row stays `Observed`.

| Agent | Status | Checked upstream sources | Notes |
| ----- | ------ | ------------------------ | ----- |
| Claude Code | `StableDocumented` | [hooks](https://code.claude.com/docs/en/hooks), [settings](https://code.claude.com/docs/en/settings), [MCP](https://code.claude.com/docs/en/mcp), [memory](https://code.claude.com/docs/en/memory), [skills](https://code.claude.com/docs/en/skills) | Settings, hook, MCP, memory, and skill paths are all explicitly documented. |
| Cursor | `Observed` | [hooks](https://cursor.com/docs/hooks), [MCP](https://docs.cursor.com/en/context/model-context-protocol), [rules](https://docs.cursor.com/en/context), [skills](https://cursor.com/docs/skills) | MCP and rules paths are documented. Hook and skill pages were reachable but not fully text-extractable during this audit, so the row remains conservative. |
| Gemini CLI | `StableDocumented` (legacy) | [transition blog](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/), [cutoff notice](https://github.com/google-gemini/gemini-cli/discussions/28017), [hooks](https://geminicli.com/docs/hooks/), [GEMINI.md](https://geminicli.com/docs/cli/gemini-md/), [MCP](https://geminicli.com/docs/tools/mcp-server/), [skills](https://geminicli.com/docs/cli/using-agent-skills/) | `.gemini` settings, `GEMINI.md`, `mcpServers`, and `.gemini/skills` remain documented. **Legacy / conditional as of 2026-06-18**: Gemini CLI stopped serving free, AI Pro, and AI Ultra tiers; enterprise and paid API-key users remain supported. Consumer-tier work belongs on `antigravitycli`. |
| OpenClaw | `Observed` | [MCP](https://docs.openclaw.ai/cli/mcp), [skills](https://docs.openclaw.ai/tools/skills), [AGENTS template](https://docs.openclaw.ai/reference/templates/AGENTS) | Prompt and skill paths are documented. The fetched MCP doc did not expose the exact `~/.openclaw/openclaw.json` path. |
| Hermes Agent | `StableDocumented` | [configuration](https://hermes-agent.nousresearch.com/docs/user-guide/configuration/), [MCP reference](https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference/), [skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills/) | `.hermes.md`, `.hermes/config.yaml`, `mcp_servers`, and `.hermes/skills` are documented. |
| Codex CLI | `StableDocumented` | [hooks](https://developers.openai.com/codex/hooks), [AGENTS.md](https://developers.openai.com/codex/guides/agents-md), [config](https://developers.openai.com/codex/config-basic), [MCP](https://developers.openai.com/codex/mcp), [skills](https://developers.openai.com/codex/skills) | OpenAI docs document user and project config layers, hooks, `AGENTS.md`, `[mcp_servers.*]`, stdio and streamable HTTP MCP, and `.agents/skills`. |
| GitHub Copilot | `StableDocumented` | [hooks](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks), [hook reference](https://docs.github.com/en/copilot/reference/hooks-configuration), [MCP CLI](https://docs.github.com/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers), [CLI config directory](https://docs.github.com/en/enterprise-cloud@latest/copilot/reference/copilot-cli-reference/cli-config-dir-reference), [CLI command reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference) | Hook directory, user MCP config, project MCP config, and skill locations are documented. This crate writes one hook file per tag inside `.github/hooks/`. |
| OpenCode | `StableDocumented` | [plugins](https://opencode.ai/docs/plugins/), [rules](https://opencode.ai/docs/rules/), [config](https://opencode.ai/docs/config/), [MCP](https://opencode.ai/docs/mcp-servers/), [skills](https://opencode.ai/docs/skills) | Plugin directories, `AGENTS.md` rules, config locations, `mcp` object, and skill paths are documented. |
| Cline | `StableDocumented` | [CLI config](https://docs.cline.bot/cline-cli/configuration), [hooks](https://docs.cline.bot/customization/hooks), [rules](https://docs.cline.bot/customization/cline-rules), [skills](https://docs.cline.bot/customization/skills), [MCP](https://docs.cline.bot/mcp/adding-and-configuring-servers) | Standalone CLI config layout is now documented: global MCP at `~/.cline/data/settings/cline_mcp_settings.json`, distinct from the VS Code extension globalStorage path. This crate writes the CLI path (env override via `CLINE_DATA_DIR`/`CLINE_DIR`). |
| Roo Code | `Observed` (retired) | [sunset notice](https://docs.roocode.com/sunset), [custom instructions](https://docs.roocode.com/features/custom-instructions), [MCP](https://docs.roocode.com/features/mcp/using-mcp-in-roo) | **Retired / EOL 2026-05-15**: all Roo Code products shut down (announced 2026-04-21). `.roo/rules` and project `.roo/mcp.json` remain documented; entry kept for compatibility. No further path work planned. |
| Windsurf | `StableDocumented` | [MCP](https://docs.windsurf.com/windsurf/cascade/mcp), [hooks](https://docs.windsurf.com/windsurf/cascade/hooks), [skills](https://docs.windsurf.com/windsurf/cascade/skills), [memories/rules](https://docs.windsurf.com/windsurf/cascade/memories) | `mcp_config.json`, `.windsurf/hooks.json`, rules, and skill paths are documented. |
| Kilo Code | `StableDocumented` | [AGENTS.md](https://kilo.ai/docs/customize/agents-md), [custom rules](https://kilo.ai/docs/customize/custom-rules), [MCP](https://kilo.ai/docs/automate/mcp/using-in-kilo-code), [skills](https://kilo.ai/docs/customize/skills) | JSONC MCP, `mcpServers`, and skill paths are documented. Rules: current standard is `.kilo/rules/*.md` referenced from `kilo.jsonc`; legacy `.kilocode/rules` still works upstream. This crate now writes `.kilo/rules`. |
| Google Antigravity | `Observed` | [rules](https://antigravity.google/docs/rules-workflows), [rules markdown](https://antigravity.google/assets/docs/antigravity-2-0/rules-workflows.md), [skills](https://antigravity.google/docs/skills), [IDE skills markdown](https://antigravity.google/assets/docs/editor/ide-skills.md), [MCP](https://antigravity.google/docs/mcp), [IDE MCP markdown](https://antigravity.google/assets/docs/editor/ide-mcp.md), [hooks](https://antigravity.google/docs/hooks) | Workspace rules and skills default to `.agents/*` and retain backward support for `.agent/*`. Global skills are at `~/.gemini/antigravity/skills`. Global MCP is at `~/.gemini/config/mcp_config.json`. Local MCP is at `.agents/mcp_config.json` with fallback support. Hooks are supported at `~/.gemini/config/hooks.json` (Global) and `.agents/hooks.json` (Local). |
| Antigravity CLI | `Observed` | [CLI blog](https://antigravity.google/blog/introducing-google-antigravity-cli?app=antigravity), [migration](https://antigravity.google/docs/gcli-migration), [migration markdown](https://antigravity.google/assets/docs/cli/gcli-migration.md), [plugins and skills](https://antigravity.google/docs/cli-plugins), [plugins markdown](https://antigravity.google/assets/docs/cli/cli-plugins.md), [settings markdown](https://antigravity.google/assets/docs/cli/cli-settings.md), [hooks](https://antigravity.google/docs/hooks), [skills](https://antigravity.google/docs/skills) | CLI rules are at `AGENTS.md` / `~/.gemini/GEMINI.md`. Local MCP is at `.agents/mcp_config.json`, global MCP at `~/.gemini/antigravity-cli/mcp_config.json`. Global hooks are at `~/.gemini/antigravity-cli/hooks.json`, local hooks at `.agents/hooks.json`. Skills are at `~/.gemini/antigravity-cli/skills` (Global) and `.agents/skills` (Local). |
| Amp | `StableDocumented` | [manual](https://ampcode.com/manual), [repository](https://github.com/sourcegraph/amp) | User settings at `~/.config/amp/settings.json`, workspace `.amp/settings.json`, global rules `~/.config/AGENTS.md`, native skills `.agents/skills/` (project) and `~/.config/amp/skills/` (global) are documented. This crate now writes the `~/.config/amp/` family instead of the previous `~/.amp/*`, which Amp does not read. |
| CodeBuddy CLI | `Observed` | [CLI](https://www.codebuddy.ai/docs/cli/), [settings](https://www.codebuddy.ai/docs/cli/settings), [hooks](https://www.codebuddy.ai/docs/cli/hooks) | Settings and hooks are documented. The fetched sources did not expose `CLAUDE.md` or skill paths. |
| Charm Crush | `StableDocumented` | [repository](https://github.com/charmbracelet/crush), [hooks doc](https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md), [schema](https://charm.land/crush.json) | Global/local hooks, prompt rules in AGENTS.md, vscode-style MCP (type: stdio under mcp), skills, and instructions are supported. Hooks currently fire only PreToolUse upstream. |
| Forge | `Observed` | [docs](https://forgecode.dev/docs), [repository](https://github.com/forge-agents/forge) | `AGENTS.md` and `.mcp.json` are documented. The fetched docs did not expose every global or local skill path this crate writes. |
| iFlow CLI | `StableDocumented` | [settings](https://platform.iflow.cn/en/cli/configuration/settings), [hooks](https://platform.iflow.cn/en/cli/examples/hooks) | `.iflow/settings.json`, hooks, and `mcpServers` are documented. |
| JetBrains Junie | `StableDocumented` | [guidelines and memory](https://junie.jetbrains.com/docs/guidelines-and-memory.html), [Junie CLI MCP](https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html), [JetBrains MCP settings](https://www.jetbrains.com/help/junie/mcp-settings.html) | `.junie/mcp/mcp.json`, `~/.junie/mcp/mcp.json`, and `mcpServers` are documented. Re-verified 2026-06-21: `.junie/AGENTS.md` is the current default guideline file (with fallback to root `AGENTS.md`); `.junie/guidelines.md` is the legacy format. This crate's `.junie/AGENTS.md` path is correct. |
| Pi | `Observed` | [repository](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent), [adapter](https://github.com/nicobailon/pi-mcp-adapter) | Prompt and skill paths are documented. MCP paths use the pi-mcp-adapter extension conventions. |
| Qoder CLI | `Observed` | [CLI docs](https://docs.qoder.com/en/cli/using-cli) | `AGENTS.md` memory layout (`~/.qoder/AGENTS.md`, `${project}/AGENTS.md`) is documented. Open re-audit item (2026-06-21): current docs expose neither `~/.qoder.json` nor a clear global MCP file path; MCP is added via `qodercli mcp add` with an undocumented file location. MCP shape stays repository-observed. |
| Qwen Code | `StableDocumented` | [MCP](https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/), [skills](https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/), [settings source](https://github.com/QwenLM/qwen-code/blob/main/docs/users/configuration/settings.md) | `.qwen/settings.json`, `~/.qwen/settings.json`, `mcpServers`, and `.qwen/skills` are documented. |
| Tabnine CLI | `StableDocumented` | [CLI](https://docs.tabnine.com/main/getting-started/tabnine-cli/), [settings reference](https://docs.tabnine.com/main/getting-started/tabnine-cli/features/settings/settings-reference) | `.tabnine/agent/settings.json`, hooks, and `mcpServers` are documented. |
| Trae | `Observed` | [rules](https://docs.trae.ai/ide/rules), [skills](https://docs.trae.ai/ide/skills), [trae-agent repository](https://github.com/bytedance/trae-agent) | The fetched official docs did not expose `.trae/project_rules.md` or `.trae/skills`. Public repository and ecosystem references indicate those paths, so the row remains `Observed`. |

## 2026-06-21 verification pass

Targeted re-verification triggered by an external review of the support matrix.
Each flagged claim was checked against current upstream docs:

- **Roo Code** — confirmed retired. All products shut down 2026-05-15
  ([sunset](https://docs.roocode.com/sunset)). Annotated EOL; code kept.
- **Gemini CLI** — confirmed consumer-tier cutoff 2026-06-18; enterprise and
  paid API-key continue ([cutoff notice](https://github.com/google-gemini/gemini-cli/discussions/28017)).
  Annotated legacy/conditional; code kept.
- **Amp** — confirmed wrong. Moved global paths from `~/.amp/*` to the documented
  `~/.config/amp/` family + `~/.config/AGENTS.md`, and local skills to
  `<root>/.agents/skills` ([manual](https://ampcode.com/manual)).
- **Cline** — confirmed. Global MCP is the standalone-CLI path
  `~/.cline/data/settings/cline_mcp_settings.json`
  ([CLI config](https://docs.cline.bot/cline-cli/configuration)); the code was
  already migrated to it (the matrix row was stale and is now corrected).
- **Kilo Code** — confirmed `.kilocode/rules` is legacy; current standard is
  `.kilo/rules` ([custom rules](https://kilo.ai/docs/customize/custom-rules)).
  Moved the rules write path.
- **Junie** — review claim disproven. Default guideline file is `.junie/AGENTS.md`,
  not `.junie/guidelines.md` (legacy)
  ([guidelines and memory](https://junie.jetbrains.com/docs/guidelines-and-memory.html)).
  No change.
- **Qoder** — unconfirmed. Official docs expose neither `~/.qoder.json` nor a clear
  global MCP file. Left as-is; logged as an open re-audit item.
- **Copilot / Crush / Pi** — re-checked; already correct in this crate
  (`.github/hooks/`, `$CRUSH_GLOBAL_CONFIG` precedence, `pi-mcp-adapter`). No change.

## Follow-up policy

Promote an `Observed` row only after the per-agent source exposes the exact
path and config shape, or after a linked upstream repository file is added as
the explicit contract source. Demote any `StableDocumented` row if a future
review finds that the upstream docs removed or changed the path contract.
