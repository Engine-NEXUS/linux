//! Command Center — n8n-style multi-step task orchestration.
//!
//! The main command center splits compound commands ("X then Y"),
//! plans each step, routes it to a sub-center, tracks one unified
//! task state, enforces confirmation gates, and merges results.
//!
//! ## Architecture
//!
//! ```text
//! Transcript
//!   → split_compound()     — "A then B" → ["A", "B"]
//!   → build_plan()         — parse + route each step → TaskPlan
//!   → execute_plan()       — run steps sequentially
//!       → LocalCommand  → command_executor (Rust, <5ms)
//!       → Mcp           → mcp_client (JSON-RPC, confirmation-gated)
//!       → WorkerBackend → Worker / 9Router
//!       → GitHub        → github_cmd (read ops inline)
//!   → merge results → single response
//! ```
//!
//! ## Task State
//!
//! Every compound task produces a `TaskPlan` — the "one state":
//! ordered `PlanStep`s, per-step `StepResult`s, and a run summary.
//! When a step hits a confirmation gate, the remaining steps are
//! stashed in `PENDING_COMPOUND` and resumed by
//! `orchestrator_mcp_confirm` after the user approves.
//!
//! ## Sequential vs parallel
//!
//! v1 executes steps sequentially ("then" implies ordering).
//! Independent parallel steps are future work.

use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::center::TurnContext;
use crate::intent_parser::{parse_deterministic, ParsedIntent};
use crate::orchestrator::{route_intent, Subsystem};

// ─── Types ─────────────────────────────────────────────────────────────

/// One step in a compound task plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    /// Step index (0-based).
    pub step_id: usize,
    /// The sub-transcript for this step (e.g. "order pizza").
    pub transcript: String,
    /// The parsed intent for this step.
    pub intent: ParsedIntent,
    /// Which subsystem will execute this step.
    pub subsystem: Subsystem,
    /// Which previous steps must complete first (v1: always previous).
    pub depends_on: Vec<usize>,
    /// If true, failure doesn't abort the task.
    pub optional: bool,
}

/// Per-step execution status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Succeeded,
    Failed,
    Skipped,
    /// Step emitted a confirmation gate — task paused until user approves.
    AwaitingConfirmation,
    /// Step subsystem isn't supported inside compound tasks.
    Unsupported,
}

/// The result of executing one plan step.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct StepResult {
    pub step_id: usize,
    pub subsystem: Subsystem,
    pub status: StepStatus,
    /// Human-readable result text (or error description).
    pub text: String,
    pub error: Option<String>,
    pub latency_ms: u64,
}

/// The unified task state — the "one state" for a compound command.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TaskPlan {
    /// Unique task ID (matches the orchestrator request_id).
    pub task_id: String,
    /// The full original transcript.
    pub transcript: String,
    /// Ordered steps.
    pub steps: Vec<PlanStep>,
    /// Results collected so far (one per executed step).
    pub results: Vec<StepResult>,
}

/// Run summary — which stages succeeded/failed.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub total_steps: usize,
    pub completed: usize,
    pub failed: usize,
    pub skipped: usize,
    /// True if the task paused on a confirmation gate.
    pub awaiting_confirmation: bool,
}

impl TaskPlan {
    /// Build a run summary from the collected results.
    pub fn summary(&self) -> RunSummary {
        let mut completed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut awaiting = false;
        for r in &self.results {
            match r.status {
                StepStatus::Succeeded => completed += 1,
                StepStatus::Failed | StepStatus::Unsupported => failed += 1,
                StepStatus::Skipped => skipped += 1,
                StepStatus::AwaitingConfirmation => awaiting = true,
                StepStatus::Pending => {}
            }
        }
        RunSummary {
            total_steps: self.steps.len(),
            completed,
            failed,
            skipped,
            awaiting_confirmation: awaiting,
        }
    }
}

/// Pending compound task — remaining steps stashed while a confirmation
/// gate is open. Resumed by `orchestrator_mcp_confirm`.
#[derive(Debug)]
pub struct PendingCompound {
    /// Steps not yet executed (the gated step is NOT included — it's
    /// executed by the confirm command itself).
    pub remaining_steps: Vec<PlanStep>,
    /// Results from steps that already ran (for the final merge).
    pub prior_results: Vec<StepResult>,
    /// The orchestrator request_id this compound belongs to.
    pub request_id: String,
    /// The full original transcript.
    #[allow(dead_code)]
    pub transcript: String,
    /// Provenance inherited from the turn that created the compound, so
    /// resumed steps keep the original ownership decision.
    pub turn: TurnContext,
}

static PENDING_COMPOUND: Mutex<Option<PendingCompound>> = Mutex::new(None);

// ─── Durable execution: crash-safe checkpoints (B5) ────────────────
// After every loop iteration the plan state is written to
// `compound_<task_id>.json` under the app data dir. A crash leaves the
// record behind (no silent loss); stale files (>1h) are swept on boot
// with a log line. Clean completion and confirmation-pause clear the
// file (confirmation resume stays owned by PENDING_COMPOUND).

/// On-disk checkpoint record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanCheckpoint {
    pub task_id: String,
    pub transcript: String,
    pub steps: Vec<PlanStep>,
    pub results: Vec<StepResult>,
    pub next_index: usize,
    pub updated_at: i64,
}

/// Checkpoints older than this are swept on boot.
#[allow(dead_code)]
pub const CHECKPOINT_STALE_SECS: i64 = 3600;

fn sanitize_task_id(task_id: &str) -> String {
    task_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(64)
        .collect()
}

fn checkpoint_path(app_data_dir: &std::path::Path, task_id: &str) -> std::path::PathBuf {
    app_data_dir.join(format!("compound_{}.json", sanitize_task_id(task_id)))
}

