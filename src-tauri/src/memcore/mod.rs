//! Memory Core — the single owner of what NEXUS remembers (plan P1).
//!
//! `store` is the SQLite layer. This module adds the process-wide handle
//! cache, the one-time import of the legacy JSON files, the `context_pack`
//! that is the ONLY thing allowed to leave the device for a cloud model, and
//! thin mirror/forget/wipe entry points used by `memory.rs`.
//!
//! Rollout is dual-write / dual-read: the legacy files stay authoritative
//! for one release, memcore mirrors every write and answers recall first,
//! and anything memcore cannot answer falls back to the legacy path. The
//! `memcore` setting (default on) turns the whole thing off.

pub mod agenda;
pub mod briefing;
pub mod crypto;
pub mod google_io;
pub mod health;
pub mod hygiene;
pub mod mailtriage;
pub mod mailwatch;
pub mod names;
pub mod offer;
pub mod leavewatch;
pub mod leases;
pub mod people;
pub mod restore;
pub mod resume;
pub mod scheduler;
pub mod session;
pub mod store;
pub mod timetable;
pub mod timetable_io;
pub mod wa;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use store::{Observation, Store, Tier, Trust};

/// Context-pack budget in chars (plan 95: 800 → 2000).
pub const DEFAULT_BUDGET: usize = 2000;
const MAX_PACK_FACTS: usize = 8;
const MAX_PACK_EPISODES: usize = 3;

static STORES: Lazy<Mutex<HashMap<PathBuf, Arc<Mutex<Store>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static EPISODE_SEQ: AtomicU64 = AtomicU64::new(0);

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Boolean flag from settings.json (camelCase key); `default` when the file
/// or key is missing/unreadable.
pub fn flag(app_data_dir: &Path, key: &str, default: bool) -> bool {
    std::fs::read_to_string(app_data_dir.join("settings.json"))
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|j| j.get(key).and_then(|v| v.as_bool()))
        .unwrap_or(default)
}

/// `memcore` setting from settings.json (default true; unreadable = true).
pub fn enabled(app_data_dir: &Path) -> bool {
    flag(app_data_dir, "memcore", true)
}

/// Run `f` against this app-data dir's store (opened + migrated on first
/// use). `None` when disabled or the database cannot be opened — callers
/// then use the legacy path.
pub fn with_store<T>(app_data_dir: &Path, f: impl FnOnce(&Store) -> T) -> Option<T> {
    let handle = store_handle(app_data_dir)?;
    let guard = handle.lock();
    Some(f(&guard))
}

/// The shared handle to this dir's store (opened + migrated on first use), for
/// callers that must not hold the lock across an `.await` (take it briefly,
/// twice, around the network work). `None` when disabled or unopenable.
pub fn store_handle(app_data_dir: &Path) -> Option<Arc<Mutex<Store>>> {
    if !enabled(app_data_dir) {
        return None;
    }
    let mut map = STORES.lock();
    if let Some(h) = map.get(app_data_dir) {
        return Some(h.clone());
    }
    let store = match Store::open(
        &crate::memory::resolve_memory_dir(app_data_dir),
        crypto::process_key(),
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("memcore: cannot open database ({e}); using legacy files");
            return None;
        }
    };
    store.batch(|s| import_legacy(s, app_data_dir));
    store.prune(now_ts());
    let h = Arc::new(Mutex::new(store));
    map.insert(app_data_dir.to_path_buf(), h.clone());
    Some(h)
}

