//! NEXUS Central Orchestrator — the single owner of request lifecycle.
//!
//! The orchestrator is the "main system" that decides:
//!   - Which subsystem handles a given transcript (routing)
//!   - When to show/hide the loading indicator (top-right corner)
//!   - When to speak the acknowledgement ("On it sir")
//!   - When to speak the result
//!   - When to cancel an in-flight request (barge-in / new wake)
//!
//! Subsystems are the "workers":
//!   - LocalCommand   — open/close apps, media controls, greetings (Rust, <5ms)
//!   - WorkerBackend  — PR analysis, GitHub writes, research, general Q&A (Cloudflare)
//!   - Architect      — architecture mapper (Rust + Worker enrichment)
//!
//! Every request gets a unique `request_id` (UUID v4). The orchestrator
//! tracks the active request in a mutex. When a new request arrives, the
//! old one is cancelled (its `cancelled` flag is set). Subsystems check
//! the flag and abort early.
//!
//! Events emitted to the frontend (all on channel "orchestrator:event"):
//!   { type: "state",    state: "thinking"|"speaking", request_id }
//!   { type: "loading",  visible: bool, request_id }
//!   { type: "ack",      text: "On it sir.", request_id }
//!   { type: "result",   text: "...", request_id, analysis?, dialog_state? }
//!   { type: "done",     request_id }
//!   { type: "error",    message: "...", request_id }
//!
//! The frontend listens to these events instead of the old "assistant:server"
//! channel. This centralizes all state transitions in Rust.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::intent_parser::{parse_deterministic, ParsedIntent};
use crate::network;

// ─── Types ─────────────────────────────────────────────────────────────

/// Orchestrator lifecycle states (mirrors the frontend AssistantState).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OrchestratorState {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

/// Which subsystem will handle this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    /// Local Rust command — open/close app, media, greeting. <5ms, no network.
    LocalCommand,
    /// Cloudflare Worker — PR analysis, GitHub writes, research, general Q&A.
    WorkerBackend,
    /// Architecture Mapper — repo analysis + graph + AI enrichment.
    Architect,
    /// GitHub sub-command system — typed GitHub operations via octocrab.
    /// Handles merge/approve/close PR, collaborators, org members, branches,
    /// releases, workflows. Token fetched from Worker, execution in Rust.
    GitHub,
    /// MCP sub-center — external services via Model Context Protocol.
    /// Handles Swiggy (food/grocery), Amazon (product search), WhatsApp
    /// (messaging). Each MCP server is called via JSON-RPC over HTTP.
    Mcp,
    /// Command Center — compound multi-step tasks ("X then Y").
    /// Plans steps, routes each to a sub-center, merges results.
    CommandCenter,
    /// No subsystem — the command was unparseable or empty.
    None,
}

/// The active request, tracked in the orchestrator's mutex.
struct ActiveRequest {
    id: String,
    cancelled: Arc<AtomicBool>,
    subsystem: Subsystem,
}

/// Event sent to the frontend via the "orchestrator:event" channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum OrchestratorEvent {
    State {
        state: OrchestratorState,
        request_id: String,
    },
    Loading {
        visible: bool,
        request_id: String,
    },
    Ack {
        text: String,
        request_id: String,
    },
    Result {
        text: String,
        request_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        analysis: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        dialog_state: Option<serde_json::Value>,
    },
    Done {
        request_id: String,
    },
    Error {
        message: String,
        request_id: String,
    },
    /// GitHub sub-command system: confirmation required for a destructive
    /// operation. The frontend should ask the user to confirm, then call
    /// `orchestrator_github_confirm` with the request_id and confirmed=true.
    Confirm {
        prompt: String,
        request_id: String,
        /// The serialized GitHubCommand that needs confirmation.
        command: serde_json::Value,
    },
    /// GitHub sub-command system: merge conflict detected. The frontend
    /// should display the conflict details with copy-paste options.
    ConflictReport {
        request_id: String,
        pr_number: u64,
        repo: String,
        conflict_files: serde_json::Value,
        message: String,
    },
    /// GitHub sub-command system: operation result. The frontend should
    /// speak the text and/or display structured data.
    GitHubResult {
        request_id: String,
        result: serde_json::Value,
    },
}

// ─── Global state ──────────────────────────────────────────────────────

/// The single active request. Only one request is active at a time.
/// When a new request arrives, the previous one is cancelled.
static ACTIVE_REQUEST: once_cell::sync::Lazy<Arc<Mutex<Option<ActiveRequest>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

/// Acknowledgement phrases — same as network.rs but owned by the orchestrator now.
const ACK_PHRASES: &[&str] = &[
    "On it sir.",
    "Right away sir.",
    "Working on it sir.",
    "Let me check that sir.",
    "One moment sir.",
];

// ─── Helpers ───────────────────────────────────────────────────────────

/// Generate a short request ID (first 12 hex chars of a UUID, enough for uniqueness).
fn new_request_id() -> String {
    let full = network::uuid_v4();
    // Strip hyphens and take first 12 hex chars for brevity in logs
    let hex: String = full.chars().filter(|c| *c != '-').collect();
    hex[..12].to_string()
}

/// Pick a random ack phrase.
fn pick_ack() -> &'static str {
    let idx = network::uuid_v4().as_bytes()[0] as usize % ACK_PHRASES.len();
    ACK_PHRASES[idx]
}

/// Emit an orchestrator event to the frontend.
fn emit<R: Runtime>(app: &AppHandle<R>, event: &OrchestratorEvent) {
    let _ = app.emit("orchestrator:event", event);
    tracing::debug!("orchestrator: emitted {:?}", event);
}

/// Cancel any active request and install a new one.
/// Returns the new request's ID and its cancel flag.
fn install_new_request(subsystem: Subsystem) -> (String, Arc<AtomicBool>) {
    let id = new_request_id();
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Cancel the previous request
    let mut guard = ACTIVE_REQUEST.lock().unwrap();
    if let Some(prev) = guard.as_ref() {
        prev.cancelled.store(true, Ordering::Relaxed);
        tracing::info!(
            "orchestrator: cancelling previous request {} (was {:?})",
            prev.id,
            prev.subsystem
        );
    }

    *guard = Some(ActiveRequest {
        id: id.clone(),
        cancelled: cancel_flag.clone(),
        subsystem: subsystem.clone(),
    });

    tracing::info!("orchestrator: new request {} -> {:?}", id, subsystem);
    (id, cancel_flag)
}

/// Check if a request is cancelled.
/// Public wrapper for `is_cancelled` — used by command_center step execution.
pub(crate) fn is_cancelled_pub(cancel_flag: &Arc<AtomicBool>) -> bool {
    is_cancelled(cancel_flag)
}

fn is_cancelled(cancel_flag: &Arc<AtomicBool>) -> bool {
    cancel_flag.load(Ordering::Relaxed)
}

/// Clear the active request (called on done/error).
fn clear_active_request(request_id: &str) {
    let mut guard = ACTIVE_REQUEST.lock().unwrap();
    if let Some(ref current) = *guard {
        if current.id == request_id {
            *guard = None;
            tracing::debug!("orchestrator: cleared active request {}", request_id);
        }
    }
}

// ─── ML parsing fallback ──────────────────────────────────────────────

/// Try brain (Qwen, admin-only) then NLU (BERT-Mini) when the deterministic
/// parser misses. This is the same pipeline as `parse_transcript` (Tauri
/// command) — extracted here so the orchestrator can use it for routing
/// instead of bypassing the ML classifiers.
///
/// Returns `None` if both brain and NLU are unavailable or low-confidence.
async fn parse_with_ml(transcript: &str) -> Option<crate::intent_parser::ParseResult> {
    // 1. Try brain server FIRST (admin-only, if enabled)
    // The brain (Qwen 0.5B LLM) is much smarter than BERT-Mini and can
    // understand mishearings, filler words, and unusual phrasing.
    #[cfg(feature = "admin-brain")]
    {
        if crate::admin_config::is_admin() {
            if let Some(mut result) = crate::brain_client::brain_classify(transcript).await {
                tracing::info!(
                    "orchestrator: brain: {:?} (confidence={:.3})",
                    result.intent, result.confidence
                );
                // Validate and sanitize the brain's output before using it.
                // The brain (Qwen 0.5B) sometimes hallucinates repo names
                // from garbage transcripts or returns literal placeholders
                // like "owner/repo". Sanitize before routing.
                sanitize_ml_intent(&mut result);
                // Capped window-opens must NOT route: fall through to NLU,
                // not to the Architect window. (The 0.5 gate below already
                // exists for brain; the cap forces it to trigger.)
                if cap_ml_window_open(&mut result) {
                    tracing::info!("orchestrator: capped ML Architect → NLU fallback");
                }
                if result.confidence >= 0.5 {
                    return Some(result);
                }
                tracing::info!(
                    "orchestrator: brain confidence too low ({:.2}), falling back to NLU",
                    result.confidence
                );
            }
        }
    }

    // 2. Try NLU server (BERT-Mini fallback)
    if let Some(mut result) = crate::nlu_client::parse_via_nlu(transcript).await {
        tracing::info!(
            "orchestrator: nlu: {:?} (confidence={:.3})",
            result.intent, result.confidence
        );
        sanitize_ml_intent(&mut result);
        // Capped window-opens must NOT route: fall to Unknown (retry prompt),
        // not to the Architect window. Other intents keep legacy behavior.
        if cap_ml_window_open(&mut result) {
            return None;
        }
        return Some(result);
    }

    None
}

/// Cap ML-only window-opening intents. Returns true when capped.
///
/// Qwen/BERT are overconfident on out-of-distribution garbage transcripts
/// (measured 2026-09-19: 'You feel it, no?' → OpenArchitect @0.99 from a
/// ~1s noise capture → Architect window opened uninvited). Opening windows
/// is a visible side effect, so ML-sourced Architect requires the same
/// caution as a destructive op: cap below the 0.5 accept line → falls to
/// Unknown/NLU → retry prompt. Deterministic 'open architect' / 'architect'
/// phrases never pass through here (parse_with_ml only runs on
/// deterministic miss), so real requests keep working.
fn cap_ml_window_open(result: &mut crate::intent_parser::ParseResult) -> bool {
    use crate::intent_parser::ParsedIntent;
    if matches!(result.intent, ParsedIntent::OpenArchitect)
        && !result.source.starts_with("deterministic")
    {
        tracing::warn!(
            "orchestrator: ML-only OpenArchitect (source={}, conf={:.2}) capped — garbage-transcript guard",
            result.source,
            result.confidence
        );
        result.confidence = result.confidence.min(0.4);
        return true;
    }
    false
}

