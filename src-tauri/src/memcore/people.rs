//! People graph (plan P6, tier T2): who matters to the user, learned from how
//! they actually chat — never from what a message says.
//!
//! Priority is a deterministic, explainable score (0-100) from four signals:
//! how much you two chat (40; counts fully only for two-way chats), how reliably you reply to them (20), how
//! recently you were in touch (20), and whether you message THEM too (20).
//! Pinning someone as VIP adds 30; muting forces 0. Calls are not observable
//! through the WhatsApp bridge, so they are not part of the score (the plan's
//! "call frequency" signal is honestly absent, not guessed).
//!
//! Message TEXT never enters this module: only timestamps, direction and
//! counts do, so a chat cannot influence who counts as a priority person.

use serde::{Deserialize, Serialize};

use super::store::{Observation, Store, Tier, Trust};
use crate::google::types::AlertUrgency;

/// Window the counters describe.
pub const WINDOW_DAYS: i64 = 30;
/// A reply counts as "reliable" if it lands within this long.
pub const REPLY_WITHIN_SECS: i64 = 3600;
/// Score at/above which a message is worth interrupting for.
pub const MEDIUM_AT: u8 = 50;
pub const HIGH_AT: u8 = 70;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Person {
    /// WhatsApp JID (stable id).
    pub jid: String,
    pub name: String,
    /// Messages they sent / you sent in the last ~30 days.
    #[serde(default)]
    pub msgs_in: u32,
    #[serde(default)]
    pub msgs_out: u32,
    /// Of their sampled messages, how many you answered within an hour.
    #[serde(default)]
    pub their_sampled: u32,
    #[serde(default)]
    pub replied: u32,
    #[serde(default)]
    pub last_in_ts: i64,
    #[serde(default)]
    pub last_out_ts: i64,
    /// Pinned by the user ("make Asha a VIP").
    #[serde(default)]
    pub vip: bool,
    /// Muted by the user ("stop WhatsApp alerts from Raj").
    #[serde(default)]
    pub muted: bool,
}

/// One message as the statistics see it: no text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interaction {
    pub ts: i64,
    pub from_me: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub value: u8,
    pub why: String,
}

/// Counters from a sample of recent messages (any order).
pub fn stats_from(msgs: &[Interaction], now: i64) -> Person {
    let cutoff = now - WINDOW_DAYS * 86_400;
    let mut sorted: Vec<Interaction> = msgs.iter().copied().filter(|m| m.ts > 0).collect();
    sorted.sort_by_key(|m| m.ts);
    let mut p = Person::default();
    for (i, m) in sorted.iter().enumerate() {
        if m.ts >= cutoff {
            if m.from_me {
                p.msgs_out += 1;
            } else {
                p.msgs_in += 1;
            }
        }
        if m.from_me {
            p.last_out_ts = p.last_out_ts.max(m.ts);
        } else {
            p.last_in_ts = p.last_in_ts.max(m.ts);
            if m.ts >= cutoff {
                p.their_sampled += 1;
                let answered = sorted[i + 1..]
                    .iter()
                    .find(|n| n.from_me)
                    .map(|n| n.ts - m.ts <= REPLY_WITHIN_SECS)
                    .unwrap_or(false);
                if answered {
                    p.replied += 1;
                }
            }
        }
    }
    p
}

pub fn score(p: &Person, now: i64) -> Score {
    if p.muted {
        return Score { value: 0, why: "muted by you".into() };
    }
    let total = (p.msgs_in + p.msgs_out) as f64;
    // Volume only counts when it is a conversation: a sender you never answer
    // (a shop, a group-style broadcaster) gets a quarter of the credit.
    let freq = (total / 60.0).min(1.0) * if p.msgs_out == 0 { 0.25 } else { 1.0 };
    let reply = if p.their_sampled >= 3 { p.replied as f64 / p.their_sampled as f64 } else { 0.0 };
    let last = p.last_in_ts.max(p.last_out_ts);
    let days = if last > 0 { ((now - last).max(0) as f64) / 86_400.0 } else { 999.0 };
    let recency = (-days / 7.0).exp();
    let two_way = if p.msgs_out > 0 { 1.0 } else { 0.0 };

    let mut v = 40.0 * freq + 20.0 * reply + 20.0 * recency + 20.0 * two_way;
    let mut why: Vec<String> = vec![];
    if freq >= 0.5 {
        why.push(format!("you chat a lot ({} messages in 30 days)", p.msgs_in + p.msgs_out));
    } else if total > 0.0 {
        why.push(format!("{} messages in 30 days", p.msgs_in + p.msgs_out));
    }
    if reply >= 0.6 {
        why.push("you usually reply quickly".into());
    }
    if recency >= 0.6 {
        why.push("recently in touch".into());
    }
    if two_way > 0.0 {
        why.push("you message them too".into());
    }
    if p.vip {
        v += 30.0;
        why.insert(0, "you pinned them as a VIP".into());
    }
    let value = v.round().clamp(0.0, 100.0) as u8;
    Score { value, why: if why.is_empty() { "little contact so far".into() } else { why.join(", ") } }
}

