//! MCP server spec, builder, and transport enum.

use std::collections::BTreeMap;

use crate::error::AgentConfigError;
use crate::scope::{Scope, ScopeKind};
use fluent_uri::Uri;

use super::validate::{validate_identifier, IdentifierKind};

/// Caller-supplied description of an MCP server to register with a harness.
///
/// MCP servers are keyed by [`name`](Self::name) (the literal string the harness
/// uses to load the server), not by an arbitrary tag. To support multi-consumer
/// coexistence the library records ownership in a sidecar ledger
/// (`<config-dir>/.agent-config-mcp.json`) keyed by name → `owner_tag`. Removing a
/// server owned by a different consumer (or by a hand-edit) returns
/// [`AgentConfigError::NotOwnedByCaller`].
///
/// Build via [`McpSpec::builder`].
#[derive(Debug, Clone)]
pub struct McpSpec {
    /// Server name. Becomes the key in `mcpServers` (Claude/Cursor/Gemini/
    /// Copilot/Windsurf), the object-based `mcp` map (OpenCode/Kilo), or the
    /// table name `[mcp_servers.<name>]` (Codex).
    /// ASCII alnum/`_`/`-`, non-empty.
    pub name: String,

    /// The consumer of this library that owns the server, recorded in the
    /// ownership ledger. ASCII alnum/`_`/`-`, non-empty.
    pub owner_tag: String,

    /// How the harness should reach the server (stdio launcher, HTTP, or SSE).
    pub transport: McpTransport,

    /// Optional human-friendly display name surfaced in install reports.
    pub friendly_name: Option<String>,

    /// Policy for inline env values that look secret-bearing when installing
    /// into project-local config files.
    pub secret_policy: SecretPolicy,
}

impl McpSpec {
    /// Start building an MCP spec with the given server name.
    pub fn builder(name: impl Into<String>) -> McpSpecBuilder {
        McpSpecBuilder {
            name: name.into(),
            owner_tag: None,
            transport: None,
            friendly_name: None,
            secret_policy: SecretPolicy::RefuseInlineSecretsInLocalScope,
            builder_error: None,
        }
    }

    /// Validate that both `name` and `owner_tag` use the same safe character
    /// set as [`HookSpec::tag`](crate::HookSpec::tag).
    pub(crate) fn validate(&self) -> Result<(), AgentConfigError> {
        Self::validate_name(&self.name)?;
        validate_identifier(&self.owner_tag, IdentifierKind::OwnerTag)?;
        validate_transport(&self.transport)
    }

    /// Validate just the server name (used by uninstall, which has no spec).
    pub(crate) fn validate_name(name: &str) -> Result<(), AgentConfigError> {
        validate_identifier(name, IdentifierKind::McpName)
    }

    /// Enforce this spec's local inline-secret policy for a target scope.
    pub(crate) fn validate_local_secret_policy(
        &self,
        scope: &Scope,
    ) -> Result<(), AgentConfigError> {
        if let Some(key) = self.refused_local_inline_secret_key(scope) {
            return Err(AgentConfigError::InlineSecretInLocalScope {
                name: self.name.clone(),
                key: key.to_string(),
            });
        }
        Ok(())
    }

    /// Returns the first env key that looks secret-bearing in local scope.
    pub(crate) fn local_inline_secret_env_key(&self, scope: &Scope) -> Option<&str> {
        if scope.kind() != ScopeKind::Local {
            return None;
        }
        let McpTransport::Stdio { env, .. } = &self.transport else {
            return None;
        };
        env.iter()
            .find(|(key, value)| is_inline_secret_env_value(key, value))
            .map(|(key, _)| key.as_str())
    }

    /// Returns the first env key refused by the current secret policy.
    pub(crate) fn refused_local_inline_secret_key(&self, scope: &Scope) -> Option<&str> {
        if self.secret_policy == SecretPolicy::RefuseInlineSecretsInLocalScope {
            self.local_inline_secret_env_key(scope)
        } else {
            None
        }
    }

    /// Returns the first env key allowed by explicit override.
    pub(crate) fn allowed_local_inline_secret_key(&self, scope: &Scope) -> Option<&str> {
        if self.secret_policy == SecretPolicy::AllowInlineSecretsInLocalScope {
            self.local_inline_secret_env_key(scope)
        } else {
            None
        }
    }
}

/// How an MCP server is reached.
#[derive(Debug, Clone)]
#[non_exhaustive]
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

/// Policy for env values that look secret-bearing in project-local MCP config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecretPolicy {
    /// Refuse local-scope installs that would write likely secret env values.
    RefuseInlineSecretsInLocalScope,
    /// Allow likely secret env values even when the config path is local.
    ///
    /// Use this only when the caller has made an explicit trust decision about
    /// the target project config.
    AllowInlineSecretsInLocalScope,
}

/// Builder for [`McpSpec`].
#[derive(Debug, Clone)]
pub struct McpSpecBuilder {
    name: String,
    owner_tag: Option<String>,
    transport: Option<McpTransport>,
    friendly_name: Option<String>,
    secret_policy: SecretPolicy,
    builder_error: Option<String>,
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