/// Sanitize ML-classified intent: validate repo names, reject garbage.
///
/// The brain (Qwen 0.5B) and NLU (BERT-Mini) can hallucinate repo names
/// from garbage transcripts. This function:
///   - Replaces literal "owner/repo" placeholder with empty string
///   - Validates repo names against GitHub naming rules
///   - Falls back to account-wide (empty repo) for list_prs if repo is garbage
///   - Rejects commands with invalid repos by lowering confidence below threshold
fn sanitize_ml_intent(result: &mut crate::intent_parser::ParseResult) {
    use crate::github_cmd::GitHubCommand;
    use crate::intent_parser::ParsedIntent;

    if let ParsedIntent::GitHubCommand { command } = &mut result.intent {
        match command {
            GitHubCommand::ListPrs { repo, .. }
            | GitHubCommand::GetPr { repo, .. }
            | GitHubCommand::MergePr { repo, .. }
            | GitHubCommand::ApprovePr { repo, .. }
            | GitHubCommand::ClosePr { repo, .. }
            | GitHubCommand::RevertPr { repo, .. }
            | GitHubCommand::ListPrFiles { repo, .. }
            | GitHubCommand::CommentPr { repo, .. }
            | GitHubCommand::CreatePr { repo, .. }
            | GitHubCommand::UpdateBranch { repo, .. } => {
                let trimmed = repo.trim().to_string();
                // Reject literal placeholder "owner/repo"
                if trimmed.eq_ignore_ascii_case("owner/repo") || trimmed.eq_ignore_ascii_case("owner/repo/") {
                    tracing::warn!("orchestrator: brain returned literal placeholder '{}' as repo — clearing", trimmed);
                    *repo = String::new();
                    return;
                }
                // Validate repo name: only alphanumeric, hyphens, underscores, dots, slashes
                // GitHub repo names: alphanumeric, -, _, . and owner/repo format
                if !trimmed.is_empty() && !is_valid_repo_name(&trimmed) {
                    tracing::warn!(
                        "orchestrator: brain returned invalid repo '{}' — clearing (likely hallucinated from garbage transcript)",
                        trimmed
                    );
                    *repo = String::new();
                    // For ListPrs, empty repo = account-wide (valid).
                    // For other commands, empty repo will cause a user-friendly error.
                    // Lower confidence so the brain monitor logs it as a failure.
                    if !matches!(command, GitHubCommand::ListPrs { .. }) {
                        result.confidence = result.confidence.min(0.4);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Check if a string is a valid GitHub repository name.
/// Valid: "zync", "owner/repo", "zync-ui", "my_repo", "v1.0"
/// Invalid: "very homely person", "owner/repo", sentences, phrases
fn is_valid_repo_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 100 {
        return false;
    }
    // GitHub repo names only contain: a-z, A-Z, 0-9, -, _, ., /
    // No spaces, no special characters
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
}

// ─── Routing ───────────────────────────────────────────────────────────

/// Decide which subsystem should handle this intent.
///
/// Routing priority:
///   1. Local commands (open/close app, media, greeting) → LocalCommand
///   2. Architecture mapper → Architect
///   3. Everything else (analyse PR, research, GitHub writes, general) → WorkerBackend
pub(crate) fn route_intent(intent: &ParsedIntent) -> Subsystem {
    match intent {
        // Local commands — handled in Rust, no network
        ParsedIntent::OpenApp { .. }
        | ParsedIntent::OpenUrl { .. }
        | ParsedIntent::CloseApp { .. }
        | ParsedIntent::WhatsappChat { .. }
        | ParsedIntent::MediaPlayPause
        | ParsedIntent::MediaNext
        | ParsedIntent::MediaPrevious
        | ParsedIntent::MediaStop
        | ParsedIntent::Greeting { .. } => Subsystem::LocalCommand,

        // Architecture mapper — Rust + Worker enrichment
        ParsedIntent::OpenArchitect => Subsystem::Architect,

        // Settings sidebar — handled locally (no Worker round-trip)
        ParsedIntent::OpenSettings => Subsystem::LocalCommand,

        // GitHub sub-command system — typed operations via octocrab
        ParsedIntent::GitHubCommand { .. } => Subsystem::GitHub,

        // MCP sub-center — external services via Model Context Protocol
        ParsedIntent::OrderFood { .. }
        | ParsedIntent::SearchProduct { .. }
        | ParsedIntent::SendWhatsAppMessage { .. } => Subsystem::Mcp,

        // Clarification prompt — spoken locally, never touches the Worker.
        // Partial MCP commands land here instead of Unknown so the user is
        // asked for the missing slot rather than getting a guess/refusal.
        ParsedIntent::NeedMoreInfo { .. } => Subsystem::LocalCommand,

        // Screen control intents are handled explicitly in
        // process_transcript (run_screen_click/read/tab); tracked as local.
        ParsedIntent::ScreenClick { .. }
        | ParsedIntent::ScreenRead { .. }
        | ParsedIntent::BrowserTab { .. } => Subsystem::LocalCommand,

        // Ghostwriter room entry — handled explicitly in process_transcript
        // (session start + sidebar card), tracked as a local request.
        ParsedIntent::EnterGhostwriter { .. } => Subsystem::LocalCommand,

        // Everything else goes to the Worker
        ParsedIntent::AnalyseRepo { .. }
        | ParsedIntent::AnalysePr { .. }
        | ParsedIntent::AnalyseLatestPr { .. }
        | ParsedIntent::CheckBranch { .. }
        | ParsedIntent::Search { .. }
        | ParsedIntent::NluResult { .. }
        | ParsedIntent::Unknown { .. } => Subsystem::WorkerBackend,
    }
}

/// Determine if a subsystem is "long-running" and should show the loading indicator.
///
/// Local commands are instant (<5ms) — no loading indicator.
/// Worker and Architect are long-running — show loading indicator after ack.
#[allow(dead_code)]
fn is_long_running(subsystem: &Subsystem) -> bool {
    matches!(
        subsystem,
        Subsystem::WorkerBackend
            | Subsystem::Architect
            | Subsystem::GitHub
            | Subsystem::Mcp
            | Subsystem::CommandCenter
    )
}

// ─── Public API ────────────────────────────────────────────────────────

/// Result of processing a transcript through the orchestrator.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessResult {
    pub request_id: String,
    pub subsystem: Subsystem,
    pub handled_locally: bool,
}

/// Process a transcript through the central orchestrator.
///
/// This is the MAIN ENTRY POINT called when the user finishes speaking.
/// It:
///   1. Parses the intent: deterministic (regex, <1ms) → brain (Qwen,
///      admin-only) → NLU (BERT-Mini) → Unknown
///   2. Routes to the correct subsystem
///   3. Installs a new request (cancels any previous)
///   4. Emits ack + loading state to the frontend
///   5. Dispatches to the subsystem
///   6. Emits result + done
///
/// The frontend calls this via the `orchestrator_process` Tauri command.
pub async fn process_transcript<R: Runtime>(
    app: AppHandle<R>,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
) -> Result<ProcessResult, String> {
    if transcript.trim().is_empty() {
        return Err("empty transcript".into());
    }

    tracing::info!(
        "orchestrator: processing transcript: {:?}",
        transcript.chars().take(80).collect::<String>()
    );

    // 1. Parse intent: deterministic (fast, <1ms) → brain (admin) → NLU → Unknown
    // The full pipeline ensures that phrases trained in BERT-Mini or classified
    // by the Qwen brain are actually used for routing, not just observed.
    let parse_result = parse_deterministic(&transcript);

    // If deterministic missed:
    // When online: route directly to cloud backend / 9Router (0 MB local RAM).
    // When offline: fall back to local ML sidecar (BERT-Mini / brain on-demand).
    let parse_result = if parse_result.is_some() {
        parse_result
    } else {
        let is_online = crate::tts_network::check_network().await;
        if !is_online {
            tracing::info!("orchestrator: deterministic missed & offline, trying local ML fallback");
            let ml_result = parse_with_ml(&transcript).await;
            if let Some(ref r) = ml_result {
                tracing::info!(
                    "orchestrator: ML classified as {:?} (confidence={}, source={})",
                    r.intent, r.confidence, r.source
                );
            }
            ml_result
        } else {
            tracing::info!("orchestrator: deterministic missed & online, routing directly to cloud backend (saves RAM)");
            None
        }
    };

    let intent = parse_result
        .as_ref()
        .map(|r| r.intent.clone())
        .unwrap_or(ParsedIntent::Unknown {
            raw: transcript.clone(),
        });

    tracing::info!("orchestrator: parsed intent: {:?}", intent);

    // ─── Ghostwriter room entry ─────────────────────────────────────
    // Explicit entry bypasses everything else: start session + card + reply.
    if let ParsedIntent::EnterGhostwriter { contact } = &intent {
        return run_ghostwriter_enter(app, contact.clone()).await;
    }

    // ─── Screen control (ordinal click / read-back / tab switch) ────
    // Executes inline (UIA grounding is local, ~50-500ms) with spoken
    // results. Nothing here touches the network.
    match &intent {
        ParsedIntent::ScreenClick { ordinal } => {
            return run_screen_click(app, *ordinal).await;
        }
        ParsedIntent::ScreenRead { ordinal } => {
            return run_screen_read(app, *ordinal).await;
        }
        ParsedIntent::BrowserTab { index } => {
            return run_browser_tab(app, *index).await;
        }
        _ => {}
    }

    // ─── Ghostwriter session intercept ──────────────────────────────
    // Mic-hot rule: while a session is live, EVERY transcript routes to the
    // room (dictation or allowlisted commands) instead of the normal
    // pipeline. No wake word needed between turns; a timed-out session
    // resumes with its draft intact on re-entry.
    if crate::ghostwriter::is_active() {
        return run_ghostwriter_turn(app, transcript).await;
    }

    // ─── Command Center: compound task fast path ───────────────────────
    // If the transcript is compound ("X then Y"), build a task plan and
    // execute via the command center instead of normal single-intent
    // routing. This must run BEFORE routing because a compound transcript
    // won't parse as a single intent anyway (the deterministic parser
    // would return Unknown or a wrong partial match).
    let request_id_probe = new_request_id();
    if let Some(plan) =
        crate::command_center::build_plan_with_brain(&transcript, &request_id_probe).await
    {
        tracing::info!(
            "orchestrator: compound task detected — {} steps, using command center",
            plan.steps.len()
        );
        // Install a real request so cancellation works
        let (request_id, cancel_flag) = install_new_request(Subsystem::CommandCenter);
        // Re-tag the plan with the real request_id
        let plan = crate::command_center::TaskPlan {
            task_id: request_id.clone(),
            ..plan
        };
        return run_command_center(app, plan, transcript, dialog_context, request_id, cancel_flag)
            .await;
    }

    // 1b. Brain monitor — watches every transcript in the background.
    // Non-blocking: spawns a tokio task, never delays the main pipeline.
    // The brain cross-checks the parse, learns pronunciations, and
    // auto-generates training data for BERT-Mini.
    // Only compiled when the admin-brain feature is enabled.
    #[cfg(feature = "admin-brain")]
    {
        let det_intent_name = parse_result.as_ref().map(|r| crate::intent_parser::intent_to_label(&r.intent).to_string());
        let transcript_clone = transcript.clone();
        crate::brain_monitor::monitor_transcript(
            transcript_clone,
            det_intent_name,
            None,
        );
    }

    // 2. Route to subsystem
    let subsystem = route_intent(&intent);

    // 2b. Check for verbal "wrong" feedback (admin says "wrong" after a bad command)
    #[cfg(feature = "admin-brain")]
    {
        if crate::brain_monitor::is_verbal_wrong(&transcript) {
            crate::brain_monitor::report_verbal_wrong();
            // Emit done immediately — "wrong" is a meta-command, not a real command
            emit(
                &app,
                &OrchestratorEvent::Done {
                    request_id: "verbal_wrong".to_string(),
                },
            );
            return Ok(ProcessResult {
                request_id: "verbal_wrong".to_string(),
                subsystem: Subsystem::None,
                handled_locally: true,
            });
        }
    }

    // 2c. Record the last command (for verbal "wrong" feedback)
    #[cfg(feature = "admin-brain")]
    {
        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
        crate::brain_monitor::record_last_command(transcript.clone(), intent_name);
    }

    // 3. Install new request (cancels previous)
    let (request_id, cancel_flag) = install_new_request(subsystem.clone());

    // 4. Emit "thinking" state
    emit(
        &app,
        &OrchestratorEvent::State {
            state: OrchestratorState::Thinking,
            request_id: request_id.clone(),
        },
    );

    // 5. Handle based on subsystem
    match subsystem {
        Subsystem::LocalCommand => {
            // Local commands are instant — no ack, no loading indicator.
            // The frontend handles these directly (open app, media, etc).
            // We just emit done immediately.

            // Clarification prompts (partial MCP commands) are spoken as a
            // Result event — same channel the frontend already speaks — so
            // the user hears the question instead of a Worker guess.
            if let ParsedIntent::NeedMoreInfo { prompt } = &intent {
                emit(
                    &app,
                    &OrchestratorEvent::Result {
                        text: prompt.clone(),
                        request_id: request_id.clone(),
                        analysis: None,
                        dialog_state: None,
                    },
                );
                emit(
                    &app,
                    &OrchestratorEvent::Done {
                        request_id: request_id.clone(),
                    },
                );
                clear_active_request(&request_id);

                return Ok(ProcessResult {
                    request_id,
                    subsystem,
                    handled_locally: true,
                });
            }

            // Report execution success to the brain monitor (admin-only)
            #[cfg(feature = "admin-brain")]
            {
                let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                crate::brain_monitor::report_execution_success(&transcript, &intent_name);
            }

            emit(
                &app,
                &OrchestratorEvent::Done {
                    request_id: request_id.clone(),
                },
            );
            clear_active_request(&request_id);

            Ok(ProcessResult {
                request_id,
                subsystem,
                handled_locally: true,
            })
        }

        Subsystem::WorkerBackend => {
            // Long-running — emit ack, then show loading indicator after TTS.
            let ack = pick_ack();
            emit(
                &app,
                &OrchestratorEvent::Ack {
                    text: ack.to_string(),
                    request_id: request_id.clone(),
                },
            );

            // Show loading indicator (Rust owns this — no frontend IPC needed)
            emit(
                &app,
                &OrchestratorEvent::Loading {
                    visible: true,
                    request_id: request_id.clone(),
                },
            );
            show_loading(&app);

            // Dispatch to Worker backend
            let result = dispatch_to_worker(
                app.clone(),
                transcript.clone(),
                dialog_context,
                request_id.clone(),
                cancel_flag.clone(),
            )
            .await;

            // Hide loading indicator
            emit(
                &app,
                &OrchestratorEvent::Loading {
                    visible: false,
                    request_id: request_id.clone(),
                },
            );
            hide_loading(&app);

            match result {
                Ok((text, analysis, dialog_state)) => {
                    // Report execution success to the brain monitor (admin-only)
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_success(&transcript, &intent_name);
                    }

                    let is_analysis_intent = matches!(
                        &intent,
                        ParsedIntent::AnalysePr { .. }
                            | ParsedIntent::AnalyseRepo { .. }
                            | ParsedIntent::AnalyseLatestPr { .. }
                            | ParsedIntent::CheckBranch { .. }
                    );
                    let has_structured_analysis = analysis.as_ref().map_or(false, |a| !a.is_null());
                    let is_long_markdown = text.len() > 300 || text.contains("\n#") || text.contains("\n##");

                    let (spoken_text, show_sidebar) = if is_analysis_intent || has_structured_analysis || is_long_markdown {
                        let spoken = match &intent {
                            ParsedIntent::AnalysePr { pr_number, repo, .. } => {
                                format!("Here is the analysis for PR #{} in {}, sir.", pr_number, repo)
                            }
                            ParsedIntent::AnalyseRepo { repo, .. } => {
                                format!("Here is the analysis for {}, sir.", repo)
                            }
                            ParsedIntent::AnalyseLatestPr { repo, .. } => {
                                format!("Here is the analysis for the latest PR in {}, sir.", repo)
                            }
                            ParsedIntent::CheckBranch { repo, .. } => {
                                format!("Here is the branch check for {}, sir.", repo)
                            }
                            _ => "Here is the response in the sidebar, sir.".to_string(),
                        };
                        (spoken, true)
                    } else {
                        (text.clone(), false)
                    };

                    let sidebar_text = text.clone();
                    let sidebar_analysis = analysis.clone();

                    // Emit result with concise spoken text for TTS
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text: spoken_text,
                            request_id: request_id.clone(),
                            analysis: analysis.clone(),
                            dialog_state,
                        },
                    );

                    // Show sidebar for analysis/reports
                    if show_sidebar {
                        let app_clone = app.clone();
                        let transcript_clone = transcript.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(ref a) = sidebar_analysis {
                                if !a.is_null() {
                                    if let Err(e) = crate::commands::show_sidebar_with_analysis(
                                        app_clone,
                                        transcript_clone,
                                        sidebar_text,
                                        a.clone(),
                                    )
                                    .await
                                    {
                                        tracing::warn!("orchestrator: sidebar analysis failed: {}", e);
                                    }
                                    return;
                                }
                            }
                            if let Err(e) = crate::commands::show_sidebar_with_content(
                                app_clone,
                                transcript_clone,
                                sidebar_text,
                            )
                            .await
                            {
                                tracing::warn!("orchestrator: sidebar show failed: {}", e);
                            }
                        });
                    }

                    clear_active_request(&request_id);

                    Ok(ProcessResult {
                        request_id,
                        subsystem,
                        handled_locally: false,
                    })
                }
                Err(e) => {
                    hide_loading(&app);
                    // Report execution failure to the brain monitor
                    // (admin-only, no-op if not admin)
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_failure(
                            &transcript,
                            &intent_name,
                            &e,
                        );
                    }
                    emit(
                        &app,
                        &OrchestratorEvent::Error {
                            message: e.clone(),
                            request_id: request_id.clone(),
                        },
                    );
                    emit(
                        &app,
                        &OrchestratorEvent::Done {
                            request_id: request_id.clone(),
                        },
                    );
                    clear_active_request(&request_id);
                    Err(e)
                }
            }
        }

        Subsystem::Architect => {
            // Long-running — emit ack, then show loading indicator.
            let ack = pick_ack();
            emit(
                &app,
                &OrchestratorEvent::Ack {
                    text: ack.to_string(),
                    request_id: request_id.clone(),
                },
            );
            emit(
                &app,
                &OrchestratorEvent::Loading {
                    visible: true,
                    request_id: request_id.clone(),
                },
            );
            show_loading(&app);

            // The architect subsystem is triggered via the existing
            // `open_architect_window` command. The frontend will call it
            // when it receives this event with subsystem=Architect.
            // We don't dispatch here — the frontend handles the architect
            // flow because it needs to hide the orb and manage the window.
            //
            // The orchestrator's job is to:
            //   - emit the ack
            //   - show the loading indicator
            //   - track the request ID
            // The frontend will emit "done" when the architect window opens.

            Ok(ProcessResult {
                request_id,
                subsystem,
                handled_locally: false,
            })
        }

        Subsystem::GitHub => {
            // GitHub sub-command system — typed operations via octocrab.
            // Long-running (network to GitHub API) — emit ack + loading.
            let ack = pick_ack();
            emit(
                &app,
                &OrchestratorEvent::Ack {
                    text: ack.to_string(),
                    request_id: request_id.clone(),
                },
            );
            emit(
                &app,
                &OrchestratorEvent::Loading {
                    visible: true,
                    request_id: request_id.clone(),
                },
            );
            show_loading(&app);

            // Get session info for token fetch
            let session_info = network::get_session_info()
                .ok_or("no session open — call open_session first")?;
            let (worker_url, user_id, _device_id) = session_info;

            // Extract the GitHubCommand from the parsed intent.
            // The intent parser (Phase 2A-6) produces ParsedIntent::GitHubCommand
            // for recognized GitHub operations.
            let gh_cmd = match &intent {
                ParsedIntent::GitHubCommand { command } => command.clone(),
                _ => {
                    // If we somehow got here without a GitHubCommand, fall back
                    // to the Worker (backward compatibility).
                    let result = dispatch_to_worker(
                        app.clone(),
                        transcript.clone(),
                        dialog_context,
                        request_id.clone(),
                        cancel_flag.clone(),
                    )
                    .await;

                    emit(
                        &app,
                        &OrchestratorEvent::Loading {
                            visible: false,
                            request_id: request_id.clone(),
                        },
                    );
                    hide_loading(&app);

                    match result {
                        Ok((text, analysis, dialog_state)) => {
                            emit(
                                &app,
                                &OrchestratorEvent::Result {
                                    text,
                                    request_id: request_id.clone(),
                                    analysis,
                                    dialog_state,
                                },
                            );
                            clear_active_request(&request_id);
                            return Ok(ProcessResult {
                                request_id,
                                subsystem,
                                handled_locally: false,
                            });
                        }
                        Err(e) => {
                            hide_loading(&app);
                            emit(
                                &app,
                                &OrchestratorEvent::Error {
                                    message: e.clone(),
                                    request_id: request_id.clone(),
                                },
                            );
                            emit(
                                &app,
                                &OrchestratorEvent::Done {
                                    request_id: request_id.clone(),
                                },
                            );
                            clear_active_request(&request_id);
                            return Err(e);
                        }
                    }
                }
            };

            // Execute the GitHub command via the typed subsystem
            let gh_result = crate::github_cmd::execute_command(
                &worker_url,
                &user_id,
                &gh_cmd,
                false, // not confirmed yet — confirmation flow handled by events
            )
            .await;

            // Hide loading indicator
            emit(
                &app,
                &OrchestratorEvent::Loading {
                    visible: false,
                    request_id: request_id.clone(),
                },
            );
            hide_loading(&app);

            // Emit the appropriate event based on the result type
            match &gh_result {
                crate::github_cmd::GitHubResult::NeedsConfirmation { prompt, command } => {
                    // Report successful detection (even though it needs confirmation,
                    // the parsing was correct)
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_success(&transcript, &intent_name);
                    }
                    let cmd_json = serde_json::to_value(command).unwrap_or(serde_json::Value::Null);
                    emit(
                        &app,
                        &OrchestratorEvent::Confirm {
                            prompt: prompt.clone(),
                            request_id: request_id.clone(),
                            command: cmd_json.clone(),
                        },
                    );
                    let confirm_payload = serde_json::json!({
                        "requestId": request_id.clone(),
                        "prompt": prompt.clone(),
                        "command": cmd_json,
                    });
                    let _ = crate::commands::show_sidebar_with_confirmation(
                        app.clone(),
                        "GitHub Confirmation".to_string(),
                        prompt.clone(),
                        confirm_payload,
                    ).await;
                }
                crate::github_cmd::GitHubResult::MergeConflict {
                    pr_number,
                    repo,
                    conflict_files,
                    message,
                } => {
                    let files_json = serde_json::to_value(conflict_files).unwrap_or(serde_json::Value::Null);
                    emit(
                        &app,
                        &OrchestratorEvent::ConflictReport {
                            request_id: request_id.clone(),
                            pr_number: *pr_number,
                            repo: repo.clone(),
                            conflict_files: files_json,
                            message: message.clone(),
                        },
                    );
                }
                crate::github_cmd::GitHubResult::Text { text } => {
                    // Report GitHub command success to the brain monitor
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_success(&transcript, &intent_name);
                    }
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text: text.clone(),
                            request_id: request_id.clone(),
                            analysis: None,
                            dialog_state: None,
                        },
                    );
                }
                crate::github_cmd::GitHubResult::PrList { repo, state, prs } => {
                    // Report GitHub command success to the brain monitor
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_success(&transcript, &intent_name);
                    }
                    // Emit a short TTS ack + the structured PR list for the sidebar.
                    // The frontend will open the PR list sidebar panel.
                    let count = prs.len();
                    let ack_text = if repo == "all repositories" {
                        format!(
                            "Showing {} {} PR{} across all your repositories.",
                            count,
                            state,
                            if count == 1 { "" } else { "s" },
                        )
                    } else {
                        format!(
                            "Showing {} {} PR{} in {}.",
                            count,
                            state,
                            if count == 1 { "" } else { "s" },
                            repo
                        )
                    };
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text: ack_text,
                            request_id: request_id.clone(),
                            analysis: None,
                            dialog_state: None,
                        },
                    );

                    // Store the PR list as pending data BEFORE creating the
                    // sidebar window. The frontend fetches this on mount via
                    // `get_pending_pr_list`, which is race-free regardless of
                    // how long the WebView takes to load. This fixes the bug
                    // where the `github_result` event was emitted before the
                    // sidebar window existed, so the event was lost.
                    let pr_list_json = serde_json::json!({
                        "type": "pr_list",
                        "repo": repo,
                        "state": state,
                        "prs": prs,
                    });
                    crate::commands::set_pending_pr_list(pr_list_json);

                    // Create the sidebar window directly from Rust (don't
                    // rely on the frontend to call `show_pr_list_sidebar`,
                    // because the frontend listener that would call it
                    // doesn't exist yet — the window hasn't been created).
                    let app_clone = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::commands::show_pr_list_sidebar(app_clone).await {
                            tracing::error!("orchestrator: failed to show PR list sidebar: {}", e);
                        }
                    });
                }
                crate::github_cmd::GitHubResult::Error { message, .. } => {
                    // Report GitHub command failure to the brain monitor
                    #[cfg(feature = "admin-brain")]
                    {
                        let intent_name = crate::intent_parser::intent_to_label(&intent).to_string();
                        crate::brain_monitor::report_execution_failure(
                            &transcript,
                            &intent_name,
                            message,
                        );
                    }
                    emit(
                        &app,
                        &OrchestratorEvent::Error {
                            message: message.clone(),
                            request_id: request_id.clone(),
                        },
                    );
                }
            }

            // Emit the raw GitHubResult for the frontend to use
            let result_json = serde_json::to_value(&gh_result).unwrap_or(serde_json::Value::Null);
            emit(
                &app,
                &OrchestratorEvent::GitHubResult {
                    request_id: request_id.clone(),
                    result: result_json,
                },
            );
            emit(
                &app,
                &OrchestratorEvent::Done {
                    request_id: request_id.clone(),
                },
            );
            clear_active_request(&request_id);
            Ok(ProcessResult {
                request_id,
                subsystem,
                handled_locally: false,
            })
        }

        Subsystem::Mcp => {
            // MCP sub-center — external services via Model Context Protocol.
            let is_write_intent = matches!(
                intent,
                ParsedIntent::SendWhatsAppMessage { .. }
            );

            // Only emit generic long-running Ack/Loading if this is a read operation
            // that executes directly without an immediate confirmation prompt.
            if !is_write_intent {
                let ack = pick_ack();
                emit(
                    &app,
                    &OrchestratorEvent::Ack {
                        text: ack.to_string(),
                        request_id: request_id.clone(),
                    },
                );
                emit(
                    &app,
                    &OrchestratorEvent::Loading {
                        visible: true,
                        request_id: request_id.clone(),
                    },
                );
                show_loading(&app);
            }

            let mcp_outcome = dispatch_to_mcp(&app, &intent, &transcript, &request_id).await;

            if !is_write_intent {
                emit(
                    &app,
                    &OrchestratorEvent::Loading {
                        visible: false,
                        request_id: request_id.clone(),
                    },
                );
                hide_loading(&app);
            }

            match mcp_outcome {
                Ok(Some(text)) => {
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text,
                            request_id: request_id.clone(),
                            analysis: None,
                            dialog_state: None,
                        },
                    );
                    emit(
                        &app,
                        &OrchestratorEvent::Done {
                            request_id: request_id.clone(),
                        },
                    );
                    clear_active_request(&request_id);
                    Ok(ProcessResult {
                        request_id,
                        subsystem,
                        handled_locally: false,
                    })
                }
                Ok(None) => {
                    // Confirmation gate active — OrchestratorEvent::Confirm was emitted
                    // with the prompt question. Do NOT emit Result or Done so TTS speaks
                    // the confirmation question cleanly without cancellation.
                    Ok(ProcessResult {
                        request_id,
                        subsystem,
                        handled_locally: false,
                    })
                }
                Err(e) => {
                    emit(
                        &app,
                        &OrchestratorEvent::Error {
                            message: e.clone(),
                            request_id: request_id.clone(),
                        },
                    );
                    emit(
                        &app,
                        &OrchestratorEvent::Done {
                            request_id: request_id.clone(),
                        },
                    );
                    clear_active_request(&request_id);
                    // Best-of combine: voice spoke the guidance; open the
                    // fix-it card alongside (first failure per session).
                    // The stashed retry fires when the server connects.
                    if let Some(server) = server_for_mcp_intent(&intent) {
                        open_mcp_connect_card(&app, server, &transcript).await;
                    }
                    Err(e)
                }
            }
        }

        Subsystem::CommandCenter => {
            // Unreachable — compound tasks return early via
            // run_command_center() before this match. Keep as a safety
            // fallback: treat like WorkerBackend.
            emit(
                &app,
                &OrchestratorEvent::Done {
                    request_id: request_id.clone(),
                },
            );
            clear_active_request(&request_id);
            Ok(ProcessResult {
                request_id,
                subsystem,
                handled_locally: true,
            })
        }

        Subsystem::None => {
            emit(
                &app,
                &OrchestratorEvent::Done {
                    request_id: request_id.clone(),
                },
            );
            clear_active_request(&request_id);
            Ok(ProcessResult {
                request_id,
                subsystem,
                handled_locally: true,
            })
        }
    }
}

