//! Caller-supplied description of a hook (or MCP server, or skill) to install.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::HookerError;

/// Everything an [`Integration`](crate::Integration) needs to install a hook.
///
/// Build via [`HookSpec::builder`].
#[derive(Debug, Clone)]
pub struct HookSpec {
    /// Unique identifier for *the consumer of this library*. Used to namespace
    /// fenced markdown blocks, JSON entries, and per-tag filenames so multiple
    /// CLIs can coexist without stomping each other. Must be ASCII alnum / `_`
    /// / `-`, non-empty.
    pub tag: String,

    /// The shell command the harness should execute. Examples:
    /// `"myapp hook claude"`, `"my-tool intercept --agent cursor"`.
    pub command: String,

    /// Which tool calls the hook should match.
    pub matcher: Matcher,

    /// Which lifecycle event to attach to.
    pub event: Event,

    /// Optional markdown to inject into the harness's rules/memory file
    /// (e.g., `~/.claude/CLAUDE.md`, `./.clinerules`, `~/.gemini/GEMINI.md`).
    pub rules: Option<RulesBlock>,

    /// Optional script body for harnesses that delegate via a script file
    /// (currently Gemini's `~/.gemini/hooks/*.sh`) or a TS plugin
    /// (OpenCode, OpenClaw).
    pub script: Option<ScriptTemplate>,

    /// Optional human-friendly display name for log/UI output. If absent the
    /// integration's `display_name` is used.
    pub friendly_name: Option<String>,
}

impl HookSpec {
    /// Start building a spec with the given consumer tag.
    pub fn builder(tag: impl Into<String>) -> HookSpecBuilder {
        HookSpecBuilder {
            tag: tag.into(),
            command: None,
            matcher: Matcher::All,
            event: Event::PreToolUse,
            rules: None,
            script: None,
            friendly_name: None,
        }
    }

    /// Validate that the tag is non-empty and contains only safe characters.
    pub(crate) fn validate_tag(tag: &str) -> Result<(), HookerError> {
        validate_identifier(tag, IdentifierKind::Tag)
    }
}

/// Which identifier we are validating; controls the wording of
/// [`HookerError::InvalidTag::reason`].
#[derive(Copy, Clone)]
pub(crate) enum IdentifierKind {
    Tag,
    OwnerTag,
    McpName,
    SkillName,
}

/// Shared identifier validator: must be non-empty and contain only ASCII
/// alphanumerics, `_`, or `-`. Reasons are static strings so callers preserve
/// the existing [`HookerError::InvalidTag`] shape.
pub(crate) fn validate_identifier(value: &str, kind: IdentifierKind) -> Result<(), HookerError> {
    let (empty, illegal) = match kind {
        IdentifierKind::Tag => (
            "tag must not be empty",
            "tag may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::OwnerTag => (
            "owner_tag must not be empty",
            "owner_tag may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::McpName => (
            "MCP server name must not be empty",
            "MCP server name may only contain ASCII letters, digits, '_' and '-'",
        ),
        IdentifierKind::SkillName => (
            "skill name must not be empty",
            "skill name may only contain ASCII letters, digits, '_' and '-'",
        ),
    };
    if value.is_empty() {
        return Err(HookerError::InvalidTag {
            tag: value.into(),
            reason: empty,
        });
    }
    let ok = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !ok {
        return Err(HookerError::InvalidTag {
            tag: value.into(),
            reason: illegal,
        });
    }
    Ok(())
}

/// Builder for [`HookSpec`].
#[derive(Debug, Clone)]
pub struct HookSpecBuilder {
    tag: String,
    command: Option<String>,
    matcher: Matcher,
    event: Event,
    rules: Option<RulesBlock>,
    script: Option<ScriptTemplate>,
    friendly_name: Option<String>,
}