/// Write the current plan state. Best-effort — failures only warn.
pub fn write_checkpoint(
    app_data_dir: &std::path::Path,
    plan: &TaskPlan,
    next_index: usize,
) {
    let cp = PlanCheckpoint {
        task_id: plan.task_id.clone(),
        transcript: plan.transcript.clone(),
        steps: plan.steps.clone(),
        results: plan.results.clone(),
        next_index,
        updated_at: chrono::Utc::now().timestamp(),
    };
    match serde_json::to_string_pretty(&cp) {
        Ok(s) => {
            if let Err(e) = std::fs::write(checkpoint_path(app_data_dir, &plan.task_id), s) {
                tracing::warn!("command_center: checkpoint write failed: {}", e);
            }
        }
        Err(e) => tracing::warn!("command_center: checkpoint serialize failed: {}", e),
    }
}

/// Delete a task's checkpoint (clean completion / confirmation-pause).
pub fn clear_checkpoint(app_data_dir: &std::path::Path, task_id: &str) {
    let _ = std::fs::remove_file(checkpoint_path(app_data_dir, task_id));
}

/// Read a checkpoint back (roundtrip / future resume UI).
#[allow(dead_code)]
pub fn read_checkpoint(
    app_data_dir: &std::path::Path,
    task_id: &str,
) -> Option<PlanCheckpoint> {
    let content = std::fs::read_to_string(checkpoint_path(app_data_dir, task_id)).ok()?;
    serde_json::from_str(&content).ok()
}

/// Delete checkpoints older than CHECKPOINT_STALE_SECS. Returns count.
/// Called once on boot — a crash never auto-executes, it only leaves a
/// record that is swept here with a log line.
#[allow(dead_code)]
pub fn sweep_stale_checkpoints(app_data_dir: &std::path::Path) -> usize {
    let now = chrono::Utc::now().timestamp();
    let Ok(entries) = std::fs::read_dir(app_data_dir) else {
        return 0;
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("compound_") || !name.ends_with(".json") {
            continue;
        }
        let path = entry.path();
        let stale = std::fs::read_to_string(&path)
            .ok()
            .and_then(|c| serde_json::from_str::<PlanCheckpoint>(&c).ok())
            .map(|cp| now - cp.updated_at > CHECKPOINT_STALE_SECS)
            .unwrap_or(true);
        if stale && std::fs::remove_file(&path).is_ok() {
            swept += 1;
        }
    }
    if swept > 0 {
        tracing::info!("command_center: swept {} stale compound checkpoint(s)", swept);
    }
    swept
}

/// Stash a pending compound task (overwrites any previous — only one
/// active request exists at a time).
fn set_pending_compound(p: PendingCompound) {
    *PENDING_COMPOUND.lock().unwrap() = Some(p);
}

/// Take the pending compound for a request_id (if any).
pub fn take_pending_compound(request_id: &str) -> Option<PendingCompound> {
    let mut guard = PENDING_COMPOUND.lock().unwrap();
    match guard.as_ref() {
        Some(p) if p.request_id == request_id => guard.take(),
        _ => None,
    }
}

/// Whether a pending compound exists for this request.
#[allow(dead_code)]
pub fn has_pending_compound(request_id: &str) -> bool {
    PENDING_COMPOUND
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.request_id == request_id)
        .unwrap_or(false)
}

// ─── Compound detection ────────────────────────────────────────────────

/// Sequencing connectors that split a compound command.
/// Deliberately does NOT include bare " and " — "search and rescue",
/// "rock and roll", "mum and dad" would all split wrongly.
const STEP_SEPARATORS: &[&str] = &[
    " and then ",
    " then ",
    ", then ",
    " after that ",
    " afterwards ",
    "; ",
];

/// Split a compound transcript into ordered sub-transcripts.
///
/// "check movies then order dinner" → ["check movies", "order dinner"]
/// "open chrome" → ["open chrome"] (single — not compound)
///
/// Returns the parts in order. Returns a 1-element vec if no separator
/// is found (caller treats len < 2 as single-step).
pub fn split_compound(transcript: &str) -> Vec<String> {
    let mut text = transcript.trim().to_lowercase();

    // Strip a dangling trailing connector — "open chrome then" (STT
    // trailing pause) isn't compound; the "then" belongs to nothing.
    for sep in [" and then", " then", " after that", " afterwards"] {
        if let Some(stripped) = text.strip_suffix(sep) {
            text = stripped.trim_end().to_string();
            break;
        }
    }

    let mut parts = vec![text];
    for sep in STEP_SEPARATORS {
        let mut next = Vec::new();
        for part in parts {
            let mut remainder = part.as_str();
            while let Some(pos) = remainder.find(sep) {
                next.push(remainder[..pos].trim().to_string());
                remainder = &remainder[pos + sep.len()..];
            }
            next.push(remainder.trim().to_string());
        }
        parts = next;
    }
    // Strip trailing punctuation each split may leave behind
    // ("open spotify, then pause" → "open spotify," → "open spotify").
    for part in &mut parts {
        let trimmed = part.trim_end_matches([',', '.', ';']).trim_end().to_string();
        *part = trimmed;
    }
    parts.retain(|p| !p.is_empty());
    parts
}

// ─── Plan building ─────────────────────────────────────────────────────

