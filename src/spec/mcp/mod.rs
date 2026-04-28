//! MCP server spec, builder, and transport enum.
//!
//! Layout: this module is a directory split into `transport.rs` (the
//! [`McpTransport`] enum, transport-shape validation, and secret-detection
//! helpers) and `builder.rs` (the fluent [`McpSpecBuilder`]). The flat
//! [`McpSpec`], [`SecretPolicy`], [`McpTransport`], and [`McpSpecBuilder`]
//! re-exports below are the public surface; the parent `spec` module
//! re-exports them again at `crate::spec::*`.

use crate::error::AgentConfigError;
use crate::scope::{Scope, ScopeKind};

use super::validate::{validate_identifier, IdentifierKind};

mod builder;
mod transport;

pub use builder::McpSpecBuilder;
pub use transport::McpTransport;

use transport::{is_inline_secret_env_value, is_inline_secret_header_value, validate_transport};

/// Caller-supplied description of an MCP server to register with a harness.
///
/// MCP servers are keyed by [`name`](Self::name) (the literal string the harness
/// uses to load the server), not by an arbitrary tag. To support multi-consumer
/// coexistence the library records ownership in a sidecar ledger
/// (`<config-dir>/.agent-config-mcp.json`) keyed by name → `owner_tag`. Removing a
/// server owned by a different consumer (or by a hand-edit) returns
/// [`AgentConfigError::NotOwnedByCaller`].
///
/// Build via [`McpSpec::builder`]. For fallible construction see
/// [`McpSpecBuilder::try_build`].
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

    /// When true, an install that finds an entry already present in the
    /// harness config but absent from the ownership ledger will adopt that
    /// entry under this spec's `owner_tag` instead of refusing. Use after a
    /// crash between config write and ledger record (`InstallStatus::PresentUnowned`).
    /// Default `false`: adoption is opt-in to avoid silently taking over a
    /// hand-installed entry the user may want to keep separate.
    pub adopt_unowned: bool,
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
            adopt_unowned: false,
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

    /// Returns the first env var or HTTP/SSE header that looks secret-bearing
    /// when the install scope is local.
    pub(crate) fn local_inline_secret_key(&self, scope: &Scope) -> Option<&str> {
        if scope.kind() != ScopeKind::Local {
            return None;
        }
        match &self.transport {
            McpTransport::Stdio { env, .. } => env
                .iter()
                .find(|(key, value)| is_inline_secret_env_value(key, value))
                .map(|(key, _)| key.as_str()),
            McpTransport::Http { headers, .. } | McpTransport::Sse { headers, .. } => headers
                .iter()
                .find(|(key, value)| is_inline_secret_header_value(key, value))
                .map(|(key, _)| key.as_str()),
        }
    }

    /// Returns the first env or header key refused by the current secret policy.
    pub(crate) fn refused_local_inline_secret_key(&self, scope: &Scope) -> Option<&str> {
        if self.secret_policy == SecretPolicy::RefuseInlineSecretsInLocalScope {
            self.local_inline_secret_key(scope)
        } else {
            None
        }
    }

    /// Returns the first env or header key allowed by explicit override.
    pub(crate) fn allowed_local_inline_secret_key(&self, scope: &Scope) -> Option<&str> {
        if self.secret_policy == SecretPolicy::AllowInlineSecretsInLocalScope {
            self.local_inline_secret_key(scope)
        } else {
            None
        }
    }
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
            .local_inline_secret_key(&Scope::Local("/tmp/project".into()))
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

        assert_eq!(spec.local_inline_secret_key(&local), Some("GITHUB_TOKEN"));
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
    fn local_inline_secret_policy_detects_http_authorization_header() {
        let local = Scope::Local("/tmp/project".into());
        let global = Scope::Global;
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Authorization", "Bearer xyz")
            .build();

        assert_eq!(spec.local_inline_secret_key(&local), Some("Authorization"));
        assert!(spec.validate_local_secret_policy(&global).is_ok());
        assert!(matches!(
            spec.validate_local_secret_policy(&local),
            Err(AgentConfigError::InlineSecretInLocalScope { key, .. }) if key == "Authorization"
        ));
    }

    #[test]
    fn local_inline_secret_policy_detects_sse_x_api_key_header() {
        let local = Scope::Local("/tmp/project".into());
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .sse("https://example.com/sse")
            .header("X-API-Key", "abc123")
            .build();

        assert_eq!(spec.local_inline_secret_key(&local), Some("X-API-Key"));
        assert!(matches!(
            spec.validate_local_secret_policy(&local),
            Err(AgentConfigError::InlineSecretInLocalScope { key, .. }) if key == "X-API-Key"
        ));
    }

    #[test]
    fn local_inline_secret_policy_detects_cookie_header() {
        let local = Scope::Local("/tmp/project".into());
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Cookie", "session=opaque")
            .build();

        assert_eq!(spec.local_inline_secret_key(&local), Some("Cookie"));
        assert!(matches!(
            spec.validate_local_secret_policy(&local),
            Err(AgentConfigError::InlineSecretInLocalScope { key, .. }) if key == "Cookie"
        ));
    }

    #[test]
    fn local_inline_secret_policy_allows_placeholder_header() {
        let local = Scope::Local("/tmp/project".into());
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Authorization", "${TOKEN}")
            .build();
        assert!(spec.local_inline_secret_key(&local).is_none());
        assert!(spec.validate_local_secret_policy(&local).is_ok());
    }

    #[test]
    fn local_inline_secret_policy_allows_innocuous_header() {
        let local = Scope::Local("/tmp/project".into());
        let spec = McpSpec::builder("remote")
            .owner("myapp")
            .http("https://example.com/mcp")
            .header("Accept", "application/json")
            .build();
        assert!(spec.local_inline_secret_key(&local).is_none());
        assert!(spec.validate_local_secret_policy(&local).is_ok());
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