    /// Finalize the spec.
    ///
    /// # Panics
    ///
    /// Panics if required fields are missing or validation fails. For a
    /// fallible variant use [`McpSpecBuilder::try_build`].
    pub fn build(self) -> McpSpec {
        self.try_build().expect("McpSpec missing required field")
    }

    /// Fallible variant of [`build`](Self::build).
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

fn validate_transport(transport: &McpTransport) -> Result<(), AgentConfigError> {
    match transport {
        McpTransport::Stdio { command, args, env } => {
            if command.trim().is_empty() {
                return Err(invalid_mcp_spec("stdio MCP command must not be empty"));
            }
            validate_no_control_chars("stdio MCP command", command)?;
            for arg in args {
                validate_no_control_chars("stdio MCP argument", arg)?;
            }
            for (name, value) in env {
                validate_env_name(name)?;
                validate_value("stdio MCP environment value", value)?;
            }
        }
        McpTransport::Http { url, headers } => {
            validate_remote_transport("HTTP", url, headers)?;
        }
        McpTransport::Sse { url, headers } => {
            validate_remote_transport("SSE", url, headers)?;
        }
    }
    Ok(())
}

fn validate_remote_transport(
    kind: &str,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<(), AgentConfigError> {
    validate_http_url(kind, url)?;
    for (name, value) in headers {
        validate_header_name(name)?;
        validate_value("MCP header value", value)?;
    }
    Ok(())
}

fn validate_http_url(kind: &str, url: &str) -> Result<(), AgentConfigError> {
    if url.chars().any(char::is_control) {
        return Err(invalid_mcp_spec(format!(
            "{kind} MCP URL must not contain control characters"
        )));
    }

    let parsed =
        Uri::parse(url).map_err(|e| invalid_mcp_spec(format!("{kind} MCP URL is invalid: {e}")))?;
    let scheme = parsed.scheme().as_str();
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(invalid_mcp_spec(format!(
            "{kind} MCP URL must use http or https"
        )));
    }
    let Some(authority) = parsed.authority() else {
        return Err(invalid_mcp_spec(format!(
            "{kind} MCP URL must include a host"
        )));
    };
    if authority.host().is_empty() {
        return Err(invalid_mcp_spec(format!(
            "{kind} MCP URL must include a host"
        )));
    }
    Ok(())
}

fn validate_env_name(name: &str) -> Result<(), AgentConfigError> {
    if name.is_empty() {
        return Err(invalid_mcp_spec(
            "MCP environment variable name must not be empty",
        ));
    }
    if name.contains('=') {
        return Err(invalid_mcp_spec(
            "MCP environment variable name must not contain '='",
        ));
    }
    validate_no_control_chars("MCP environment variable name", name)
}

fn validate_header_name(name: &str) -> Result<(), AgentConfigError> {
    if name.is_empty() {
        return Err(invalid_mcp_spec("MCP header name must not be empty"));
    }
    if !name.chars().all(is_header_token_char) {
        return Err(invalid_mcp_spec(
            "MCP header name must contain only HTTP token characters",
        ));
    }
    Ok(())
}

fn validate_value(kind: &str, value: &str) -> Result<(), AgentConfigError> {
    validate_no_control_chars(kind, value)
}

fn validate_no_control_chars(kind: &str, value: &str) -> Result<(), AgentConfigError> {
    if value.chars().any(char::is_control) {
        return Err(invalid_mcp_spec(format!(
            "{kind} must not contain control characters"
        )));
    }
    Ok(())
}

fn is_header_token_char(c: char) -> bool {
    matches!(
        c,
        'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '!'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '*'
            | '+'
            | '-'
            | '.'
            | '^'
            | '_'
            | '`'
            | '|'
            | '~'
    )
}

fn invalid_mcp_spec(message: impl Into<String>) -> AgentConfigError {
    AgentConfigError::Other(anyhow::anyhow!(message.into()))
}

fn is_inline_secret_env_value(name: &str, value: &str) -> bool {
    likely_secret_env_name(name) && !value.trim().is_empty() && !is_placeholder_value(value)
}

fn likely_secret_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "KEY",
        "PASSWORD",
        "AUTH",
        "BEARER",
        "CREDENTIAL",
    ]
    .iter()
    .any(|keyword| upper.contains(keyword))
}

fn is_placeholder_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.starts_with("${") && trimmed.ends_with('}') && trimmed.len() > 3 {
        return true;
    }
    trimmed
        .strip_prefix('$')
        .is_some_and(|name| !name.is_empty() && name.chars().all(is_env_name_char))
}