/// One-time import of core.json / facts.json / episodes.jsonl. Idempotent
/// via a `meta` flag; the legacy files are left untouched.
fn import_legacy(store: &Store, app_data_dir: &Path) {
    if store.meta_get("legacy_imported").is_some() {
        return;
    }
    let dir = crate::memory::resolve_memory_dir(app_data_dir);
    let now = now_ts();
    let mut n = 0usize;

    if let Some(obj) = crate::memory::read_json(&dir.join("core.json")).as_object() {
        for (k, v) in obj {
            if let Some(v) = v.as_str() {
                let o = Observation {
                    tier: Tier::Fact,
                    key: k.clone(),
                    value: v.to_string(),
                    source: "legacy:core".into(),
                    trust: Trust::UserSaid,
                    pinned: true,
                };
                if store.observe(&o, now).is_ok() {
                    n += 1;
                }
            }
        }
    }
    if let serde_json::Value::Array(arr) = crate::memory::read_json(&dir.join("facts.json")) {
        for item in arr {
            let k = item.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let v = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let ts = item.get("updated_at").and_then(|v| v.as_i64()).unwrap_or(now);
            let o = Observation {
                tier: Tier::Fact,
                key: k.into(),
                value: v.into(),
                source: "legacy:facts".into(),
                trust: Trust::Derived,
                pinned: false,
            };
            // Keep the original timestamp so old facts still decay on schedule.
            if store.observe(&o, ts).is_ok() {
                n += 1;
            }
        }
    }
    if let Ok(content) = std::fs::read_to_string(dir.join("episodes.jsonl")) {
        for (i, line) in content.lines().enumerate() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let ts = v.get("ts").and_then(|t| t.as_i64()).unwrap_or(now);
            let text = v.get("transcript").and_then(|t| t.as_str()).unwrap_or("");
            let o = Observation {
                tier: Tier::Episode,
                key: format!("ep:{ts}:i{i}"),
                value: text.into(),
                source: "legacy:episodes".into(),
                trust: Trust::Derived,
                pinned: false,
            };
            if store.observe(&o, ts).is_ok() {
                n += 1;
            }
        }
    }
    store.meta_set("legacy_imported", "1");
    tracing::info!("memcore: imported {n} legacy records");
}

/// Mirror an explicit "remember" (user said it → pinned).
pub fn mirror_core_fact(app_data_dir: &Path, key: &str, value: &str) {
    with_store(app_data_dir, |s| {
        let _ = s.observe(
            &Observation {
                tier: Tier::Fact,
                key: key.into(),
                value: value.into(),
                source: "user:remember".into(),
                trust: Trust::UserSaid,
                pinned: true,
            },
            now_ts(),
        );
    });
}

/// Mirror a mined fact (derived, decays).
pub fn mirror_learned_fact(app_data_dir: &Path, key: &str, value: &str) {
    with_store(app_data_dir, |s| {
        let _ = s.observe(
            &Observation {
                tier: Tier::Fact,
                key: key.into(),
                value: value.into(),
                source: "miner:episode".into(),
                trust: Trust::Derived,
                pinned: false,
            },
            now_ts(),
        );
    });
}

pub fn mirror_episode(app_data_dir: &Path, transcript: &str) {
    let seq = EPISODE_SEQ.fetch_add(1, Ordering::Relaxed);
    let now = now_ts();
    with_store(app_data_dir, |s| {
        let _ = s.observe(
            &Observation {
                tier: Tier::Episode,
                key: format!("ep:{now}:{seq}"),
                value: transcript.into(),
                source: "turn".into(),
                trust: Trust::Derived,
                pinned: false,
            },
            now,
        );
        s.prune(now);
    });
}

pub fn forget_key(app_data_dir: &Path, key: &str) -> usize {
    with_store(app_data_dir, |s| s.forget_key(key, "user", now_ts())).unwrap_or(0)
}

/// Erase every record. When encrypted, also rotate the data key so any old
/// ciphertext left in free disk blocks becomes permanently unreadable.
pub fn wipe(app_data_dir: &Path) -> usize {
    with_store(app_data_dir, |s| {
        let n = s.wipe("user", now_ts());
        if s.is_encrypted() {
            #[cfg(not(test))]
            if let Some(k) = crypto::rotate_key() {
                s.rekey(k);
            }
            #[cfg(test)]
            if let Some(k) = crypto::generate_key() {
                s.rekey(k);
            }
        }
        n
    })
    .unwrap_or(0)
}

/// Record what is leaving the device for a cloud model on this turn.
pub fn log_egress(app_data_dir: &Path, route: &str, request: &str, memory: &str) {
    with_store(app_data_dir, |s| s.log_egress(now_ts(), route, request, memory));
}

