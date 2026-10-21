//! Self-learning STT correction system.
//!
//! When STT mishears a word and the parser fails, the user typically repeats
//! the command with the correct word. This module detects that pattern and
//! learns the correction automatically.
//!
//! Flow:
//!   1. STT produces transcript ΓåÆ parser fails ΓåÆ log_failed_transcript()
//!   2. User repeats ΓåÆ STT produces correct transcript ΓåÆ parser succeeds
//!   3. log_successful_transcript() compares with recent failure
//!   4. If 1-2 words differ at the same positions ΓåÆ learn the correction
//!   5. After 3 consistent corrections ΓåÆ auto_apply = true
//!   6. Frontend loads auto_apply corrections at startup and applies them
//!
//! Storage: %APPDATA%/com.nexus.assistant/learned_corrections.json
//! RAM cost: ~1-10 KB (in-memory HashMap)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Number of consistent corrections required before auto-applying.
const LEARN_THRESHOLD: u32 = 3;

/// Time window (seconds) within which a success is considered a correction
/// of a recent failure. If the user waits too long, they probably said
/// something unrelated.
const CORRECTION_WINDOW_SECS: u64 = 30;

/// Maximum number of differing word positions for a correction to be learned.
/// If too many words differ, the user probably said something completely
/// different, not a correction.
const MAX_DIFF_POSITIONS: usize = 2;

/// A single learned correction entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedCorrection {
    /// The misheard word (what STT produced).
    pub from: String,
    /// The correct word (what the user actually said).
    pub to: String,
    /// The word immediately before the corrected word, for context.
    /// Empty string if the corrected word was at the start.
    pub context_before: String,
    /// Number of times this correction has been observed.
    pub count: u32,
    /// Whether this correction should be auto-applied.
    /// Set to true after `LEARN_THRESHOLD` consistent observations.
    pub auto_apply: bool,
    /// Unix timestamp of first observation.
    pub first_seen: u64,
    /// Unix timestamp of most recent observation.
    pub last_seen: u64,
}

/// The JSON file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CorrectionFile {
    corrections: Vec<LearnedCorrection>,
}

impl Default for CorrectionFile {
    fn default() -> Self {
        Self {
            corrections: Vec::new(),
        }
    }
}

/// In-memory state for the learning system.
pub struct SttLearningState {
    /// The most recent failed transcript + timestamp.
    /// Used to compare with the next successful transcript.
    pending_failure: Arc<Mutex<Option<PendingFailure>>>,
    /// All learned corrections, keyed by "context_before|from" for fast lookup.
    corrections: Arc<Mutex<HashMap<String, LearnedCorrection>>>,
    /// Path to the JSON file.
    file_path: std::path::PathBuf,
}

#[derive(Debug, Clone)]
struct PendingFailure {
    transcript: String,
    timestamp: Instant,
}

impl SttLearningState {
    pub fn new() -> Self {
        let file_path = get_corrections_file_path();

        // Load existing corrections from file
        let corrections = load_corrections(&file_path);
        let corrections_map = corrections
            .corrections
            .into_iter()
            .map(|c| (make_key(&c.context_before, &c.from), c))
            .collect::<HashMap<_, _>>();

        tracing::info!(
            "stt_learning: loaded {} corrections from {}",
            corrections_map.len(),
            file_path.display()
        );

        Self {
            pending_failure: Arc::new(Mutex::new(None)),
            corrections: Arc::new(Mutex::new(corrections_map)),
            file_path,
        }
    }

    /// Log a failed transcript. Called when the parser can't handle the STT output.
    pub async fn log_failure(&self, transcript: &str) {
        let mut pending = self.pending_failure.lock().await;
        *pending = Some(PendingFailure {
            transcript: transcript.to_lowercase(),
            timestamp: Instant::now(),
        });
        crate::missed_intent_logger::log_missed_intent(transcript, "stt_learning", "parser_failure");
        tracing::info!("stt_learning: logged failure: \"{}\"", transcript);
    }