fn is_env_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn mcp_builder_env_from_host_uses_placeholder() {
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["server"])
            .env_from_host("GITHUB_TOKEN")
            .build();

        assert_eq!(
            spec.secret_policy,
            SecretPolicy::RefuseInlineSecretsInLocalScope
        );
        assert!(spec
            .local_inline_secret_env_key(&Scope::Local("/tmp/project".into()))
            .is_none());
        match spec.transport {
            McpTransport::Stdio { env, .. } => {
                assert_eq!(
                    env.get("GITHUB_TOKEN").map(String::as_str),
                    Some("${GITHUB_TOKEN}")
                );
            }
            other => panic!("expected stdio, got {other:?}"),
        }
    }

    #[test]
    fn local_inline_secret_policy_detects_likely_secret_env() {
        let local = Scope::Local("/tmp/project".into());
        let global = Scope::Global;
        let spec = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["server"])
            .env("GITHUB_TOKEN", "abc")
            .build();

        assert_eq!(
            spec.local_inline_secret_env_key(&local),
            Some("GITHUB_TOKEN")
        );
        assert!(spec.validate_local_secret_policy(&global).is_ok());
        assert!(matches!(
            spec.validate_local_secret_policy(&local),
            Err(AgentConfigError::InlineSecretInLocalScope { key, .. }) if key == "GITHUB_TOKEN"
        ));

        let allowed = McpSpec::builder("github")
            .owner("myapp")
            .stdio("npx", ["server"])
            .env("GITHUB_TOKEN", "abc")
            .allow_local_inline_secrets()
            .build();
        assert!(allowed.validate_local_secret_policy(&local).is_ok());
        assert_eq!(
            allowed.allowed_local_inline_secret_key(&local),
            Some("GITHUB_TOKEN")
        );
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
    fn mcp_builder_http_accepts_rfc_url_forms() {
        for url in [
            "HTTP://example.com/mcp",
            "https://[2001:db8::1]/mcp?x=y#frag",
            "https://example.com/a/%7Bencoded%7D",
        ] {
            let spec = McpSpec::builder("remote")
                .owner("myapp")
                .http(url)
                .try_build()
                .unwrap();
            assert!(matches!(spec.transport, McpTransport::Http { .. }));
        }
    }

    #[test]
    fn mcp_try_build_rejects_missing_owner() {
        let err = McpSpec::builder("x")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(
            matches!(err, AgentConfigError::MissingSpecField { field, .. } if field == "owner")
        );
    }

    #[test]
    fn mcp_try_build_rejects_missing_transport() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .try_build()
            .unwrap_err();
        assert!(
            matches!(err, AgentConfigError::MissingSpecField { field, .. } if field == "transport")
        );
    }

    #[test]
    fn mcp_try_build_rejects_invalid_name() {
        let err = McpSpec::builder("bad name")
            .owner("myapp")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn mcp_try_build_rejects_invalid_owner() {
        let err = McpSpec::builder("x")
            .owner("bad owner")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::InvalidTag { .. }));
    }

    #[test]
    fn mcp_env_on_non_stdio_is_rejected() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .http("https://example.com")
            .env("IGNORED", "yes")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_header_on_stdio_is_rejected() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .stdio("cmd", Vec::<String>::new())
            .header("Authorization", "Bearer token")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_env_before_transport_is_rejected() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .env("FOO", "bar")
            .stdio("cmd", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_header_before_transport_is_rejected() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .header("Authorization", "Bearer token")
            .http("https://example.com/mcp")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_try_build_rejects_empty_stdio_command() {
        let err = McpSpec::builder("x")
            .owner("myapp")
            .stdio("  ", Vec::<String>::new())
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_try_build_rejects_invalid_remote_urls() {
        for bad in [
            "",
            "ftp://example.com/mcp",
            "https://",
            "http:///mcp",
            "http:/mcp",
            "https://exa mple.com",
        ] {
            let err = McpSpec::builder("x")
                .owner("myapp")
                .http(bad)
                .try_build()
                .unwrap_err();
            assert!(
                matches!(err, AgentConfigError::Other(_)),
                "expected invalid URL for {bad:?}"
            );
        }
    }

    #[test]
    fn mcp_try_build_rejects_invalid_env_names_and_values() {
        for key in ["", "BAD=NAME", "BAD\nNAME"] {
            let err = McpSpec::builder("x")
                .owner("myapp")
                .stdio("cmd", Vec::<String>::new())
                .env(key, "value")
                .try_build()
                .unwrap_err();
            assert!(
                matches!(err, AgentConfigError::Other(_)),
                "expected invalid env key for {key:?}"
            );
        }

        let err = McpSpec::builder("x")
            .owner("myapp")
            .stdio("cmd", Vec::<String>::new())
            .env("GOOD_NAME", "line\nbreak")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn mcp_try_build_rejects_invalid_header_names_and_values() {
        for key in ["", "Bad Header", "Bad:Header", "Bad\nHeader"] {
            let err = McpSpec::builder("x")
                .owner("myapp")
                .http("https://example.com/mcp")
                .header(key, "value")
                .try_build()
                .unwrap_err();
            assert!(
                matches!(err, AgentConfigError::Other(_)),
                "expected invalid header key for {key:?}"
            );
        }

        let err = McpSpec::builder("x")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Authorization", "line\nbreak")
            .try_build()
            .unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }
}