impl HookSpecBuilder {
    /// Set the command the harness should execute when the hook fires.
    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.command = Some(cmd.into());
        self
    }

    /// Set the tool-call matcher.
    pub fn matcher(mut self, m: Matcher) -> Self {
        self.matcher = m;
        self
    }

    /// Set the lifecycle event to attach to.
    pub fn event(mut self, e: Event) -> Self {
        self.event = e;
        self
    }

    /// Attach a markdown rules block to be injected into the harness's memory
    /// file.
    pub fn rules(mut self, content: impl Into<String>) -> Self {
        self.rules = Some(RulesBlock {
            content: content.into(),
        });
        self
    }

    /// Attach a script template (shell or TS) for harnesses that need one.
    pub fn script(mut self, script: ScriptTemplate) -> Self {
        self.script = Some(script);
        self
    }

    /// Set a human-friendly display name shown in install reports.
    pub fn friendly_name(mut self, name: impl Into<String>) -> Self {
        self.friendly_name = Some(name.into());
        self
    }

    /// Finalize the spec.
    ///
    /// # Panics
    ///
    /// Panics if `command` was never set. For a fallible variant use
    /// [`HookSpecBuilder::try_build`].
    pub fn build(self) -> HookSpec {
        self.try_build().expect("HookSpec missing `command`")
    }

    /// Fallible variant of [`build`](Self::build).
    pub fn try_build(self) -> Result<HookSpec, HookerError> {
        HookSpec::validate_tag(&self.tag)?;
        let command = self.command.ok_or(HookerError::MissingSpecField {
            id: "<builder>",
            field: "command",
        })?;
        Ok(HookSpec {
            tag: self.tag,
            command,
            matcher: self.matcher,
            event: self.event,
            rules: self.rules,
            script: self.script,
            friendly_name: self.friendly_name,
        })
    }
}

/// Which tool calls a hook should match.
///
/// Each integration translates this to its harness's native syntax. For
/// example, Claude Code accepts a regex when the matcher contains characters
/// outside `[A-Za-z0-9_|]`, so [`Matcher::Regex`] passes through verbatim.
#[derive(Debug, Clone)]
pub enum Matcher {
    /// Match any tool call.
    All,
    /// Match the harness's "Bash"/"Shell"/"shell" tool (whatever it calls
    /// command execution). Each integration maps this to the right literal.
    Bash,
    /// Match exactly this tool name (e.g., `"Edit"`, `"Read"`).
    Exact(String),
    /// Match any of these tool names.
    AnyOf(Vec<String>),
    /// Match using the harness's regex syntax (passed through unchanged).
    Regex(String),
}

/// Lifecycle event to attach to. Each integration maps this to its harness's
/// own event name (e.g., `PreToolUse` on Claude Code, `BeforeTool` on Gemini,
/// `tool.execute.before` on OpenCode).
#[derive(Debug, Clone)]
pub enum Event {
    /// Fire before a tool call is executed (the most common case; lets the
    /// hook modify or block the call).
    PreToolUse,
    /// Fire after a tool call completes.
    PostToolUse,
    /// Pass through a custom event name verbatim.
    Custom(String),
}

/// Markdown content to inject into the harness's memory/rules file, fenced by
/// HTML comments keyed on the [`HookSpec::tag`].
#[derive(Debug, Clone)]
pub struct RulesBlock {
    /// Raw markdown body (no fences — they are added by the library).
    pub content: String,
}

/// Optional script body for harnesses that need a shell script or TS plugin.
#[derive(Debug, Clone)]
pub enum ScriptTemplate {
    /// POSIX shell script body. The library adds the shebang if absent and
    /// chmods the file `0755`. A SHA-256 sidecar is written for harnesses
    /// (Gemini) that verify integrity.
    Shell(String),
    /// TypeScript plugin body for OpenCode / OpenClaw.
    TypeScript(String),
}

/// Caller-supplied description of an MCP server to register with a harness.
///
/// MCP servers are keyed by [`name`](Self::name) (the literal string the harness
/// uses to load the server), not by an arbitrary tag. To support multi-consumer
/// coexistence the library records ownership in a sidecar ledger
/// (`<config-dir>/.ai-hooker-mcp.json`) keyed by name → `owner_tag`. Removing a
/// server owned by a different consumer (or by a hand-edit) returns
/// [`HookerError::NotOwnedByCaller`].
///
/// Build via [`McpSpec::builder`].
#[derive(Debug, Clone)]
pub struct McpSpec {
    /// Server name. Becomes the key in `mcpServers` (Claude/Cursor/Gemini/
    /// Windsurf), the `name` field within the `mcp` array (OpenCode), or the
    /// table name `[mcp_servers.<name>]` (Codex). ASCII alnum/`_`/`-`,
    /// non-empty.
    pub name: String,

    /// The consumer of this library that owns the server, recorded in the
    /// ownership ledger. ASCII alnum/`_`/`-`, non-empty.
    pub owner_tag: String,

    /// How the harness should reach the server (stdio launcher, HTTP, or SSE).
    pub transport: McpTransport,

    /// Optional human-friendly display name surfaced in install reports.
    pub friendly_name: Option<String>,
}

impl McpSpec {
    /// Start building an MCP spec with the given server name.
    pub fn builder(name: impl Into<String>) -> McpSpecBuilder {
        McpSpecBuilder {
            name: name.into(),
            owner_tag: None,
            transport: None,
            friendly_name: None,
        }
    }

