//! Fluent builder for [`McpSpec`].

use std::collections::BTreeMap;

use crate::error::AgentConfigError;

use super::transport::McpTransport;
use super::{McpSpec, SecretPolicy};

/// Builder for [`McpSpec`].
#[derive(Debug, Clone)]
pub struct McpSpecBuilder {
    pub(super) name: String,
    pub(super) owner_tag: Option<String>,
    pub(super) transport: Option<McpTransport>,
    pub(super) friendly_name: Option<String>,
    pub(super) secret_policy: SecretPolicy,
    pub(super) adopt_unowned: bool,
    pub(super) builder_error: Option<String>,
}

impl McpSpecBuilder {
    /// Set the consumer's owner tag (recorded in the sidecar ownership ledger).
    pub fn owner(mut self, tag: impl Into<String>) -> Self {
        self.owner_tag = Some(tag.into());
        self
    }

    /// Adopt a config entry that exists on disk but has no recorded owner.
    ///
    /// Use this to recover from a crash between an earlier install's config
    /// write and ledger record. With `false` (default) such entries are
    /// refused with [`AgentConfigError::NotOwnedByCaller`] (`actual: None`)
    /// to avoid silently taking over a hand-installed entry.
    pub fn adopt_unowned(mut self, adopt: bool) -> Self {
        self.adopt_unowned = adopt;
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

    /// Set or replace one environment variable on a stdio transport.
    ///
    /// Calling this before configuring stdio, or after configuring a non-stdio
    /// transport, records a builder error returned by
    /// [`try_build`](Self::try_build).
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self.transport {
            Some(McpTransport::Stdio { env, .. }) => {
                env.insert(key.into(), value.into());
            }
            Some(McpTransport::Http { .. }) | Some(McpTransport::Sse { .. }) => {
                self.record_builder_error("env() can only be used with stdio MCP transports");
            }
            None => {
                self.record_builder_error("env() called before stdio transport was configured");
            }
        }
        self
    }

    /// Set an env variable to a placeholder that references the host
    /// environment, e.g. `GITHUB_TOKEN=${GITHUB_TOKEN}`.
    ///
    /// Placeholders are not treated as inline secrets by the local-scope
    /// secret policy because the actual secret value is not written.
    pub fn env_from_host(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        let placeholder = format!("${{{key}}}");
        self = self.env(key, placeholder);
        self
    }

    /// Set an env variable to a caller-provided placeholder.
    ///
    /// This is useful when a harness supports its own placeholder syntax. The
    /// value is still validated as a normal MCP env value.
    pub fn env_placeholder(
        mut self,
        key: impl Into<String>,
        placeholder: impl Into<String>,
    ) -> Self {
        self = self.env(key, placeholder);
        self
    }

    /// Set or replace one header on an HTTP/SSE transport.
    ///
    /// Calling this before configuring HTTP/SSE, or after configuring stdio,
    /// records a builder error returned by [`try_build`](Self::try_build).
    pub fn header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        match &mut self.transport {
            Some(McpTransport::Http { headers, .. }) | Some(McpTransport::Sse { headers, .. }) => {
                headers.insert(key.into(), value.into());
            }
            Some(McpTransport::Stdio { .. }) => {
                self.record_builder_error("header() can only be used with HTTP/SSE MCP transports");
            }
            None => {
                self.record_builder_error(
                    "header() called before HTTP/SSE transport was configured",
                );
            }
        }
        self
    }

    /// Set a human-friendly display name.
    pub fn friendly_name(mut self, name: impl Into<String>) -> Self {
        self.friendly_name = Some(name.into());
        self
    }

    /// Explicitly allow likely secret env values in project-local MCP configs.
    ///
    /// The default policy refuses this because local project config files are
    /// easy to commit, sync, or share accidentally.
    pub fn allow_local_inline_secrets(mut self) -> Self {
        self.secret_policy = SecretPolicy::AllowInlineSecretsInLocalScope;
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
    /// Panics if required fields are missing or validation fails.
    pub fn build(self) -> McpSpec {
        self.try_build().expect("McpSpec missing required field")
    }

    /// Finalize the spec, returning [`Result`] on missing or invalid fields.
    ///
    /// This is the recommended way to build a spec in production code.
    /// See [crate-level documentation](crate#production-usage) for a full example.
    ///
    /// # Errors
    ///
    /// - [`AgentConfigError::Other`] when an earlier builder method recorded
    ///   a deferred error (e.g. invalid env var name).
    /// - [`AgentConfigError::MissingSpecField`] with `field = "owner"` or
    ///   `field = "transport"` when those calls were skipped.
    /// - [`AgentConfigError::InvalidTag`] when `name`, `owner_tag`, or any
    ///   transport field (command path, env var, header value) fails
    ///   identifier or transport validation.
    pub fn try_build(self) -> Result<McpSpec, AgentConfigError> {
        if let Some(error) = self.builder_error {
            return Err(AgentConfigError::Other(anyhow::anyhow!(error)));
        }
        let owner_tag = self.owner_tag.ok_or(AgentConfigError::MissingSpecField {
            id: "<mcp builder>",
            field: "owner",
        })?;
        let transport = self.transport.ok_or(AgentConfigError::MissingSpecField {
            id: "<mcp builder>",
            field: "transport",
        })?;
        let spec = McpSpec {
            name: self.name,
            owner_tag,
            transport,
            friendly_name: self.friendly_name,
            secret_policy: self.secret_policy,
            adopt_unowned: self.adopt_unowned,
        };
        spec.validate()?;
        Ok(spec)
    }

    fn record_builder_error(&mut self, message: &'static str) {
        if self.builder_error.is_none() {
            self.builder_error = Some(message.to_string());
        }
    }
}
