//! Agent-context detection, ported from `@vercel/detect-agent` 1.2.7 plus the
//! upstream CLI's refinement and name mapping.

use std::sync::OnceLock;

/// The result of detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentResult {
    /// Whether the CLI appears to run inside a coding agent.
    pub is_agent: bool,
    /// Detected agent name (cursor, claude, codex, ...).
    pub agent: Option<String>,
}

fn cached() -> &'static AgentResult {
    static CACHE: OnceLock<AgentResult> = OnceLock::new();
    CACHE.get_or_init(determine_agent)
}

fn has_strong_cursor_agent_signal() -> bool {
    std::env::var("CURSOR_AGENT").ok().is_some_and(|v| !v.trim().is_empty())
        || std::env::var("CURSOR_EXTENSION_HOST_ROLE").as_deref() == Ok("agent-exec")
}

fn determine_agent() -> AgentResult {
    if let Ok(name) = std::env::var("AI_AGENT") {
        let name = name.trim().to_string();
        if !name.is_empty() {
            return AgentResult { is_agent: true, agent: Some(name) };
        }
    }
    if std::env::var_os("CURSOR_TRACE_ID").is_some() {
        return AgentResult { is_agent: true, agent: Some("cursor".into()) };
    }
    if std::env::var_os("CURSOR_AGENT").is_some()
        || std::env::var("CURSOR_EXTENSION_HOST_ROLE").as_deref() == Ok("agent-exec")
    {
        return AgentResult { is_agent: true, agent: Some("cursor-cli".into()) };
    }
    if std::env::var_os("GEMINI_CLI").is_some() {
        return AgentResult { is_agent: true, agent: Some("gemini".into()) };
    }
    if std::env::var_os("CODEX_SANDBOX").is_some()
        || std::env::var_os("CODEX_CI").is_some()
        || std::env::var_os("CODEX_THREAD_ID").is_some()
    {
        return AgentResult { is_agent: true, agent: Some("codex".into()) };
    }
    if std::env::var_os("ANTIGRAVITY_AGENT").is_some() {
        return AgentResult { is_agent: true, agent: Some("antigravity".into()) };
    }
    if std::env::var_os("AUGMENT_AGENT").is_some() {
        return AgentResult { is_agent: true, agent: Some("augment-cli".into()) };
    }
    if std::env::var("OPENCODE").as_deref() == Ok("1") || std::env::var_os("OPENCODE_CLIENT").is_some()
    {
        return AgentResult { is_agent: true, agent: Some("opencode".into()) };
    }
    if std::env::var_os("CLAUDECODE").is_some() || std::env::var_os("CLAUDE_CODE").is_some() {
        if std::env::var_os("CLAUDE_CODE_IS_COWORK").is_some() {
            return AgentResult { is_agent: true, agent: Some("cowork".into()) };
        }
        return AgentResult { is_agent: true, agent: Some("claude".into()) };
    }
    if std::env::var_os("REPL_ID").is_some() {
        return AgentResult { is_agent: true, agent: Some("replit".into()) };
    }
    if std::env::var_os("COPILOT_MODEL").is_some()
        || std::env::var_os("COPILOT_ALLOW_ALL").is_some()
        || std::env::var_os("COPILOT_GITHUB_TOKEN").is_some()
    {
        return AgentResult { is_agent: true, agent: Some("github-copilot".into()) };
    }
    if std::path::Path::new("/opt/.devin").exists() {
        return AgentResult { is_agent: true, agent: Some("devin".into()) };
    }
    AgentResult { is_agent: false, agent: None }
}

/// Detect (and refine) whether we run inside an agent; results are cached.
#[must_use]
pub fn detect_agent() -> AgentResult {
    let result = cached().clone();
    refine_agent_result(result)
}

fn refine_agent_result(result: AgentResult) -> AgentResult {
    if !result.is_agent {
        return result;
    }
    let name = result.agent.clone().unwrap_or_default();
    if name == "cursor" || name == "cursor-cli" {
        if !has_strong_cursor_agent_signal() {
            return AgentResult { is_agent: false, agent: None };
        }
        if name == "cursor" {
            return AgentResult { is_agent: true, agent: Some("cursor-cli".into()) };
        }
    }
    result
}

/// Whether we are running inside a detected AI agent.
#[must_use]
pub fn is_running_in_agent() -> bool {
    detect_agent().is_agent
}

/// Map a detected agent name to a skills-store agent id (upstream
/// `agentNameToType`).
#[must_use]
pub fn get_agent_type(agent_name: &str) -> Option<&'static str> {
    Some(match agent_name {
        "cursor" | "cursor-cli" => "cursor",
        "claude" | "cowork" => "claude-code",
        "devin" => "universal",
        "replit" => "replit",
        "gemini" => "gemini-cli",
        "codex" => "codex",
        "antigravity" => "antigravity",
        "augment-cli" => "augment",
        "opencode" => "opencode",
        "github-copilot" => "github-copilot",
        _ => return None,
    })
}

/// Whether CI is detected (upstream `isCI`).
#[must_use]
pub fn is_ci() -> bool {
    [
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "TRAVIS",
        "BUILDKITE",
        "JENKINS_URL",
        "TEAMCITY_VERSION",
    ]
    .iter()
    .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_type_mapping() {
        assert_eq!(get_agent_type("claude"), Some("claude-code"));
        assert_eq!(get_agent_type("cursor-cli"), Some("cursor"));
        assert_eq!(get_agent_type("unknown"), None);
    }
}