    /// Validate that both `name` and `owner_tag` use the same safe character
    /// set as [`HookSpec::tag`].
    pub(crate) fn validate(&self) -> Result<(), HookerError> {
        Self::validate_name(&self.name)?;
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)
    }

    /// Validate just the server name (used by uninstall, which has no spec).
    pub(crate) fn validate_name(name: &str) -> Result<(), HookerError> {
        validate_identifier(name, IdentifierKind::McpName)
    }
}

/// How an MCP server is reached.
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// Local subprocess launched via stdio (most common). The harness spawns
    /// `command` with `args`, inheriting `env` overrides on top of the harness
    /// environment.
    Stdio {
        /// Executable name or absolute path.
        command: String,
        /// Arguments passed to the command.
        args: Vec<String>,
        /// Environment variables set when launching the command. `BTreeMap`
        /// for stable serialization order.
        env: BTreeMap<String, String>,
    },
    /// HTTP endpoint (Cursor and Claude support; many harnesses do not).
    Http {
        /// Server URL.
        url: String,
        /// Additional headers (e.g. `Authorization`).
        headers: BTreeMap<String, String>,
    },
    /// Server-sent-events endpoint.
    Sse {
        /// Server URL.
        url: String,
        /// Additional headers (e.g. `Authorization`).
        headers: BTreeMap<String, String>,
    },
}

/// Builder for [`McpSpec`].
#[derive(Debug, Clone)]
pub struct McpSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    transport: Option<McpTransport>,
    friendly_name: Option<String>,
}

impl McpSpecBuilder {
    /// Set the consumer's owner tag (recorded in the sidecar ownership ledger).
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Configure a stdio launcher.
    pub fn stdio<I, S>(mut self, command: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.transport = Some(McpTransport::Stdio {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
        });
        self
    }

    /// Configure an HTTP transport.
    pub fn http(mut self, url: impl Into<String>) -> Self {
        self.transport = Some(McpTransport::Http {
            url: url.into(),
            headers: BTreeMap::new(),
        });
        self
    }

    /// Configure an SSE transport.
    pub fn sse(mut self, url: impl Into<String>) -> Self {
        self.transport = Some(McpTransport::Sse {
            url: url.into(),
            headers: BTreeMap::new(),
        });
        self
    }

    /// Set or replace one environment variable on a stdio transport. Inserting
    /// env on a non-stdio transport is silently a no-op (caller should set
    /// transport first).
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Some(McpTransport::Stdio { env, .. }) = &mut self.transport {
            env.insert(key.into(), value.into());
        }
        self
    }

    /// Set or replace one header on an HTTP/SSE transport. No-op on stdio.
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self.transport {
            Some(McpTransport::Http { headers, .. }) | Some(McpTransport::Sse { headers, .. }) => {
                headers.insert(key.into(), value.into());
            }
            _ => {}
        }
        self
    }

    /// Set a human-friendly display name.
    pub fn friendly_name(mut self, name: impl Into<String>) -> Self {
        self.friendly_name = Some(name.into());
        self
    }

    /// Finalize the spec.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or a transport were never set. For a fallible variant
    /// use [`McpSpecBuilder::try_build`].
    pub fn build(self) -> McpSpec {
        self.try_build().expect("McpSpec missing required field")
    }

    /// Fallible variant of [`build`](Self::build).
    pub fn try_build(self) -> Result<McpSpec, HookerError> {
        let owner_tag = self.owner_tag.ok_or(HookerError::MissingSpecField {
            id: "<mcp builder>",
            field: "owner",
        })?;
        let transport = self.transport.ok_or(HookerError::MissingSpecField {
            id: "<mcp builder>",
            field: "transport",
        })?;
        let spec = McpSpec {
            name: self.name,
            owner_tag,
            transport,
            friendly_name: self.friendly_name,
        };
        spec.validate()?;
        Ok(spec)
    }
}

/// Caller-supplied description of an agent skill to install.
///
/// Skills are directory-scoped: each one occupies a subdirectory under the
/// harness's `skills/` root, with a required `SKILL.md` and any number of
/// supporting files in `scripts/`, `references/`, and `assets/`.
///
/// Build via [`SkillSpec::builder`].
#[derive(Debug, Clone)]
pub struct SkillSpec {
    /// Skill directory name. Becomes the folder under the harness's
    /// `skills/` root. ASCII alnum/`_`/`-`, non-empty. Conventionally
    /// kebab-case (e.g., `git-commit-formatter`).
    pub name: String,

