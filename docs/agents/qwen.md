# Qwen Code

ID: `qwen` — `agent_config::by_id("qwen")`

Qwen Code is Alibaba's terminal coding agent, a Gemini-CLI fork. The on-disk
shape mirrors Gemini's.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Qwen's documented surface  |
| Prompt  | Global + Local | Fenced block in `QWEN.md`              |
| MCP     | Global + Local | `mcpServers` JSON map in `settings.json` |
| Skills  | Global + Local | `SKILL.md` directories                 |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.qwen/QWEN.md` |
| Project scope file | `<root>/QWEN.md` |
| Format | Tagged HTML-comment fence |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.qwen/settings.json` |
| Local | `<root>/.qwen/settings.json` |

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

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.qwen/skills/<name>/` |
| Project | `<root>/.qwen/skills/<name>/` |

## References

- <https://qwenlm.github.io/qwen-code-docs/>
- <https://github.com/QwenLM/qwen-code>

Accessed: 2026-04-26.
