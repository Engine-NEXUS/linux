//! Proactive diary (C2): append-only local event log + daily rollup.
//!
//! The diary records notable assistant events (wakes, compound outcomes,
//! ghost sessions) to `diary.jsonl`. A boot-time rollup summarizes
//! yesterday so continuity survives restarts. This module NEVER speaks —
//! autonomous TTS is explicitly out of scope (safety: unprompted speech
//! during meetings would be startling). The pulse is record + summarize,
//! surfacing via the `diary_summary` command for UI later.

use std::path::Path;

pub(crate) const DIARY_FILE: &str = "diary.jsonl";
/// Keep the diary bounded: prune beyond this many lines on write.
const MAX_LINES: usize = 2000;

/// Notable event kinds. Keep the set small — the diary is a pulse
/// record, not a debug log.
pub fn log_event(app_data_dir: &Path, kind: &str, text: &str) {
    let path = app_data_dir.join(DIARY_FILE);
    let line = serde_json::json!({
        "ts": chrono::Utc::now().timestamp(),
        "kind": kind,
        "text": text.chars().take(200).collect::<String>(),
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", line);
    }
    prune(&path);
}

fn prune(path: &Path) {
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= MAX_LINES {
        return;
    }
    let keep = lines[lines.len() - MAX_LINES..].join("\n") + "\n";
    let _ = std::fs::write(path, keep);
}

/// Read the last `n` events (newest first). Pure-ish (file read).
pub fn recent_events(app_data_dir: &Path, n: usize) -> Vec<serde_json::Value> {
    let path = app_data_dir.join(DIARY_FILE);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    content
        .lines()
        .rev()
        .take(n)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// One-line rollup: "<total> events yesterday (wakes W, compounds C, ghost G, failures F)".
/// Pure over event slices — unit-tested.
pub fn rollup_label(events: &[serde_json::Value], now_ts: i64) -> String {
    let day_start = now_ts - (now_ts % 86400) - 86400;
    let day_end = day_start + 86400;
    let mut wakes = 0;
    let mut compounds = 0;
    let mut ghost = 0;
    let mut failures = 0;
    let mut total = 0;
    for e in events {
        let ts = e.get("ts").and_then(|t| t.as_i64()).unwrap_or(0);
        if ts < day_start || ts >= day_end {
            continue;
        }
        total += 1;
        match e.get("kind").and_then(|k| k.as_str()).unwrap_or("") {
            "wake" => wakes += 1,
            "compound_done" | "compound_failed" => compounds += 1,
            "ghost_enter" | "ghost_exit" => ghost += 1,
            "mcp_failed" => failures += 1,
            _ => {}
        }
    }
    if total == 0 {
        return "Quiet yesterday — no notable events.".to_string();
    }
    format!(
        "Yesterday: {} events ({} wakes, {} compounds, {} ghost, {} failures).",
        total, wakes, compounds, ghost, failures
    )
}

/// Boot rollup: log yesterday's summary line to stdout. Returns the label.
pub fn log_boot_rollup(app_data_dir: &Path) -> String {
    let events = recent_events(app_data_dir, MAX_LINES);
    let label = rollup_label(&events, chrono::Utc::now().timestamp());
    tracing::info!("diary: {}", label);
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("nexus_diary_test_{}_{}", name, std::process::id()));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    fn ev(ts: i64, kind: &str) -> serde_json::Value {
        serde_json::json!({"ts": ts, "kind": kind, "text": "t"})
    }

    #[test]
    fn test_log_and_recent() {
        let d = tmpdir("basic");
        log_event(&d, "wake", "nexus heard");
        log_event(&d, "ghost_enter", "session start");
        let evs = recent_events(&d, 10);
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0]["kind"], "ghost_enter");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn test_rollup_counts_yesterday() {
        // Fixed "now": day boundary math must isolate yesterday.
        let now = 1_752_000_000i64;
        let day_start = now - (now % 86400) - 86400;
        let evs = vec![
            ev(day_start + 100, "wake"),
            ev(day_start + 200, "wake"),
            ev(day_start + 300, "compound_done"),
            ev(day_start + 400, "ghost_enter"),
            ev(day_start + 500, "mcp_failed"),
            ev(now - 100, "wake"), // today — excluded
            ev(day_start - 100, "wake"), // day before — excluded
        ];
        let label = rollup_label(&evs, now);
        assert!(label.contains("5 events"), "got: {}", label);
        assert!(label.contains("2 wakes"), "got: {}", label);
    }

    #[test]
    fn test_rollup_quiet() {
        let label = rollup_label(&[], 1_752_000_000);
        assert!(label.contains("Quiet"));
    }

    #[test]
    fn test_prune_caps_lines() {
        let d = tmpdir("prune");
        for i in 0..10 {
            log_event(&d, "wake", &format!("w{i}"));
        }
        let content = std::fs::read_to_string(d.join(DIARY_FILE)).unwrap();
        assert_eq!(content.lines().count(), 10);
        let _ = std::fs::remove_dir_all(&d);
    }
}
