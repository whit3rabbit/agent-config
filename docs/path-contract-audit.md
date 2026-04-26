# Harness path contract audit

This audit records how each registered harness path contract was checked on
2026-04-26. The release-facing path list remains in
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
| Gemini CLI | `StableDocumented` | [hooks](https://geminicli.com/docs/hooks/), [GEMINI.md](https://geminicli.com/docs/cli/gemini-md/), [MCP](https://geminicli.com/docs/tools/mcp-server/), [skills](https://geminicli.com/docs/cli/skills/) | `.gemini` settings, `GEMINI.md`, `mcpServers`, and `.gemini/skills` are documented. Gemini also documents `.agents/skills` aliases that this crate does not currently write. |
| OpenClaw | `Observed` | [MCP](https://docs.openclaw.ai/cli/mcp), [skills](https://docs.openclaw.ai/tools/skills), [AGENTS template](https://docs.openclaw.ai/reference/templates/AGENTS) | Prompt and skill paths are documented. The fetched MCP doc did not expose the exact `~/.openclaw/openclaw.json` path. |
| Hermes Agent | `StableDocumented` | [configuration](https://hermes-agent.nousresearch.com/docs/user-guide/configuration/), [MCP reference](https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference/), [skills](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills/) | `.hermes.md`, `.hermes/config.yaml`, `mcp_servers`, and `.hermes/skills` are documented. |
| Codex CLI | `StableDocumented` | [hooks](https://developers.openai.com/codex/hooks), [AGENTS.md](https://developers.openai.com/codex/guides/agents-md), [config](https://developers.openai.com/codex/config-basic), [MCP](https://developers.openai.com/codex/mcp), [skills](https://developers.openai.com/codex/skills) | OpenAI docs document user and project config layers, hooks, `AGENTS.md`, `[mcp_servers.*]`, and `.agents/skills`. |
| GitHub Copilot | `StableDocumented` | [hooks](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-hooks), [hook reference](https://docs.github.com/en/copilot/reference/hooks-configuration), [MCP CLI](https://docs.github.com/copilot/how-tos/copilot-cli/customize-copilot/add-mcp-servers), [CLI config directory](https://docs.github.com/en/enterprise-cloud@latest/copilot/reference/copilot-cli-reference/cli-config-dir-reference), [CLI command reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference) | Hook directory, user MCP config, project MCP config, and skill locations are documented. This crate writes one hook file per tag inside `.github/hooks/`. |
| OpenCode | `StableDocumented` | [plugins](https://opencode.ai/docs/plugins/), [config](https://opencode.ai/docs/config/), [MCP](https://opencode.ai/docs/mcp-servers/), [skills](https://opencode.ai/docs/skills) | Plugin directories, config locations, `mcp` object, and skill paths are documented. |
| Cline | `Observed` | [hooks](https://docs.cline.bot/customization/hooks), [rules](https://docs.cline.bot/customization/cline-rules), [skills](https://docs.cline.bot/customization/skills), [MCP](https://docs.cline.bot/mcp/adding-and-configuring-servers) | Hooks, rules, skills, and `mcpServers` shape are documented. The exact VS Code globalStorage MCP path is an adapter path rather than a public upstream path. |
| Roo Code | `Observed` | [custom instructions](https://docs.roocode.com/features/custom-instructions), [MCP](https://docs.roocode.com/features/mcp/using-mcp-in-roo) | `.roo/rules` and project `.roo/mcp.json` are documented. The exact VS Code globalStorage MCP path is an adapter path rather than a public upstream path. |
| Windsurf | `StableDocumented` | [MCP](https://docs.windsurf.com/windsurf/cascade/mcp), [hooks](https://docs.windsurf.com/windsurf/cascade/hooks), [skills](https://docs.windsurf.com/windsurf/cascade/skills), [memories/rules](https://docs.windsurf.com/windsurf/cascade/memories) | `mcp_config.json`, `.windsurf/hooks.json`, rules, and skill paths are documented. |
| Kilo Code | `StableDocumented` | [AGENTS.md](https://kilo.ai/docs/agent-behavior/agents-md), [custom rules](https://kilo.ai/docs/customize/custom-rules), [MCP](https://kilo.ai/docs/automate/mcp/using-in-kilo-code), [skills](https://kilo.ai/docs/customize/skills) | Rules, JSONC MCP files, `mcpServers`, and skill paths are documented. |
| Google Antigravity | `Observed` | [rules](https://antigravity.codes/blog/user-rules), [skills](https://antigravity.google/docs/skills), [MCP note](https://www.devopness.com/docsmcp/antigravity/) | `.agent/rules` is documented. The fetched skill and MCP sources did not expose every implemented path, and one MCP source is third-party. |
| Amp | `Observed` | [manual](https://ampcode.com/manual), [repository](https://github.com/sourcegraph/amp) | Project `.amp/settings.json`, project skills, `AGENTS.md`, and `mcpServers` are documented. The fetched manual did not expose every global path this crate writes. |
| CodeBuddy CLI | `Observed` | [CLI](https://www.codebuddy.ai/docs/cli/), [settings](https://www.codebuddy.ai/docs/cli/settings), [hooks](https://www.codebuddy.ai/docs/cli/hooks) | Settings and hooks are documented. The fetched sources did not expose `CLAUDE.md` or skill paths. |
| Forge | `Observed` | [docs](https://forgecode.dev/docs), [repository](https://github.com/forge-agents/forge) | `AGENTS.md` and `.mcp.json` are documented. The fetched docs did not expose every global or local skill path this crate writes. |
| iFlow CLI | `StableDocumented` | [settings](https://platform.iflow.cn/en/cli/configuration/settings), [hooks](https://platform.iflow.cn/en/cli/examples/hooks) | `.iflow/settings.json`, hooks, and `mcpServers` are documented. |
| JetBrains Junie | `StableDocumented` | [Junie CLI MCP](https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html), [JetBrains MCP settings](https://www.jetbrains.com/help/junie/mcp-settings.html) | `.junie/mcp/mcp.json`, `~/.junie/mcp/mcp.json`, and `mcpServers` are documented. |
| Qoder CLI | `Observed` | [CLI docs](https://docs.qoder.com/cli/using-cli) | `.qoder.json` and `AGENTS.md` are documented. The fetched page did not expose `mcpServers`, so the MCP shape stays repository-observed. |
| Qwen Code | `StableDocumented` | [MCP](https://qwenlm.github.io/qwen-code-docs/en/users/features/mcp/), [skills](https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/), [settings source](https://github.com/QwenLM/qwen-code/blob/main/docs/users/configuration/settings.md) | `.qwen/settings.json`, `~/.qwen/settings.json`, `mcpServers`, and `.qwen/skills` are documented. |
| Tabnine CLI | `StableDocumented` | [CLI](https://docs.tabnine.com/main/getting-started/tabnine-cli/), [settings reference](https://docs.tabnine.com/main/getting-started/tabnine-cli/features/settings/settings-reference) | `.tabnine/agent/settings.json`, hooks, and `mcpServers` are documented. |
| Trae | `Observed` | [rules](https://docs.trae.ai/ide/rules), [skills](https://docs.trae.ai/ide/skills), [trae-agent repository](https://github.com/bytedance/trae-agent) | The fetched official docs did not expose `.trae/project_rules.md` or `.trae/skills`. Public repository and ecosystem references indicate those paths, so the row remains `Observed`. |

## Follow-up policy

Promote an `Observed` row only after the per-agent source exposes the exact
path and config shape, or after a linked upstream repository file is added as
the explicit contract source. Demote any `StableDocumented` row if a future
review finds that the upstream docs removed or changed the path contract.