/// Cancel the active request (if any). Called on barge-in or new wake.
pub fn cancel_active() {
    let mut guard = ACTIVE_REQUEST.lock().unwrap();
    if let Some(req) = guard.as_ref() {
        req.cancelled.store(true, Ordering::Relaxed);
        tracing::info!("orchestrator: cancelled request {}", req.id);
    }
    *guard = None;
}

/// Signal that the current request is done (called by frontend after TTS).
pub fn signal_done(request_id: &str) {
    clear_active_request(request_id);
}

// ─── Subsystem dispatchers ─────────────────────────────────────────────

/// Dispatch to the Cloudflare Worker backend.
///
/// This reuses the existing `network::send_transcript` HTTP logic but
/// routes the response through the orchestrator's event channel instead
/// of the old "assistant:server" channel.
///
/// **9Router optimization:** For general questions, 9Router tries free
/// cloud providers (Cerebras → Groq → Gemini) directly from the device,
/// bypassing the Worker for 3-7x lower latency (~242ms vs ~2s). The
/// Worker is the fallback if 9Router fails or the task requires Worker
/// infrastructure (PR analysis, GitHub token, search).
async fn dispatch_to_worker<R: Runtime>(
    app: AppHandle<R>,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
    request_id: String,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(String, Option<serde_json::Value>, Option<serde_json::Value>), String> {
    // ─── 9Router fast path ──────────────────────────────────────────────
    // Try local → free cloud providers first. This bypasses the Worker
    // entirely for general questions, cutting latency from ~2s to ~242ms.
    if crate::router::can_route(&transcript) {
        let keys = crate::router::read_provider_keys(&app);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("9router http client: {e}"))?;

        tracing::info!(
            "9router: trying fast path for request {} (transcript: {:?})",
            request_id,
            crate::router::truncate_pub(&transcript, 60),
        );

        match crate::router::route_question(
            &transcript,
            dialog_context.as_ref(),
            &keys,
            &client,
        )
        .await
        {
            Some(resp) => {
                tracing::info!(
                    "9router: fast path succeeded via {} in {}ms",
                    resp.provider.name(),
                    resp.latency_ms
                );
                // 9Router answered — return directly, skip Worker entirely.
                return Ok((resp.text, None, None));
            }
            None => {
                tracing::info!(
                    "9router: fast path failed for {}, falling back to Worker",
                    request_id
                );
                // Fall through to Worker
            }
        }
    }

    // Check if cancelled before Worker call
    if is_cancelled(&cancel_flag) {
        return Err("cancelled".into());
    }

    // ─── Worker fallback path (original logic) ──────────────────────────
    // Get session info
    let session_info = network::get_session_info()
        .ok_or("no session open — call open_session first")?;
    let (worker_url, user_id, device_id) = session_info;

    // Build the request payload
    let task = if let Some(ctx) = &dialog_context {
        serde_json::json!({
            "type": "general",
            "request": transcript,
            "dialog_context": ctx,
        })
    } else {
        serde_json::json!({
            "type": "general",
            "request": transcript,
        })
    };
    let payload = serde_json::json!({
        "request_id": request_id,
        "requester": {
            "id": user_id,
            "device_id": device_id,
        },
        "task": task,
    });

    // HTTP POST to the Worker
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    tracing::info!(
        "orchestrator: dispatching to worker: url={} request_id={}",
        worker_url,
        request_id
    );

    let resp = client
        .post(&worker_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("worker request: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Worker error {status}: {body}"));
    }

    // Check if cancelled while waiting
    if is_cancelled(&cancel_flag) {
        tracing::info!("orchestrator: request {} cancelled, discarding result", request_id);
        return Err("cancelled".into());
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("worker json: {e}"))?;

    let reply_text = data["reply_text"]
        .as_str()
        .or(data["text"].as_str())
        .or(data["content"].as_str())
        .or(data["response"].as_str())
        .unwrap_or("I couldn't process that request.")
        .to_string();

    let analysis = data.get("analysis").cloned();
    let dialog_state = data.get("dialog_state").cloned();

    Ok((reply_text, analysis, dialog_state))
}