/// Build a `TaskPlan` from a compound transcript.
///
/// Returns `None` when:
/// - the transcript isn't compound (< 2 parts after splitting), or
/// - any part fails deterministic parsing (fall back to single-intent
///   path — the Worker/9Router can still answer it), or
/// - any part routes to Architect (window-managed, not composable).
///
/// This is intentionally conservative: a bad compound split should never
/// produce a worse outcome than today's single-intent path.
pub fn build_plan(transcript: &str, task_id: &str) -> Option<TaskPlan> {
    let parts = split_compound(transcript);
    if parts.len() < 2 {
        return None;
    }

    let mut steps = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        let parsed = parse_deterministic(part)?;
        let subsystem = route_intent(&parsed.intent);
        // Architect is window-managed by the frontend — can't compose.
        if subsystem == Subsystem::Architect || subsystem == Subsystem::None {
            return None;
        }
        steps.push(PlanStep {
            step_id: i,
            transcript: part.clone(),
            intent: parsed.intent,
            subsystem,
            depends_on: if i == 0 { vec![] } else { vec![i - 1] },
            optional: false,
        });
    }

    Some(TaskPlan {
        task_id: task_id.to_string(),
        transcript: transcript.to_string(),
        steps,
        results: Vec::new(),
    })
}

/// Async variant of `build_plan` — admin machines can let the Qwen
/// brain classify steps the deterministic parser misses.
///
/// Falls back identically to `build_plan`: if every step parses
/// deterministically, the brain is never invoked (zero added latency).
/// On non-admin devices or when the brain is unavailable, returns the
/// same `None` the sync path would.
///
/// Safety: the brain path keeps the same conservative gates —
/// Architect/None steps still abort the plan, and brain-classified
/// intents flow through the same confirmation-gated dispatch as
/// deterministic ones.
pub async fn build_plan_with_brain(transcript: &str, task_id: &str) -> Option<TaskPlan> {
    // Fast path — fully deterministic plan needs no brain.
    if let Some(plan) = build_plan(transcript, task_id) {
        return Some(plan);
    }

    #[cfg(not(feature = "admin-brain"))]
    return None;

    #[cfg(feature = "admin-brain")]
    {
        if !crate::admin_config::is_admin() {
            return None;
        }
        let parts = split_compound(transcript);
        if parts.len() < 2 {
            return None;
        }
        let mut steps = Vec::with_capacity(parts.len());
        for (i, part) in parts.iter().enumerate() {
            let parsed = match parse_deterministic(part) {
                Some(p) => p,
                None => match crate::brain_client::brain_classify(part).await {
                    Some(p) => p,
                    None => return None,
                },
            };
            let subsystem = route_intent(&parsed.intent);
            if subsystem == Subsystem::Architect
                || subsystem == Subsystem::None
                || subsystem == Subsystem::CommandCenter
            {
                return None;
            }
            steps.push(PlanStep {
                step_id: i,
                transcript: part.clone(),
                intent: parsed.intent,
                subsystem,
                depends_on: if i == 0 { vec![] } else { vec![i - 1] },
                optional: false,
            });
        }
        Some(TaskPlan {
            task_id: task_id.to_string(),
            transcript: transcript.to_string(),
            steps,
            results: Vec::new(),
        })
    }
}

// ─── Step execution ────────────────────────────────────────────────────

/// Outcome of executing one step.
pub(crate) enum StepOutcome {
    /// Step finished (success or failure) — result recorded.
    Done(StepResult),
    /// Step hit a confirmation gate — Confirm event already emitted,
    /// remaining steps must pause.
    AwaitingConfirmation(StepResult),
}

/// Map a `ParsedIntent` to the local `command_executor::Intent`.
/// Returns `None` for intents that can't run locally.
pub(crate) fn to_local_intent(intent: &ParsedIntent) -> Option<crate::command_executor::Intent> {
    use crate::command_executor::Intent as L;
    Some(match intent {
        ParsedIntent::OpenApp { target } => L::OpenApp {
            target: target.clone(),
        },
        ParsedIntent::OpenUrl { target, url } => L::OpenUrl {
            target: target.clone(),
            url: url.clone(),
        },
        ParsedIntent::CloseApp { target } => L::CloseApp {
            target: target.clone(),
        },
        ParsedIntent::WhatsappChat { contact } => L::WhatsappChat {
            contact: contact.clone(),
        },
        ParsedIntent::Search { query } => L::Search {
            query: query.clone(),
        },
        ParsedIntent::MediaPlayPause => L::MediaPlayPause,
        ParsedIntent::MediaNext => L::MediaNext,
        ParsedIntent::MediaPrevious => L::MediaPrevious,
        ParsedIntent::MediaStop => L::MediaStop,
        ParsedIntent::Greeting { reply } => L::Greeting {
            reply: reply.clone(),
        },
        _ => return None,
    })
}

