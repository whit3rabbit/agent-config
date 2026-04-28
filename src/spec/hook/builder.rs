//! Fluent builder for [`HookSpec`].

use crate::error::AgentConfigError;

use super::{
    validate_hook_string, Event, HookCommand, HookSpec, HookStringKind, Matcher, RulesBlock,
    ScriptTemplate,
};

/// Builder for [`HookSpec`].
#[derive(Debug, Clone)]
pub struct HookSpecBuilder {
    pub(super) tag: String,
    pub(super) command: Option<HookCommand>,
    pub(super) matcher: Matcher,
    pub(super) event: Event,
    pub(super) rules: Option<RulesBlock>,
    pub(super) script: Option<ScriptTemplate>,
    pub(super) friendly_name: Option<String>,
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

    /// Finalize the spec, panicking on missing or invalid fields.
    ///
    /// Convenience wrapper around [`try_build()`](Self::try_build) for tests
    /// and examples. Production code should prefer [`try_build()`](Self::try_build)
    /// to propagate errors instead of panicking.
    ///
    /// # Panics
    ///
    /// Panics if `command` was never set.
    pub fn build(self) -> HookSpec {
        self.try_build().expect("HookSpec missing `command`")
    }

    /// Finalize the spec, returning [`Result`] on missing or invalid fields.
    ///
    /// This is the recommended way to build a spec in production code.
    /// See [crate-level documentation](crate#production-usage) for a full example.
    ///
    /// # Errors
    ///
    /// - [`AgentConfigError::InvalidTag`] when `tag`, the matcher string, or
    ///   any custom event name fails identifier or hook-string validation.
    /// - [`AgentConfigError::MissingSpecField`] (`field = "command"`) when
    ///   neither [`HookSpecBuilder::command_program`] nor
    ///   [`HookSpecBuilder::command_shell_unchecked`] was called.
    /// - [`AgentConfigError::InvalidTag`] from `command.validate()` when the
    ///   command string is empty or contains control characters.
    pub fn try_build(self) -> Result<HookSpec, AgentConfigError> {
        HookSpec::validate_tag(&self.tag)?;
        let command = self.command.ok_or(AgentConfigError::MissingSpecField {
            id: "<builder>",
            field: "command",
        })?;
        command.validate()?;
        match &self.matcher {
            Matcher::All | Matcher::Bash => {}
            Matcher::Exact(s) | Matcher::Regex(s) => {
                validate_hook_string(s, HookStringKind::Matcher)?;
            }
            Matcher::AnyOf(list) => {
                if list.is_empty() {
                    return Err(AgentConfigError::InvalidTag {
                        tag: String::new(),
                        reason: "matcher AnyOf must contain at least one entry",
                    });
                }
                for s in list {
                    validate_hook_string(s, HookStringKind::Matcher)?;
                }
            }
        }
        if let Event::Custom(name) = &self.event {
            validate_hook_string(name, HookStringKind::CustomEvent)?;
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