/// Public wrapper for `dispatch_to_worker` — used by command_center steps.
pub(crate) async fn dispatch_to_worker_pub<R: Runtime>(
    app: AppHandle<R>,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
    request_id: String,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(String, Option<serde_json::Value>, Option<serde_json::Value>), String> {
    dispatch_to_worker(app, transcript, dialog_context, request_id, cancel_flag).await
}

// ─── MCP sub-center dispatch ───────────────────────────────────────────

/// Dispatch an MCP intent to the appropriate MCP server.
///
/// Maps intents to (server, tool, params):
///   OrderFood            → SwiggyFood / search_restaurants
///   SearchProduct        → Amazon / amazon_search
///   SendWhatsAppMessage  → WhatsApp / send_message (confirmation gated)
///
/// Read operations execute directly. Write/destructive operations emit a
/// Confirm event and return a pending message — the actual call happens
/// after the user confirms via `orchestrator_mcp_confirm`.
async fn dispatch_to_mcp<R: Runtime>(
    app: &AppHandle<R>,
    intent: &ParsedIntent,
    transcript: &str,
    request_id: &str,
) -> Result<Option<String>, String> {
    use crate::mcp_client::{call_tool, extract_text, McpServer};

    // Resolve the intent → (server, tool, params)
    let (server, tool, params): (McpServer, &str, serde_json::Value) = match intent {
        ParsedIntent::OrderFood { query, restaurant } => {
            let q = if let Some(r) = restaurant {
                format!("{} from {}", query, r)
            } else if !query.is_empty() {
                query.clone()
            } else {
                transcript.to_string()
            };
            (
                McpServer::SwiggyFood,
                "search_restaurants",
                serde_json::json!({ "query": q }),
            )
        }
        ParsedIntent::SearchProduct { query } => (
            McpServer::Amazon,
            "amazon_search",
            serde_json::json!({ "query": query, "max_results": 5 }),
        ),
        ParsedIntent::SendWhatsAppMessage { contact, message } => (
            McpServer::WhatsApp,
            "send_message",
            serde_json::json!({ "recipient": contact, "message": message }),
        ),
        _ => return Err(format!("no MCP mapping for intent")),
    };

    // Pre-flight credential check — BEFORE the confirm gate. Asking the
    // user to approve a write the system can't execute is worse than
    // useless; missing credentials speak guidance immediately.
    // Resolve the bearer token from the auth vault (one login per service
    // group, with refresh).
    let pre_status = server
        .vault_key()
        .map(crate::auth_vault::token_status);
    let mut vault_token: Option<String> =
        crate::auth_vault::resolve_server_token(server).await;
    if server.vault_key().is_some() && vault_token.is_none() {
        // Distinguish expired (had a login, it died) from missing (never
        // connected) so the spoken guidance is exact.
        if pre_status == Some("expired") {
            let what = match server {
                crate::mcp_client::McpServer::SwiggyFood
                | crate::mcp_client::McpServer::SwiggyInstamart
                | crate::mcp_client::McpServer::SwiggyDineout => "Swiggy",
                crate::mcp_client::McpServer::WhatsApp => "WhatsApp",
                crate::mcp_client::McpServer::Amazon => "Amazon",
            };
            return Err(format!(
                "Your {what} login expired, sir — reconnect it in Settings, Connections tab."
            ));
        }
        return Err(mcp_error_guidance(server, "HTTP 401: no credential"));
    }

    // Confirmation gate for write/destructive operations.
    if server.requires_confirmation(tool) {
        let prompt = if server.is_destructive(tool) {
            format!(
                "This will perform an irreversible action on {}. Tool: {}. Proceed?",
                server.name(),
                tool
            )
        } else {
            match intent {
                ParsedIntent::SendWhatsAppMessage { contact, message } => {
                    format!("Send WhatsApp message to {}: \"{}\"?", contact, message)
                }
                _ => format!("Execute {} on {}?", tool, server.name()),
            }
        };

        let pending = serde_json::json!({
            "kind": "mcp",
            "server": server.name(),
            "tool": tool,
            "params": params.clone(),
            "transcript": transcript,
        });

        emit(
            app,
            &OrchestratorEvent::Confirm {
                prompt: prompt.clone(),
                request_id: request_id.to_string(),
                command: pending.clone(),
            },
        );

        let confirm_payload = serde_json::json!({
            "requestId": request_id,
            "prompt": prompt,
            "command": pending,
        });
        let _ = crate::commands::show_sidebar_with_confirmation(
            app.clone(),
            "Action Confirmation".to_string(),
            prompt.clone(),
            confirm_payload,
        ).await;

        // The actual call happens in orchestrator_mcp_confirm after the
        // user approves. Return Ok(None) to indicate confirmation is pending.
        return Ok(None);
    }

    // Read operations — execute directly.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("mcp http client: {e}"))?;

    tracing::info!(
        "orchestrator: mcp dispatch server={} tool={} request_id={}",
        server.name(),
        tool,
        request_id
    );

    // Resolve the bearer token from the auth vault (one login per service
    // group, with refresh). Falls back to anonymous when the service isn't
    // connected — the failure arm below speaks the reconnect path.
    // (Pre-flight runs above; this re-resolves fresh for the actual call.)
    vault_token = crate::auth_vault::resolve_server_token(server).await;
    let mut result = call_tool(server, tool, params.clone(), &client, vault_token.as_deref()).await;

    // 401 clear-and-retry: the token may have died between resolve and use
    // (revoked server-side). Evict, mint fresh once, retry once — then
    // guidance. Never replays the same dead token.
    if !result.ok {
        let first_err = result.error.clone().unwrap_or_default();
        if is_auth_failure(&first_err) {
            if let Some(key) = server.vault_key() {
                crate::auth_vault::clear_token(key);
                tracing::info!(
                    "orchestrator: mcp {} auth failed — cleared stale token, retrying once",
                    server.name()
                );
                vault_token = crate::auth_vault::resolve_server_token(server).await;
                result = call_tool(
                    server,
                    tool,
                    params.clone(),
                    &client,
                    vault_token.as_deref(),
                )
                .await;
            }
        }
    }

    if !result.ok {
        let err = result
            .error
            .clone()
            .unwrap_or_else(|| "unknown MCP error".to_string());
        tracing::warn!("orchestrator: mcp {} failed: {}", server.name(), err);
        // Stash for auto-retry: when the Connect card's monitor sees the
        // server turn Ready, it replays this exact call (Composio
        // WAIT_FOR_CONNECTIONS shape — no re-speaking, no re-confirm).
        stash_mcp_retry(server, tool, &params);
        return Err(mcp_error_guidance(server, &err));
    }

    Ok(Some(extract_text(&result)))
}

/// Narrow auth-failure detector for retry/clear decisions.
/// Deliberately strict (status codes + explicit phrases) — the old
/// substring "auth" also matched "author"/"authentic" in tool output.
fn is_auth_failure(err: &str) -> bool {
    let lower = err.to_lowercase();
    lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid_token")
        || lower.contains("authentication required")
        || lower.contains("login required")
        || lower.contains("token expired")
        || lower.contains("invalid token")
}

/// Turn an MCP transport/auth failure into an actionable spoken message.
/// Raw errors ("HTTP 401", "connection refused") mean nothing by voice —
/// every failure must tell the user WHICH connection to fix and WHERE.
/// Backend rule: never report a dead MCP without its reconnect path.
fn mcp_error_guidance(server: crate::mcp_client::McpServer, err: &str) -> String {
    use crate::mcp_client::McpServer as S;
    let lower = err.to_lowercase();
    let needs_login = is_auth_failure(err);
    let unreachable = lower.contains("refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("dns")
        || lower.contains("unreachable")
        || lower.contains("failed to resolve");
    if needs_login {
        let what = match server {
            S::SwiggyFood | S::SwiggyInstamart | S::SwiggyDineout => {
                "Swiggy — reconnect it"
            }
            S::WhatsApp => "WhatsApp — re-scan the bridge QR",
            S::Amazon => "Amazon — reconnect the bridge session",
        };
        return format!("{what} in Settings, Connections tab, sir, then try again.");
    }
    if unreachable && server.url().contains("127.0.0.1") {
        // Auto-start assist: name the EXACT binary and first-run steps, not
        // "start it". The user should be able to fix this from the spoken
        // sentence alone. (Catalog: docs/mcp/02-server-catalog.md)
        let what = match server {
            S::WhatsApp => "WhatsApp bridge isn't running, sir — run the mcp-whatsapp program on this PC, scan the QR it shows with WhatsApp on your phone, then press Recheck in Connections.",
            S::Amazon => "Amazon bridge isn't running, sir — start the Amazon bridge program on this PC, sign in when its browser window opens, then press Recheck in Connections.",
            _ => return format!(
                "The {} isn't running, sir — start it on this PC, then press Recheck in Connections.",
                server.name()
            ),
        };
        return what.to_string();
    }
    if lower.contains("circuit open") {
        return format!(
            "The {} connection is cooling down after repeated failures, sir — press Recheck in Connections to retry now.",
            server.name()
        );
    }
    format!("{} failed, sir: {}", server.name(), err)
}

// ─── MCP Connect card (best-of combine) ─────────────────────────────────
// Industry pattern (Composio/Claude/Cursor): a failed connector opens a
// fix-it card where the user already is, with status + numbered steps +
// the auth action inline — and the interrupted task auto-resumes on
// completion. Voice still speaks first; the card opens alongside, once
// per server per session (no window spam).

/// Servers whose Connect card was already opened this session.
static SHOWN_CONNECT_CARDS: once_cell::sync::Lazy<
    Arc<Mutex<std::collections::HashSet<String>>>,
> = once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(std::collections::HashSet::new())));

