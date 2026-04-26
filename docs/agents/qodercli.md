# Qoder CLI

ID: `qodercli` — `ai_hooker::by_id("qodercli")`

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Qoder's documented surface |
| Prompt  | Global + Local | Fenced block in `AGENTS.md`            |
| MCP     | Global + Local | `mcpServers` JSON map; per-scope files |
| Skills  | -              | Not part of Qoder's documented surface |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.qoder/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.qoder.json` (a flat file in `$HOME`, not under `~/.qoder/`) |
| Local | `<root>/.mcp.json` |

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@example/server"]
    }
  }
}
```

Ownership is recorded in a sidecar `<config-dir>/.ai-hooker-mcp.json` ledger.
Qoder's CLI commands `qodercli mcp add/remove/list` write the same shape
directly.

## References

- <https://docs.qoder.com/cli/using-cli>

Accessed: 2026-04-26.
