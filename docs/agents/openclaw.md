# OpenClaw — STUB

**Status:** deferred from v0.1. Not registered with `ai_hooker::by_id`.

## Why deferred

OpenClaw's plugin contract differs structurally from every other harness in
this crate:

1. **Manifest required.** A plugin needs `openclaw.plugin.json` with `id` and
   `configSchema` (a JSON Schema). Optional fields include `kind`, `channels`,
   `providers`, `skills`, plus ~25 others.
2. **Entry registered via `package.json`.** The TS entry is declared under
   `"openclaw": { "extensions": ["./index.ts"] }` in `package.json`, not
   the manifest.
3. **CLI install step.** Plugins are normally installed via
   `openclaw plugins install <pkg>` (resolves through ClawHub or npm),
   not by dropping a file into a directory.
4. **Different hook semantics.** `before_tool_call` returns
   `{ block, requireApproval }`, not a generic mutate-args hook like
   OpenCode's `tool.execute.before`.

A correct integration likely shells out to `openclaw plugins install`, not
the file-drop pattern this library otherwise uses.

## Plugin structure

A complete OpenClaw plugin requires three files:

### openclaw.plugin.json

```json
{
  "id": "myorg.myplugin",
  "name": "My Plugin",
  "description": "Plugin description",
  "version": "1.0.0",
  "configSchema": {
    "type": "object",
    "properties": {}
  },
  "channels": ["..."],
  "providers": ["..."],
  "skills": ["..."],
  "kind": "..."
}
```

**Required fields:**

- `id`: Canonical plugin identifier (reverse domain notation)
- `configSchema`: JSON Schema object (may be empty: `{}`)

**Optional fields:**

- `name`, `description`, `version`
- `channels`: Communication channels (Slack, Teams, Discord, etc.)
- `providers`: AI/capability providers (speech, transcription, voice, media understanding, image generation, video generation, web fetch, web search, etc.)
- `skills`: Custom agent skills
- Additional metadata and capability contracts

### package.json

```json
{
  "name": "myorg-myplugin",
  "version": "1.0.0",
  "main": "dist/index.js",
  "openclaw": {
    "extensions": ["./index.ts"]
  }
}
```

Entry points declared under `"openclaw": { "extensions": [...] }`.

### index.ts

TypeScript plugin implementation with hook subscriptions.

## Installation model

Plugins are installed via the CLI, **not** file-drop:

```bash
openclaw plugins install <package-name>
openclaw plugins uninstall <package-name>
```

Plugins resolve through **ClawHub** (OpenClaw marketplace) or **npm**.

## Hook semantics

OpenClaw hooks differ from other harnesses:

```typescript
before_tool_call: {
  block: boolean,
  requireApproval: boolean
}
```

Not a generic mutate-args model; instead a blocking/approval contract.

## Planned shape (if file-drop implemented)

If/when `ai-hooker` adds OpenClaw support via file-drop (instead of CLI):

| | |
| --- | --- |
| User scope | `~/.openclaw/<plugin-root>/<tag>/` |
| Project scope | `<root>/.openclaw/<plugin-root>/<tag>/` |

The exact `<plugin-root>` segment is configurable in OpenClaw's settings.

**Note:** File-drop approach would need to generate `package.json`, `openclaw.plugin.json`,
and plugin source, then trigger or assume a separate `openclaw plugins install` step.

## References

- <https://docs.openclaw.ai/plugins/>
- <https://docs.openclaw.ai/plugins/manifest>
- <https://docs.openclaw.ai/plugins/before-tool-call>
- <https://github.com/openclaw/openclaw>
