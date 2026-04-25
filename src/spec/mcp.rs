//! MCP server spec, builder, and transport enum.

use std::collections::BTreeMap;

use crate::error::HookerError;

use super::validate::{validate_identifier, IdentifierKind};

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
    /// Windsurf), the `servers` key (Copilot), the object-based `mcp` map
    /// (OpenCode/Kilo), or the table name `[mcp_servers.<name>]` (Codex).
    /// ASCII alnum/`_`/`-`, non-empty.
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
    /// set as [`HookSpec::tag`](crate::HookSpec::tag).
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