    /// Log a successful transcript. Called when the parser succeeds.
    /// If there's a recent failure, compares the two and learns any corrections.
    pub async fn log_success(&self, transcript: &str) {
        let pending = {
            let mut pending_guard = self.pending_failure.lock().await;
            pending_guard.take()
        };

        let Some(failure) = pending else {
            // No recent failure ΓÇö nothing to learn
            return;
        };

        // Check time window
        if failure.timestamp.elapsed().as_secs() > CORRECTION_WINDOW_SECS {
            tracing::debug!("stt_learning: success outside correction window, ignoring");
            return;
        }

        let success_text = transcript.to_lowercase();

        // Diff the two transcripts
        let diffs = word_diff(&failure.transcript, &success_text);

        if diffs.is_empty() {
            // No differences ΓÇö the failure was probably a parser issue, not STT
            return;
        }

        if diffs.len() > MAX_DIFF_POSITIONS {
            tracing::debug!(
                "stt_learning: {} positions differ (max {}), not a correction",
                diffs.len(),
                MAX_DIFF_POSITIONS
            );
            return;
        }

        // Learn each differing word pair
        let mut learned = Vec::new();
        for diff in &diffs {
            let context_before = diff.context_before.clone();
            let from = diff.from_word.clone();
            let to = diff.to_word.clone();

            // Skip if both words are identical (shouldn't happen, but guard)
            if from == to {
                continue;
            }

            // Skip very short words (1-2 chars) ΓÇö likely noise
            if from.len() < 3 || to.len() < 3 {
                continue;
            }

            // Skip if the words are too different (Levenshtein > 3)
            let dist = levenshtein(&from, &to);
            if dist > 3 {
                tracing::debug!(
                    "stt_learning: \"{}\" ΓåÆ \"{}\" distance {} too large, skipping",
                    from,
                    to,
                    dist
                );
                continue;
            }

            let key = make_key(&context_before, &from);
            let mut corrections = self.corrections.lock().await;
            let entry = corrections.entry(key).or_insert(LearnedCorrection {
                from: from.clone(),
                to: to.clone(),
                context_before: context_before.clone(),
                count: 0,
                auto_apply: false,
                first_seen: now_unix(),
                last_seen: now_unix(),
            });

            entry.count += 1;
            entry.last_seen = now_unix();

            if entry.count >= LEARN_THRESHOLD && !entry.auto_apply {
                entry.auto_apply = true;
                tracing::info!(
                    "stt_learning: correction \"{}\" ΓåÆ \"{}\" (context: \"{}\") now auto-apply (count={})",
                    entry.from,
                    entry.to,
                    entry.context_before,
                    entry.count
                );
            } else {
                tracing::info!(
                    "stt_learning: observed \"{}\" ΓåÆ \"{}\" (context: \"{}\") count={}/{}",
                    entry.from,
                    entry.to,
                    entry.context_before,
                    entry.count,
                    LEARN_THRESHOLD
                );
            }

            learned.push(entry.clone());
        }

        // Save to file if we learned anything
        if !learned.is_empty() {
            self.save_to_file().await;
        }
    }

    /// Get all corrections that are ready for auto-apply.
    pub async fn get_auto_apply_corrections(&self) -> Vec<LearnedCorrection> {
        let corrections = self.corrections.lock().await;
        corrections
            .values()
            .filter(|c| c.auto_apply)
            .cloned()
            .collect()
    }

    /// Save corrections to the JSON file.
    async fn save_to_file(&self) {
        let corrections = self.corrections.lock().await;
        let file_data = CorrectionFile {
            corrections: corrections.values().cloned().collect(),
        };

        // Ensure parent dir exists
        if let Some(parent) = self.file_path.parent() {
            if !parent.exists() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("stt_learning: failed to create dir: {}", e);
                    return;
                }
            }
        }

        match serde_json::to_string_pretty(&file_data) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.file_path, json) {
                    tracing::warn!("stt_learning: failed to write file: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("stt_learning: failed to serialize: {}", e);
            }
        }
    }
}

/// A single word-level difference between two transcripts.
#[derive(Debug)]
struct WordDiff {
    /// Word before the differing position (context). Empty if position 0.
    context_before: String,
    /// The word in the failed transcript.
    from_word: String,
    /// The word in the successful transcript.
    to_word: String,
}

/// Compare two transcripts word-by-word and find positions where they differ.
/// Only returns diffs where the words are at the same position in both transcripts.
fn word_diff(failed: &str, succeeded: &str) -> Vec<WordDiff> {
    let failed_words: Vec<&str> = failed.split_whitespace().collect();
    let succeeded_words: Vec<&str> = succeeded.split_whitespace().collect();

    // If word counts differ by more than 1, it's probably not a simple correction
    if failed_words.len().abs_diff(succeeded_words.len()) > 1 {
        return Vec::new();
    }

    let mut diffs = Vec::new();
    let max_len = failed_words.len().max(succeeded_words.len());

    for i in 0..max_len {
        let f_word = failed_words.get(i).copied().unwrap_or("");
        let s_word = succeeded_words.get(i).copied().unwrap_or("");

        if f_word != s_word && !f_word.is_empty() && !s_word.is_empty() {
            let context_before = if i > 0 {
                failed_words.get(i - 1).copied().unwrap_or("").to_string()
            } else {
                String::new()
            };

            diffs.push(WordDiff {
                context_before,
                from_word: f_word.to_string(),
                to_word: s_word.to_string(),
            });
        }
    }

    diffs
}

/// Compute Levenshtein edit distance between two strings.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1)
                .min(curr[j - 1] + 1)
                .min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Create a lookup key from context + from word.
fn make_key(context: &str, from: &str) -> String {
    format!("{}|{}", context, from)
}

