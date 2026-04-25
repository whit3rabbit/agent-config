---
name: synrepo
description: Use synrepo in repositories with a .synrepo/ directory. Prefer synrepo cards and search before reading source files cold.
---

# synrepo context

## Agent doctrine

synrepo is a code-context compiler. When `.synrepo/` exists in the repo root, prefer MCP tools (or the CLI fallback) over cold file reads for orientation and navigation.

### Default path

The required sequence is orient, find, impact or risks, edit, tests, changed.

1. Start with `synrepo_orient` before reading the repo cold.
2. Use `synrepo_find` or `synrepo_search` to find candidate files and symbols.
3. Use `tiny` cards to route and `normal` cards to understand. Use `synrepo_minimum_context` once a focal target is known but the surrounding neighborhood risk is unclear.
4. Use `synrepo_impact` (or its shorthand `synrepo_risks`) before editing and `synrepo_tests` before claiming done.
5. Use `synrepo_changed` after edits to review changed context and validation commands.
6. Read full source files or request `deep` cards only after bounded cards identify the target or when the card content is insufficient. Full-file reads are an explicit escalation, not the default first step.

Graph-backed structural facts (files, symbols, edges) remain the authoritative source of truth. Overlay commentary, explain docs, and proposed cross-links are advisory, labeled machine-authored, and freshness-sensitive. Treat stale labels as information, not as errors. **Refresh is explicit**: every tool returns what is currently in the overlay. To get fresh commentary after a code change, you must call `synrepo_refresh_commentary(target)`.

### Do not

- Do not open large files first. Start at `tiny` and escalate only when a specific field forces it.
- Do not read a full source file before synrepo routing has identified it; treat a full-file read as an escalation after the bounded card is insufficient.
- Do not treat overlay commentary, explain docs, or proposed cross-links as canonical source truth. They are advisory prose layered on structural cards.
- Do not trigger explain (`--generate-cross-links`, deep commentary refresh) unless the task justifies the cost.
- Do not expect watch or background behavior unless `synrepo watch` is explicitly running.

### Product boundary

- synrepo stores code facts and bounded operational memory. It is not a task tracker, not session memory, and not cross-session agent memory.
- Any handoff or next-action list is a derived recommendation regenerated from repo state. External task systems own assignment, status, and collaboration.
- Freshness is explicit. A stale label is information, not an error; it is not silently refreshed on read.

## MCP tools (primary interface)

Use these when the synrepo MCP server is running:

- `synrepo_card target=<id> budget=<tiny|normal|deep>` — structured card for a file or symbol.
- `synrepo_search query=<text>` — lexical search across indexed files.
- `synrepo_overview` — graph counts and mode summary.
- `synrepo_where_to_edit task=<description>` — file suggestions for a plain-language task.
- `synrepo_change_impact target=<id>` — first-pass reverse dependencies for this file or symbol.
- `synrepo_minimum_context target=<id> budget=<...>` — budget-bounded 1-hop neighborhood.
- `synrepo_entrypoints` — entry-point discovery (binaries, CLI commands, HTTP handlers, lib roots).
- `synrepo_findings [node_id=<id>] [kind=<kind>] [freshness=<state>]` — operator-facing cross-link findings.
- `synrepo_recent_activity [kinds=<list>] [limit=<n>]` — bounded synrepo operational events.

Use `synrepo_search` to find node IDs (format: `file_0000000000000042`, `symbol_0000000000000024`) before calling card, impact, or findings tools.

Treat `synrepo_change_impact` as routing help, not exact blast-radius proof. The current impact signal is file-level and approximate.

## CLI fallback (when MCP is not running)

```
synrepo status                                   # health: mode, graph counts, last reconcile
synrepo status --recent                          # bounded operational history
synrepo graph stats                              # node and edge counts as JSON
synrepo search <query>                           # lexical search
synrepo node <id>                                # node metadata as JSON
synrepo graph query "inbound <node_id>"          # what depends on this node
synrepo graph query "outbound <node_id>"         # what this node depends on
synrepo graph query "outbound <node_id> defines" # filtered by edge kind
synrepo reconcile                                # refresh graph against current files
synrepo links list [--tier <tier>]               # active cross-link candidates
synrepo findings [--node <id>] [--freshness <state>] # audit findings
```
