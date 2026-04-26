# JetBrains Junie

ID: `junie` — `ai_hooker::by_id("junie")`

**Status:** registered for project-local prompt rules and MCP (Global + Local).
Hook lifecycle support is tracked upstream (JUNIE-1961) but not yet released.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Pending upstream                       |
| Prompt  | Local          | Fenced block in `<root>/.junie/AGENTS.md` |
| MCP     | Global + Local | `mcpServers` JSON map at `mcp/mcp.json` |
| Skills  | -              | Not part of Junie's documented surface |

## Prompt instructions

| | |
| --- | --- |
| Project scope file | `<root>/.junie/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Global prompt install is unsupported. Junie also reads a custom filename via
the `JUNIE_GUIDELINES_FILENAME` env var; this crate writes the documented
default `AGENTS.md`.

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.junie/mcp/mcp.json` |
| Local | `<root>/.junie/mcp/mcp.json` |

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

## References

- <https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html>
- <https://www.jetbrains.com/help/junie/mcp-settings.html>

Accessed: 2026-04-26.