/// Execute one plan step. Returns the step outcome.
///
/// The caller (execute_plan / resume flow) owns all event emission —
/// this function only runs the step and reports the result. The one
/// exception is confirmation gating, which emits the Confirm event
/// itself because it needs the AppHandle.
pub(crate) async fn execute_step<R: Runtime>(
    app: &AppHandle<R>,
    step: &PlanStep,
    request_id: &str,
    cancel_flag: &Arc<AtomicBool>,
    dialog_context: Option<&serde_json::Value>,
    turn: &TurnContext,
) -> StepOutcome {
    let start = Instant::now();
    let mk = |status: StepStatus, text: String, error: Option<String>| StepResult {
        step_id: step.step_id,
        subsystem: step.subsystem.clone(),
        status,
        text,
        error,
        latency_ms: start.elapsed().as_millis() as u64,
    };

    if crate::orchestrator::is_cancelled_pub(cancel_flag) {
        return StepOutcome::Done(mk(
            StepStatus::Skipped,
            "cancelled".to_string(),
            Some("cancelled".to_string()),
        ));
    }

    match step.subsystem {
        Subsystem::LocalCommand => {
            let Some(local) = to_local_intent(&step.intent) else {
                return StepOutcome::Done(mk(
                    StepStatus::Unsupported,
                    "not supported locally".to_string(),
                    None,
                ));
            };
            match crate::command_executor::execute_command(local).await {
                Ok(res) => StepOutcome::Done(mk(
                    if res.success {
                        StepStatus::Succeeded
                    } else {
                        StepStatus::Failed
                    },
                    res.message.clone(),
                    if res.success { None } else { Some(res.message) },
                )),
                Err(e) => StepOutcome::Done(mk(StepStatus::Failed, e.clone(), Some(e))),
            }
        }

        Subsystem::Mcp => {
            // If the resolved tool needs confirmation, dispatch_to_mcp
            // emits the Confirm event and returns None.
            // Detect that case and pause the compound.
            let gated = mcp_step_is_gated(&step.intent);
            match crate::orchestrator::dispatch_to_mcp_pub(
                app,
                &step.intent,
                &step.transcript,
                request_id,
            )
            .await
            {
                Ok(None) | Ok(Some(_)) if gated => StepOutcome::AwaitingConfirmation(mk(
                    StepStatus::AwaitingConfirmation,
                    "Awaiting your confirmation, sir.".to_string(),
                    None,
                )),
                Ok(Some(text)) => StepOutcome::Done(mk(StepStatus::Succeeded, text, None)),
                Ok(None) => StepOutcome::Done(mk(StepStatus::Succeeded, "Done, sir.".to_string(), None)),
                Err(e) => StepOutcome::Done(mk(StepStatus::Failed, e.clone(), Some(e))),
            }
        }

        Subsystem::WorkerBackend => {
            match crate::orchestrator::dispatch_to_worker_pub(
                app.clone(),
                step.transcript.clone(),
                dialog_context.cloned(),
                request_id.to_string(),
                cancel_flag.clone(),
                turn,
                crate::intent_parser::intent_to_label(&step.intent),
                false,
            )
            .await
            {
                Ok((text, _analysis, _dialog)) => {
                    StepOutcome::Done(mk(StepStatus::Succeeded, text, None))
                }
                Err(e) => StepOutcome::Done(mk(StepStatus::Failed, e.clone(), Some(e))),
            }
        }

        Subsystem::GitHub => {
            // Read-only GitHub ops run inline. Destructive ops emit the
            // existing Confirm flow — the frontend's github_execute path
            // handles it; remaining compound steps are dropped (v1 limit).
            let crate::intent_parser::ParsedIntent::GitHubCommand { command } = &step.intent else {
                return StepOutcome::Done(mk(
                    StepStatus::Unsupported,
                    "invalid github step".to_string(),
                    None,
                ));
            };

            let Some((worker_url, user_id, _)) = crate::network::get_session_info() else {
                return StepOutcome::Done(mk(
                    StepStatus::Failed,
                    "no session open".to_string(),
                    Some("no session open".to_string()),
                ));
            };

            match crate::github_cmd::execute_command(&worker_url, &user_id, command, false).await {
                crate::github_cmd::GitHubResult::Text { text } => {
                    StepOutcome::Done(mk(StepStatus::Succeeded, text, None))
                }
                crate::github_cmd::GitHubResult::PrList { repo, state, prs } => {
                    let text = format!(
                        "Found {} {} PR{} in {}.",
                        prs.len(),
                        state,
                        if prs.len() == 1 { "" } else { "s" },
                        repo
                    );
                    StepOutcome::Done(mk(StepStatus::Succeeded, text, None))
                }
                crate::github_cmd::GitHubResult::NeedsConfirmation { prompt, command } => {
                    // Emit the Confirm event — constructed from the real
                    // enum (not hand-typed JSON) so this can never silently
                    // drift from the GitHub path's own Confirm shape if the
                    // enum ever changes.
                    let cmd_json =
                        serde_json::to_value(command).unwrap_or(serde_json::Value::Null);
                    app.emit(
                        "orchestrator:event",
                        &crate::orchestrator::OrchestratorEvent::Confirm {
                            prompt: prompt.clone(),
                            request_id: request_id.to_string(),
                            command: cmd_json,
                        },
                    )
                    .ok();
                    StepOutcome::AwaitingConfirmation(mk(
                        StepStatus::AwaitingConfirmation,
                        prompt.clone(),
                        None,
                    ))
                }
                crate::github_cmd::GitHubResult::MergeConflict { message, .. } => {
                    StepOutcome::Done(mk(StepStatus::Failed, message.clone(), Some(message)))
                }
                crate::github_cmd::GitHubResult::Error { message, .. } => {
                    StepOutcome::Done(mk(StepStatus::Failed, message.clone(), Some(message)))
                }
            }
        }

        // Architect can't compose inside compounds (window-managed).
        // CommandCenter can't nest (build_plan never produces it).
        Subsystem::Architect | Subsystem::CommandCenter | Subsystem::None => {
            StepOutcome::Done(mk(
                StepStatus::Unsupported,
                "unsupported in multi-step".to_string(),
                None,
            ))
        }
    }
}

/// Whether the MCP tool this intent resolves to needs confirmation.
/// Mirrors the intent→(server, tool) mapping in dispatch_to_mcp.
fn mcp_step_is_gated(intent: &ParsedIntent) -> bool {
    use crate::mcp_client::McpServer;
    let (server, tool) = match intent {
        ParsedIntent::OrderFood { .. } => (McpServer::SwiggyFood, "search_restaurants"),
        ParsedIntent::SearchProduct { .. } => (McpServer::Amazon, "amazon_search"),
        ParsedIntent::SendWhatsAppMessage { .. } => (McpServer::WhatsApp, "send_message"),
        _ => return false,
    };
    server.requires_confirmation(tool)
}

// ─── Plan execution ────────────────────────────────────────────────────

