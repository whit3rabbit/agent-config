//! Per-harness [`Integration`](crate::Integration) implementations.
//!
//! Each submodule is independent. Adding a harness is: create a new file,
//! implement [`Integration`](crate::Integration), and register it in
//! [`crate::registry::all`].

pub(crate) mod amp;
pub(crate) mod antigravity;
pub(crate) mod claude;
pub(crate) mod cline;
pub(crate) mod codebuddy;
pub(crate) mod codex;
pub(crate) mod copilot;
pub(crate) mod crush;
pub(crate) mod cursor;
pub(crate) mod forge;
pub(crate) mod gemini;
pub(crate) mod hermes;
pub(crate) mod iflow;
pub(crate) mod junie;
pub(crate) mod kilocode;
pub(crate) mod openclaw;
pub(crate) mod opencode;
pub(crate) mod pi;
mod planning;
#[allow(dead_code)]
mod prompt;
pub(crate) mod qodercli;
pub(crate) mod qwen;
pub(crate) mod roo;
pub(crate) mod tabnine;
pub(crate) mod trae;
pub(crate) mod windsurf;

pub use amp::AmpAgent;
pub use antigravity::AntigravityAgent;
pub use claude::ClaudeAgent;
pub use cline::ClineAgent;
pub use codebuddy::CodeBuddyAgent;
pub use codex::CodexAgent;
pub use copilot::CopilotAgent;
pub use crush::CrushAgent;
pub use cursor::CursorAgent;
pub use forge::ForgeAgent;
pub use gemini::GeminiAgent;
pub use hermes::HermesAgent;
pub use iflow::IFlowAgent;
pub use junie::JunieAgent;
pub use kilocode::KiloCodeAgent;
pub use openclaw::OpenClawAgent;
pub use opencode::OpenCodeAgent;
pub use pi::PiAgent;
pub use qodercli::QoderCliAgent;
pub use qwen::QwenAgent;
pub use roo::RooAgent;
pub use tabnine::TabnineAgent;
pub use trae::TraeAgent;
pub use windsurf::WindsurfAgent;
