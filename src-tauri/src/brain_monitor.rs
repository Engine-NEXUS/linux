//! Brain monitor — watches every transcript and auto-trains BERT-Mini.
#![allow(dead_code)]
//!
//! This is the continuous learning loop:
//!   1. Every transcript passes through the brain (in background, non-blocking)
//!   2. The brain classifies it + generates phrasings
//!   3. 3-gate auto-approval decides if phrasings go into training data
//!   4. Every 50 new approved examples, triggers a background retrain
//!   5. New model must score ≥ old model or it's discarded (rollback safety)
//!
//! Admin-only: the monitor only runs when is_admin is true.
//! Non-blocking: the monitor runs in a tokio task, never blocks the main pipeline.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::brain_client;

/// Number of approved examples needed to trigger a retrain.
const RETRAIN_THRESHOLD: u32 = 50;

/// Minimum brain confidence for auto-approval.
const MIN_CONFIDENCE: f32 = 0.90;

/// Below this confidence, brain output is considered noise and ignored
/// (not written as a rejection — there's nothing to unlearn).
const NOISE_THRESHOLD: f32 = 0.30;

static PENDING_COUNT: AtomicU32 = AtomicU32::new(0);

/// Approved phrasing entry (written to approved_phrasings.jsonl).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovedPhrasing {
    text: String,
    intent: String,
    slots: serde_json::Value,
    source: String,
    brain_confidence: f32,
    timestamp: f64,
}

/// Gap entry (written to gaps.jsonl when the brain disagrees with BERT-Mini).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GapEntry {
    transcript: String,
    deterministic_result: Option<String>,
    nlu_result: Option<String>,
    brain_result: String,
    brain_confidence: f32,
    timestamp: f64,
}

/// Rejected example entry (written to rejected_examples.jsonl when a
/// command executes wrongly). These are REMOVED from the dataset before
/// retraining — BERT-Mini unlearns the mistake.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RejectedExample {
    text: String,
    intent: String,
    reason: String,       // "execution_failure", "verbal_wrong", "low_confidence"
    error: Option<String>, // error message if available
    timestamp: f64,
}

/// Track the last command's transcript + intent for verbal "wrong" feedback.
use std::sync::Mutex as StdMutex;
static LAST_COMMAND: StdMutex<Option<(String, String)>> = StdMutex::new(None);

/// Get the path to the brain data directory.
/// Uses the admin data directory at server/admin/data/ (relative to the
/// executable in production, or the project root in dev).
fn brain_data_dir() -> PathBuf {
    // In production: exe_dir/resources/server/admin/data/
    // In dev: project_root/server/admin/data/
    let exe = std::env::current_exe().ok();
    if let Some(exe_path) = exe {
        let prod = exe_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("resources").join("server").join("admin").join("data"));
        if let Some(p) = prod {
            if p.exists() {
                return p;
            }
        }
    }
    // Dev fallback: C:\PROJECTS\ULTRON\server\admin\data
    PathBuf::from("server").join("admin").join("data")
}

/// Get the path to approved_phrasings.jsonl.
fn approved_phrasings_path() -> PathBuf {
    brain_data_dir().join("approved_phrasings.jsonl")
}

/// Get the path to gaps.jsonl.
fn gaps_path() -> PathBuf {
    brain_data_dir().join("gaps.jsonl")
}

/// Get the path to rejected_examples.jsonl (bad examples to discard).
fn rejected_examples_path() -> PathBuf {
    brain_data_dir().join("rejected_examples.jsonl")
}

/// Ensure the brain data directory exists.
fn ensure_data_dir() {
    let dir = brain_data_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).ok();
    }
}

/// Append a JSON line to a file.
fn append_jsonl(path: &PathBuf, entry: &impl Serialize) {
    ensure_data_dir();
    if let Ok(line) = serde_json::to_string(entry) {
        // Use OpenOptions to append
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            writeln!(file, "{}", line).ok();
        }
    }
}

