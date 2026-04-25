# Hermes Agent

**ID:** `hermes`

**Status:** registered for prompt rules, global MCP, and global skills.

## Prompt

Hermes loads project context from `.hermes.md` / `HERMES.md` before falling
back to other project instruction files. This crate writes `.hermes.md`.

| Scope | File |
| --- | --- |
| Local | `<root>/.hermes.md` |

`ai-hooker` inserts a fenced markdown block keyed by the consumer tag. Global
prompt install is unsupported, and this crate does not modify `SOUL.md`.

## MCP

Hermes reads MCP server config from `~/.hermes/config.yaml` under
`mcp_servers`.

| Scope | File | Shape |
| --- | --- | --- |
| Global | `~/.hermes/config.yaml` | `mcp_servers.<name>` |

MCP is global-only in this crate. Local MCP install returns
`HookerError::UnsupportedScope`.

Transport mapping:

- `Stdio`: `command`, `args`, optional `env`
- `Http` / `Sse`: `url`, optional `headers`

Ownership is recorded in `.ai-hooker-mcp.json` beside `config.yaml`.

## Skills

Hermes stores local writable skills under `~/.hermes/skills`, with category
directories. This crate writes a dedicated category:

| Scope | Root |
| --- | --- |
| Global | `~/.hermes/skills/ai-hooker/<name>/` |

Local skill install is unsupported. External shared skill directories can be
configured in Hermes separately, but this crate does not edit
`skills.external_dirs`.

## References

- <https://hermes-agent.nousresearch.com/docs/user-guide/configuration/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp>
- <https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/skills/>

Accessed: 2026-04-25.
