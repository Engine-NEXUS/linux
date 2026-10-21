//! Missed Intent Logger — logs unmatched voice transcripts to disk.
//!
//! When STT produces a transcript that cannot be matched to any deterministic,
//! NLU, or local command intent, this module records it to:
//! `%APPDATA%/com.nexus.assistant/missed_intents.jsonl`
//!
//! This provides an empirical audit trail of user commands and STT mishearings,
//! allowing continuous extension of the phonetic alias map and intent dataset.

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FILE_SIZE_BYTES: u64 = 5 * 1024 * 1024; // 5 MB rotation limit

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissedIntentRecord {
    pub timestamp_ms: u64,
    pub datetime: String,
    pub transcript: String,
    pub source: String,
    pub reason: String,
}

/// Resolves path to %APPDATA%/com.nexus.assistant/missed_intents.jsonl
pub fn get_missed_intents_file_path() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("com.nexus.assistant")
        .join("missed_intents.jsonl")
}

/// Rotates log file if it exceeds MAX_FILE_SIZE_BYTES
fn maybe_rotate_log(path: &PathBuf) {
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.len() > MAX_FILE_SIZE_BYTES {
            let mut backup_path = path.clone();
            backup_path.set_extension("old.jsonl");
            let _ = fs::rename(path, backup_path);
        }
    }
}

/// Log an unmatched or failed voice transcript to missed_intents.jsonl
pub fn log_missed_intent(transcript: &str, source: &str, reason: &str) {
    let text = transcript.trim();
    if text.is_empty() {
        return;
    }
    // Memory hygiene: this log feeds phrase suggestions and holds the user's own words. Fragments,
    // hallucinations and ambient chatter ("then the", "love us.") are noise AND needless private
    // text on disk; log only real, multi-word speech.
    if crate::memcore::hygiene::transcript_quality(text).is_err() {
        return;
    }

    let path = get_missed_intents_file_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    maybe_rotate_log(&path);

    let now = SystemTime::now();
    let timestamp_ms = now
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Simple formatted timestamp representation
    let datetime = match now.duration_since(UNIX_EPOCH) {
        Ok(dur) => {
            let secs = dur.as_secs();
            let hours = (secs / 3600) % 24;
            let mins = (secs / 60) % 60;
            let s = secs % 60;
            format!("{:02}:{:02}:{:02} UTC", hours, mins, s)
        }
        Err(_) => "unknown".to_string(),
    };

    let record = MissedIntentRecord {
        timestamp_ms,
        datetime,
        transcript: text.to_string(),
        source: source.to_string(),
        reason: reason.to_string(),
    };

    if let Ok(json_line) = serde_json::to_string(&record) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{}", json_line);
            tracing::info!(
                "[missed_intent] logged unmatched transcript: '{}' (reason: {}, src: {})",
                text, reason, source
            );
        }
    }
}

/// Retrieve recent missed intent records (newest first).
pub fn read_recent_missed_intents(limit: usize) -> Vec<MissedIntentRecord> {
    let path = get_missed_intents_file_path();
    if !path.exists() {
        return Vec::new();
    }

    let file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines().flatten() {
        if let Ok(rec) = serde_json::from_str::<MissedIntentRecord>(&line) {
            records.push(rec);
        }
    }

    records.reverse(); // Newest first
    if records.len() > limit {
        records.truncate(limit);
    }
    records
}

/// Tauri command to fetch recent missed intents
#[tauri::command]
pub async fn get_missed_intents(limit: Option<usize>) -> Result<Vec<MissedIntentRecord>, String> {
    let lim = limit.unwrap_or(50);
    Ok(read_recent_missed_intents(lim))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_serialization() {
        let record = MissedIntentRecord {
            timestamp_ms: 1727280000000,
            datetime: "21:00:00 UTC".to_string(),
            transcript: "goes to mode".to_string(),
            source: "orchestrator".to_string(),
            reason: "unknown_intent".to_string(),
        };
        let serialized = serde_json::to_string(&record).unwrap();
        assert!(serialized.contains("goes to mode"));
        let deserialized: MissedIntentRecord = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.transcript, "goes to mode");
    }
}
