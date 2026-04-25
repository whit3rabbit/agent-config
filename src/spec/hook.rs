//! Hook spec, builder, and supporting types.

use crate::error::HookerError;

use super::validate::{validate_identifier, IdentifierKind};

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
#[non_exhaustive]
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
#[non_exhaustive]
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
#[non_exhaustive]
pub enum ScriptTemplate {
    /// POSIX shell script body. The library adds the shebang if absent and
    /// chmods the file `0755`. A SHA-256 sidecar is written for harnesses
    /// (Gemini) that verify integrity.
    Shell(String),
    /// TypeScript plugin body for OpenCode / OpenClaw.
    TypeScript(String),
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
}