/// Monitor a transcript in the background.
///
/// This is called after the deterministic parser + BERT-Mini have processed
/// the transcript. The brain watches the result and learns from it.
///
/// NON-BLOCKING: spawns a tokio task, returns immediately.
/// NO-OP if not admin (runtime check via admin_config).
pub fn monitor_transcript(
    transcript: String,
    deterministic_intent: Option<String>,
    nlu_intent: Option<String>,
) {
    // Runtime admin check — no-op if not admin
    if !crate::admin_config::is_admin() {
        return;
    }

    // Don't await — fire and forget
    tokio::spawn(async move {
        monitor_transcript_inner(transcript, deterministic_intent, nlu_intent).await;
    });
}

/// Inner monitor logic — runs in a background task.
async fn monitor_transcript_inner(
    transcript: String,
    deterministic_intent: Option<String>,
    nlu_intent: Option<String>,
) {
    // 1. Classify via brain
    let brain_result = match brain_client::brain_classify(&transcript).await {
        Some(r) => r,
        None => return, // brain not available, skip
    };

    let brain_intent_name = crate::intent_parser::intent_to_label(&brain_result.intent).to_string();
    let brain_confidence = brain_result.confidence;

    // 1b. Low-confidence rejection — if the brain classified something
    // but with low confidence (between NOISE_THRESHOLD and MIN_CONFIDENCE),
    // write it as a rejected example so BERT-Mini doesn't learn a weak
    // or uncertain classification. Below NOISE_THRESHOLD, we treat it as
    // noise and ignore it entirely (nothing to unlearn).
    if brain_confidence >= NOISE_THRESHOLD && brain_confidence < MIN_CONFIDENCE {
        let rejected = RejectedExample {
            text: transcript.clone(),
            intent: brain_intent_name.clone(),
            reason: "low_confidence".to_string(),
            error: Some(format!("brain confidence {:.2} < {:.2}", brain_confidence, MIN_CONFIDENCE)),
            timestamp: chrono::Utc::now().timestamp() as f64,
        };
        append_jsonl(&rejected_examples_path(), &rejected);
        tracing::info!(
            "[brain_monitor] low-confidence rejection: transcript='{}' intent='{}' conf={:.2}",
            transcript,
            brain_intent_name,
            brain_confidence
        );
    }

    // 2. Check for gaps (brain disagrees with both deterministic and NLU)
    let det_matches = deterministic_intent
        .as_ref()
        .map(|d| d == &brain_intent_name)
        .unwrap_or(false);
    let nlu_matches = nlu_intent
        .as_ref()
        .map(|n| n == &brain_intent_name)
        .unwrap_or(false);

    if !det_matches && !nlu_matches && brain_confidence > 0.8 {
        // Brain disagrees with both — log a gap
        let gap = GapEntry {
            transcript: transcript.clone(),
            deterministic_result: deterministic_intent.clone(),
            nlu_result: nlu_intent.clone(),
            brain_result: brain_intent_name.clone(),
            brain_confidence,
            timestamp: chrono::Utc::now().timestamp() as f64,
        };
        append_jsonl(&gaps_path(), &gap);
        tracing::info!(
            "[brain_monitor] gap detected: transcript='{}' brain='{}' conf={:.2}",
            transcript,
            brain_intent_name,
            brain_confidence
        );
    }

    // 3. Auto-approval gate (3 checks)
    let approved = auto_approve(
        &brain_intent_name,
        brain_confidence,
        &deterministic_intent,
        &nlu_intent,
    );

    if !approved {
        return;
    }

    // 4. Generate phrasings for the approved intent
    let slots = serde_json::to_value(&brain_result.intent).unwrap_or(serde_json::Value::Null);
    let phrasings = brain_client::brain_generate_phrasings(&brain_intent_name, &slots, 20).await;

    if let Some(phrasings) = phrasings {
        let phrasing_count = phrasings.len();
        for phrasing in phrasings {
            let entry = ApprovedPhrasing {
                text: phrasing,
                intent: brain_intent_name.clone(),
                slots: slots.clone(),
                source: "brain_auto".to_string(),
                brain_confidence,
                timestamp: chrono::Utc::now().timestamp() as f64,
            };
            append_jsonl(&approved_phrasings_path(), &entry);
        }

        let count = PENDING_COUNT.fetch_add(phrasing_count as u32, Ordering::Relaxed) + phrasing_count as u32;
        tracing::info!(
            "[brain_monitor] auto-approved {} phrasings for '{}' (total pending: {})",
            phrasing_count,
            brain_intent_name,
            count
        );

        // 5. Check if we should trigger a retrain
        if count >= RETRAIN_THRESHOLD {
            PENDING_COUNT.store(0, Ordering::Relaxed);
            trigger_retrain().await;
        }
    }
}

