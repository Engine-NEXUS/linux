//! NLU client — calls the Python NLU server (BERT-Mini) for intent classification.
//!
//! The NLU server is a lazy-started Python sidecar (like the STT server).
//! It loads a BERT-Mini ONNX model and provides a /parse endpoint that
//! returns intent + slots + confidence.
//!
//! If the NLU server is not running or unavailable, this module returns None
//! and the caller falls back to the deterministic parser or unknown intent.

use crate::intent_parser::{ParseResult, ParsedIntent};
use serde::Deserialize;
use std::time::Duration;

/// NLU server port (separate from the old STT sidecar port).
const NLU_PORT: u16 = 39218;

/// NLU server response format.
#[derive(Debug, Deserialize)]
struct NluResponse {
    intent: String,
    slots: serde_json::Value,
    confidence: f32,
}

/// Parse a transcript via the NLU server.
///
/// Returns None if the server is not running or the request fails.
/// Returns Some(ParseResult) if the server returns a valid classification.
pub async fn parse_via_nlu(transcript: &str) -> Option<ParseResult> {
    // Ensure the NLU server is running (lazy-start).
    // Use spawn_blocking because ensure_nlu_running() does std::thread::sleep
    // in a loop (up to 15s) which would block the Tokio worker thread.
    tokio::task::spawn_blocking(|| {
        crate::lazy_nlu::ensure_nlu_running();
    })
    .await
    .ok()?;

    let url = format!("http://127.0.0.1:{}/parse", NLU_PORT);

    // Short timeout — if NLU is slow, fall back to deterministic
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .ok()?;

    let response = client
        .post(&url)
        .json(&serde_json::json!({ "text": transcript }))
        .send()
        .await
        .ok()?;

    if !response.status().is_success() {
        tracing::debug!("[nlu_client] server returned non-success status");
        return None;
    }

    let nlu: NluResponse = response.json().await.ok()?;

    // Mark that a request was made (resets idle timer)
    crate::lazy_nlu::mark_nlu_request();

    if nlu.confidence < 0.85 {
        tracing::info!(
            "[nlu_client] rejected low-confidence intent '{}' ({:.3})",
            nlu.intent,
            nlu.confidence
        );
        return None;
    }

    // Convert NLU response to ParsedIntent
    let intent = nlu_to_parsed_intent(&nlu.intent, &nlu.slots)?;

    Some(ParseResult {
        intent,
        confidence: nlu.confidence,
        source: "nlu".to_string(),
    })
}

/// Repo slot with sounding-tolerance: "cervix"/"srvx" resolve to "servx".
/// Same canonical map the deterministic parser uses (`clean_repo_name`),
/// so every category — deterministic, NLU, brain — agrees on the entity.
fn repo_slot(slots: &serde_json::Value) -> String {
    let raw = slots.get("repo").and_then(|v| v.as_str()).unwrap_or("");
    crate::intent_parser::canonical_repo_name(raw)
}