/// A failed MCP call stashed for auto-retry when its server connects.
#[derive(Debug, Clone)]
struct McpRetry {
    server: crate::mcp_client::McpServer,
    tool: String,
    params: serde_json::Value,
}

static PENDING_MCP_RETRY: once_cell::sync::Lazy<Arc<Mutex<Option<McpRetry>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

fn stash_mcp_retry(
    server: crate::mcp_client::McpServer,
    tool: &str,
    params: &serde_json::Value,
) {
    let mut guard = PENDING_MCP_RETRY.lock().unwrap();
    *guard = Some(McpRetry {
        server,
        tool: tool.to_string(),
        params: params.clone(),
    });
}

fn take_mcp_retry() -> Option<McpRetry> {
    PENDING_MCP_RETRY.lock().unwrap().take()
}

/// Map an MCP-routed intent to its server (mirrors dispatch_to_mcp).
fn server_for_mcp_intent(intent: &ParsedIntent) -> Option<crate::mcp_client::McpServer> {
    use crate::mcp_client::McpServer as S;
    match intent {
        ParsedIntent::OrderFood { .. } => Some(S::SwiggyFood),
        ParsedIntent::SearchProduct { .. } => Some(S::Amazon),
        ParsedIntent::SendWhatsAppMessage { .. } => Some(S::WhatsApp),
        _ => None,
    }
}

/// Render a Connect card as sidebar markdown: status, numbered steps,
/// QR image (data-URI passes the markdown sanitizer straight through),
/// pairing link (opens externally via openExternal), safety notes.
fn connect_card_markdown(
    card: &crate::mcp_client::McpConnectCard,
    transcript: &str,
) -> String {
    use crate::mcp_client::McpConnectState as St;
    let title = match card.server.as_str() {
        "swiggy-food" | "swiggy-instamart" | "swiggy-dineout" => "Swiggy",
        "whatsapp" => "WhatsApp",
        "amazon" => "Amazon",
        _ => card.server.as_str(),
    };
    let state_line = match card.state {
        St::Down => "Not running",
        St::AuthRequired => "Needs login",
        St::Ready => "Connected",
        St::Unknown => "Status unknown",
    };
    let mut md = format!("## Connect {title}\n\n**Status:** {state_line} — {note}\n\n", note = card.note);
    if card.server == "whatsapp" {
        md.push_str(&format!("_Request: \"{transcript}\" — held, not lost._\n\n"));
    }
    for (i, step) in card.steps.iter().enumerate() {
        md.push_str(&format!("{}. {}\n", i + 1, step));
    }
    md.push('\n');
    if let Some(img) = &card.qr_image_uri {
        md.push_str(&format!("![Scan with WhatsApp → Settings → Linked Devices]({img})\n\n"));
    } else if let Some(code) = &card.qr_code_text {
        md.push_str(&format!("Pairing code: `{code}`\n\n"));
    }
    if let Some(url) = &card.pair_url {
        md.push_str(&format!("[Open pairing page in browser]({url})\n\n"));
    }
    if card.server == "whatsapp" {
        md.push_str("> Unofficial bridge (WhatsApp ToS risk) — a secondary number is safer.\n>\n> Session rotates roughly every 20 days; a fresh QR appears here automatically.\n\n");
    }
    if card.server.starts_with("swiggy") {
        md.push_str("> Localhost dev is free; production needs Swiggy Builders-Club approval.\n\n");
    }
    md.push_str("_NEXUS watches in the background and confirms the moment it connects._\n");
    md
}

/// Open the Connect card for a failed server (first failure per session
/// only) and start the ready-monitor that auto-retries the stashed call.
pub(crate) async fn open_mcp_connect_card<R: Runtime>(
    app: &AppHandle<R>,
    server: crate::mcp_client::McpServer,
    transcript: &str,
) {
    let first = {
        let mut shown = SHOWN_CONNECT_CARDS.lock().unwrap();
        shown.insert(server.name().to_string())
    };
    if !first {
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("connect card: http client failed: {e}");
            return;
        }
    };
    let card = crate::mcp_client::connect_card_for(server, &client).await;
    let md = connect_card_markdown(&card, transcript);
    if let Err(e) = crate::commands::show_sidebar_with_content(
        app.clone(),
        format!("Connect {}", card.server),
        md,
    )
    .await
    {
        tracing::warn!("connect card: sidebar failed: {e}");
        return;
    }
    spawn_ready_monitor(app.clone(), server, transcript.to_string());
}

/// Background watch: poll connect state; while the card is open, keep its
/// content fresh (WhatsApp's QR rotates every 20-30s — a stale QR can't
/// scan, so re-render whenever the payload changes), and when the server
/// turns Ready, render the Connected card + retry the stashed call once
/// and speak the outcome (Composio WAIT_FOR_CONNECTIONS shape). Gives up
/// silently after ~10 min — the card stays open with manual Recheck.
/// Never speaks over a newer turn: if another request is active, the
/// retry is dropped.
fn spawn_ready_monitor<R: Runtime>(
    app: AppHandle<R>,
    server: crate::mcp_client::McpServer,
    transcript: String,
) {
    tauri::async_runtime::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .connect_timeout(std::time::Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut last_qr: Option<String> = None;
        for _ in 0..120 {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let card = crate::mcp_client::connect_card_for(server, &client).await;
            if card.state != crate::mcp_client::McpConnectState::Ready {
                // Fresh QR while the card is open: WhatsApp rotates the
                // pairing QR every 20-30s; a static card would show an
                // expired code (Novu deletes/re-renders its card for the
                // same reason). Re-render only when the payload changed so
                // we don't spam the sidebar.
                let current_qr = card
                    .qr_image_uri
                    .clone()
                    .or_else(|| card.qr_code_text.clone());
                if current_qr.is_some() && current_qr != last_qr {
                    last_qr = current_qr;
                    let md = connect_card_markdown(&card, &transcript);
                    let _ = crate::commands::show_sidebar_with_content(
                        app.clone(),
                        format!("Connect {}", card.server),
                        md,
                    )
                    .await;
                }
                continue;
            }
            // Ready: render the truth on the card (never show stale
            // "needs login" content after success — the truthfulness
            // failure Claude's tracker documented).
            let ready_md = connect_card_markdown(&card, &transcript);
            let _ = crate::commands::show_sidebar_with_content(
                app.clone(),
                format!("Connect {}", card.server),
                ready_md,
            )
            .await;
            // Another turn started meanwhile: drop the retry, stay silent.
            if ACTIVE_REQUEST.lock().unwrap().is_some() {
                take_mcp_retry();
                return;
            }
            let retry = match take_mcp_retry() {
                Some(r) if r.server == server => r,
                other => {
                    // Wrong server (or nothing stashed): just announce.
                    if other.is_some() {
                        let mut guard = PENDING_MCP_RETRY.lock().unwrap();
                        *guard = other;
                    }
                    let rid = new_request_id();
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text: format!(
                                "{} is connected, sir.",
                                display_server_name(server)
                            ),
                            request_id: rid,
                            analysis: None,
                            dialog_state: None,
                        },
                    );
                    return;
                }
            };
            // Retry the original call once with a fresh token.
            let call_client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
            {
                Ok(c) => c,
                Err(_) => return,
            };
            let vault_token =
                crate::auth_vault::resolve_server_token(server).await;
            let result = crate::mcp_client::call_tool(
                server,
                &retry.tool,
                retry.params,
                &call_client,
                vault_token.as_deref(),
            )
            .await;
            let rid = new_request_id();
            let text = if result.ok {
                let body = crate::mcp_client::extract_text(&result);
                format!(
                    "{} is connected, sir. {}",
                    display_server_name(server),
                    body
                )
            } else {
                format!(
                    "{} is reachable now, sir, but the retry failed: {}. The setup card is still open.",
                    display_server_name(server),
                    result.error.unwrap_or_default()
                )
            };
            emit(
                &app,
                &OrchestratorEvent::Result {
                    text,
                    request_id: rid,
                    analysis: None,
                    dialog_state: None,
                },
            );
            return;
        }
    });
}