/// How urgently a new message from this person deserves a spoken alert.
pub fn urgency_for(score: u8) -> Option<AlertUrgency> {
    if score >= HIGH_AT {
        Some(AlertUrgency::High)
    } else if score >= MEDIUM_AT {
        Some(AlertUrgency::Medium)
    } else {
        None
    }
}

// ─── Name lookup ("make Asha a VIP") ────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Lookup<'a> {
    Found(&'a Person),
    Ambiguous(Vec<&'a Person>),
    NotFound,
}

fn norm(s: &str) -> String {
    s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Exact name wins; otherwise a whole-word / prefix match. Several matches
/// are reported, never guessed.
pub fn find<'a>(people: &'a [Person], query: &str) -> Lookup<'a> {
    let q = norm(query);
    if q.is_empty() {
        return Lookup::NotFound;
    }
    let exact: Vec<&Person> = people.iter().filter(|p| norm(&p.name) == q).collect();
    if exact.len() == 1 {
        return Lookup::Found(exact[0]);
    }
    if exact.len() > 1 {
        return Lookup::Ambiguous(exact);
    }
    let partial: Vec<&Person> = people
        .iter()
        .filter(|p| {
            let n = norm(&p.name);
            n.split(' ').any(|w| w == q || w.starts_with(&q)) || n.starts_with(&q)
        })
        .collect();
    match partial.len() {
        0 => Lookup::NotFound,
        1 => Lookup::Found(partial[0]),
        _ => Lookup::Ambiguous(partial),
    }
}

// ─── Storage ────────────────────────────────────────────────────────

fn key(jid: &str) -> String {
    format!("person:{jid}")
}

pub fn get(store: &Store, jid: &str) -> Option<Person> {
    store.get_value(Tier::Person, &key(jid)).and_then(|v| serde_json::from_str(&v).ok())
}

/// Statistics update (mined). The store will not let a mined write unpin a
/// record, so this can never undo the user's VIP / mute choice.
pub fn save(store: &Store, p: &Person, now: i64) {
    save_as(store, p, now, Trust::Derived, "whatsapp:stats");
}

/// The user changed VIP / mute: written as a user statement, so it is allowed
/// to clear a flag as well as set one.
pub fn save_user_choice(store: &Store, p: &Person, now: i64) {
    save_as(store, p, now, Trust::UserSaid, "user");
}

fn save_as(store: &Store, p: &Person, now: i64, trust: Trust, source: &str) {
    let value = serde_json::to_string(p).unwrap_or_default();
    let _ = store.observe(
        &Observation {
            tier: Tier::Person,
            key: key(&p.jid),
            value,
            source: source.into(),
            trust,
            pinned: p.vip || p.muted,
        },
        now,
    );
}

pub fn load_all(store: &Store) -> Vec<Person> {
    store
        .recent(Tier::Person, 300)
        .into_iter()
        .filter_map(|h| serde_json::from_str::<Person>(&h.value).ok())
        .collect()
}

