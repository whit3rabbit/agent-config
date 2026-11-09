# DeepSeek Harness

**ID:** `dsh` — `agent_config::by_id("dsh")`

**Status:** registered for prompt rules (Global + Local), instructions
(Global + Local, InlineBlock), and skills (Global + Local). Native Cordis
plugins, MCP client plugins, and hook bridges are deferred.

## Surfaces

| Surface      | Scope          | Notes                                                                                       |
| ------------ | -------------- | ------------------------------------------------------------------------------------------- |
| Hooks        | -              | Upstream DSH handles hooks via Cordis plugins (`@deepseek-ai/dsh-hooks-claude-code`); deferred |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`                                                                 |
| MCP          | -              | Upstream DSH configures MCP via `@deepseek-ai/dsh-mcp-client` in `cordis.patch.yml`; deferred |
| Skills       | Global + Local | `SKILL.md` directories (`<root>/.dsh/skills/` and `$DSH_HOME/skills/`)                      |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`                                                                |

## Prompt instructions

### Memory file support

- **Supported format**: `AGENTS.md` (and `CLAUDE.md`).
- **Primary memory file**: `<root>/AGENTS.md` (Local) and `$DSH_HOME/AGENTS.md` (Global, default `~/.dsh/AGENTS.md`).
- **`AGENTS.md` support**: Native; DeepSeek Harness's instruction discovery subsystem (`@deepseek-ai/dsh-agent-instructions`) scans `instructionFileCandidates = ['AGENTS.md', 'CLAUDE.md']` from the project root down to the working directory, and loads the user-global `$DSH_HOME/AGENTS.md`.

| Scope | File | Format |
| ----- | ---- | ------ |
| Local | `<root>/AGENTS.md` | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |
| Global | `$DSH_HOME/AGENTS.md` (default `~/.dsh/AGENTS.md`) | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |

`agent-config` inserts a fenced markdown block keyed by the consumer tag. Both
Local and Global scopes are supported.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` for both Local and Global scopes:

| Scope | Host file | Mechanism | Ledger |
| ----- | --------- | --------- | ------ |
| Local | `<root>/AGENTS.md` | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) | `<root>/.dsh/.agent-config-instructions.json` |
| Global | `$DSH_HOME/AGENTS.md` (default `~/.dsh/AGENTS.md`) | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) | `$DSH_HOME/.agent-config-instructions.json` |

## Skills

DeepSeek Harness loads skills through `@deepseek-ai/dsh-skill` and
`@deepseek-ai/dsh-skill-filesystem`. Discovery scans directories one level
deep for `<root>/<name>/SKILL.md` bundles:

| Scope | Root | Discovery rank in DSH |
| ----- | ---- | --------------------- |
| Local | `<root>/.dsh/skills/<name>/` | Rank 100 (`project-dsh`) |
| Global | `$DSH_HOME/skills/<name>/` (default `~/.dsh/skills/<name>/`) | Rank 400 (`user-dsh`) |

Each skill directory contains `SKILL.md` (with YAML frontmatter containing
kebab-case `name` and `description`) plus optional caller-supplied `scripts/`,
`references/`, and `assets/`. DSH honors `$DSH_HOME` when resolving the user
directory (defaulting to `~/.dsh`).

Ownership is recorded in `.agent-config-skills.json` beside the skills root.

## Deferred Hooks and MCP

In DeepSeek Harness's Cordis architecture, "everything is a plugin":

- **Hooks**: Upstream DSH executes lifecycle interception points via typed
  Cordis plugins or compatibility bridges (`@deepseek-ai/dsh-hooks-claude-code`,
  `@deepseek-ai/dsh-hooks-codex`) mounted in `cordis.patch.yml`. DSH does not
  ship an autonomous file-backed hooks format.
- **MCP**: External Model Context Protocol servers are configured as Cordis
  plugin instances using `@deepseek-ai/dsh-mcp-client` in profile patch files
  (`cordis.patch.yml`).

Because these surfaces are plugin-managed and lifecycle-driven rather than
static file maps, native hooks and MCP installation are deferred in `agent-config`.

## References

- Quickstart: <https://deepseekdocs.com/en/docs/getting-started/quickstart>
- Configuration & Cordis Patches: <https://deepseekdocs.com/en/docs/configuration>
- Skills System (`dsh-skill-filesystem`): <https://deepseekdocs.com/en/docs/skills>
- Hooks Bridge (`dsh-hooks-claude-code`): <https://deepseekdocs.com/en/docs/hooks>
- MCP Client (`dsh-mcp-client`): <https://deepseekdocs.com/en/docs/mcp>
- Upstream repository: <https://github.com/deepseek-ai/deepseek-harness>

Accessed: 2026-11-08.