    /// The consumer of this library that owns the skill. Recorded in the
    /// sidecar ownership ledger; refusal to remove a skill owned by another
    /// consumer matches the same `NotOwnedByCaller` model used for MCP.
    pub owner_tag: String,

    /// YAML frontmatter required by Claude / Antigravity. Written verbatim
    /// at the head of `SKILL.md`.
    pub frontmatter: SkillFrontmatter,

    /// Markdown body of `SKILL.md` (no frontmatter — that is rendered from
    /// [`SkillSpec::frontmatter`]).
    pub body: String,

    /// Optional supporting files under `scripts/`, `references/`, `assets/`.
    /// The `relative_path` is interpreted under the skill directory; any
    /// leading `scripts/`/`references/`/`assets/` prefix is honoured as-is.
    pub assets: Vec<SkillAsset>,
}

impl SkillSpec {
    /// Start building a skill spec.
    pub fn builder(name: impl Into<String>) -> SkillSpecBuilder {
        let name = name.into();
        SkillSpecBuilder {
            name: name.clone(),
            owner_tag: None,
            frontmatter: SkillFrontmatter {
                name,
                description: String::new(),
                allowed_tools: None,
            },
            body: String::new(),
            assets: Vec::new(),
        }
    }

    /// Validate `name` and `owner_tag`.
    pub(crate) fn validate(&self) -> Result<(), HookerError> {
        Self::validate_name(&self.name)?;
        if self.frontmatter.description.trim().is_empty() {
            return Err(HookerError::MissingSpecField {
                id: "<skill spec>",
                field: "frontmatter.description",
            });
        }
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)
    }

    /// Validate just the skill name (used by uninstall, which has no spec).
    pub(crate) fn validate_name(name: &str) -> Result<(), HookerError> {
        validate_identifier(name, IdentifierKind::SkillName)
    }
}

/// YAML frontmatter prepended to `SKILL.md`. `description` is the field
/// harnesses use to decide when to activate the skill.
#[derive(Debug, Clone)]
pub struct SkillFrontmatter {
    /// Skill identifier surfaced in tooling (typically matches the directory
    /// name).
    pub name: String,

    /// Sentence (or short paragraph) explaining when the skill should
    /// activate. Required by both Claude and Antigravity; the activation
    /// model matches against this string.
    pub description: String,

    /// Optional `allowed-tools` list (Claude). When `None`, the field is
    /// omitted from the frontmatter.
    pub allowed_tools: Option<Vec<String>>,
}

/// One supporting file inside a skill directory.
#[derive(Debug, Clone)]
pub struct SkillAsset {
    /// Path relative to the skill directory, e.g.
    /// `PathBuf::from("scripts/run.sh")`. Must be a relative path; absolute
    /// paths or `..` segments are rejected at install time.
    pub relative_path: PathBuf,

    /// Raw bytes of the file. Lets callers ship binary references (e.g.
    /// images under `assets/`) as well as text scripts.
    pub bytes: Vec<u8>,

    /// On Unix, set the file mode to `0o755` after writing. No-op on
    /// Windows. Use for shell/python scripts under `scripts/`.
    pub executable: bool,
}

/// Builder for [`SkillSpec`].
#[derive(Debug, Clone)]
pub struct SkillSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    frontmatter: SkillFrontmatter,
    body: String,
    assets: Vec<SkillAsset>,
}