/// Result of running a full plan.
pub struct PlanOutcome {
    /// Merged human-readable response text.
    pub text: String,
    /// The run summary.
    pub summary: RunSummary,
    /// True if the task paused on a confirmation gate. When true, the
    /// remaining steps are in PENDING_COMPOUND and the caller should
    /// NOT emit the final Result (the confirm flow will).
    pub awaiting_confirmation: bool,
}

/// Execute a task plan sequentially, merging results.
///
/// Steps run in order. A failed non-optional step aborts the task and
/// the merged text reports the partial outcome. A confirmation-gated
/// step pauses the task — remaining steps are stashed in
/// `PENDING_COMPOUND` and resumed by `orchestrator_mcp_confirm`.
pub async fn execute_plan<R: Runtime>(
    app: &AppHandle<R>,
    mut plan: TaskPlan,
    request_id: &str,
    cancel_flag: &Arc<AtomicBool>,
    dialog_context: Option<serde_json::Value>,
    turn: &TurnContext,
) -> PlanOutcome {
    tracing::info!(
        "command_center: executing {}-step plan for request {}",
        plan.steps.len(),
        request_id
    );

    let mut i = 0;
    while i < plan.steps.len() {
        // B5: checkpoint before each step — a crash leaves the record behind.
        if let Ok(dir) = app.path().app_data_dir() {
            write_checkpoint(&dir, &plan, i);
        }
        if crate::orchestrator::is_cancelled_pub(cancel_flag) {            let step = plan.steps[i].clone();
            plan.results.push(StepResult {
                step_id: step.step_id,
                subsystem: step.subsystem.clone(),
                status: StepStatus::Skipped,
                text: "cancelled".to_string(),
                error: Some("cancelled".to_string()),
                latency_ms: 0,
            });
            break;
        }

        // B4: maximal run of parallel-safe (WorkerBackend Q&A) steps runs
        // concurrently; everything else stays sequential.
        let end = batch_end(&plan.steps, i);
        if end - i > 1 {
            tracing::info!(
                "command_center: executing {}-step parallel batch (steps {}-{})",
                end - i,
                i,
                end - 1
            );
            let batch: Vec<PlanStep> = plan.steps[i..end].to_vec();
            let outs = execute_batch(app, &batch, request_id, cancel_flag, dialog_context.as_ref(), turn).await;
            let mut aborted = false;
            for (idx, (_id, outcome)) in outs.into_iter().enumerate() {
                let step = &batch[idx];
                match outcome {
                    StepOutcome::Done(res) => {
                        let failed = res.status == StepStatus::Failed;
                        plan.results.push(res);
                        if failed && !step.optional {
                            for remaining in plan.steps.iter().skip(end) {
                                plan.results.push(StepResult {
                                    step_id: remaining.step_id,
                                    subsystem: remaining.subsystem.clone(),
                                    status: StepStatus::Skipped,
                                    text: "skipped (previous step failed)".to_string(),
                                    error: None,
                                    latency_ms: 0,
                                });
                            }
                            aborted = true;
                            break;
                        }
                    }
                    StepOutcome::AwaitingConfirmation(res) => {
                        plan.results.push(res);
                        let mut remaining = batch[idx + 1..].to_vec();
                        remaining.extend_from_slice(&plan.steps[end..]);
                        set_pending_compound(PendingCompound {
                            remaining_steps: remaining,
                            prior_results: plan.results.clone(),
                            request_id: request_id.to_string(),
                            transcript: plan.transcript.clone(),
                            turn: turn.clone(),
                        });
                        if let Ok(dir) = app.path().app_data_dir() {
                            clear_checkpoint(&dir, &plan.task_id);
                        }
                        return PlanOutcome {
                            text: merge_results(&plan.results),
                            summary: plan.summary(),
                            awaiting_confirmation: true,
                        };
                    }
                }
            }
            if aborted {
                break;
            }
            i = end;
            continue;
        }

        let step = plan.steps[i].clone();

        let outcome = execute_step(
            app,
            &step,
            request_id,
            cancel_flag,
            dialog_context.as_ref(),
            turn,
        )
        .await;

        match outcome {
                StepOutcome::Done(res) => {
                    let failed = res.status == StepStatus::Failed;
                    plan.results.push(res);
                    if failed && !step.optional {
                        // Abort remaining steps — mark them skipped.
                        for remaining in &plan.steps[i + 1..] {
                        plan.results.push(StepResult {
                            step_id: remaining.step_id,
                            subsystem: remaining.subsystem.clone(),
                            status: StepStatus::Skipped,
                            text: "skipped (previous step failed)".to_string(),
                            error: None,
                            latency_ms: 0,
                        });
                    }
                    break;
                }
            }
            StepOutcome::AwaitingConfirmation(res) => {
                plan.results.push(res);
                // Stash remaining steps — resumed by orchestrator_mcp_confirm.
                let remaining = plan.steps[i + 1..].to_vec();
                set_pending_compound(PendingCompound {
                    remaining_steps: remaining,
                    prior_results: plan.results.clone(),
                    request_id: request_id.to_string(),
                    transcript: plan.transcript.clone(),
                    turn: turn.clone(),
                });
                if let Ok(dir) = app.path().app_data_dir() {
                    clear_checkpoint(&dir, &plan.task_id);
                }
                return PlanOutcome {
                    text: merge_results(&plan.results),
                    summary: plan.summary(),
                    awaiting_confirmation: true,
                };
            }
        }
        i += 1;
    }

    if let Ok(dir) = app.path().app_data_dir() {
        clear_checkpoint(&dir, &plan.task_id);
    }

    let summary = plan.summary();
    if let Ok(dir) = app.path().app_data_dir() {
        crate::diary::log_event(
            &dir,
            "compound_done",
            &format!("{} steps, {} ok", summary.total_steps, summary.completed),
        );
        if summary.failed > 0 {
            crate::diary::log_event(
                &dir,
                "compound_failed",
                &format!("{} of {} steps failed: {}", summary.failed, summary.total_steps, plan.transcript.chars().take(120).collect::<String>()),
            );
        }
    }

    PlanOutcome {
        text: merge_results(&plan.results),
        summary,
        awaiting_confirmation: false,
    }
}

