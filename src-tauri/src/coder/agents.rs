//! Universal Coding Agent Adapter & Prompt Engineer.
//!
//! Interfaces with CLI agents (`antigravity`, `claude`, `aider`, `codex`)
//! and AI IDEs (`Cursor`, `Trae`, `OpenCode`, `Windsurf`, `VS Code`).
//! Generates structured, high-context autonomous coding prompts.

use std::path::Path;
use super::grounding::ProjectContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodingAgent {
    OpenCode,
    Trae,
    Cursor,
    Windsurf,
    VSCode,
    Antigravity,
    Claude,
    Aider,
    Generic(String),
}

impl CodingAgent {
    pub fn from_str(s: &str) -> Self {
        let norm = s.trim().to_lowercase();
        if norm.contains("claude") {
            Self::Claude
        } else if norm.contains("antigravity") || norm.contains("agy") {
            Self::Antigravity
        } else if norm.contains("aider") {
            Self::Aider
        } else if norm.contains("opencode") {
            Self::OpenCode
        } else if norm.contains("trae") {
            Self::Trae
        } else if norm.contains("cursor") {
            Self::Cursor
        } else if norm.contains("windsurf") {
            Self::Windsurf
        } else if norm.contains("vscode") || norm == "code" || norm.contains("vs code") {
            Self::VSCode
        } else {
            Self::Generic(s.to_string())
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::OpenCode => "OpenCode",
            Self::Trae => "Trae",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::VSCode => "VS Code",
            Self::Antigravity => "Antigravity",
            Self::Claude => "Claude Code",
            Self::Aider => "Aider",
            Self::Generic(name) => name.as_str(),
        }
    }
}

/// Build a rigorous, fully-specified engineering prompt for autonomous coding agents.
pub fn build_autonomous_prompt(
    task_goal: &str,
    project_context: Option<&ProjectContext>,
) -> String {
    let project_info = match project_context {
        Some(ctx) => format!(
            "Target Project: {} ({:?})\nProject Type: {:?}\nDev Command: {}\n",
            ctx.name, ctx.path, ctx.project_type, ctx.dev_command
        ),
        None => "Target Project: Current Workspace\n".to_string(),
    };

    format!(
r#"# Autonomous Engineering Objective: {}

## Context & Workspace
{}
## Core Requirements & Constraints
1. Quality System & Integrity: Preserve all existing functionality, verify with compiler and test suite, avoid regressions.
2. Architecture & Design: Keep code modular, type-safe, and cleanly separated into cohesive files.
3. Verification: Add exhaustive unit tests and mutation checks for any new branches or logic.

## Execution Plan
1. Research & Analysis: Review relevant existing modules, dependencies, and patterns.
2. Implementation: Build requested features step-by-step.
3. Validation: Run cargo test, linters, or appropriate test harness to ensure clean pass.
"#,
        task_goal, project_info
    )
}

/// What actually happened to the task, so the spoken reply can say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    /// A CLI agent was started with the prompt (its process id).
    Started(u32),
    /// The agent cannot be driven from here: the specification is now on the
    /// clipboard for the user to paste.
    Copied,
}

/// How each agent can really be reached. Only agents whose CLI accepts the
/// prompt as an argument are started; the rest get the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Spawn(&'static str, &'static str),
    Clipboard,
}

pub fn reach(agent: &CodingAgent) -> Reach {
    match agent {
        CodingAgent::Claude => Reach::Spawn("claude", "-p"),
        CodingAgent::Aider => Reach::Spawn("aider", "--message"),
        // Antigravity's CLI flags are not verified, and the IDEs have no CLI
        // that takes a task: never pretend to have sent them anything.
        _ => Reach::Clipboard,
    }
}

/// Dispatch a prompt to a target coding agent.
pub fn dispatch_to_agent(
    agent: &CodingAgent,
    prompt: &str,
    project_dir: Option<&Path>,
) -> Result<Dispatch, String> {
    match reach(agent) {
        Reach::Spawn(exe, flag) => {
            let mut cmd = std::process::Command::new(exe);
            cmd.arg(flag).arg(prompt);
            if let Some(dir) = project_dir {
                cmd.current_dir(dir);
            }
            let child = cmd
                .spawn()
                .map_err(|e| format!("I couldn't start {} ({e})", agent.display_name()))?;
            Ok(Dispatch::Started(child.id()))
        }
        Reach::Clipboard => {
            arboard::Clipboard::new()
                .and_then(|mut c| c.set_text(prompt.to_string()))
                .map_err(|e| format!("I couldn't copy the specification ({e})"))?;
            Ok(Dispatch::Copied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coding_agent_resolution() {
        assert_eq!(CodingAgent::from_str("claude code"), CodingAgent::Claude);
        assert_eq!(CodingAgent::from_str("agy"), CodingAgent::Antigravity);
        assert_eq!(CodingAgent::from_str("cursor ide"), CodingAgent::Cursor);
        assert_eq!(CodingAgent::from_str("trae"), CodingAgent::Trae);
        assert_eq!(CodingAgent::from_str("opencode"), CodingAgent::OpenCode);
    }

    #[test]
    fn only_agents_with_a_prompt_flag_are_spawned() {
        assert_eq!(reach(&CodingAgent::Claude), Reach::Spawn("claude", "-p"));
        assert_eq!(reach(&CodingAgent::Aider), Reach::Spawn("aider", "--message"));
        // These used to be reported as "dispatched" although nothing was sent.
        for a in [
            CodingAgent::Antigravity,
            CodingAgent::Cursor,
            CodingAgent::Trae,
            CodingAgent::OpenCode,
            CodingAgent::Windsurf,
            CodingAgent::VSCode,
            CodingAgent::Generic("x".into()),
        ] {
            assert_eq!(reach(&a), Reach::Clipboard, "{a:?}");
        }
    }

    #[test]
    fn test_build_autonomous_prompt() {
        let prompt = build_autonomous_prompt("Build REST API", None);
        assert!(prompt.contains("Autonomous Engineering Objective: Build REST API"));
        assert!(prompt.contains("Quality System & Integrity"));
    }
}