pub fn egress_log(app_data_dir: &Path, limit: usize) -> Vec<store::EgressEntry> {
    with_store(app_data_dir, |s| s.egress_recent(limit)).unwrap_or_default()
}

/// Everything remembered, for the "what do you remember" views.
pub fn list_rows(app_data_dir: &Path, limit: usize) -> Vec<store::Row> {
    with_store(app_data_dir, |s| s.list_rows(limit)).unwrap_or_default()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    pub enabled: bool,
    /// At-rest encryption active (false = plaintext fallback or disabled).
    pub encrypted: bool,
    pub facts: usize,
    pub episodes: usize,
    pub egress_7d: usize,
    pub redact_names: bool,
    /// Foreground-window recording (resume points) is on.
    pub activity: bool,
    /// Resume points currently stored.
    pub resume_points: usize,
    /// Once-a-day boot briefing is on.
    pub briefing: bool,
    /// Timetable reminders are on.
    pub timetable: bool,
    /// Saved timetable slots.
    pub slots: usize,
    /// Inbox watching + calendar are on.
    pub mail: bool,
    /// A Google account with mail access is connected.
    pub google_connected: bool,
    /// Important emails currently filed (7 days).
    pub mail_items: usize,
    /// Google rejected the stored sign-in (typically the 7-day Testing-mode
    /// expiry): the inbox watcher is paused until the user signs in again.
    pub google_needs_signin: bool,
    /// WhatsApp priority watcher is switched on (default off).
    pub whatsapp: bool,
    /// Message text may be spoken aloud (off: sidebar only).
    pub whatsapp_speak: bool,
    /// Leave-by reminders (calendar + traffic + remembered home) are on.
    pub leave_by: bool,
    /// Learned WhatsApp people.
    pub people: usize,
    /// "What was open" snapshots (reopen offer after a restart) are on.
    pub session: bool,
    /// Startup connection check in the greeting is on.
    pub health: bool,
}

/// Erase recorded resume points (activity history) and nothing else.
pub fn clear_activity(app_data_dir: &Path) -> usize {
    // The "what was open" snapshots are activity history too.
    with_store(app_data_dir, |s| {
        s.meta_set("session_last", "");
        s.meta_set("session_pending", "");
        s.clear_tier(Tier::Resume, "user", now_ts())
    })
    .unwrap_or(0)
}

pub fn status(app_data_dir: &Path) -> Status {
    let redact = names::enabled(app_data_dir);
    let activity = flag(app_data_dir, "memcoreActivity", true);
    let briefing = flag(app_data_dir, "memcoreBriefing", true);
    let timetable = flag(app_data_dir, "memcoreTimetable", true);
    let mail = flag(app_data_dir, "memcoreMail", true);
    let whatsapp = flag(app_data_dir, "memcoreWhatsapp", false);
    let whatsapp_speak = flag(app_data_dir, "memcoreWhatsappSpeak", false);
    let leave_by = flag(app_data_dir, "memcoreLeaveBy", true);
    let session = flag(app_data_dir, "memcoreSession", true);
    let health = flag(app_data_dir, "memcoreHealth", true);
    let google_connected = !google_io::list_accounts(google_io::can_read_mail).is_empty();
    let google_needs_signin = with_store(app_data_dir, |s| s.meta_get("mail_needs_signin").map(|v| !v.is_empty()).unwrap_or(false))
        .unwrap_or(false);
    match with_store(app_data_dir, |s| {
        (
            s.is_encrypted(),
            s.count(Tier::Fact),
            s.count(Tier::Episode),
            s.egress_recent(10_000).len(),
            s.count(Tier::Resume),
            s.count(Tier::Slot),
            s.count(Tier::Mail),
            s.count(Tier::Person),
        )
    }) {
        Some((encrypted, facts, episodes, egress_7d, resume_points, slots, mail_items, people)) => Status {
            enabled: true,
            encrypted,
            facts,
            episodes,
            egress_7d,
            redact_names: redact,
            activity,
            resume_points,
            briefing,
            timetable,
            slots,
            mail,
            google_connected,
            mail_items,
            google_needs_signin,
            whatsapp,
            whatsapp_speak,
            leave_by,
            people,
            session,
            health,
        },
        None => Status {
            enabled: false,
            encrypted: false,
            facts: 0,
            episodes: 0,
            egress_7d: 0,
            redact_names: redact,
            activity,
            resume_points: 0,
            briefing,
            timetable,
            slots: 0,
            mail,
            google_connected,
            mail_items: 0,
            google_needs_signin,
            whatsapp,
            whatsapp_speak,
            leave_by,
            people: 0,
            session,
            health,
        },
    }
}

