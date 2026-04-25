//! Per-harness [`Integration`](crate::Integration) implementations.
//!
//! Each submodule is independent. Adding a harness is: create a new file,
//! implement [`Integration`](crate::Integration), and register it in
//! [`crate::registry::all`].

pub mod antigravity;
pub mod claude;
pub mod cline;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod gemini;
pub mod opencode;
pub mod prompt;
pub mod windsurf;

pub use antigravity::AntigravityAgent;
pub use claude::ClaudeAgent;
pub use cline::ClineAgent;
pub use codex::CodexAgent;
pub use copilot::CopilotAgent;
pub use cursor::CursorAgent;
pub use gemini::GeminiAgent;
pub use opencode::OpenCodeAgent;
pub use prompt::PromptAgent;
pub use windsurf::WindsurfAgent;