/// 3-gate auto-approval logic.
///
/// Gate 1: Brain confidence ≥ 0.90
/// Gate 2: Brain intent matches deterministic OR NLU (cross-validation)
/// Gate 3: Slot values are valid (checked in brain_client during classification)
fn auto_approve(
    brain_intent: &str,
    brain_confidence: f32,
    deterministic_intent: &Option<String>,
    nlu_intent: &Option<String>,
) -> bool {
    // Gate 1: confidence check
    if brain_confidence < MIN_CONFIDENCE {
        tracing::debug!(
            "[brain_monitor] gate 1 failed: confidence {:.2} < {:.2}",
            brain_confidence,
            MIN_CONFIDENCE
        );
        return false;
    }

    // Gate 2: cross-validation
    // The brain's classification must agree with EITHER the deterministic
    // parser OR BERT-Mini. If all three disagree, something is ambiguous.
    //
    // Exception: if the deterministic parser returned None AND NLU returned
    // None (both missed), we trust the brain if it's confident enough.
    let det_matches = deterministic_intent
        .as_ref()
        .map(|d| intent_names_match(d, brain_intent))
        .unwrap_or(false);
    let nlu_matches = nlu_intent
        .as_ref()
        .map(|n| intent_names_match(n, brain_intent))
        .unwrap_or(false);

    let both_missed = deterministic_intent.is_none() && nlu_intent.is_none();

    if !det_matches && !nlu_matches && !both_missed {
        tracing::debug!(
            "[brain_monitor] gate 2 failed: brain='{}' det={:?} nlu={:?}",
            brain_intent,
            deterministic_intent,
            nlu_intent
        );
        return false;
    }

    // Gate 3: slot validation is done in brain_client during classification
    // (the brain server validates slots before returning)

    true
}

/// Check if two intent names match (handling minor format differences).
fn intent_names_match(a: &str, b: &str) -> bool {
    // Normalize both to lowercase snake_case
    let normalize = |s: &str| -> String {
        s.to_lowercase()
            .replace("parsedintent::", "")
            .replace("githubcommand::", "")
    };
    normalize(a) == normalize(b)
}

/// Trigger a background retrain of BERT-Mini.
///
/// This runs the merge_and_train.py script which:
///   1. Merges approved_phrasings.jsonl into dataset.json
///   2. Deduplicates
///   3. Balances classes
///   4. Runs train.py
///   5. Compares new model accuracy vs old
///   6. Hot-swaps if new ≥ old, discards if new < old
async fn trigger_retrain() {
    tracing::info!("[brain_monitor] triggering background retrain...");

    // Run the retrain script in a blocking task
    let result = tokio::task::spawn_blocking(|| {
        let script = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("resources").join("server").join("nlu").join("merge_and_train.py")))
            .unwrap_or_else(|| PathBuf::from("server/nlu/merge_and_train.py"));

        std::process::Command::new("python")
            .arg(&script)
            .output()
    })
    .await;

    match result {
        Ok(Ok(output)) => {
            if output.status.success() {
                tracing::info!("[brain_monitor] retrain completed successfully");
                // The retrain script handles hot-swapping the ONNX model
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("[brain_monitor] retrain failed: {}", stderr.chars().take(200).collect::<String>());
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("[brain_monitor] failed to run retrain script: {}", e);
        }
        Err(e) => {
            tracing::warn!("[brain_monitor] retrain task panicked: {}", e);
        }
    }
}

/// Get the current pending count (for UI display).
pub fn pending_count() -> u32 {
    PENDING_COUNT.load(Ordering::Relaxed)
}