/// Ranked fact recall. `None` = memcore unavailable; `Some(vec![])` = it
/// answered and found nothing (caller may still try the legacy substring path).
pub fn recall(app_data_dir: &Path, query: &str, limit: usize) -> Option<Vec<(String, String)>> {
    with_store(app_data_dir, |s| {
        s.search(Tier::Fact, query, limit, now_ts())
            .into_iter()
            .map(|h| (h.key, h.value))
            .collect()
    })
}

fn md_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn age_label(now: i64, ts: i64) -> String {
    let days = ((now - ts).max(0)) / 86_400;
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        n => format!("{n}d ago"),
    }
}

/// Sidebar card for "what do you remember": pinned (you told me), learned,
/// recent conversations — each with where it came from and how old it is.
pub fn card_markdown(rows: &[store::Row], st: &Status, now: i64) -> String {
    let mut md = String::from("## What I remember\n\n");
    let said: Vec<_> = rows.iter().filter(|r| r.tier == "fact" && r.trust == "user_said").collect();
    let learned: Vec<_> = rows.iter().filter(|r| r.tier == "fact" && r.trust != "user_said").collect();
    let eps: Vec<_> = rows.iter().filter(|r| r.tier == "episode").collect();
    if said.is_empty() && learned.is_empty() && eps.is_empty() {
        md.push_str("Nothing yet. Tell me *remember that my dog is Bruno* and I will.\n\n");
    }
    if !said.is_empty() {
        md.push_str("### You told me\n");
        for r in &said {
            md.push_str(&format!(
                "- **{}**: {} — {}\n",
                md_escape(&r.key.replace('_', " ")),
                md_escape(&r.value),
                age_label(now, r.created)
            ));
        }
        md.push('\n');
    }
    if !learned.is_empty() {
        md.push_str("### I picked up (fades if unused)\n");
        for r in &learned {
            md.push_str(&format!(
                "- **{}**: {} — {}, last used {}\n",
                md_escape(&r.key.replace('_', " ")),
                md_escape(&r.value),
                r.source.replace(':', " "),
                age_label(now, r.last_seen)
            ));
        }
        md.push('\n');
    }
    if !eps.is_empty() {
        md.push_str(&format!("### Recent conversations ({})\n", eps.len()));
        for r in eps.iter().take(3) {
            let t: String = r.value.chars().take(60).collect();
            md.push_str(&format!("- {} — {}\n", md_escape(&t), age_label(now, r.last_seen)));
        }
        md.push('\n');
    }
    md.push_str("---\n");
    md.push_str(&format!(
        "Stored only on this PC, encrypted: **{}**. Sent to cloud models in the last 7 days: **{}** turns (Command Hub → Memory shows exactly what).\n\n",
        if st.encrypted { "yes" } else { "no" },
        st.egress_7d
    ));
    if st.activity {
        md.push_str(&format!(
            "Where-you-left-off history: **{}** points kept for 7 days, on this PC only (Command Hub → Memory can switch it off or clear it).

",
            st.resume_points
        ));
    } else {
        md.push_str("Where-you-left-off recording is **off**.

");
    }
    md.push_str("Say *forget* followed by the name to remove one thing, or *forget everything* to erase it all.\n");
    md
}

/// Truncate to `budget` chars, preferring to cut at a line boundary.
fn fit_budget(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let cut: String = text.chars().take(budget).collect();
    match cut.rfind('\n') {
        Some(i) if i > budget / 2 => cut[..i].to_string(),
        _ => cut,
    }
}

/// The only memory text allowed to leave the device. Ranked facts + the most
/// recent episodes, PII-redacted, hard-capped to `budget` chars. Used facts
/// get their frequency signal bumped. `None` = memcore unavailable or empty
/// (caller falls back to the legacy context).
pub fn context_pack(app_data_dir: &Path, query: &str, budget: usize) -> Option<String> {
    let now = now_ts();
    let (facts, episodes) = with_store(app_data_dir, |s| {
        let facts = s.search(Tier::Fact, query, MAX_PACK_FACTS, now);
        let ids: Vec<i64> = facts.iter().map(|h| h.id).collect();
        s.mark_used(&ids, now);
        // Over-fetch, then keep only real speech: junk that predates the admit gate (or slipped
        // past it) must never be sent to the cloud as "Recent:" context.
        let recent: Vec<_> = s
            .recent(Tier::Episode, MAX_PACK_EPISODES * 4)
            .into_iter()
            .filter(|h| hygiene::transcript_quality(&h.value).is_ok())
            .take(MAX_PACK_EPISODES)
            .collect();
        (facts, recent)
    })?;
    let mut parts: Vec<String> = vec![];
    if !facts.is_empty() {
        let lines: Vec<String> = facts.iter().map(|h| format!("- {}: {}", h.key, h.value)).collect();
        parts.push(format!("Known facts:\n{}", lines.join("\n")));
    }
    if !episodes.is_empty() {
        let lines: Vec<String> = episodes
            .iter()
            .map(|h| format!("- past: {}", h.value.chars().take(120).collect::<String>()))
            .collect();
        parts.push(format!("Recent:\n{}", lines.join("\n")));
    }
    if parts.is_empty() {
        return None;
    }
    let mut redacted = crate::pii_filter::sanitize(&parts.join("\n"));
    if names::enabled(app_data_dir) {
        redacted = names::redact(&redacted, &names::roster(app_data_dir));
    }
    Some(fit_budget(&redacted, budget))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("nexus_memcore_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn imports_legacy_files_once_and_leaves_them_in_place() {
        let d = tmpdir("import");
        let mem = crate::memory::resolve_memory_dir(&d);
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(mem.join("core.json"), r#"{"dog_name":"Bruno"}"#).unwrap();
        let recent = now_ts() - 86_400;
        std::fs::write(
            mem.join("facts.json"),
            format!(
                r#"[{{"key":"employer","value":"Acme","source":"episode","updated_at":{recent}}}]"#
            ),
        )
        .unwrap();
        std::fs::write(
            mem.join("episodes.jsonl"),
            "{\"ts\":4102444800,\"transcript\":\"open chrome\",\"response\":\"ok\"}\n",
        )
        .unwrap();
        let hits = recall(&d, "dog", 5).unwrap();
        assert_eq!(hits, vec![("dog_name".to_string(), "Bruno".to_string())]);
        // Second open must not re-import (count stays put).
        let (facts, eps) = with_store(&d, |s| (s.count(Tier::Fact), s.count(Tier::Episode))).unwrap();
        assert_eq!((facts, eps), (2, 1));
        assert!(mem.join("core.json").exists() && mem.join("facts.json").exists());
    }

    #[test]
    fn disabled_flag_bypasses_memcore() {
        let d = tmpdir("disabled");
        std::fs::write(d.join("settings.json"), r#"{"memcore": false}"#).unwrap();
        assert!(!enabled(&d));
        assert!(recall(&d, "anything", 5).is_none());
        assert!(context_pack(&d, "anything", 500).is_none());
        let mem = crate::memory::resolve_memory_dir(&d);
        assert!(!mem.join(store::DB_FILE).exists() && !mem.join(store::ENC_FILE).exists());
    }

    #[test]
    fn context_pack_is_ranked_redacted_and_budgeted() {
        let d = tmpdir("pack");
        mirror_core_fact(&d, "favorite_pizza_topping", "pepperoni");
        mirror_core_fact(&d, "work_email", "lakshya@example.com");
        mirror_core_fact(&d, "dog_name", "Bruno");
        mirror_episode(&d, "open chrome please");
        let pack = context_pack(&d, "what pizza do I like", DEFAULT_BUDGET).unwrap();
        assert!(pack.contains("favorite_pizza_topping: pepperoni"), "{pack}");
        assert!(pack.contains("past: open chrome please"), "{pack}");
        // Email is redacted before it can leave the device.
        let all = context_pack(&d, "email", DEFAULT_BUDGET).unwrap();
        assert!(!all.contains("lakshya@example.com"), "{all}");
        // Budget is honoured and never splits a multi-byte char.
        let tiny = context_pack(&d, "pizza", 20).unwrap();
        assert!(tiny.chars().count() <= 20);
    }

    #[test]
    fn fit_budget_cuts_on_line_boundary() {
        let text = "Known facts:\n- aaaa: 1111\n- bbbb: 2222\n- cccc: 3333";
        let cut = fit_budget(text, 40);
        assert!(cut.chars().count() <= 40);
        assert!(!cut.ends_with(' '));
        assert!(text.starts_with(&cut));
        assert_eq!(fit_budget("héllo wörld ünï", 5).chars().count(), 5);
    }

    #[test]
    fn status_reports_encryption_counts_and_egress() {
        let d = tmpdir("status");
        mirror_core_fact(&d, "dog_name", "Bruno");
        mirror_episode(&d, "open chrome");
        log_egress(&d, "cloud", "hello", "Known facts: dog_name Bruno");
        let st = status(&d);
        assert!(st.enabled && st.encrypted, "{st:?}");
        assert_eq!((st.facts, st.episodes, st.egress_7d), (1, 1, 1));
        let log = egress_log(&d, 10);
        assert_eq!(log[0].memory, "Known facts: dog_name Bruno");
        // On disk: sealed snapshot only, and no plaintext of the fact.
        let mem = crate::memory::resolve_memory_dir(&d);
        let raw = std::fs::read(mem.join(store::ENC_FILE)).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"Bruno"));
        assert!(!mem.join(store::DB_FILE).exists());
    }

    #[test]
    fn wipe_keeps_the_store_usable_after_key_rotation() {
        let d = tmpdir("wipe_rekey");
        mirror_core_fact(&d, "dog_name", "Bruno");
        assert_eq!(wipe(&d), 1);
        assert_eq!(status(&d).facts, 0);
        mirror_core_fact(&d, "city", "Pune");
        assert_eq!(recall(&d, "city", 5).unwrap()[0].1, "Pune");
    }

    #[test]
    fn name_redaction_applies_to_the_context_pack_when_enabled() {
        let d = tmpdir("redact_pack");
        std::fs::write(d.join("contacts.json"), r#"{"Asha": "+9111"}"#).unwrap();
        mirror_core_fact(&d, "sister_name", "Asha");
        let plain = context_pack(&d, "sister", DEFAULT_BUDGET).unwrap();
        assert!(plain.contains("Asha"), "{plain}");
        std::fs::write(d.join("settings.json"), r#"{"memcoreRedactNames": true}"#).unwrap();
        let masked = context_pack(&d, "sister", DEFAULT_BUDGET).unwrap();
        assert!(!masked.contains("Asha") && masked.contains("Person A"), "{masked}");
    }

    #[test]
    fn card_markdown_groups_by_provenance_escapes_and_dates() {
        let now = 100 * 86_400;
        let row = |tier: &str, key: &str, value: &str, trust: &str, created: i64| store::Row {
            id: 1,
            tier: tier.into(),
            key: key.into(),
            value: value.into(),
            source: "miner:episode".into(),
            trust: trust.into(),
            created,
            last_seen: created,
            pinned: trust == "user_said",
            uses: 0,
        };
        let rows = vec![
            row("fact", "dog_name", "Bruno [x](http://evil)", "user_said", now),
            row("fact", "employer", "Acme", "derived", now - 3 * 86_400),
            row("episode", "ep:1", "open chrome", "derived", now - 86_400),
        ];
        let st = Status { enabled: true, encrypted: true, facts: 2, episodes: 1, egress_7d: 4, redact_names: false, activity: true, resume_points: 12, briefing: true, timetable: true, slots: 0, mail: true, google_connected: false, mail_items: 0, google_needs_signin: false, whatsapp: false, whatsapp_speak: false, leave_by: true, people: 0, session: true, health: true };
        let md = card_markdown(&rows, &st, now);
        assert!(md.contains("### You told me") && md.contains("**dog name**"), "{md}");
        assert!(md.contains("\\[x\\]\\(http") || md.contains("\\[x\\]("), "markdown must be escaped: {md}");
        assert!(md.contains("### I picked up") && md.contains("last used 3d ago"), "{md}");
        assert!(md.contains("Recent conversations (1)") && md.contains("yesterday"), "{md}");
        assert!(md.contains("encrypted: **yes**") && md.contains("**4** turns"), "{md}");
        let empty = card_markdown(&[], &Status { enabled: true, encrypted: false, facts: 0, episodes: 0, egress_7d: 0, redact_names: false, activity: true, resume_points: 0, briefing: true, timetable: true, slots: 0, mail: true, google_connected: false, mail_items: 0, google_needs_signin: false, whatsapp: false, whatsapp_speak: false, leave_by: true, people: 0, session: true, health: true }, now);
        assert!(empty.contains("Nothing yet"), "{empty}");
    }

    #[test]
    fn forget_and_wipe_reach_the_store() {
        let d = tmpdir("forget");
        mirror_core_fact(&d, "dog_name", "Bruno");
        mirror_learned_fact(&d, "employer", "Acme");
        assert_eq!(forget_key(&d, "dog_name"), 1);
        assert!(recall(&d, "dog", 5).unwrap().is_empty());
        assert_eq!(wipe(&d), 1);
        assert!(recall(&d, "employer", 5).unwrap().is_empty());
    }

    /// Recall eval (plan 95 `recall_fixtures`): a fixed fact set and queries
    /// with the expected top hit. Lexical retrieval cannot bridge vocabulary
    /// ("work" vs "employer"); that gap is pinned as an explicit known miss
    /// so nobody mistakes the ranking for semantic search.
    #[test]
    fn recall_fixtures_hit_at_1() {
        let d = tmpdir("fixtures");
        let facts = [
            ("favorite_pizza_topping", "pepperoni"),
            ("dog_name", "Bruno"),
            ("employer", "Acme Robotics"),
            ("city", "Pune"),
            ("birthday", "14 March"),
            ("favorite_color", "blue"),
            ("sister_name", "Asha"),
            ("wifi_router_location", "hallway shelf"),
            ("gym_days", "Monday Wednesday Friday"),
            ("allergic_to", "peanuts"),
            ("car_model", "Honda City"),
            ("college_name", "VIT"),
            ("favorite_music", "jazz"),
            ("coffee_order", "oat latte"),
        ];
        for (k, v) in facts {
            mirror_core_fact(&d, k, v);
        }
        let cases: &[(&str, Option<&str>)] = &[
            ("what pizza do I like", Some("favorite_pizza_topping")),
            ("what is my dog's name", Some("dog_name")),
            ("which city do I live in", Some("city")),
            ("when is my birthday", Some("birthday")),
            ("what is my favorite color", Some("favorite_color")),
            ("what is my sister called", Some("sister_name")),
            ("which days do I go to the gym", Some("gym_days")),
            ("what am I allergic to", Some("allergic_to")),
            ("which car do I drive", Some("car_model")),
            ("which college am I in", Some("college_name")),
            ("what music do I like", Some("favorite_music")),
            ("what is my coffee order", Some("coffee_order")),
            ("who is Bruno", Some("dog_name")),
            ("where is the router", Some("wifi_router_location")),
            ("peanut", Some("allergic_to")),
            // Known lexical gap: no shared word with "employer"/"Acme Robotics".
            ("where do I work", None),
        ];
        let mut misses = vec![];
        for (q, want) in cases {
            let got = recall(&d, q, 3).unwrap();
            let top = got.first().map(|(k, _)| k.as_str());
            if top != *want {
                misses.push(format!("{q:?}: wanted {want:?}, got {top:?}"));
            }
        }
        assert!(misses.is_empty(), "recall misses:
{}", misses.join("
"));
    }
}
