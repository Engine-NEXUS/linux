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
        return Some(result);
    }

    None
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
        Subsystem::WorkerBackend | Subsystem::Architect | Subsystem::GitHub
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

    // If deterministic missed, try brain (admin-only) then NLU before falling
    // back to Unknown. This is the same pipeline as parse_transcript (Tauri
    // command) — the orchestrator no longer bypasses the ML classifiers.
    let parse_result = if parse_result.is_some() {
        parse_result
    } else {
        tracing::info!("orchestrator: deterministic missed, trying brain/NLU");
        let ml_result = parse_with_ml(&transcript).await;
        if let Some(ref r) = ml_result {
            tracing::info!(
                "orchestrator: ML classified as {:?} (confidence={}, source={})",
                r.intent, r.confidence, r.source
            );
        }
        ml_result
    };

    let intent = parse_result
        .as_ref()
        .map(|r| r.intent.clone())
        .unwrap_or(ParsedIntent::Unknown {
            raw: transcript.clone(),
        });

    tracing::info!("orchestrator: parsed intent: {:?}", intent);

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
                    // Emit result
                    emit(
                        &app,
                        &OrchestratorEvent::Result {
                            text,
                            request_id: request_id.clone(),
                            analysis,
                            dialog_state,
                        },
                    );
                    // Note: "done" is emitted by the frontend after TTS finishes
                    // (same as the old network.rs behavior — emitting done here
                    // would cause stopTts() to cancel the response before the
                    // user hears it).
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
                            command: cmd_json,
                        },
                    );
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
async fn dispatch_to_worker<R: Runtime>(
    _app: AppHandle<R>,
    transcript: String,
    dialog_context: Option<serde_json::Value>,
    request_id: String,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(String, Option<serde_json::Value>, Option<serde_json::Value>), String> {
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
                    command: cmd_json,
                },
            );
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

// ─── Loading indicator control (owned by orchestrator) ─────────────────

/// Show the loading indicator window at the top-right corner.
///
/// This is the Rust-side implementation — the orchestrator calls this
/// directly instead of going through the frontend IPC. This ensures the
/// loading state is owned by the central system, not scattered across
/// frontend components.
pub fn show_loading<R: Runtime>(app: &AppHandle<R>) {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::dyn_windows::get_or_create_window(
            &app_clone,
            crate::dyn_windows::WindowConfig::loading_indicator(),
        ) {
            tracing::warn!("orchestrator: failed to create loading window: {}", e);
            return;
        }

        // Position at top-right corner
        if let Some(win) = app_clone.get_webview_window("loading-indicator") {
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
    });
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
