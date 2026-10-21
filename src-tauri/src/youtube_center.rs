//! YouTubeCenter — Sub-Center for YouTube Intelligence & Video Journaling.
//!
//! Carbon-copied from production open-source `youtube-mcp-server` engine,
//! providing zero-API-key video search, full timestamped transcript retrieval,
//! and structured video-to-journal digestion integrated into `diary.rs`.

use crate::center::{ConfirmKind, SubCenter, Validity};
use crate::diary;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Actions this center owns.
#[allow(dead_code)]
pub const ACTIONS: &[&str] = &[
    "youtube_search",
    "youtube_transcript",
    "youtube_journal",
];

pub struct YouTubeCenter;

impl YouTubeCenter {
    pub fn new() -> Self {
        YouTubeCenter
    }
}

impl Default for YouTubeCenter {
    fn default() -> Self {
        Self::new()
    }
}

fn slot_str(slots: &Value, key: &str) -> String {
    slots
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

impl SubCenter for YouTubeCenter {
    fn name(&self) -> &'static str {
        "YouTubeCenter"
    }

    fn validate(&self, action: &str, slots: &Value) -> Validity {
        match action {
            "youtube_search" => {
                let query = slot_str(slots, "query");
                if query.is_empty() {
                    Validity::NeedSlot {
                        slot: "query",
                        prompt: "What would you like me to search for on YouTube, sir?".to_string(),
                    }
                } else {
                    Validity::Ok
                }
            }
            "youtube_transcript" => {
                let url = slot_str(slots, "url");
                if url.is_empty() {
                    Validity::NeedSlot {
                        slot: "url",
                        prompt: "Which YouTube video URL should I fetch the transcript for, sir?".to_string(),
                    }
                } else {
                    Validity::Ok
                }
            }
            "youtube_journal" => {
                let url = slot_str(slots, "url");
                let topic = slot_str(slots, "topic");
                if url.is_empty() && topic.is_empty() {
                    Validity::NeedSlot {
                        slot: "url",
                        prompt: "Which video should I summarize and add to your journal, sir?".to_string(),
                    }
                } else {
                    Validity::Ok
                }
            }
            _ => Validity::Invalid {
                reason: "not_a_youtube_action",
                prompt: "That's not a recognized YouTube command, sir.".to_string(),
            },
        }
    }

    fn confirm_kind(&self, action: &str, slots: &Value) -> ConfirmKind {
        match action {
            "youtube_search" => {
                let query = slot_str(slots, "query");
                ConfirmKind::RepeatBack {
                    ack: format!("Searching YouTube for {}, sir.", query),
                }
            }
            "youtube_transcript" => ConfirmKind::RepeatBack {
                ack: "Extracting video transcript, sir.".to_string(),
            },
            "youtube_journal" => {
                let target = if !slot_str(slots, "url").is_empty() {
                    "this video"
                } else {
                    "that topic"
                };
                ConfirmKind::RepeatBack {
                    ack: format!("Analyzing {} and creating a journal entry, sir.", target),
                }
            }
            _ => ConfirmKind::None,
        }
    }
}

/// Locate Python bridge script `server/youtube/engine.py`.
pub fn resolve_youtube_engine_path() -> Option<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("..").join("server").join("youtube").join("engine.py"),
        manifest_dir.join("server").join("youtube").join("engine.py"),
        PathBuf::from("server").join("youtube").join("engine.py"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Executes the local YouTube python engine.
pub fn execute_engine(action: &str, arg: &str, limit: Option<usize>) -> Result<Value, String> {
    let script_path = resolve_youtube_engine_path()
        .ok_or_else(|| "YouTube engine not found at server/youtube/engine.py".to_string())?;

    let mut cmd = Command::new("python");
    cmd.arg(&script_path).arg(action).arg(arg);

    if let Some(l) = limit {
        cmd.arg("--limit").arg(l.to_string());
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to spawn python YouTube engine: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "YouTube engine failed (exit {}): {}",
            output.status, stderr_str
        ));
    }

    serde_json::from_str::<Value>(&stdout_str)
        .map_err(|e| format!("Invalid JSON from YouTube engine: {e} (stdout: {stdout_str})"))
}

/// Searches YouTube and returns top results.
pub fn search_videos(query: &str, limit: usize) -> Result<Value, String> {
    let res = execute_engine("search", query, Some(limit))?;
    if res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(res.get("data").cloned().unwrap_or(json!([])))
    } else {
        Err(res.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown search error").to_string())
    }
}

/// Digests a video and appends it to NEXUS's local proactive diary.
pub fn summarize_to_diary(app_data_dir: &Path, video_url: &str) -> Result<Value, String> {
    let res = execute_engine("journal", video_url, None)?;
    if !res.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(res.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown journal error").to_string());
    }

    let data = res.get("data").cloned().unwrap_or(json!({}));
    let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("YouTube Video");
    let channel = data.get("channel").and_then(|v| v.as_str()).unwrap_or("");
    let duration = data.get("duration").and_then(|v| v.as_u64()).unwrap_or(0);
    let excerpt = data.get("transcript_excerpt").and_then(|v| v.as_str()).unwrap_or("");

    let summary_line = format!(
        "Watched '{}' by {} ({}s). Excerpt: {}",
        title, channel, duration, excerpt.chars().take(120).collect::<String>()
    );

    // Append to NEXUS diary.jsonl
    diary::log_event(app_data_dir, "youtube_journal", &summary_line);

    Ok(json!({
        "status": "logged_to_diary",
        "title": title,
        "channel": channel,
        "summary": summary_line,
        "full_data": data,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_youtube_center_name_and_actions() {
        let center = YouTubeCenter::new();
        assert_eq!(center.name(), "YouTubeCenter");
        assert!(ACTIONS.contains(&"youtube_search"));
        assert!(ACTIONS.contains(&"youtube_transcript"));
        assert!(ACTIONS.contains(&"youtube_journal"));
    }

    #[test]
    fn test_youtube_validation() {
        let center = YouTubeCenter::new();

        // Missing query
        let empty_search = json!({ "query": "" });
        assert!(matches!(
            center.validate("youtube_search", &empty_search),
            Validity::NeedSlot { slot: "query", .. }
        ));

        // Valid search
        let valid_search = json!({ "query": "Rust programming" });
        assert_eq!(center.validate("youtube_search", &valid_search), Validity::Ok);

        // Unknown action
        assert!(matches!(
            center.validate("unknown_action", &valid_search),
            Validity::Invalid { .. }
        ));
    }

    #[test]
    fn test_youtube_confirm_kind() {
        let center = YouTubeCenter::new();
        let search = json!({ "query": "Iron Man" });
        let confirm = center.confirm_kind("youtube_search", &search);
        assert!(matches!(confirm, ConfirmKind::RepeatBack { ack } if ack.contains("Iron Man")));
    }
}