impl SkillSpecBuilder {
    /// Set the consumer's owner tag.
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Set the SKILL.md frontmatter `description`.
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.frontmatter.description = d.into();
        self
    }

    /// Set the SKILL.md frontmatter `allowed-tools` list (Claude only).
    pub fn allowed_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.frontmatter.allowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Set the markdown body of SKILL.md.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// Add one supporting file.
    pub fn asset(mut self, asset: SkillAsset) -> Self {
        self.assets.push(asset);
        self
    }

    /// Finalize the spec.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `description` were never set.
    pub fn build(self) -> SkillSpec {
        self.try_build().expect("SkillSpec missing required field")
    }

    /// Fallible variant of [`build`](Self::build).
    pub fn try_build(self) -> Result<SkillSpec, HookerError> {
        let owner_tag = self.owner_tag.ok_or(HookerError::MissingSpecField {
            id: "<skill builder>",
            field: "owner",
        })?;
        let spec = SkillSpec {
            name: self.name,
            owner_tag,
            frontmatter: self.frontmatter,
            body: self.body,
            assets: self.assets,
        };
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_build_rejects_empty_tag() {
        let err = HookSpec::builder("").command("x").try_build().unwrap_err();
        assert!(
            matches!(err, HookerError::InvalidTag { reason, .. } if reason == "tag must not be empty")
        );
    }

    #[test]
    fn try_build_rejects_tag_with_spaces() {
        let err = HookSpec::builder("not valid")
            .command("x")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, HookerError::InvalidTag { reason, .. } if reason.contains("ASCII")));
    }

    #[test]
    fn try_build_rejects_tag_with_special_chars() {
        for bad in ["tag/slash", "tag.dot", "tag!bang", "tag@at"] {
            let err = HookSpec::builder(bad).command("x").try_build().unwrap_err();
            assert!(
                matches!(err, HookerError::InvalidTag { .. }),
                "expected InvalidTag for {bad:?}"
            );
        }
    }

    #[test]
    fn try_build_accepts_valid_tags() {
        for ok in ["myapp", "my-app", "my_app", "App123", "A", "z9_z"] {
            HookSpec::builder(ok)
                .command("x")
                .try_build()
                .expect("expected valid tag");
        }
    }

    #[test]
    fn try_build_rejects_missing_command() {
        let err = HookSpec::builder("ok").try_build().unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { field, .. } if field == "command"));
    }

    #[test]
    fn build_panics_on_missing_command() {
        let result = std::panic::catch_unwind(|| {
            HookSpec::builder("ok").build();
        });
        assert!(result.is_err());
    }

    #[test]
    fn builder_sets_all_fields() {
        let spec = HookSpec::builder("myapp")
            .command("run")
            .matcher(Matcher::Bash)
            .event(Event::PostToolUse)
            .rules("my rules")
            .script(ScriptTemplate::Shell("set -e".into()))
            .friendly_name("My App")
            .build();

        assert_eq!(spec.tag, "myapp");
        assert_eq!(spec.command, "run");
        assert!(matches!(spec.matcher, Matcher::Bash));
        assert!(matches!(spec.event, Event::PostToolUse));
        assert!(spec.rules.is_some());
        assert!(spec.script.is_some());
        assert_eq!(spec.friendly_name.as_deref(), Some("My App"));
    }

    #[test]
    fn builder_defaults() {
        let spec = HookSpec::builder("myapp").command("run").build();

        assert!(matches!(spec.matcher, Matcher::All));
        assert!(matches!(spec.event, Event::PreToolUse));
        assert!(spec.rules.is_none());
        assert!(spec.script.is_none());
        assert!(spec.friendly_name.is_none());
    }

    #[test]
    fn mcp_builder_stdio_round_trip() {
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
            .env("GITHUB_TOKEN", "abc")
            .friendly_name("GitHub MCP")
            .build();

        assert_eq!(spec.name, "github");
        assert_eq!(spec.owner_tag, "myapp");
        assert_eq!(spec.friendly_name.as_deref(), Some("GitHub MCP"));
        match spec.transport {
            McpTransport::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args, vec!["-y", "@modelcontextprotocol/server-github"]);
                assert_eq!(env.get("GITHUB_TOKEN").map(String::as_str), Some("abc"));
            }
            other => panic!("expected stdio, got {other:?}"),
        }
    }

    #[test]
    fn mcp_builder_http_with_headers() {
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Authorization", "Bearer xyz")
            .build();
        match spec.transport {
            McpTransport::Http { url, headers } => {
                assert_eq!(url, "https://example.com/mcp");
                assert_eq!(
                    headers.get("Authorization").map(String::as_str),
                    Some("Bearer xyz")
                );
            }
            other => panic!("expected http, got {other:?}"),
        }
    }

    #[test]
    fn mcp_try_build_rejects_missing_owner() {
        let err = McpSpec::builder("x")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { field, .. } if field == "owner"));
    }

    #[test]
    fn mcp_try_build_rejects_missing_transport() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, HookerError::MissingSpecField { field, .. } if field == "transport"));
    }

    #[test]
    fn mcp_try_build_rejects_invalid_name() {
        let err = McpSpec::builder("bad name")
            .owner("myapp")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, HookerError::InvalidTag { .. }));
    }

    #[test]
    fn mcp_try_build_rejects_invalid_owner() {
        let err = McpSpec::builder("x")
            .owner("bad owner")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, HookerError::InvalidTag { .. }));
    }

    #[test]
    fn mcp_env_on_non_stdio_is_noop() {
        let spec = McpSpec::builder("x")
            .owner("myapp")
            .http("https://example.com")
            .env("IGNORED", "yes")
            .build();
        assert!(matches!(spec.transport, McpTransport::Http { .. }));
    }
}