/// A step is parallel-safe if it is side-effect-free and never hits a
/// confirmation gate: WorkerBackend Q&A only. LocalCommand opens apps,
/// Mcp/GitHub mutate external state — always sequential. Pure.
pub fn is_parallel_safe(step: &PlanStep) -> bool {
    matches!(step.subsystem, Subsystem::WorkerBackend)
}

/// End index (exclusive) of the maximal parallel-safe run from `start`.
/// Pure + unit-tested.
pub fn batch_end(steps: &[PlanStep], start: usize) -> usize {
    let mut end = start;
    while end < steps.len() && is_parallel_safe(&steps[end]) {
        end += 1;
    }
    end
}

/// Execute a batch of parallel-safe steps concurrently (JoinSet), returning
/// outcomes sorted by step_id so results merge in plan order.
async fn execute_batch<R: Runtime>(
    app: &AppHandle<R>,
    steps: &[PlanStep],
    request_id: &str,
    cancel_flag: &Arc<AtomicBool>,
    dialog_context: Option<&serde_json::Value>,
    turn: &TurnContext,
) -> Vec<(usize, StepOutcome)> {
    let mut set = tokio::task::JoinSet::new();
    for step in steps {
        let app_c = app.clone();
        let step_c = step.clone();
        let req_c = request_id.to_string();
        let flag_c = Arc::clone(cancel_flag);
        let ctx_c = dialog_context.cloned();
        let turn_c = turn.clone();
        set.spawn(async move {
            let out =
                execute_step(&app_c, &step_c, &req_c, &flag_c, ctx_c.as_ref(), &turn_c).await;
            (step_c.step_id, out)
        });
    }
    let mut outs = Vec::with_capacity(steps.len());
    while let Some(res) = set.join_next().await {
        match res {
            Ok(pair) => outs.push(pair),
            Err(e) => {
                tracing::error!("command_center: batch task panicked: {}", e);
            }
        }
    }
    outs.sort_by_key(|(id, _)| *id);
    outs
}

/// Resume a pending compound after the gated step was confirmed.
///
/// Called by `orchestrator_mcp_confirm` after the MCP call succeeds.
/// Runs the remaining steps, merges them with the prior results and
/// the confirmed step's text, returns the merged response.
pub async fn resume_compound<R: Runtime>(
    app: &AppHandle<R>,
    pending: PendingCompound,
    confirmed_step_text: String,
    cancel_flag: Arc<AtomicBool>,
) -> PlanOutcome {
    let total = pending.prior_results.len() + 1 + pending.remaining_steps.len();
    let mut results = pending.prior_results;
    results.push(StepResult {
        step_id: results.len(),
        subsystem: Subsystem::Mcp,
        status: StepStatus::Succeeded,
        text: confirmed_step_text,
        error: None,
        latency_ms: 0,
    });

    for step in &pending.remaining_steps {
        if crate::orchestrator::is_cancelled_pub(&cancel_flag) {
            results.push(StepResult {
                step_id: step.step_id,
                subsystem: step.subsystem.clone(),
                status: StepStatus::Skipped,
                text: "cancelled".to_string(),
                error: Some("cancelled".to_string()),
                latency_ms: 0,
            });
            break;
        }

            match execute_step(
                app,
                step,
                &pending.request_id,
                &cancel_flag,
                None,
                &pending.turn,
            )
            .await
            {
            StepOutcome::Done(res) => {
                let failed = res.status == StepStatus::Failed;
                results.push(res);
                if failed && !step.optional {
                    break;
                }
            }
            // Nested confirmation inside a resumed compound — v1 treats
            // the task as done at this point (the Confirm was emitted).
            StepOutcome::AwaitingConfirmation(res) => {
                results.push(res);
                break;
            }
        }
    }

    let completed = results
        .iter()
        .filter(|r| r.status == StepStatus::Succeeded)
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r.status, StepStatus::Failed | StepStatus::Unsupported))
        .count();
    let skipped = results
        .iter()
        .filter(|r| r.status == StepStatus::Skipped)
        .count();
    let awaiting = results
        .iter()
        .any(|r| r.status == StepStatus::AwaitingConfirmation);

    PlanOutcome {
        text: merge_results(&results),
        summary: RunSummary {
            total_steps: total,
            completed,
            failed,
            skipped,
            awaiting_confirmation: awaiting,
        },
        awaiting_confirmation: awaiting,
    }
}

// ─── Result merging ────────────────────────────────────────────────────