/// Get the number of approved phrasings in the file.
pub fn approved_phrasings_count() -> usize {
    let path = approved_phrasings_path();
    if !path.exists() {
        return 0;
    }
    std::fs::read_to_string(&path)
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

/// Get the number of gaps in the file.
pub fn gaps_count() -> usize {
    let path = gaps_path();
    if !path.exists() {
        return 0;
    }
    std::fs::read_to_string(&path)
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

/// Get the number of rejected examples in the file.
pub fn rejected_count() -> usize {
    let path = rejected_examples_path();
    if !path.exists() {
        return 0;
    }
    std::fs::read_to_string(&path)
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0)
}

/// Record the last command's transcript + intent.
///
/// Called by the orchestrator after a command is parsed. This allows
/// the admin to say "wrong" later and have the brain mark the correct
/// command as bad.
pub fn record_last_command(transcript: String, intent: String) {
    if !crate::admin_config::is_admin() {
        return;
    }
    let mut guard = LAST_COMMAND.lock().unwrap();
    *guard = Some((transcript, intent));
}

/// Report that a command executed successfully.
///
/// This confirms the phrasing was correct — the brain can be more
/// confident about it in future classifications. The success is logged
/// to approved_phrasings.jsonl as a "execution_verified" source, which
/// gets merged into training data on the next retrain cycle.
pub fn report_execution_success(transcript: &str, intent: &str) {
    if !crate::admin_config::is_admin() {
        return;
    }
    tracing::debug!(
        "[brain_monitor] execution success: transcript='{}' intent='{}'",
        transcript,
        intent
    );

    // Log the successful command as an approved phrasing with high confidence.
    // This is a "verified by execution" example — stronger than auto-approval
    // because we know the command actually worked.
    let entry = ApprovedPhrasing {
        text: transcript.to_string(),
        intent: intent.to_string(),
        slots: serde_json::Value::Null,
        source: "execution_verified".to_string(),
        brain_confidence: 1.0, // verified by execution
        timestamp: chrono::Utc::now().timestamp() as f64,
    };
    append_jsonl(&approved_phrasings_path(), &entry);

    // Increment pending count — execution-verified examples count toward retrain
    let count = PENDING_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count >= RETRAIN_THRESHOLD {
        // Trigger retrain in background
        let count_copy = count;
        tokio::spawn(async move {
            if count_copy >= RETRAIN_THRESHOLD {
                PENDING_COUNT.store(0, Ordering::Relaxed);
                trigger_retrain().await;
            }
        });
    }
}

/// Report that a command executed wrongly (automatic feedback).
///
/// This is called by the orchestrator when a command fails (GitHub API
/// error, app not found, etc.). The brain marks the phrasing as bad
/// and adds it to rejected_examples.jsonl. On the next retrain, this
/// example is REMOVED from the dataset — BERT-Mini unlearns the mistake.
pub fn report_execution_failure(transcript: &str, intent: &str, error: &str) {
    if !crate::admin_config::is_admin() {
        return;
    }

    tracing::info!(
        "[brain_monitor] execution failure: transcript='{}' intent='{}' error='{}'",
        transcript,
        intent,
        error
    );

    let rejected = RejectedExample {
        text: transcript.to_string(),
        intent: intent.to_string(),
        reason: "execution_failure".to_string(),
        error: Some(error.to_string()),
        timestamp: chrono::Utc::now().timestamp() as f64,
    };

    append_jsonl(&rejected_examples_path(), &rejected);
}

/// Report that the admin said "wrong" about the last command (verbal feedback).
///
/// The admin says "wrong" or "no" or "that was wrong" after a bad command.
/// The brain detects this (via the deterministic parser) and calls this
/// function. It marks the LAST command's phrasing as bad.
pub fn report_verbal_wrong() {
    if !crate::admin_config::is_admin() {
        return;
    }

    let last = {
        let mut guard = LAST_COMMAND.lock().unwrap();
        guard.take()
    };

    if let Some((transcript, intent)) = last {
        tracing::info!(
            "[brain_monitor] verbal 'wrong' — rejecting last command: transcript='{}' intent='{}'",
            transcript,
            intent
        );

        let rejected = RejectedExample {
            text: transcript,
            intent,
            reason: "verbal_wrong".to_string(),
            error: None,
            timestamp: chrono::Utc::now().timestamp() as f64,
        };

        append_jsonl(&rejected_examples_path(), &rejected);
    } else {
        tracing::debug!("[brain_monitor] verbal 'wrong' but no last command recorded");
    }
}

/// Check if a transcript is a "wrong" / "no" / "that was wrong" command.
///
/// This is called by the orchestrator to detect verbal feedback.
/// Returns true if the transcript matches a "wrong" pattern.
pub fn is_verbal_wrong(transcript: &str) -> bool {
    let lower = transcript.to_lowercase().trim().to_string();
    matches!(lower.as_str(),
        "wrong" | "no" | "nope" | "that was wrong" | "that's wrong"
        | "not right" | "incorrect" | "bad" | "mistake" | "error"
        | "not what i meant" | "not what i wanted"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_verbal_wrong_exact() {
        assert!(is_verbal_wrong("wrong"));
        assert!(is_verbal_wrong("no"));
        assert!(is_verbal_wrong("nope"));
        assert!(is_verbal_wrong("that was wrong"));
        assert!(is_verbal_wrong("that's wrong"));
        assert!(is_verbal_wrong("not right"));
        assert!(is_verbal_wrong("incorrect"));
        assert!(is_verbal_wrong("bad"));
        assert!(is_verbal_wrong("mistake"));
        assert!(is_verbal_wrong("error"));
        assert!(is_verbal_wrong("not what i meant"));
        assert!(is_verbal_wrong("not what i wanted"));
    }

    #[test]
    fn test_is_verbal_wrong_case_insensitive() {
        assert!(is_verbal_wrong("Wrong"));
        assert!(is_verbal_wrong("WRONG"));
        assert!(is_verbal_wrong("No"));
        assert!(is_verbal_wrong("That Was Wrong"));
    }

    #[test]
    fn test_is_verbal_wrong_with_whitespace() {
        assert!(is_verbal_wrong("  wrong  "));
        assert!(is_verbal_wrong(" wrong "));
        assert!(is_verbal_wrong("\twrong\t"));
    }

    #[test]
    fn test_is_verbal_wrong_not_triggered_by_real_commands() {
        assert!(!is_verbal_wrong("open whatsapp"));
        assert!(!is_verbal_wrong("analyse pr 254 in zync"));
        assert!(!is_verbal_wrong("close chrome"));
        assert!(!is_verbal_wrong("search for cats"));
        assert!(!is_verbal_wrong("merge pr 23 in owner/repo"));
        assert!(!is_verbal_wrong("hello"));
        assert!(!is_verbal_wrong("pause"));
        assert!(!is_verbal_wrong("next track"));
        // "no" is tricky — it's a valid verbal wrong, but "no problem" is not
        assert!(!is_verbal_wrong("no problem"));
        assert!(!is_verbal_wrong("no thanks"));
    }

    #[test]
    fn test_record_and_reject_last_command() {
        // Test the record + verbal wrong flow
        // Set the last command directly
        {
            let mut guard = LAST_COMMAND.lock().unwrap();
            *guard = Some(("open wrongapp".to_string(), "open_app".to_string()));
        }

        // Simulate verbal wrong
        report_verbal_wrong();

        // If admin: LAST_COMMAND should be cleared and rejected file should have an entry
        // If not admin: report_verbal_wrong() is a no-op, LAST_COMMAND stays set
        if crate::admin_config::is_admin() {
            {
                let guard = LAST_COMMAND.lock().unwrap();
                assert!(guard.is_none(), "LAST_COMMAND should be cleared after verbal wrong (admin)");
            }
            let count = rejected_count();
            assert!(count > 0, "rejected_examples.jsonl should have at least one entry (admin)");
        } else {
            // Not admin — report_verbal_wrong is a no-op, LAST_COMMAND stays
            {
                let guard = LAST_COMMAND.lock().unwrap();
                assert!(guard.is_some(), "LAST_COMMAND should stay when not admin");
            }
        }

        // Clean up
        {
            let mut guard = LAST_COMMAND.lock().unwrap();
            *guard = None;
        }
    }

    #[test]
    fn test_rejected_examples_path() {
        let path = rejected_examples_path();
        // Should end with rejected_examples.jsonl
        assert!(path.to_string_lossy().contains("rejected_examples.jsonl"));
    }
}
