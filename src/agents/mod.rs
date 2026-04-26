//! Per-harness [`Integration`](crate::Integration) implementations.
//!
//! Each submodule is independent. Adding a harness is: create a new file,
//! implement [`Integration`](crate::Integration), and register it in
//! [`crate::registry::all`].

pub mod amp;
pub mod antigravity;
pub mod claude;
pub mod cline;
pub mod codebuddy;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod forge;
pub mod gemini;
pub mod hermes;
pub mod iflow;
pub mod junie;
pub mod kilocode;
pub mod openclaw;
pub mod opencode;
mod planning;
#[allow(dead_code)]
mod prompt;
pub mod qodercli;
pub mod qwen;
pub mod roo;
pub mod tabnine;
pub mod trae;
pub mod windsurf;

pub use amp::AmpAgent;
pub use antigravity::AntigravityAgent;
pub use claude::ClaudeAgent;
pub use cline::ClineAgent;
pub use codebuddy::CodeBuddyAgent;
pub use codex::CodexAgent;
pub use copilot::CopilotAgent;
pub use cursor::CursorAgent;
pub use forge::ForgeAgent;
pub use gemini::GeminiAgent;
pub use hermes::HermesAgent;
pub use iflow::IFlowAgent;
pub use junie::JunieAgent;
pub use kilocode::KiloCodeAgent;
pub use openclaw::OpenClawAgent;
pub use opencode::OpenCodeAgent;
pub use qodercli::QoderCliAgent;
pub use qwen::QwenAgent;
pub use roo::RooAgent;
pub use tabnine::TabnineAgent;
pub use trae::TraeAgent;
pub use windsurf::WindsurfAgent;
