//! `McpTransport` enum, transport-shape validators, and secret-detection
//! helpers for project-local inline-secret policy.

use std::collections::BTreeMap;

use fluent_uri::Uri;

use crate::error::AgentConfigError;

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

pub(super) fn validate_transport(transport: &McpTransport) -> Result<(), AgentConfigError> {
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

pub(super) fn is_inline_secret_env_value(name: &str, value: &str) -> bool {
    likely_secret_env_name(name) && !value.trim().is_empty() && !is_placeholder_value(value)
}

pub(super) fn is_inline_secret_header_value(name: &str, value: &str) -> bool {
    likely_secret_header_name(name) && !value.trim().is_empty() && !is_placeholder_value(value)
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

fn likely_secret_header_name(name: &str) -> bool {
    // Reuse env-name keywords (TOKEN/SECRET/KEY/PASSWORD/AUTH/BEARER/CREDENTIAL),
    // which already match `Authorization`, `Proxy-Authorization`, `X-API-Key`,
    // `X-Auth-Token`, etc. Add `COOKIE` since session cookies do not match the
    // env-style keyword set.
    if likely_secret_env_name(name) {
        return true;
    }
    name.to_ascii_uppercase().contains("COOKIE")
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