/// Get the current Unix timestamp in seconds.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Get the path to the learned corrections JSON file.
fn get_corrections_file_path() -> std::path::PathBuf {
    let data_dir = dirs_next::data_dir().unwrap_or_else(|| {
        std::path::PathBuf::from(".")
    });
    data_dir
        .join("com.nexus.assistant")
        .join("learned_corrections.json")
}

/// Load corrections from the JSON file.
fn load_corrections(path: &std::path::Path) -> CorrectionFile {
    if !path.exists() {
        return CorrectionFile::default();
    }

    match std::fs::read_to_string(path) {
        Ok(content) => {
            match serde_json::from_str::<CorrectionFile>(&content) {
                Ok(file) => file,
                Err(e) => {
                    tracing::warn!("stt_learning: failed to parse {}: {}", path.display(), e);
                    CorrectionFile::default()
                }
            }
        }
        Err(e) => {
            tracing::warn!("stt_learning: failed to read {}: {}", path.display(), e);
            CorrectionFile::default()
        }
    }
}

// ΓöÇΓöÇΓöÇ Tauri IPC commands ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

/// IPC: Log a failed transcript (parser couldn't handle it).
#[tauri::command]
pub async fn log_failed_transcript(
    transcript: String,
    state: tauri::State<'_, SttLearningState>,
) -> Result<(), String> {
    state.log_failure(&transcript).await;
    Ok(())
}

/// IPC: Log a successful transcript (parser handled it).
/// Compares with any recent failure and learns corrections.
#[tauri::command]
pub async fn log_successful_transcript(
    transcript: String,
    state: tauri::State<'_, SttLearningState>,
) -> Result<(), String> {
    state.log_success(&transcript).await;
    Ok(())
}

/// IPC: Get all corrections that are ready for auto-apply.
/// The frontend calls this at startup and applies them in correctSttTranscript().
#[tauri::command]
pub async fn get_learned_corrections(
    state: tauri::State<'_, SttLearningState>,
) -> Result<Vec<LearnedCorrection>, String> {
    Ok(state.get_auto_apply_corrections().await)
}

// ΓöÇΓöÇΓöÇ Tests ΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇΓöÇ

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_diff_single_word() {
        let diffs = word_diff("analyse pr 254 in zink", "analyse pr 254 in zync");
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].from_word, "zink");
        assert_eq!(diffs[0].to_word, "zync");
        assert_eq!(diffs[0].context_before, "in");
    }

    #[test]
    fn test_word_diff_no_diff() {
        let diffs = word_diff("open chrome", "open chrome");
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_word_diff_completely_different() {
        let diffs = word_diff("open chrome", "close firefox");
        assert_eq!(diffs.len(), 2);
    }

    #[test]
    fn test_word_diff_too_many_diffs() {
        // Lengths differ by 1, all 3 positions differ ΓåÆ 3 diffs > MAX_DIFF_POSITIONS (2)
        let diffs = word_diff("open chrome browser", "close firefox now");
        assert!(diffs.len() > MAX_DIFF_POSITIONS);
    }

    #[test]
    fn test_word_diff_different_lengths() {
        let diffs = word_diff("open the chrome", "open chrome");
        // Lengths differ by 1 ΓÇö should still find diffs
        // "the" vs "chrome" at position 1, "" vs "chrome" at position 2
        // Actually: pos 0: open==open, pos 1: the vs chrome (diff), pos 2: chrome vs "" (skip, empty)
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].from_word, "the");
        assert_eq!(diffs[0].to_word, "chrome");
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("zink", "zync"), 2); // iΓåÆy + kΓåÆc
        assert_eq!(levenshtein("cervix", "servx"), 2); // cΓåÆs + delete i
        assert_eq!(levenshtein("hello", "hello"), 0);
        assert_eq!(levenshtein("abc", "xyz"), 3);
        assert_eq!(levenshtein("zinc", "zync"), 1); // iΓåÆy only
    }

    #[test]
    fn test_levenshtein_edge() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn test_make_key() {
        assert_eq!(make_key("in", "zink"), "in|zink");
        assert_eq!(make_key("", "zink"), "|zink");
    }

    #[test]
    fn test_correction_file_default() {
        let f = CorrectionFile::default();
        assert!(f.corrections.is_empty());
    }

    #[test]
    fn test_word_diff_context_at_start() {
        let diffs = word_diff("zink 254", "zync 254");
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].context_before, ""); // First word, no context
        assert_eq!(diffs[0].from_word, "zink");
        assert_eq!(diffs[0].to_word, "zync");
    }

    #[test]
    fn test_word_diff_preserves_order() {
        let diffs = word_diff("analyse pr 254 in zink now", "analyse pr 254 in zync now");
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].from_word, "zink");
        assert_eq!(diffs[0].to_word, "zync");
        assert_eq!(diffs[0].context_before, "in");
    }
}