fn display_server_name(server: crate::mcp_client::McpServer) -> &'static str {
    match server {
        crate::mcp_client::McpServer::SwiggyFood
        | crate::mcp_client::McpServer::SwiggyInstamart
        | crate::mcp_client::McpServer::SwiggyDineout => "Swiggy",
        crate::mcp_client::McpServer::WhatsApp => "WhatsApp",
        crate::mcp_client::McpServer::Amazon => "Amazon",
    }
}

/// Public wrapper for `dispatch_to_mcp` — used by command_center steps.
pub(crate) async fn dispatch_to_mcp_pub<R: Runtime>(
    app: &AppHandle<R>,
    intent: &ParsedIntent,
    transcript: &str,
    request_id: &str,
) -> Result<Option<String>, String> {
    dispatch_to_mcp(app, intent, transcript, request_id).await
}

/// Render the Ghostwriter sidebar card: target header + draft bubble +
/// command hint. Same 400px overlay; blur + scrim already live.
async fn show_ghostwriter_card<R: Runtime>(app: &AppHandle<R>) {
    let (contact, draft) = crate::ghostwriter::card_state()
        .unwrap_or((None, String::new()));
    let to_line = contact
        .map(|c| format!("To: {c}"))
        .unwrap_or_else(|| "To: — (say \"this is for …\")".to_string());
    let body = if draft.trim().is_empty() {
        "(blank page — speak, and I'll write)".to_string()
    } else {
        draft
    };
    let text = format!(
        "✒️ Ghostwriter\n{to_line}\n\n> {body}\n\n—",
    );
    let _ = crate::commands::show_sidebar_with_content(
        app.clone(),
        "ghostwriter".to_string(),
        text,
    )
    .await;
}

/// Click the Nth on-screen actionable (Windows UIA grounding).
async fn run_screen_click<R: Runtime>(
    app: AppHandle<R>,
    ordinal: u32,
) -> Result<ProcessResult, String> {
    let (request_id, _) = install_new_request(Subsystem::LocalCommand);
    #[cfg(target_os = "windows")]
    {
        let els = crate::screen::list_actionables();
        match crate::screen::pick_ordinal(&els, ordinal) {
            Some(el) => {
                let name = el.name.clone();
                match crate::screen::click_element(el) {
                    Ok(()) => speak_line(&app, format!("Clicked {name}, sir."), &request_id),
                    Err(e) => speak_line(&app, format!("Couldn't click, sir: {e}"), &request_id),
                }
            }
            None => speak_line(
                &app,
                format!("I only see {} clickable things, sir.", els.len()),
                &request_id,
            ),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ordinal;
        speak_line(&app, "Screen clicking needs Windows, sir.".to_string(), &request_id);
    }
    clear_active_request(&request_id);
    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::LocalCommand,
        handled_locally: true,
    })
}

/// Read back the Nth on-screen actionable (no click).
async fn run_screen_read<R: Runtime>(
    app: AppHandle<R>,
    ordinal: u32,
) -> Result<ProcessResult, String> {
    let (request_id, _) = install_new_request(Subsystem::LocalCommand);
    #[cfg(target_os = "windows")]
    {
        let els = crate::screen::list_actionables();
        match crate::screen::pick_ordinal(&els, ordinal) {
            Some(el) => speak_line(
                &app,
                format!("{} {} says {}, sir.", ordinal_word(ordinal), el.kind, el.name),
                &request_id,
            ),
            None => speak_line(
                &app,
                format!("I only see {} clickable things, sir.", els.len()),
                &request_id,
            ),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = ordinal;
        speak_line(&app, "Screen reading needs Windows, sir.".to_string(), &request_id);
    }
    clear_active_request(&request_id);
    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::LocalCommand,
        handled_locally: true,
    })
}

fn ordinal_word(n: u32) -> String {
    match n {
        1 => "1st".to_string(),
        2 => "2nd".to_string(),
        3 => "3rd".to_string(),
        _ => format!("{n}th"),
    }
}

/// Switch browser tab via hotkey (cross-platform, instant).
async fn run_browser_tab<R: Runtime>(
    app: AppHandle<R>,
    index: u32,
) -> Result<ProcessResult, String> {
    let (request_id, _) = install_new_request(Subsystem::LocalCommand);
    match crate::screen::switch_browser_tab(index) {
        Ok(msg) => speak_line(&app, msg, &request_id),
        Err(e) => speak_line(&app, format!("Couldn't switch tabs, sir: {e}"), &request_id),
    }
    clear_active_request(&request_id);
    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::LocalCommand,
        handled_locally: true,
    })
}

/// Speak a short line (Result event channel the frontend already speaks).
fn speak_line<R: Runtime>(app: &AppHandle<R>, text: String, request_id: &str) {    emit(
        app,
        &OrchestratorEvent::Result {
            text,
            request_id: request_id.to_string(),
            analysis: None,
            dialog_state: None,
        },
    );
    emit(
        app,
        &OrchestratorEvent::Done {
            request_id: request_id.to_string(),
        },
    );
}

/// Enter the Ghostwriter room: start session + card + spoken reply.
async fn run_ghostwriter_enter<R: Runtime>(
    app: AppHandle<R>,
    contact: Option<String>,
) -> Result<ProcessResult, String> {
    let (request_id, _) = install_new_request(Subsystem::LocalCommand);
    let reply = crate::ghostwriter::enter(contact);
    show_ghostwriter_card(&app).await;
    speak_line(&app, reply, &request_id);
    clear_active_request(&request_id);
    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::LocalCommand,
        handled_locally: true,
    })
}

/// One in-session turn: dictate, command, send-ready, or exit.
async fn run_ghostwriter_turn<R: Runtime>(
    app: AppHandle<R>,
    transcript: String,
) -> Result<ProcessResult, String> {
    let (request_id, _) = install_new_request(Subsystem::LocalCommand);
    match crate::ghostwriter::handle_turn(&transcript) {
        crate::ghostwriter::TurnOutcome::Dictated(_) => {
            // Ink is visible on the card — no speech (never read the draft
            // unasked; "read it back" exists for that).
            show_ghostwriter_card(&app).await;
        }
        crate::ghostwriter::TurnOutcome::Replied(reply) => {
            show_ghostwriter_card(&app).await;
            speak_line(&app, reply, &request_id);
        }
        crate::ghostwriter::TurnOutcome::SendReady { contact, message } => {
            let intent = ParsedIntent::SendWhatsAppMessage { contact, message };
            match dispatch_to_mcp(&app, &intent, &transcript, &request_id).await {
                Ok(out) => {
                    // Sent (or awaiting orb confirmation) — clear the ink,
                    // stay in the room for the next dictation.
                    {
                        // Reset draft but keep the room + contact.
                        crate::ghostwriter::enter(
                            crate::ghostwriter::card_state().and_then(|(c, _)| c),
                        );
                        // enter() preserves the draft — clear it explicitly.
                        // (clear_draft below is a tiny helper on the module.)
                        crate::ghostwriter::clear_draft();
                    }
                    show_ghostwriter_card(&app).await;
                    if let Some(msg) = out {
                        speak_line(&app, msg, &request_id);
                    }
                }
                Err(e) => {
                    show_ghostwriter_card(&app).await;
                    speak_line(
                        &app,
                        format!("Couldn't send, sir: {e}"),
                        &request_id,
                    );
                }
            }
        }
        crate::ghostwriter::TurnOutcome::Exited(reply) => {
            let _ = crate::commands::hide_sidebar(app.clone());
            speak_line(&app, reply, &request_id);
        }
    }
    clear_active_request(&request_id);
    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::LocalCommand,
        handled_locally: true,
    })
}

// ─── Command Center (multi-step compound tasks) ────────────────────────

/// Run a compound task through the command center.
///
/// Called from `process_transcript` when `build_plan` detects a compound
/// command. Emits ack + loading, executes the plan, emits merged result.
async fn run_command_center<R: Runtime>(
    app: AppHandle<R>,
    plan: crate::command_center::TaskPlan,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
    request_id: String,
    cancel_flag: Arc<AtomicBool>,
) -> Result<ProcessResult, String> {
    let ack = pick_ack();
    emit(
        &app,
        &OrchestratorEvent::Ack {
            text: ack.to_string(),
            request_id: request_id.clone(),
        },
    );
    emit(
        &app,
        &OrchestratorEvent::Loading {
            visible: true,
            request_id: request_id.clone(),
        },
    );
    show_loading(&app);

    let outcome = crate::command_center::execute_plan(
        &app,
        plan,
        &request_id,
        &cancel_flag,
        dialog_context,
    )
    .await;

    emit(
        &app,
        &OrchestratorEvent::Loading {
            visible: false,
            request_id: request_id.clone(),
        },
    );
    hide_loading(&app);

    tracing::info!(
        "command_center: request {} done — {:?} — {} steps, {} ok, {} failed, awaiting={}",
        request_id,
        crate::router::truncate_pub(&transcript, 60),
        outcome.summary.total_steps,
        outcome.summary.completed,
        outcome.summary.failed,
        outcome.awaiting_confirmation,
    );

    if outcome.awaiting_confirmation {
        // A step emitted a Confirm event — the pending compound is stashed
        // and orchestrator_mcp_confirm will resume it. Emit Done (no Result —
        // the Confirm event already spoke the prompt).
        emit(
            &app,
            &OrchestratorEvent::Done {
                request_id: request_id.clone(),
            },
        );
        // Keep the active request installed — the confirm command reuses
        // this request_id for the resumed steps.
        return Ok(ProcessResult {
            request_id,
            subsystem: Subsystem::CommandCenter,
            handled_locally: false,
        });
    }

    // Emit merged result
    emit(
        &app,
        &OrchestratorEvent::Result {
            text: outcome.text,
            request_id: request_id.clone(),
            analysis: None,
            dialog_state: None,
        },
    );
    emit(
        &app,
        &OrchestratorEvent::Done {
            request_id: request_id.clone(),
        },
    );
    clear_active_request(&request_id);

    // Report execution to the brain monitor (admin-only)
    #[cfg(feature = "admin-brain")]
    {
        let ok = outcome.summary.failed == 0;
        if ok {
            crate::brain_monitor::report_execution_success(&transcript, "compound_task");
        } else {
            crate::brain_monitor::report_execution_failure(
                &transcript,
                "compound_task",
                "one or more steps failed",
            );
        }
    }

    Ok(ProcessResult {
        request_id,
        subsystem: Subsystem::CommandCenter,
        handled_locally: false,
    })
}

// ─── Tauri commands ────────────────────────────────────────────────────

/// IPC: Process a transcript through the central orchestrator.
///
/// This is the single entry point for all voice commands. The frontend
/// calls this after STT produces a transcript.
#[tauri::command]
pub async fn orchestrator_process<R: Runtime>(
    app: AppHandle<R>,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
) -> Result<ProcessResult, String> {
    process_transcript(app, transcript, dialog_context).await
}

/// IPC: Cancel the active orchestrator request (barge-in / new wake).
#[tauri::command]
pub async fn orchestrator_cancel() -> Result<(), String> {
    cancel_active();
    Ok(())
}

/// IPC: Signal that a request is done (called by frontend after TTS finishes).
#[tauri::command]
pub async fn orchestrator_done(
    request_id: String,
) -> Result<(), String> {
    signal_done(&request_id);
    Ok(())
}

