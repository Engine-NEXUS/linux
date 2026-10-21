//! Coder: Universal Autonomous Coding Agent Orchestrator & Grounding.
//!
//! Provides workspace and screen grounding ("run this on localhost")
//! and universal adaptation across CLI agents (`antigravity`, `claude`, `aider`)
//! and AI IDEs (`Cursor`, `Trae`, `OpenCode`, `Windsurf`, `VS Code`).

pub mod agents;
pub mod grounding;

pub use agents::{build_autonomous_prompt, dispatch_to_agent, CodingAgent, Dispatch};
pub use grounding::{detect_active_project, launch_localhost};