/// People ranked by current score, best first.
pub fn ranked(people: &[Person], now: i64) -> Vec<(Person, Score)> {
    let mut v: Vec<(Person, Score)> = people.iter().map(|p| (p.clone(), score(p, now))).collect();
    v.sort_by(|a, b| b.1.value.cmp(&a.1.value).then(a.0.name.cmp(&b.0.name)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const H: i64 = 3600;
    const D: i64 = 86_400;

    fn conv(pairs: &[(i64, bool)]) -> Vec<Interaction> {
        pairs.iter().map(|(ts, me)| Interaction { ts: NOW - ts, from_me: *me }).collect()
    }

    #[test]
    fn stats_count_directions_replies_and_ignore_old_messages() {
        // They write 3 times (2 answered within the hour, 1 answered next day), plus one 40-day-old message.
        let msgs = conv(&[
            (40 * D, false),
            (3 * D, false),
            (3 * D - 600, true),
            (2 * D, false),
            (2 * D - 1800, true),
            (D, false),
            (D - 5 * H, true),
        ]);
        let p = stats_from(&msgs, NOW);
        assert_eq!((p.msgs_in, p.msgs_out), (3, 3));
        assert_eq!((p.their_sampled, p.replied), (3, 2));
        assert_eq!(p.last_in_ts, NOW - D);
        assert_eq!(p.last_out_ts, NOW - (D - 5 * H));
    }

    #[test]
    fn a_close_contact_outranks_a_stranger_and_explains_why() {
        let close = Person {
            jid: "a".into(),
            name: "Asha".into(),
            msgs_in: 40,
            msgs_out: 35,
            their_sampled: 20,
            replied: 18,
            last_in_ts: NOW - 2 * H,
            last_out_ts: NOW - H,
            ..Default::default()
        };
        let stranger = Person { jid: "b".into(), name: "Delivery".into(), msgs_in: 2, last_in_ts: NOW - 20 * D, ..Default::default() };
        let (c, s) = (score(&close, NOW), score(&stranger, NOW));
        assert!(c.value >= HIGH_AT, "{c:?}");
        assert!(s.value < MEDIUM_AT, "{s:?}");
        assert!(c.why.contains("you chat a lot") && c.why.contains("you message them too"), "{}", c.why);
        assert_eq!(urgency_for(c.value), Some(AlertUrgency::High));
        assert_eq!(urgency_for(s.value), None);
    }

    #[test]
    fn one_sided_senders_never_reach_the_alert_threshold() {
        // A shop that messages you 50 times a month and you never answer.
        let spam = Person { jid: "s".into(), name: "Shop".into(), msgs_in: 50, their_sampled: 50, last_in_ts: NOW - H, ..Default::default() };
        assert!(score(&spam, NOW).value < MEDIUM_AT, "{:?}", score(&spam, NOW));
    }

    #[test]
    fn vip_boosts_and_mute_silences() {
        let mut p = Person { jid: "x".into(), name: "Raj".into(), msgs_in: 6, msgs_out: 4, last_in_ts: NOW - D, ..Default::default() };
        let base = score(&p, NOW).value;
        p.vip = true;
        let vip = score(&p, NOW);
        assert!(vip.value >= base + 25 && vip.why.starts_with("you pinned them"), "{base} -> {vip:?}");
        p.muted = true;
        assert_eq!(score(&p, NOW), Score { value: 0, why: "muted by you".into() });
    }

    #[test]
    fn recency_decays() {
        let mk = |days: i64| Person { jid: "r".into(), name: "R".into(), msgs_in: 20, msgs_out: 20, last_in_ts: NOW - days * D, ..Default::default() };
        assert!(score(&mk(0), NOW).value > score(&mk(14), NOW).value);
    }

    #[test]
    fn name_lookup_prefers_exact_and_reports_ambiguity() {
        let people = vec![
            Person { jid: "1".into(), name: "Asha Verma".into(), ..Default::default() },
            Person { jid: "2".into(), name: "Asha Rao".into(), ..Default::default() },
            Person { jid: "3".into(), name: "Mom".into(), ..Default::default() },
        ];
        assert!(matches!(find(&people, "mom"), Lookup::Found(p) if p.jid == "3"));
        assert!(matches!(find(&people, "Asha Rao"), Lookup::Found(p) if p.jid == "2"));
        assert!(matches!(find(&people, "asha"), Lookup::Ambiguous(v) if v.len() == 2));
        assert!(matches!(find(&people, "verma"), Lookup::Found(p) if p.jid == "1"));
        assert_eq!(find(&people, "nobody"), Lookup::NotFound);
        assert_eq!(find(&people, "  "), Lookup::NotFound);
    }

    #[test]
    fn storage_round_trip_keeps_flags() {
        let s = Store::open_in_memory().unwrap();
        let mut p = Person { jid: "9199@s.whatsapp.net".into(), name: "Asha".into(), msgs_in: 3, ..Default::default() };
        save(&s, &p, 10);
        p.vip = true;
        save(&s, &p, 20);
        let back = get(&s, "9199@s.whatsapp.net").unwrap();
        assert!(back.vip && back.msgs_in == 3);
        assert_eq!(load_all(&s).len(), 1);
        assert!(get(&s, "nobody").is_none());
        assert!(s.list_rows(10).is_empty(), "people stay out of the memory list");
    }

    #[test]
    fn ranking_is_best_first() {
        let a = Person { jid: "a".into(), name: "A".into(), msgs_in: 30, msgs_out: 30, last_in_ts: NOW - H, their_sampled: 10, replied: 9, ..Default::default() };
        let b = Person { jid: "b".into(), name: "B".into(), msgs_in: 1, last_in_ts: NOW - 30 * D, ..Default::default() };
        let r = ranked(&[b.clone(), a.clone()], NOW);
        assert_eq!(r[0].0.jid, "a");
    }
}