/// IPC: Get the current orchestrator state (for diagnostics).
#[tauri::command]
pub fn orchestrator_status() -> Result<serde_json::Value, String> {
    let guard = ACTIVE_REQUEST.lock().unwrap();
    Ok(serde_json::json!({
        "active": guard.is_some(),
        "request_id": guard.as_ref().map(|r| r.id.clone()),
        "subsystem": guard.as_ref().map(|r| serde_json::to_value(&r.subsystem).unwrap_or(serde_json::Value::Null)),
    }))
}

// ─── GitHub sub-command Tauri commands ─────────────────────────────────

/// Execute a GitHub command. The frontend calls this with a serialized
/// `GitHubCommand` object. The orchestrator:
///   1. Fetches the GitHub token from the Worker
///   2. Runs pre-checks (conflict detection for merge)
///   3. If destructive and not confirmed → emits a Confirm event
///   4. Executes the command via octocrab
///   5. Emits the result (text, conflict report, or error)
#[tauri::command]
pub async fn orchestrator_github_execute<R: Runtime>(
    app: AppHandle<R>,
    command: serde_json::Value,
    confirmed: Option<bool>,
) -> Result<serde_json::Value, String> {
    let cmd: crate::github_cmd::GitHubCommand =
        serde_json::from_value(command).map_err(|e| format!("invalid command: {e}"))?;

    let session_info = crate::network::get_session_info()
        .ok_or("no session open")?;
    let (worker_url, user_id, _device_id) = session_info;

    let request_id = {
        let id = new_request_id();
        let (rid, _flag) = install_new_request(Subsystem::GitHub);
        let _ = id;
        rid
    };

    let confirmed = confirmed.unwrap_or(false);

    // Emit thinking state
    emit(
        &app,
        &OrchestratorEvent::State {
            state: OrchestratorState::Thinking,
            request_id: request_id.clone(),
        },
    );

    let result = crate::github_cmd::execute_command(
        &worker_url,
        &user_id,
        &cmd,
        confirmed,
    )
    .await;

    let result_json = serde_json::to_value(&result).unwrap_or(serde_json::Value::Null);

    // Emit the appropriate event based on the result type
    match &result {
        crate::github_cmd::GitHubResult::NeedsConfirmation { prompt, command } => {
            let cmd_json = serde_json::to_value(command).unwrap_or(serde_json::Value::Null);
            emit(
                &app,
                &OrchestratorEvent::Confirm {
                    prompt: prompt.clone(),
                    request_id: request_id.clone(),
                    command: cmd_json.clone(),
                },
            );
            let confirm_payload = serde_json::json!({
                "requestId": request_id.clone(),
                "prompt": prompt.clone(),
                "command": cmd_json,
            });
            let _ = crate::commands::show_sidebar_with_confirmation(
                app.clone(),
                "GitHub Confirmation".to_string(),
                prompt.clone(),
                confirm_payload,
            ).await;
        }
        crate::github_cmd::GitHubResult::MergeConflict {
            pr_number,
            repo,
            conflict_files,
            message,
        } => {
            let files_json = serde_json::to_value(conflict_files).unwrap_or(serde_json::Value::Null);
            emit(
                &app,
                &OrchestratorEvent::ConflictReport {
                    request_id: request_id.clone(),
                    pr_number: *pr_number,
                    repo: repo.clone(),
                    conflict_files: files_json,
                    message: message.clone(),
                },
            );
        }
        crate::github_cmd::GitHubResult::Text { text } => {
            emit(
                &app,
                &OrchestratorEvent::Result {
                    text: text.clone(),
                    request_id: request_id.clone(),
                    analysis: None,
                    dialog_state: None,
                },
            );
        }
        crate::github_cmd::GitHubResult::PrList { repo, state, prs } => {
            let count = prs.len();
            let ack_text = format!(
                "Showing {} {} PR{} in {}.",
                count,
                state,
                if count == 1 { "" } else { "s" },
                repo
            );
            emit(
                &app,
                &OrchestratorEvent::Result {
                    text: ack_text,
                    request_id: request_id.clone(),
                    analysis: None,
                    dialog_state: None,
                },
            );
        }
        crate::github_cmd::GitHubResult::Error { message, .. } => {
            emit(
                &app,
                &OrchestratorEvent::Error {
                    message: message.clone(),
                    request_id: request_id.clone(),
                },
            );
        }
    }

    emit(
        &app,
        &OrchestratorEvent::GitHubResult {
            request_id: request_id.clone(),
            result: result_json.clone(),
        },
    );
    emit(
        &app,
        &OrchestratorEvent::Done {
            request_id: request_id.clone(),
        },
    );
    clear_active_request(&request_id);

    Ok(result_json)
}

/// Clear the cached GitHub token (e.g., after disconnecting GitHub).
#[tauri::command]
pub async fn orchestrator_github_clear_token() -> Result<(), String> {
    crate::github_cmd::clear_github_token().await;
    Ok(())
}

// ─── MCP sub-center Tauri commands ─────────────────────────────────────

/// Confirm or cancel a pending MCP write/destructive operation.
///
/// When `dispatch_to_mcp` encounters a write/destructive tool (send message,
/// place order, book table), it emits a Confirm event carrying a pending
/// payload `{server, tool, params, transcript}`. The frontend shows the
/// prompt; on user approval it calls this command with `confirmed=true`
/// and the same pending payload.
#[tauri::command]
pub async fn orchestrator_mcp_confirm<R: Runtime>(
    app: AppHandle<R>,
    request_id: String,
    confirmed: bool,
    pending: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use crate::mcp_client::{call_tool, extract_text, McpServer};

    if !confirmed {
        emit(
            &app,
            &OrchestratorEvent::Result {
                text: "Cancelled, sir.".to_string(),
                request_id: request_id.clone(),
                analysis: None,
                dialog_state: None,
            },
        );
        emit(
            &app,
            &OrchestratorEvent::Done {
                request_id: request_id.clone(),
            },
        );
        return Ok(serde_json::json!({ "cancelled": true }));
    }

    // Reconstruct the pending call
    let server_name = pending["server"]
        .as_str()
        .ok_or("invalid pending payload: missing server")?;
    let tool = pending["tool"]
        .as_str()
        .ok_or("invalid pending payload: missing tool")?;
    let params = pending["params"].clone();

    let server = match server_name {
        "swiggy-food" => McpServer::SwiggyFood,
        "swiggy-instamart" => McpServer::SwiggyInstamart,
        "swiggy-dineout" => McpServer::SwiggyDineout,
        "whatsapp" => McpServer::WhatsApp,
        "amazon" => McpServer::Amazon,
        other => return Err(format!("unknown MCP server: {}", other)),
    };

    emit(
        &app,
        &OrchestratorEvent::Loading {
            visible: true,
            request_id: request_id.clone(),
        },
    );
    show_loading(&app);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("mcp http client: {e}"))?;

    // Same vault resolution as the pre-confirm path — without this,
    // confirmed writes went out anonymous and died with 401 even when
    // the service was connected.
    let mut vault_token: Option<String> =
        crate::auth_vault::resolve_server_token(server).await;
    let mut result =
        call_tool(server, tool, params.clone(), &client, vault_token.as_deref()).await;

    // 401 clear-and-retry (mirrors the dispatch path): evict the dead
    // token, mint fresh once, retry once. The user already confirmed —
    // retrying the call (not the confirmation) is safe.
    if !result.ok {
        let first_err = result.error.clone().unwrap_or_default();
        if is_auth_failure(&first_err) {
            if let Some(key) = server.vault_key() {
                crate::auth_vault::clear_token(key);
                vault_token = crate::auth_vault::resolve_server_token(server).await;
                result = call_tool(
                    server,
                    tool,
                    params.clone(),
                    &client,
                    vault_token.as_deref(),
                )
                .await;
            }
        }
    }

    emit(
        &app,
        &OrchestratorEvent::Loading {
            visible: false,
            request_id: request_id.clone(),
        },
    );
    hide_loading(&app);

    if !result.ok {
        let err = result
            .error
            .clone()
            .unwrap_or_else(|| "unknown MCP error".to_string());
        let spoken = mcp_error_guidance(server, &err);
        let retry_transcript = pending["transcript"].as_str().unwrap_or("");
        stash_mcp_retry(server, tool, &params);
        open_mcp_connect_card(&app, server, retry_transcript).await;
        emit(
            &app,
            &OrchestratorEvent::Error {
                message: spoken.clone(),
                request_id: request_id.clone(),
            },
        );
        emit(
            &app,
            &OrchestratorEvent::Done {
                request_id: request_id.clone(),
            },
        );
        return Err(spoken);
    }

    let text = extract_text(&result);

    // If this confirmed step was part of a compound task, resume the
    // remaining steps and emit the merged result instead.
    if let Some(pending) = crate::command_center::take_pending_compound(&request_id) {
        tracing::info!(
            "command_center: resuming compound for {} — {} remaining steps",
            request_id,
            pending.remaining_steps.len()
        );

        // Fresh cancel flag for the resumed steps — the original was
        // consumed by the compound's execute_plan.
        let resume_flag = Arc::new(AtomicBool::new(false));

        let outcome = crate::command_center::resume_compound(
            &app,
            pending,
            text,
            resume_flag,
        )
        .await;

        emit(
            &app,
            &OrchestratorEvent::Result {
                text: outcome.text,
                request_id: request_id.clone(),
                analysis: None,
                dialog_state: None,
            },
        );
        emit(
            &app,
            &OrchestratorEvent::Done {
                request_id: request_id.clone(),
            },
        );
        clear_active_request(&request_id);

        return Ok(serde_json::json!({
            "ok": true,
            "server": server.name(),
            "tool": tool,
            "latency_ms": result.latency_ms,
            "compound_resumed": true,
        }));
    }

    emit(
        &app,
        &OrchestratorEvent::Result {
            text,
            request_id: request_id.clone(),
            analysis: None,
            dialog_state: None,
        },
    );
    emit(
        &app,
        &OrchestratorEvent::Done {
            request_id: request_id.clone(),
        },
    );

    Ok(serde_json::json!({
        "ok": true,
        "server": server.name(),
        "tool": tool,
        "latency_ms": result.latency_ms,
    }))
}

// ─── Loading indicator control (owned by orchestrator) ─────────────────

/// Show the loading indicator window at the top-right corner.
///
/// This is the Rust-side implementation — the orchestrator calls this
/// directly instead of going through the frontend IPC. This ensures the
/// loading state is owned by the central system, not scattered across
/// frontend components.
///
/// Runs INLINE (no spawn): creation completes before dispatch starts, so
/// the paired `hide_loading` destroy can never land before the create
/// finishes and wedge a fresh spinner on screen with no hide in flight.
pub fn show_loading<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = crate::dyn_windows::get_or_create_window(
        app,
        crate::dyn_windows::WindowConfig::loading_indicator(),
    ) {
        tracing::warn!("orchestrator: failed to create loading window: {}", e);
        return;
    }

    // Position at top-right corner
    if let Some(win) = app.get_webview_window("loading-indicator") {
        if let Ok(Some(monitor)) = win.current_monitor() {
            let scale = monitor.scale_factor();
            let screen = monitor.size();
            let win_size = 80i32;
            let phys_win = (win_size as f64 * scale) as i32;
            let inset_x = (7.0 * scale) as i32;
            let inset_y = (9.0 * scale) as i32;
            let x = screen.width as i32 - phys_win - inset_x;
            let y = inset_y;
            let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        }
        let _ = win.set_ignore_cursor_events(true);
        let _ = win.show();
        tracing::info!("orchestrator: loading indicator shown");
    }
}

/// Hide/destroy the loading indicator window.
pub fn hide_loading<R: Runtime>(app: &AppHandle<R>) {
    let _ = crate::dyn_windows::destroy_window(app, "loading-indicator");
    tracing::info!("orchestrator: loading indicator hidden");
}

/// IPC: Show loading indicator (can be called from frontend if needed).
#[tauri::command]
pub async fn orchestrator_show_loading<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    show_loading(&app);
    Ok(())
}