/// Convert NLU server response to ParsedIntent.
/// Handles all 46 intent labels (ParsedIntent + GitHubCommand variants).
pub fn nlu_to_parsed_intent(intent: &str, slots: &serde_json::Value) -> Option<ParsedIntent> {    match intent {
        // ─── Local commands ───
        "open_app" => {
            let target = slots.get("app_name").and_then(|v| v.as_str()).unwrap_or("");
            if target.is_empty() { return None; }
            Some(ParsedIntent::OpenApp { target: target.to_string() })
        }
        "open_url" => {
            let url = slots.get("url").and_then(|v| v.as_str()).unwrap_or("");
            if url.is_empty() { return None; }
            let target = url.to_string();
            Some(ParsedIntent::OpenUrl { target, url: url.to_string() })
        }
        "close_app" => {
            let target = slots.get("app_name").and_then(|v| v.as_str()).unwrap_or("");
            if target.is_empty() { return None; }
            Some(ParsedIntent::CloseApp { target: target.to_string() })
        }
        "whatsapp_chat" => {
            let contact = slots.get("contact").and_then(|v| v.as_str()).unwrap_or("");
            if contact.is_empty() { return None; }
            Some(ParsedIntent::WhatsappChat { contact: contact.to_string() })
        }
        "open_architect" => Some(ParsedIntent::OpenArchitect),
        "open_settings" => Some(ParsedIntent::OpenSettings),
        "search" => {
            let query = slots.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if query.is_empty() { return None; }
            Some(ParsedIntent::Search { query: query.to_string() })
        }
        "media_play_pause" => Some(ParsedIntent::MediaPlayPause),
        "media_next" => Some(ParsedIntent::MediaNext),
        "media_previous" => Some(ParsedIntent::MediaPrevious),
        "media_stop" => Some(ParsedIntent::MediaStop),
        "greeting" => {
            // Greetings are handled locally by the deterministic parser.
            // If NLU classifies something as greeting, treat as unknown so
            // the orchestrator routes it to the Worker for a conversational reply.
            None
        }

        // ─── Analysis commands ───
        "analyse_repo" => {
            let repo = repo_slot(slots);
            let owner = slots.get("owner").and_then(|v| v.as_str()).map(String::from);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::AnalyseRepo { owner, repo: repo.to_string() })
        }
        "analyse_pr" => {
            let repo = repo_slot(slots);
            let owner = slots.get("owner").and_then(|v| v.as_str()).map(String::from);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::AnalysePr { owner, repo: repo.to_string(), pr_number })
        }
        "analyse_latest_pr" => {
            let repo = repo_slot(slots);
            let owner = slots.get("owner").and_then(|v| v.as_str()).map(String::from);
            let author = slots.get("author").and_then(|v| v.as_str()).map(String::from);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::AnalyseLatestPr { owner, repo: repo.to_string(), author })
        }
        "check_branch" => {
            let repo = repo_slot(slots);
            let owner = slots.get("owner").and_then(|v| v.as_str()).map(String::from);
            let author = slots.get("author").and_then(|v| v.as_str()).map(String::from);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::CheckBranch { owner, repo: repo.to_string(), author })
        }

        // ─── GitHub PR operations ───
        "merge_pr" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::MergePr {
                    repo: repo.to_string(),
                    pr_number,
                    method: crate::github_cmd::MergeMethod::Squash,
                },
            })
        }
        "approve_pr" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ApprovePr {
                    repo: repo.to_string(),
                    pr_number,
                },
            })
        }
        "close_pr" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ClosePr {
                    repo: repo.to_string(),
                    pr_number,
                },
            })
        }
        "list_prs" => {
            let repo = repo_slot(slots);
            // Read state from slots if provided (open/closed/all/merged)
            // Default to "open" if not specified.
            let raw_state = slots.get("state").and_then(|v| v.as_str()).unwrap_or("open");
            let state = match raw_state.to_lowercase().as_str() {
                "open" | "live" | "active" | "latest" => "open",
                "closed" | "merged" => "closed",
                "all" => "all",
                _ => "open",
            }.to_string();
            // If no repo in slots, try auto-detection (browser URL, clipboard, etc.)
            // If that fails too, use empty repo for account-wide PR search.
            let repo = if repo.is_empty() {
                match crate::architect::get_active_repo_url() {
                    Some(repo_id) => format!("{}/{}", repo_id.owner, repo_id.repo),
                    None => String::new(),
                }
            } else {
                repo.to_string()
            };
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListPrs {
                    repo,
                    state,
                },
            })
        }
        "get_pr" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::GetPr {
                    repo: repo.to_string(),
                    pr_number,
                },
            })
        }
        "update_branch" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::UpdateBranch {
                    repo: repo.to_string(),
                    pr_number,
                },
            })
        }
        "revert_pr" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::RevertPr {
                    repo: repo.to_string(),
                    pr_number,
                    title: None,
                },
            })
        }
        "list_pr_files" => {
            let repo = repo_slot(slots);
            let pr_number = slots.get("pr_number").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || pr_number == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListPrFiles {
                    repo: repo.to_string(),
                    pr_number,
                },
            })
        }

        // ─── GitHub collaborator/org ───
        "add_collaborator" => {
            let repo = repo_slot(slots);
            let username = slots.get("username").and_then(|v| v.as_str()).unwrap_or("");
            if repo.is_empty() || username.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::AddCollaborator {
                    repo: repo.to_string(),
                    username: username.to_string(),
                    permission: crate::github_cmd::CollaboratorPermission::Push,
                },
            })
        }
        "remove_collaborator" => {
            let repo = repo_slot(slots);
            let username = slots.get("username").and_then(|v| v.as_str()).unwrap_or("");
            if repo.is_empty() || username.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::RemoveCollaborator {
                    repo: repo.to_string(),
                    username: username.to_string(),
                },
            })
        }
        "list_collaborators" => {
            let repo = repo_slot(slots);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListCollaborators {
                    repo: repo.to_string(),
                },
            })
        }
        "add_org_member" => {
            let org = slots.get("org").and_then(|v| v.as_str()).unwrap_or("");
            let username = slots.get("username").and_then(|v| v.as_str()).unwrap_or("");
            if org.is_empty() || username.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::AddOrgMember {
                    org: org.to_string(),
                    username: username.to_string(),
                    role: crate::github_cmd::OrgRole::Member,
                },
            })
        }
        "remove_org_member" => {
            let org = slots.get("org").and_then(|v| v.as_str()).unwrap_or("");
            let username = slots.get("username").and_then(|v| v.as_str()).unwrap_or("");
            if org.is_empty() || username.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::RemoveOrgMember {
                    org: org.to_string(),
                    username: username.to_string(),
                },
            })
        }
        "list_org_members" => {
            let org = slots.get("org").and_then(|v| v.as_str()).unwrap_or("");
            if org.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListOrgMembers {
                    org: org.to_string(),
                },
            })
        }

        // ─── GitHub branch/release/workflow ───
        "delete_branch" => {
            let repo = repo_slot(slots);
            let branch = slots.get("branch").and_then(|v| v.as_str()).unwrap_or("");
            if repo.is_empty() || branch.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::DeleteBranch {
                    repo: repo.to_string(),
                    branch: branch.to_string(),
                },
            })
        }
        "list_branches" => {
            let repo = repo_slot(slots);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListBranches {
                    repo: repo.to_string(),
                },
            })
        }
        "list_releases" => {
            let repo = repo_slot(slots);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListReleases {
                    repo: repo.to_string(),
                },
            })
        }
        "list_workflows" => {
            let repo = repo_slot(slots);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListWorkflows {
                    repo: repo.to_string(),
                },
            })
        }
        "list_workflow_runs" => {
            let repo = repo_slot(slots);
            if repo.is_empty() { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::ListWorkflowRuns {
                    repo: repo.to_string(),
                    workflow_file: None,
                },
            })
        }
        "rerun_workflow" => {
            let repo = repo_slot(slots);
            let run_id = slots.get("workflow_id").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || run_id == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::RerunWorkflow {
                    repo: repo.to_string(),
                    run_id,
                },
            })
        }
        "cancel_workflow" => {
            let repo = repo_slot(slots);
            let run_id = slots.get("workflow_id").and_then(|v| v.as_u64()).unwrap_or(0);
            if repo.is_empty() || run_id == 0 { return None; }
            Some(ParsedIntent::GitHubCommand {
                command: crate::github_cmd::GitHubCommand::CancelWorkflow {
                    repo: repo.to_string(),
                    run_id,
                },
            })
        }

        // create_pr, comment_pr, create_release — need complex slot extraction
        // that's better handled by the deterministic parser. Return None so
        // the caller falls back to Unknown → Worker.
        "create_pr" | "comment_pr" | "create_release" => None,

        // ─── Live mode commands (11) ───
        // These use the generic NluResult wrapper so the orchestrator can
        // route them to the live command executor.
        "type_text" | "press_key" | "press_hotkey" | "confirm_send" |
        "cancel_action" | "browser_new_tab" | "browser_navigate" |
        "browser_search" | "whatsapp_open" | "whatsapp_search" |
        "focus_app" => {
            Some(ParsedIntent::NluResult {
                intent: intent.to_string(),
                slots: slots.clone(),
                confidence: 0.85,
            })
        }

        // ─── Commerce + social MCP commands (3) ───
        // These route to Subsystem::Mcp — the deterministic parser is the
        // primary path; these mappings let NLU/brain fallback reach MCP too.
        "order_food" => {
            // NLU slot is "food_item"; accept "query" too (brain phrasing)
            let query = slots.get("food_item").and_then(|v| v.as_str())
                .or_else(|| slots.get("query").and_then(|v| v.as_str()))
                .unwrap_or("");
            let restaurant = slots.get("restaurant").and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from);
            Some(ParsedIntent::OrderFood { query: query.to_string(), restaurant })
        }
        "search_product" => {
            let query = slots.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if query.is_empty() { return None; }
            Some(ParsedIntent::SearchProduct { query: query.to_string() })
        }
        "send_whatsapp_message" => {
            let contact = slots.get("contact").and_then(|v| v.as_str()).unwrap_or("");
            let message = slots.get("message").and_then(|v| v.as_str()).unwrap_or("");
            if contact.is_empty() || message.is_empty() { return None; }
            Some(ParsedIntent::SendWhatsAppMessage {
                contact: contact.to_string(),
                message: message.to_string(),
            })
        }

        // unknown or unrecognized
        _ => None,
    }
}

/// Check if the NLU server is running.
#[allow(dead_code)]
pub async fn is_nlu_available() -> bool {
    let url = format!("http://127.0.0.1:{}/health", NLU_PORT);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}
