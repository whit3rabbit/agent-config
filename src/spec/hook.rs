//! Hook spec, builder, and supporting types.

use crate::error::AgentConfigError;

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

    /// The command the harness should execute.
    ///
    /// Use [`HookSpecBuilder::command_program`] for the safe default. Raw shell
    /// remains available through [`HookSpecBuilder::command_shell_unchecked`]
    /// for callers that intentionally need shell syntax.
    pub command: HookCommand,

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
    pub(crate) fn validate_tag(tag: &str) -> Result<(), AgentConfigError> {
        validate_identifier(tag, IdentifierKind::Tag)
    }
}

/// Builder for [`HookSpec`].
#[derive(Debug, Clone)]
pub struct HookSpecBuilder {
    tag: String,
    command: Option<HookCommand>,
    matcher: Matcher,
    event: Event,
    rules: Option<RulesBlock>,
    script: Option<ScriptTemplate>,
    friendly_name: Option<String>,
}

impl HookSpecBuilder {
    /// Set the program and arguments the harness should execute when the hook
    /// fires.
    ///
    /// Integrations that only accept shell strings render this command with
    /// POSIX shell quoting, so arguments containing spaces or shell
    /// metacharacters remain arguments instead of becoming shell syntax.
    pub fn command_program<I, S>(mut self, program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.command = Some(HookCommand::Program {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        });
        self
    }

