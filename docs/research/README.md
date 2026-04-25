# Research Notes

Last researched: 2026-04-25.

## Implemented In This Pass

### OpenClaw

Verified surfaces:

- Prompt context: OpenClaw workspace uses `AGENTS.md`.
- MCP config: documented `mcp.servers` config supports stdio, SSE, and
  streamable HTTP entries.
- Skills: AgentSkills-compatible folders are documented under global and
  workspace roots.

Native hooks and plugins are deferred. OpenClaw documents native plugin and
hook-pack installation through `openclaw plugins install`, with required
manifest/package files and runtime loading.

Sources:

- <https://docs.openclaw.ai/tools/plugin>
- <https://docs.openclaw.ai/plugins/manifest>
- <https://docs.openclaw.ai/cli/mcp>
- <https://docs.openclaw.ai/tools/skills>
- <https://docs.openclaw.ai/tools/creating-skills>
- <https://docs.openclaw.ai/reference/templates/AGENTS>

Accessed: 2026-04-25.

### Hermes Agent

Verified surfaces:

- Project context: Hermes reads `.hermes.md` / `HERMES.md` project files and
  global `SOUL.md`.
- MCP config: Hermes reads `mcp_servers` from `~/.hermes/config.yaml`.
- Skills: Hermes stores writable local skills under `~/.hermes/skills` and can
  scan read-only external skill dirs when configured by the user.

This crate does not modify `SOUL.md` or `skills.external_dirs`.

Sources:

- <https://hermes-agent.nousresearch.com/docs/user-guide/configuration/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp>
- <https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/skills/>

Accessed: 2026-04-25.

## Researched Only

### NanoClaw

NanoClaw appears to use a repository and skill-branch model rather than a
simple file-backed harness config. Its docs describe a `claw` CLI, container
execution, channel forks, and Claude Code skills that transform a fork.

Sources:

- <https://docs.nanoclaw.dev/features/cli>
- <https://docs.nanoclaw.dev/integrations/skills-system>
- <https://github.com/qwibitai/nanoclaw/blob/main/docs/SPEC.md>

Accessed: 2026-04-25.

### PicoClaw

PicoClaw has documented process hooks and JSON config for tools/MCP. It looks
implementable later, but it is intentionally out of scope for this OpenClaw and
Hermes pass.

Sources:

- <https://docs.picoclaw.io/docs/hooks/>
- <https://docs.picoclaw.io/docs/configuration/tools/>
- <https://docs.picoclaw.io/docs/configuration/config-reference/>

Accessed: 2026-04-25.