/// Merge step results into one human-readable response.
///
/// Single success: return the step's text.
/// Multi-step: join successful step texts; append failure notes.
fn merge_results(results: &[StepResult]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    for r in results {
        match r.status {
            StepStatus::Succeeded | StepStatus::AwaitingConfirmation => {
                if !r.text.is_empty() {
                    parts.push(r.text.clone());
                }
            }
            StepStatus::Failed | StepStatus::Unsupported => {
                failures.push(
                    r.error
                        .clone()
                        .unwrap_or_else(|| r.text.clone())
                );
            }
            StepStatus::Skipped | StepStatus::Pending => {}
        }
    }

    if parts.is_empty() && failures.is_empty() {
        return "Done, sir.".to_string();
    }

    let mut out = parts.join(" ");
    if !failures.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!(
            "However: {}.",
            failures.join("; ")
        ));
    }
    out
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_compound_then() {
        let parts = split_compound("check movies then order dinner");
        assert_eq!(parts, vec!["check movies", "order dinner"]);
    }

    #[test]
    fn test_split_compound_and_then() {
        let parts = split_compound("open chrome and then search for cats");
        assert_eq!(parts, vec!["open chrome", "search for cats"]);
    }

    #[test]
    fn test_split_compound_comma_then() {
        let parts = split_compound("open spotify, then pause music");
        assert_eq!(parts, vec!["open spotify", "pause music"]);
    }

    #[test]
    fn test_split_compound_after_that() {
        let parts = split_compound("open chrome after that open notepad");
        assert_eq!(parts, vec!["open chrome", "open notepad"]);
    }

    #[test]
    fn test_split_compound_single() {
        let parts = split_compound("open chrome");
        assert_eq!(parts, vec!["open chrome"]);
    }

    #[test]
    fn test_split_compound_does_not_split_on_and() {
        // Bare "and" must NOT split — "mum and dad", "search and rescue"
        let parts = split_compound("open chrome and notepad");
        assert_eq!(parts, vec!["open chrome and notepad"]);
    }

    #[test]
    fn test_split_compound_three_steps() {
        let parts = split_compound("open chrome then pause music then open notepad");
        assert_eq!(parts, vec!["open chrome", "pause music", "open notepad"]);
    }

    #[test]
    fn test_split_compound_strips_empty_parts() {
        let parts = split_compound("open chrome then ");
        assert_eq!(parts, vec!["open chrome"]);
    }

    #[test]
    fn test_build_plan_single_returns_none() {
        assert!(build_plan("open chrome", "t1").is_none());
    }

    #[test]
    fn test_build_plan_compound() {
        let plan = build_plan("open chrome then search for cats", "t2");
        assert!(plan.is_some());
        let plan = plan.unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].subsystem, Subsystem::LocalCommand);
        assert_eq!(plan.steps[1].subsystem, Subsystem::WorkerBackend);
        assert_eq!(plan.steps[1].depends_on, vec![0]);
    }

    #[test]
    fn test_build_plan_unparseable_part_returns_none() {
        // "gibberish xyzzy" won't parse → whole thing falls back to
        // single-intent path (Worker/9Router answers the full text).
        let plan = build_plan("open chrome then gibberish xyzzy foobar", "t3");
        assert!(plan.is_none());
    }

    #[test]
    fn test_build_plan_architect_part_returns_none() {
        let plan = build_plan("open chrome then open the architecture mapper", "t4");
        assert!(plan.is_none());
    }

    #[test]
    fn test_build_plan_mcp_step() {
        let plan = build_plan("open chrome then order biryani", "t5");
        assert!(plan.is_some());
        let plan = plan.unwrap();
        assert_eq!(plan.steps[0].subsystem, Subsystem::LocalCommand);
        assert_eq!(plan.steps[1].subsystem, Subsystem::Mcp);
    }

    #[test]
    fn test_summary_counts() {
        let plan = TaskPlan {
            task_id: "t".to_string(),
            transcript: "x".to_string(),
            steps: vec![],
            results: vec![
                StepResult {
                    step_id: 0,
                    subsystem: Subsystem::LocalCommand,
                    status: StepStatus::Succeeded,
                    text: "ok".into(),
                    error: None,
                    latency_ms: 1,
                },
                StepResult {
                    step_id: 1,
                    subsystem: Subsystem::Mcp,
                    status: StepStatus::Failed,
                    text: "err".into(),
                    error: Some("err".into()),
                    latency_ms: 2,
                },
                StepResult {
                    step_id: 2,
                    subsystem: Subsystem::WorkerBackend,
                    status: StepStatus::Skipped,
                    text: "skipped".into(),
                    error: None,
                    latency_ms: 0,
                },
            ],
        };
        let s = plan.summary();
        assert_eq!(s.completed, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.skipped, 1);
        assert!(!s.awaiting_confirmation);
    }

    #[test]
    fn test_merge_results_single() {
        let results = vec![StepResult {
            step_id: 0,
            subsystem: Subsystem::LocalCommand,
            status: StepStatus::Succeeded,
            text: "Opened Chrome sir.".into(),
            error: None,
            latency_ms: 1,
        }];
        assert_eq!(merge_results(&results), "Opened Chrome sir.");
    }

    #[test]
    fn test_merge_results_multi() {
        let results = vec![
            StepResult {
                step_id: 0,
                subsystem: Subsystem::LocalCommand,
                status: StepStatus::Succeeded,
                text: "Opened Chrome.".into(),
                error: None,
                latency_ms: 1,
            },
            StepResult {
                step_id: 1,
                subsystem: Subsystem::Mcp,
                status: StepStatus::Succeeded,
                text: "Found 3 restaurants.".into(),
                error: None,
                latency_ms: 500,
            },
        ];
        assert_eq!(merge_results(&results), "Opened Chrome. Found 3 restaurants.");
    }

    #[test]
    fn test_merge_results_with_failure() {
        let results = vec![
            StepResult {
                step_id: 0,
                subsystem: Subsystem::LocalCommand,
                status: StepStatus::Succeeded,
                text: "Opened Chrome.".into(),
                error: None,
                latency_ms: 1,
            },
            StepResult {
                step_id: 1,
                subsystem: Subsystem::Mcp,
                status: StepStatus::Failed,
                text: "".into(),
                error: Some("swiggy unavailable".into()),
                latency_ms: 100,
            },
        ];
        let merged = merge_results(&results);
        assert!(merged.contains("Opened Chrome."));
        assert!(merged.contains("swiggy unavailable"));
    }

    #[test]
    fn test_pending_compound_roundtrip() {
        let pending = PendingCompound {
            remaining_steps: vec![],
            prior_results: vec![],
            request_id: "req-xyz".to_string(),
            transcript: "test".to_string(),
            turn: TurnContext::default(),
        };
        set_pending_compound(pending);
        assert!(has_pending_compound("req-xyz"));
        let taken = take_pending_compound("req-xyz");
        assert!(taken.is_some());
        assert!(!has_pending_compound("req-xyz"));
        // Taking again returns None
        assert!(take_pending_compound("req-xyz").is_none());
    }

    #[test]
    fn test_pending_compound_wrong_request_id() {
        let pending = PendingCompound {
            remaining_steps: vec![],
            prior_results: vec![],
            request_id: "req-a".to_string(),
            transcript: "test".to_string(),
            turn: TurnContext::default(),
        };
        set_pending_compound(pending);
        assert!(take_pending_compound("req-b").is_none());
        // Still there for the right id
        assert!(take_pending_compound("req-a").is_some());
    }

    fn mk_step(id: usize, subsystem: Subsystem) -> PlanStep {
        let intent = match subsystem {
            Subsystem::WorkerBackend => ParsedIntent::Unknown { raw: "q".into() },
            Subsystem::LocalCommand => ParsedIntent::OpenApp { target: "x".into() },
            Subsystem::Mcp => ParsedIntent::OpenApp { target: "y".into() },
            _ => ParsedIntent::Unknown { raw: "z".into() },
        };
        PlanStep {
            step_id: id,
            transcript: "t".into(),
            intent,
            subsystem,
            depends_on: vec![],
            optional: false,
        }
    }

    #[test]
    fn test_is_parallel_safe_worker_only() {
        assert!(is_parallel_safe(&mk_step(0, Subsystem::WorkerBackend)));
        assert!(!is_parallel_safe(&mk_step(0, Subsystem::LocalCommand)));
        assert!(!is_parallel_safe(&mk_step(0, Subsystem::Mcp)));
        assert!(!is_parallel_safe(&mk_step(0, Subsystem::GitHub)));
        assert!(!is_parallel_safe(&mk_step(0, Subsystem::Architect)));
    }

    #[test]
    fn test_batch_end_all_parallel() {
        let steps = vec![
            mk_step(0, Subsystem::WorkerBackend),
            mk_step(1, Subsystem::WorkerBackend),
            mk_step(2, Subsystem::WorkerBackend),
        ];
        assert_eq!(batch_end(&steps, 0), 3);
    }

    #[test]
    fn test_batch_end_mixed() {
        let steps = vec![
            mk_step(0, Subsystem::LocalCommand),
            mk_step(1, Subsystem::WorkerBackend),
            mk_step(2, Subsystem::WorkerBackend),
            mk_step(3, Subsystem::Mcp),
        ];
        assert_eq!(batch_end(&steps, 0), 0);
        assert_eq!(batch_end(&steps, 1), 3);
        assert_eq!(batch_end(&steps, 3), 3);
    }

    #[test]
    fn test_batch_end_single_is_not_batch() {
        // A lone parallel-safe step still runs the sequential path
        // (execute_plan only batches runs of length > 1).
        let steps = vec![
            mk_step(0, Subsystem::LocalCommand),
            mk_step(1, Subsystem::WorkerBackend),
            mk_step(2, Subsystem::LocalCommand),
        ];
        assert_eq!(batch_end(&steps, 1), 2);
    }

    #[test]
    fn test_batch_end_empty() {
        let steps: Vec<PlanStep> = vec![];
        assert_eq!(batch_end(&steps, 0), 0);
    }

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("nexus_cc_test_{}_{}", name, std::process::id()));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let d = tmpdir("roundtrip");
        let plan = build_plan("open chrome then search for cats", "t-ckpt").unwrap();
        write_checkpoint(&d, &plan, 1);
        let cp = read_checkpoint(&d, "t-ckpt").expect("checkpoint readable");
        assert_eq!(cp.task_id, "t-ckpt");
        assert_eq!(cp.steps.len(), 2);
        assert_eq!(cp.next_index, 1);
        assert_eq!(cp.transcript, "open chrome then search for cats");
        clear_checkpoint(&d, "t-ckpt");
        assert!(read_checkpoint(&d, "t-ckpt").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_checkpoint_sweep_stale() {
        let d = tmpdir("sweep");
        let plan = build_plan("open chrome then search for cats", "t-stale").unwrap();
        write_checkpoint(&d, &plan, 0);
        // Backdate the checkpoint beyond the stale window.
        let path = checkpoint_path(&d, "t-stale");
        let mut cp = read_checkpoint(&d, "t-stale").unwrap();
        cp.updated_at -= CHECKPOINT_STALE_SECS + 10;
        std::fs::write(&path, serde_json::to_string_pretty(&cp).unwrap()).unwrap();
        assert_eq!(sweep_stale_checkpoints(&d), 1);
        assert!(read_checkpoint(&d, "t-stale").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_checkpoint_sweep_keeps_fresh() {
        let d = tmpdir("fresh");
        let plan = build_plan("open chrome then search for cats", "t-fresh").unwrap();
        write_checkpoint(&d, &plan, 0);
        assert_eq!(sweep_stale_checkpoints(&d), 0);
        assert!(read_checkpoint(&d, "t-fresh").is_some());
        clear_checkpoint(&d, "t-fresh");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_checkpoint_task_id_sanitized() {
        assert_eq!(sanitize_task_id("req-abc-123"), "req-abc-123");
        assert_eq!(sanitize_task_id("../../etc"), "______etc");
    }
}