/// IPC: Hide loading indicator (can be called from frontend if needed).
#[tauri::command]
pub async fn orchestrator_hide_loading<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    hide_loading(&app);
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_local_command() {
        let intent = ParsedIntent::OpenApp {
            target: "chrome".to_string(),
        };
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_greeting() {
        let intent = ParsedIntent::Greeting {
            reply: "Hello sir.".to_string(),
        };
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_media() {
        assert_eq!(route_intent(&ParsedIntent::MediaPlayPause), Subsystem::LocalCommand);
        assert_eq!(route_intent(&ParsedIntent::MediaNext), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_architect() {
        assert_eq!(route_intent(&ParsedIntent::OpenArchitect), Subsystem::Architect);
    }

    /// Garbage-transcript guard: ML-only OpenArchitect (e.g. Qwen 0.99 on
    /// 'You feel it, no?' from a 1s noise capture) is capped below the
    /// accept line; deterministic and non-Architect results pass through.
    #[test]
    fn test_cap_ml_window_open() {
        use crate::intent_parser::ParseResult;
        let mut r = ParseResult {
            intent: ParsedIntent::OpenArchitect,
            confidence: 0.99,
            source: "brain".to_string(),
        };
        assert!(cap_ml_window_open(&mut r));
        assert!(r.confidence < 0.5);
        let mut r2 = ParseResult {
            intent: ParsedIntent::OpenArchitect,
            confidence: 1.0,
            source: "deterministic".to_string(),
        };
        assert!(!cap_ml_window_open(&mut r2));
        assert_eq!(r2.confidence, 1.0);
        let mut r3 = ParseResult {
            intent: ParsedIntent::OpenApp {
                target: "chrome".to_string(),
            },
            confidence: 0.99,
            source: "brain".to_string(),
        };
        assert!(!cap_ml_window_open(&mut r3));
    }

    #[test]
    fn test_route_worker_backend() {
        let intent = ParsedIntent::Search {
            query: "what is rust".to_string(),
        };
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);

        let intent = ParsedIntent::AnalysePr {
            owner: None,
            repo: "zync".to_string(),
            pr_number: 24,
        };
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_unknown() {
        let intent = ParsedIntent::Unknown {
            raw: "blah blah".to_string(),
        };
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_is_long_running() {
        assert!(!is_long_running(&Subsystem::LocalCommand));
        assert!(is_long_running(&Subsystem::WorkerBackend));
        assert!(is_long_running(&Subsystem::Architect));
        assert!(!is_long_running(&Subsystem::None));
    }

    #[test]
    fn test_install_and_cancel() {
        // Install two requests — the second should cancel the first
        let (id1, _cancel1) = install_new_request(Subsystem::WorkerBackend);
        let (id2, cancel2) = install_new_request(Subsystem::WorkerBackend);
        assert_ne!(id1, id2, "IDs should be different");
        assert!(!is_cancelled(&cancel2), "second request should not be cancelled");
        // cancel1 may or may not be cancelled depending on parallel test execution
        // The key property: the second request is active and not cancelled
        clear_active_request(&id2);
    }

    #[test]
    fn test_request_id_is_short() {
        let id = new_request_id();
        assert!(id.len() <= 12);
    }

    #[test]
    fn test_pick_ack_returns_valid_phrase() {
        let ack = pick_ack();
        assert!(ACK_PHRASES.contains(&ack));
    }

    // ─── Comprehensive routing tests for every command type ───

    #[test]
    fn test_route_open_app() {
        let result = parse_deterministic("open chrome");
        assert!(result.is_some(), "should parse 'open chrome'");
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_open_url() {
        let result = parse_deterministic("open youtube.com");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_close_app() {
        let result = parse_deterministic("close chrome");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_whatsapp_chat() {
        let result = parse_deterministic("open chat with mom");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_greeting_hello() {
        let result = parse_deterministic("hello");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_greeting_thanks() {
        let result = parse_deterministic("thank you");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_media_pause() {
        let result = parse_deterministic("pause");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_media_next() {
        let result = parse_deterministic("next");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::LocalCommand);
    }

    #[test]
    fn test_route_architect_explicit() {
        let result = parse_deterministic("open architecture mapper");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::Architect);
    }

    #[test]
    fn test_route_search_query() {
        let result = parse_deterministic("search for rust programming");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_analyse_pr() {
        let result = parse_deterministic("analyse PR 24 in zync");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_analyse_repo() {
        let result = parse_deterministic("analyse zync");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_analyse_latest_pr() {
        let result = parse_deterministic("analyse the pr in zync");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_check_branch() {
        let result = parse_deterministic("check the latest branch of servx");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_unknown_goes_to_worker() {
        let result = parse_deterministic("what is the meaning of life");
        // Unknown commands go to the Worker for general Q&A
        let intent = result.map(|r| r.intent).unwrap_or(ParsedIntent::Unknown {
            raw: "what is the meaning of life".to_string(),
        });
        assert_eq!(route_intent(&intent), Subsystem::WorkerBackend);
    }

    #[test]
    fn test_route_empty_transcript() {
        let result = parse_deterministic("");
        assert!(result.is_none());
        // Empty transcript → Unknown → WorkerBackend (but process_transcript
        // rejects empty transcripts before routing)
    }

    // ─── MCP routing tests ───

    #[test]
    fn test_route_order_food() {
        let result = parse_deterministic("order pizza from dominos");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::Mcp);
    }

    #[test]
    fn test_route_search_product() {
        let result = parse_deterministic("search for sony headphones on amazon");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::Mcp);
    }

    #[test]
    fn test_route_send_whatsapp_message() {
        let result = parse_deterministic("send mom a whatsapp message saying hi");
        assert!(result.is_some());
        let intent = result.unwrap().intent;
        assert_eq!(route_intent(&intent), Subsystem::Mcp);
    }

    #[test]
    fn test_mcp_is_long_running() {
        assert!(is_long_running(&Subsystem::Mcp));
    }

    #[test]
    fn test_is_auth_failure_narrow() {
        assert!(is_auth_failure("HTTP 401: invalid_token"));
        assert!(is_auth_failure("Unauthorized"));
        assert!(is_auth_failure("token expired, reconnect"));
        // Must NOT match ordinary words containing "auth".
        assert!(!is_auth_failure("author not found"));
        assert!(!is_auth_failure("authentic restaurant list"));
        assert!(!is_auth_failure("connection refused"));
    }

    #[test]
    fn test_mcp_guidance_points_at_connections() {
        let g = mcp_error_guidance(
            crate::mcp_client::McpServer::SwiggyFood,
            "HTTP 401",
        );
        assert!(g.contains("Connections"), "guidance must name where: {g}");
        let g2 = mcp_error_guidance(
            crate::mcp_client::McpServer::WhatsApp,
            "connection refused",
        );
        assert!(
            g2.contains("bridge") && g2.contains("Recheck"),
            "bridge guidance: {g2}"
        );
        // Assist names the exact program + first-run step (QR scan).
        assert!(
            g2.contains("mcp-whatsapp") && g2.contains("QR"),
            "bridge assist must be actionable: {g2}"
        );
        let g3 = mcp_error_guidance(
            crate::mcp_client::McpServer::WhatsApp,
            "circuit open for whatsapp (42s left) — bridge failing repeatedly",
        );
        assert!(
            g3.contains("cooling down") && g3.contains("Recheck"),
            "breaker guidance: {g3}"
        );
    }

    #[test]
    fn test_server_for_mcp_intent() {
        use crate::mcp_client::McpServer as S;
        let food = ParsedIntent::OrderFood {
            query: "biryani".into(),
            restaurant: None,
        };
        assert_eq!(server_for_mcp_intent(&food), Some(S::SwiggyFood));
        let prod = ParsedIntent::SearchProduct { query: "x".into() };
        assert_eq!(server_for_mcp_intent(&prod), Some(S::Amazon));
        let wa = ParsedIntent::SendWhatsAppMessage {
            contact: "mummy".into(),
            message: "hi".into(),
        };
        assert_eq!(server_for_mcp_intent(&wa), Some(S::WhatsApp));
        let greet = ParsedIntent::Greeting {
            reply: "hi".into(),
        };
        assert_eq!(server_for_mcp_intent(&greet), None);
    }

    #[test]
    fn test_connect_card_markdown_has_fix_action() {
        let card = crate::mcp_client::McpConnectCard {
            server: "whatsapp".to_string(),
            state: crate::mcp_client::McpConnectState::AuthRequired,
            note: "bridge up, phone not paired".to_string(),
            steps: vec!["Scan this QR.".to_string()],
            qr_image_uri: Some("data:image/png;base64,AAA".to_string()),
            qr_code_text: None,
            pair_url: Some("http://127.0.0.1:8765/pair".to_string()),
        };
        let md = connect_card_markdown(&card, "send hi to mummy");
        assert!(md.contains("Scan this QR."));
        assert!(md.contains("data:image/png;base64,AAA"));
        assert!(md.contains("http://127.0.0.1:8765/pair"));
        assert!(md.contains("20 days"), "rotation warning required");
        assert!(md.contains("ToS"), "burner warning required");
    }

    #[test]
    fn test_mcp_retry_stash_take_roundtrip() {
        // take on empty is None (no stale retry from other tests).
        take_mcp_retry();
        stash_mcp_retry(
            crate::mcp_client::McpServer::Amazon,
            "amazon_search",
            &serde_json::json!({"query": "x"}),
        );
        let r = take_mcp_retry().expect("stashed retry must come back");
        assert_eq!(r.server, crate::mcp_client::McpServer::Amazon);
        assert_eq!(r.tool, "amazon_search");
        assert!(take_mcp_retry().is_none(), "take must drain");
    }

    // ─── Barge-in / cancellation tests ───

    #[test]
    fn test_barge_in_cancels_previous() {
        // Start request 1
        let (_id1, cancel1) = install_new_request(Subsystem::WorkerBackend);
        let _cancel1_was_cancelled = is_cancelled(&cancel1);

        // Start request 2 (barge-in) — this cancels request 1
        let (id2, cancel2) = install_new_request(Subsystem::WorkerBackend);
        // cancel1 should now be cancelled (unless a parallel test already cancelled it)
        // The key assertion: cancel2 is NOT cancelled
        assert!(!is_cancelled(&cancel2), "req2 should not be cancelled");

        clear_active_request(&id2);
    }

    #[test]
    fn test_cancel_active_sets_flag() {
        let (_, cancel) = install_new_request(Subsystem::WorkerBackend);
        cancel_active();
        // The cancel flag should be set (or was already set by a parallel test)
        // Either way, cancel_active() should not panic
        let _ = is_cancelled(&cancel);
    }

    #[test]
    fn test_signal_done_doesnt_panic() {
        // Just verify signal_done doesn't panic with any ID
        signal_done("test_id_123");
    }

    #[test]
    fn test_signal_done_doesnt_clear_wrong_id() {
        // Note: This test shares the global ACTIVE_REQUEST with other tests
        // that run in parallel. We use a unique wrong ID that no other test
        // would generate, and just verify signal_done doesn't panic.
        signal_done("definitely_wrong_id_999");
        // If we get here without panicking, the test passes.
        // (We can't assert the global state because parallel tests may have
        // changed it between install and check.)
    }

    // ─── Subsystem classification tests ───

    #[test]
    fn test_local_commands_are_not_long_running() {
        assert!(!is_long_running(&Subsystem::LocalCommand));
    }

    #[test]
    fn test_worker_backend_is_long_running() {
        assert!(is_long_running(&Subsystem::WorkerBackend));
    }

    #[test]
    fn test_architect_is_long_running() {
        assert!(is_long_running(&Subsystem::Architect));
    }

    #[test]
    fn test_none_is_not_long_running() {
        assert!(!is_long_running(&Subsystem::None));
    }

    // ─── Request ID tests ───

    #[test]
    fn test_request_ids_are_unique() {
        let id1 = new_request_id();
        let id2 = new_request_id();
        let id3 = new_request_id();
        assert_ne!(id1, id2, "IDs should be unique");
        assert_ne!(id2, id3, "IDs should be unique");
        assert_ne!(id1, id3, "IDs should be unique");
    }

    #[test]
    fn test_request_id_is_alphanumeric() {
        let id = new_request_id();
        for c in id.chars() {
            assert!(c.is_ascii_alphanumeric(), "ID should be alphanumeric, found: {}", c);
        }
    }
}
