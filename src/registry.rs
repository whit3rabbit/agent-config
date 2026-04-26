//! Lookup of registered integrations.
//!
//! Consumers call [`all`] to enumerate every supported hooks-capable harness,
//! or [`by_id`] to fetch a single one by stable identifier.
//!
//! For the MCP surface, use [`mcp_capable`] / [`mcp_by_id`]; for the skill
//! surface, [`skill_capable`] / [`skill_by_id`]. Only the agents whose
//! upstream harness exposes that surface appear in those lists.

use crate::integration::{Integration, McpSurface, SkillSurface};

/// Returns a fresh `Box` per integration. The list is the source of truth for
/// what `ai-hooker` supports today; adding a harness means adding one line
/// here.
pub fn all() -> Vec<Box<dyn Integration>> {
    use crate::agents::{
        AmpAgent, AntigravityAgent, ClaudeAgent, ClineAgent, CodexAgent, CopilotAgent, CursorAgent,
        ForgeAgent, GeminiAgent, HermesAgent, JunieAgent, KiloCodeAgent, OpenClawAgent,
        OpenCodeAgent, QoderCliAgent, QwenAgent, RooAgent, WindsurfAgent,
    };
    vec![
        Box::new(ClaudeAgent::new()),
        Box::new(CursorAgent::new()),
        Box::new(GeminiAgent::new()),
        Box::new(OpenClawAgent::new()),
        Box::new(HermesAgent::new()),
        Box::new(CodexAgent::new()),
        Box::new(CopilotAgent::new()),
        Box::new(OpenCodeAgent::new()),
        Box::new(ClineAgent::new()),
        Box::new(RooAgent::new()),
        Box::new(WindsurfAgent::new()),
        Box::new(KiloCodeAgent::new()),
        Box::new(AntigravityAgent::new()),
        Box::new(AmpAgent::new()),
        Box::new(ForgeAgent::new()),
        Box::new(JunieAgent::new()),
        Box::new(QoderCliAgent::new()),
        Box::new(QwenAgent::new()),
    ]
}

/// Returns the integration with this id, or `None` if unrecognised.
pub fn by_id(id: &str) -> Option<Box<dyn Integration>> {
    all().into_iter().find(|i| i.id() == id)
}

/// Returns a fresh `Box` per [`McpSurface`]-capable agent. Adding a new MCP
/// integration means adding one line here.
///
/// Currently: Claude, Cursor, Gemini, OpenClaw, Hermes, Codex, Copilot,
/// OpenCode, Cline, Roo, Windsurf, Kilo Code, Antigravity, Amp, Forge,
/// Junie, Qoder CLI, Qwen Code.
pub fn mcp_capable() -> Vec<Box<dyn McpSurface>> {
    use crate::agents::{
        AmpAgent, AntigravityAgent, ClaudeAgent, ClineAgent, CodexAgent, CopilotAgent, CursorAgent,
        ForgeAgent, GeminiAgent, HermesAgent, JunieAgent, KiloCodeAgent, OpenClawAgent,
        OpenCodeAgent, QoderCliAgent, QwenAgent, RooAgent, WindsurfAgent,
    };
    vec![
        Box::new(ClaudeAgent::new()),
        Box::new(CursorAgent::new()),
        Box::new(GeminiAgent::new()),
        Box::new(OpenClawAgent::new()),
        Box::new(HermesAgent::new()),
        Box::new(CodexAgent::new()),
        Box::new(CopilotAgent::new()),
        Box::new(OpenCodeAgent::new()),
        Box::new(ClineAgent::new()),
        Box::new(RooAgent::new()),
        Box::new(WindsurfAgent::new()),
        Box::new(KiloCodeAgent::new()),
        Box::new(AntigravityAgent::new()),
        Box::new(AmpAgent::new()),
        Box::new(ForgeAgent::new()),
        Box::new(JunieAgent::new()),
        Box::new(QoderCliAgent::new()),
        Box::new(QwenAgent::new()),
    ]
}

/// Returns the MCP-capable integration with this id, or `None` if the agent
/// does not implement [`McpSurface`].
pub fn mcp_by_id(id: &str) -> Option<Box<dyn McpSurface>> {
    mcp_capable().into_iter().find(|i| i.id() == id)
}

/// Returns a fresh `Box` per [`SkillSurface`]-capable agent. Adding a new
/// skill integration means adding one line here.
///
/// Currently: Claude, Cursor, Gemini, OpenClaw, Hermes, Codex, Copilot,
/// OpenCode, Cline, Windsurf, Kilo Code, Antigravity, Amp, Forge, Qwen Code.
pub fn skill_capable() -> Vec<Box<dyn SkillSurface>> {
    use crate::agents::{
        AmpAgent, AntigravityAgent, ClaudeAgent, ClineAgent, CodexAgent, CopilotAgent, CursorAgent,
        ForgeAgent, GeminiAgent, HermesAgent, KiloCodeAgent, OpenClawAgent, OpenCodeAgent,
        QwenAgent, WindsurfAgent,
    };
    vec![
        Box::new(ClaudeAgent::new()),
        Box::new(CursorAgent::new()),
        Box::new(GeminiAgent::new()),
        Box::new(OpenClawAgent::new()),
        Box::new(HermesAgent::new()),
        Box::new(CodexAgent::new()),
        Box::new(CopilotAgent::new()),
        Box::new(OpenCodeAgent::new()),
        Box::new(ClineAgent::new()),
        Box::new(WindsurfAgent::new()),
        Box::new(KiloCodeAgent::new()),
        Box::new(AntigravityAgent::new()),
        Box::new(AmpAgent::new()),
        Box::new(ForgeAgent::new()),
        Box::new(QwenAgent::new()),
    ]
}

/// Returns the skill-capable integration with this id, or `None` if the
/// agent does not implement [`SkillSurface`].
pub fn skill_by_id(id: &str) -> Option<Box<dyn SkillSurface>> {
    skill_capable().into_iter().find(|i| i.id() == id)
}
