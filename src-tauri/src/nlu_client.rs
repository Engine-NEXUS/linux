//! NLU client — in-process BERT-Mini intent classification (see nlu_local.rs).
//!
//! Through 2026-10-02 this called a lazy-started Python sidecar over HTTP.
//! That added a process-spawn + network round-trip (and up to 15s of cold
//! start) to every transcript the deterministic parser missed — the actual
//! mechanism behind "it always thinks instead of executing" for anything
//! not matching an exact literal phrase (docs/research/ghost-mode/07).
//! `nlu_local::parse_local` runs the same ONNX model + tokenizer in-process
//! via tract-onnx: no subprocess, no network, single-digit milliseconds.
//!
//! If the model files aren't present (e.g. a fresh dev checkout with no
//! trained model yet) this module returns None and the caller falls back
//! to the deterministic parser or unknown intent, exactly as it did when
//! the old sidecar was unreachable.

use crate::intent_parser::{ParseResult, ParsedIntent};

/// Feature 99 P4: semantic-anchor sanity check for high-confidence
/// classifications. BERT-Mini confidently maps ambient chatter onto
/// learned intents (e.g. "So you have to list" → media_play_pause @0.90).
/// For each guarded family the transcript must contain at least one
/// anchor word, or the classification is rejected (→ None) so the turn
/// can fall through to the admin brain / Worker instead of misfiring.
/// Pure + unit-tested.
pub(crate) fn nlu_sanity_check(intent: &str, transcript: &str) -> bool {
    let t = transcript.to_lowercase();
    let anchors: &[&str] = match intent {
        // Media family — the documented hallucination vector.
        "media_play_pause" | "media_next" | "media_previous" | "media_stop" => &[
            "play", "pause", "track", "music", "song", "video", "media", "next",
            "previous", "spotify", "resume", "last",
        ],
        _ => return true, // other families are slot-gated or safe
    };
    anchors.iter().any(|a| t.contains(a))
}

/// Parse a transcript via the in-process NLU model.
///
/// Returns None if the model isn't loaded or confidence is too low.
/// Returns Some(ParseResult) if a valid classification is produced.
pub async fn parse_via_nlu(transcript: &str) -> Option<ParseResult> {
    let text = transcript.to_string();
    // Inference is CPU-bound (ONNX forward pass) — run it off the async
    // executor thread so a slow classification never stalls other turns.
    let nlu = tokio::task::spawn_blocking(move || crate::nlu_local::parse_local(&text))
        .await
        .ok()
        .flatten()?;

    if nlu.confidence < 0.85 {
        tracing::info!(
            "[nlu_client] rejected low-confidence intent '{}' ({:.3})",
            nlu.intent,
            nlu.confidence
        );
        return None;
    }

    // Feature 99 P4: confident-hallucination guard — the transcript must
    // share a semantic anchor with the classified intent family.
    if !nlu_sanity_check(&nlu.intent, transcript) {
        tracing::info!(
            "[nlu_client] sanity guard rejected intent '{}' ({:.3}) — no semantic anchor in {:?}",
            nlu.intent,
            nlu.confidence,
            transcript
        );
        return None;
    }

    // Convert NLU response to ParsedIntent
    let Some(intent) = nlu_to_parsed_intent(&nlu.intent, &nlu.slots, transcript) else {
        return None;
    };

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
/// Handles all intent labels (ParsedIntent + GitHubCommand variants).
/// `raw` is the original transcript — used for whole-text prompt slots
/// (screen_analysis) where the NLU does no slot extraction.
pub fn nlu_to_parsed_intent(intent: &str, slots: &serde_json::Value, raw: &str) -> Option<ParsedIntent> {
    match intent {
        // ─── Feature 86: spatial screen analysis ───
        "screen_analysis" => {
            // The whole transcript IS the prompt (no slot extraction).
            Some(ParsedIntent::NluResult {
                intent: "screen_analysis".to_string(),
                slots: serde_json::json!({ "prompt": raw.trim() }),
                confidence: 1.0,
            })
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Feature 99 P4: media-family hallucinations without any media anchor
    /// are rejected; anchored transcripts pass; non-media intents are never
    /// gated.
    #[test]
    fn nlu_sanity_check_gates_media_family() {
        // The documented failure: ambient chatter → media_play_pause @0.90.
        assert!(!nlu_sanity_check("media_play_pause", "So you have to list"));
        assert!(!nlu_sanity_check("media_next", "what do you think"));
        assert!(!nlu_sanity_check("media_stop", "and then he goes home"));
        // Anchored media commands pass.
        assert!(nlu_sanity_check("media_play_pause", "pause the music"));
        assert!(nlu_sanity_check("media_next", "next track please"));
        assert!(nlu_sanity_check("media_play_pause", "play the video"));
        assert!(nlu_sanity_check("media_stop", "stop the song on spotify"));
        // Non-media intents are never gated.
        assert!(nlu_sanity_check("search", "So you have to list"));
        assert!(nlu_sanity_check("open_app", "anything at all"));
    }
}
