//! Outbound API request counters for the Command Hub "Requests" insight:
//! **today** and **all-time**, broken down by kind (vision, llm, stt, worker,
//! mcp). Persisted to `usage_stats.json` in the app-data dir.
//!
//! Honest scope: only requests NEXUS itself sends are counted, and counting
//! starts the day this ships (no back-fill). Provider-console totals are not
//! readable from here. The pure `Counter` takes the date as a parameter so
//! day roll-over is unit-tested without a clock.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub const FILE: &str = "usage_stats.json";
/// Days of per-day history kept (today's bucket + this many older ones).
const KEEP_DAYS: usize = 30;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counter {
    /// All-time total per kind.
    #[serde(default)]
    pub all: BTreeMap<String, u64>,
    /// Per-day totals per kind, keyed `YYYY-MM-DD`.
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsageSnapshot {
    pub today: u64,
    pub all_time: u64,
    pub today_by_kind: BTreeMap<String, u64>,
    pub all_by_kind: BTreeMap<String, u64>,
    pub date: String,
}

impl Counter {
    /// Count one request of `kind` on `date`. Prunes old days. Pure.
    pub fn bump(&mut self, date: &str, kind: &str) {
        *self.all.entry(kind.to_string()).or_insert(0) += 1;
        *self
            .days
            .entry(date.to_string())
            .or_default()
            .entry(kind.to_string())
            .or_insert(0) += 1;
        while self.days.len() > KEEP_DAYS + 1 {
            let Some(oldest) = self.days.keys().next().cloned() else { break };
            self.days.remove(&oldest);
        }
    }

    /// Totals for `date` and all time. Pure.
    pub fn snapshot(&self, date: &str) -> UsageSnapshot {
        let today_by_kind = self.days.get(date).cloned().unwrap_or_default();
        UsageSnapshot {
            today: today_by_kind.values().sum(),
            all_time: self.all.values().sum(),
            today_by_kind,
            all_by_kind: self.all.clone(),
            date: date.to_string(),
        }
    }
}

struct State {
    path: PathBuf,
    counter: Counter,
}

static STATE: Lazy<Mutex<Option<State>>> = Lazy::new(|| Mutex::new(None));

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Load persisted counters. Call once at startup; `bump` is a no-op before.
pub fn init(app_data_dir: &Path) {
    let path = app_data_dir.join(FILE);
    let counter = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Counter>(&s).ok())
        .unwrap_or_default();
    *STATE.lock() = Some(State { path, counter });
}

/// Count one outbound API request. Best-effort, never panics, cheap.
pub fn bump(kind: &str) {
    let mut guard = STATE.lock();
    let Some(st) = guard.as_mut() else { return };
    st.counter.bump(&today(), kind);
    if let Ok(json) = serde_json::to_string(&st.counter) {
        let _ = std::fs::write(&st.path, json);
    }
}

pub fn snapshot_now() -> UsageSnapshot {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(st) => st.counter.snapshot(&today()),
        None => Counter::default().snapshot(&today()),
    }
}

/// IPC: today + all-time request counts for the Command Hub insights card.
#[tauri::command]
pub fn get_usage_stats() -> UsageSnapshot {
    snapshot_now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_today_and_all_time_by_kind() {
        let mut c = Counter::default();
        c.bump("2026-10-07", "llm");
        c.bump("2026-10-07", "llm");
        c.bump("2026-10-07", "stt");
        let s = c.snapshot("2026-10-07");
        assert_eq!((s.today, s.all_time), (3, 3));
        assert_eq!(s.today_by_kind["llm"], 2);
        assert_eq!(s.all_by_kind["stt"], 1);
    }

    #[test]
    fn day_rollover_resets_today_but_keeps_all_time() {
        let mut c = Counter::default();
        c.bump("2026-10-07", "vision");
        c.bump("2026-10-07", "vision");
        c.bump("2026-10-08", "vision");
        assert_eq!(c.snapshot("2026-10-08").today, 1);
        assert_eq!(c.snapshot("2026-10-08").all_time, 3);
        assert_eq!(c.snapshot("2026-10-09").today, 0); // a day with no requests
        assert_eq!(c.snapshot("2026-10-07").today, 2); // history intact
    }

    #[test]
    fn old_days_are_pruned_but_all_time_is_not() {
        let mut c = Counter::default();
        for d in 1..=40 {
            c.bump(&format!("2026-09-{d:02}"), "mcp");
        }
        assert!(c.days.len() <= KEEP_DAYS + 1);
        assert_eq!(c.snapshot("2026-09-40").all_time, 40);
    }

    #[test]
    fn empty_counter_is_zero() {
        let s = Counter::default().snapshot("2026-10-07");
        assert_eq!((s.today, s.all_time), (0, 0));
    }

    #[test]
    fn round_trips_through_json() {
        let mut c = Counter::default();
        c.bump("2026-10-07", "worker");
        let back: Counter = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        // Old/partial files load (serde defaults).
        let partial: Counter = serde_json::from_str("{}").unwrap();
        assert_eq!(partial, Counter::default());
    }
}