    /// Set an unchecked raw shell command.
    ///
    /// This is intentionally explicit: the command string is passed through as
    /// shell syntax for harnesses and generated scripts. Use this only when the
    /// full command is trusted and already sanitized.
    pub fn command_shell_unchecked(mut self, command: impl Into<String>) -> Self {
        self.command = Some(HookCommand::ShellUnchecked {
            command: command.into(),
        });
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
    pub fn try_build(self) -> Result<HookSpec, AgentConfigError> {
        HookSpec::validate_tag(&self.tag)?;
        let command = self.command.ok_or(AgentConfigError::MissingSpecField {
            id: "<builder>",
            field: "command",
        })?;
        command.validate()?;
        match &self.matcher {
            Matcher::All | Matcher::Bash => {}
            Matcher::Exact(s) | Matcher::Regex(s) => validate_hook_string(s, "matcher")?,
            Matcher::AnyOf(list) => {
                if list.is_empty() {
                    return Err(AgentConfigError::InvalidTag {
                        tag: String::new(),
                        reason: "matcher AnyOf must contain at least one entry",
                    });
                }
                for s in list {
                    validate_hook_string(s, "matcher")?;
                }
            }
        }
        if let Event::Custom(name) = &self.event {
            validate_hook_string(name, "event")?;
        }
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

/// Hook command representation.
///
/// [`HookCommand::Program`] is the safe default. It stores argv-like program
/// and argument values, then renders them with shell quoting for harnesses
/// whose hook APIs only accept strings. [`HookCommand::ShellUnchecked`] is an
/// escape hatch for trusted raw shell syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HookCommand {
    /// A program plus arguments, rendered safely when a shell string is needed.
    Program {
        /// Executable name or path.
        program: String,
        /// Command arguments.
        args: Vec<String>,
    },
    /// Trusted raw shell syntax.
    ShellUnchecked {
        /// Shell command passed through without quoting or escaping.
        command: String,
    },
}

impl HookCommand {
    /// Construct a safe program command.
    pub fn program<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Program {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Construct an unchecked raw shell command.
    pub fn shell_unchecked(command: impl Into<String>) -> Self {
        Self::ShellUnchecked {
            command: command.into(),
        }
    }

    /// Render the command for harnesses whose hook contract accepts a shell
    /// command string.
    pub fn render_shell(&self) -> String {
        match self {
            Self::Program { program, args } => std::iter::once(program.as_str())
                .chain(args.iter().map(String::as_str))
                .map(shell_quote)
                .collect::<Vec<_>>()
                .join(" "),
            Self::ShellUnchecked { command } => command.clone(),
        }
    }

    fn validate(&self) -> Result<(), AgentConfigError> {
        match self {
            Self::Program { program, args } => {
                if program.is_empty() {
                    return Err(AgentConfigError::InvalidCommand {
                        reason: "program must not be empty",
                    });
                }
                validate_no_nul(program)?;
                for arg in args {
                    validate_no_nul(arg)?;
                }
            }
            Self::ShellUnchecked { command } => {
                if command.trim().is_empty() {
                    return Err(AgentConfigError::InvalidCommand {
                        reason: "shell command must not be empty",
                    });
                }
                validate_no_nul(command)?;
            }
        }
        Ok(())
    }
}

fn validate_no_nul(value: &str) -> Result<(), AgentConfigError> {
    if value.contains('\0') {
        return Err(AgentConfigError::InvalidCommand {
            reason: "command values must not contain NUL bytes",
        });
    }
    Ok(())
}

fn validate_hook_string(value: &str, field: &'static str) -> Result<(), AgentConfigError> {
    if value.is_empty() {
        return Err(AgentConfigError::InvalidTag {
            tag: value.to_string(),
            reason: match field {
                "matcher" => "matcher value must not be empty",
                "event" => "custom event name must not be empty",
                _ => "hook spec value must not be empty",
            },
        });
    }
    for c in value.chars() {
        if (c as u32) < 0x20 || c == '\u{007F}' {
            return Err(AgentConfigError::InvalidTag {
                tag: value.to_string(),
                reason: match field {
                    "matcher" => "matcher value must not contain control characters",
                    "event" => "custom event name must not contain control characters",
                    _ => "hook spec value must not contain control characters",
                },
            });
        }
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value
        .bytes()
        .all(|b| matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'=' | b','))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
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
        let err = HookSpec::builder("")
            .command_program("x", [] as [&str; 0])
            .try_build()
            .unwrap_err();
        assert!(
            matches!(err, AgentConfigError::InvalidTag { reason, .. } if reason == "tag must not be empty")
        );
    }

    #[test]
    fn try_build_rejects_tag_with_spaces() {
        let err = HookSpec::builder("not valid")
            .command_program("x", [] as [&str; 0])
            .try_build()
            .unwrap_err();
        assert!(
            matches!(err, AgentConfigError::InvalidTag { reason, .. } if reason.contains("ASCII"))
        );
    }

    #[test]
    fn try_build_rejects_tag_with_special_chars() {
        for bad in ["tag/slash", "tag.dot", "tag!bang", "tag@at"] {
            let err = HookSpec::builder(bad)
                .command_program("x", [] as [&str; 0])
                .try_build()
                .unwrap_err();
            assert!(
                matches!(err, AgentConfigError::InvalidTag { .. }),
                "expected InvalidTag for {bad:?}"
            );
        }
    }

    #[test]
    fn try_build_accepts_valid_tags() {
        for ok in ["myapp", "my-app", "my_app", "App123", "A", "z9_z"] {
            HookSpec::builder(ok)
                .command_program("x", [] as [&str; 0])
                .try_build()
                .expect("expected valid tag");
        }
    }

    #[test]
    fn try_build_rejects_missing_command() {
        let err = HookSpec::builder("ok").try_build().unwrap_err();
        assert!(
            matches!(err, AgentConfigError::MissingSpecField { field, .. } if field == "command")
        );
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
            .command_program("run", ["--flag"])
            .matcher(Matcher::Bash)
            .event(Event::PostToolUse)
            .rules("my rules")
            .script(ScriptTemplate::Shell("set -e".into()))
            .friendly_name("My App")
            .build();

        assert_eq!(spec.tag, "myapp");
        assert_eq!(
            spec.command,
            HookCommand::Program {
                program: "run".into(),
                args: vec!["--flag".into()]
            }
        );
        assert!(matches!(spec.matcher, Matcher::Bash));
        assert!(matches!(spec.event, Event::PostToolUse));
        assert!(spec.rules.is_some());
        assert!(spec.script.is_some());
        assert_eq!(spec.friendly_name.as_deref(), Some("My App"));
    }

    #[test]
    fn builder_defaults() {
        let spec = HookSpec::builder("myapp")
            .command_program("run", [] as [&str; 0])
            .build();

        assert!(matches!(spec.matcher, Matcher::All));
        assert!(matches!(spec.event, Event::PreToolUse));
        assert!(spec.rules.is_none());
        assert!(spec.script.is_none());
        assert!(spec.friendly_name.is_none());
    }

    #[test]
    fn program_command_renders_shell_safe_arguments() {
        let command = HookCommand::program(
            "my hook",
            [
                "repo path",
                "semi;colon",
                "$(not run)",
                "`not run`",
                "line\nbreak",
                "quote's",
                "",
            ],
        );
        assert_eq!(
            command.render_shell(),
            "'my hook' 'repo path' 'semi;colon' '$(not run)' '`not run`' 'line\nbreak' 'quote'\\''s' ''"
        );
    }

    #[test]
    fn raw_shell_command_is_explicitly_unchecked() {
        let spec = HookSpec::builder("myapp")
            .command_shell_unchecked("myapp hook \"$REPO\"")
            .build();
        assert_eq!(spec.command.render_shell(), "myapp hook \"$REPO\"");
    }

    #[test]
    fn try_build_rejects_invalid_command_values() {
        let empty_program = HookSpec::builder("myapp")
            .command_program("", [] as [&str; 0])
            .try_build()
            .unwrap_err();
        assert!(matches!(
            empty_program,
            AgentConfigError::InvalidCommand { .. }
        ));

        let nul_arg = HookSpec::builder("myapp")
            .command_program("myapp", ["bad\0arg"])
            .try_build()
            .unwrap_err();
        assert!(matches!(nul_arg, AgentConfigError::InvalidCommand { .. }));

        let empty_shell = HookSpec::builder("myapp")
            .command_shell_unchecked(" ")
            .try_build()
            .unwrap_err();
        assert!(matches!(
            empty_shell,
            AgentConfigError::InvalidCommand { .. }
        ));
    }

    #[test]
    fn try_build_rejects_empty_exact_matcher() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .matcher(Matcher::Exact(String::new()))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_rejects_empty_anyof_matcher() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .matcher(Matcher::AnyOf(Vec::new()))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_rejects_empty_string_in_anyof() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .matcher(Matcher::AnyOf(vec!["Edit".into(), String::new()]))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_rejects_control_char_in_regex() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .matcher(Matcher::Regex("foo\u{0007}".into()))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_rejects_empty_custom_event() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .event(Event::Custom(String::new()))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_rejects_control_char_in_custom_event() {
        let err = HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .event(Event::Custom("before\nShell".into()))
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn try_build_accepts_valid_custom_event_and_matcher() {
        HookSpec::builder("ok")
            .command_program("x", [] as [&str; 0])
            .event(Event::Custom("beforeShellExecution".into()))
            .matcher(Matcher::AnyOf(vec!["Edit".into(), "Read".into()]))
            .try_build()
            .expect("valid");
    }
}
