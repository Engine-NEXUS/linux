//! WhatsApp watcher (plan P6): tells the user when a PRIORITY person messages,
//! reads a chat aloud on request — and leaves no trace on the other side.
//!
//! Three guarantees, each enforced in code and tested:
//! 1. **Read-only.** Everything here goes through `read_tool`, which only
//!    accepts the allowlist in `READ_TOOLS`. `mark_read`, `mark_chat_read`,
//!    `send_presence` and `send_typing` are additionally refused for every
//!    caller in `mcp_client::call_tool`.
//! 2. **Message text is data.** It is clipped, PII-redacted and stored as an
//!    untrusted preview; it is never used to decide priority (only counts and
//!    timestamps are) and never reaches a cloud model.
//! 3. **Quiet start.** First sight of a chat records its history statistics
//!    and speaks nothing; only messages that arrive afterwards can interrupt.
//!
//! What is NOT verified: whether the bridge's *read* tools send receipts or
//! presence. The bridge's README is silent on it. The feature therefore ships
//! OFF (`memcoreWhatsapp`) until the two-phone check in
//! `docs/testing/whatsapp-read-receipt-test.md` has been done.
//!
//! The response format of the bridge's list tools is not documented either,
//! so the parsers are deliberately tolerant and `selftest` reports exactly
//! what could and could not be understood.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::google::types::AlertUrgency;
use crate::mcp_client::{self, McpServer, PairingState};

use super::people::{self, Interaction};
use super::store::{Observation, Store, Tier, Trust};

/// The ONLY bridge tools the watcher may call.
pub const READ_TOOLS: &[&str] = &["list_chats", "list_messages", "get_chat", "search_contacts", "pairing_status", "get_status"];
const CHATS_PER_POLL: usize = 20;
/// New contacts examined per poll (each costs one `list_messages` call).
const MAX_BOOTSTRAP_PER_POLL: usize = 6;
const SAMPLE_MESSAGES: usize = 50;
pub const ACTIVE_POLL_SECS: u64 = 45;
pub const IDLE_POLL_SECS: u64 = 5 * 60;
pub const DOWN_POLL_SECS: u64 = 5 * 60;
/// "Read that message" works for an alert this recent.
const LAST_ALERT_WINDOW: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, PartialEq)]
pub struct ChatInfo {
    pub jid: String,
    pub name: String,
    pub last_ts: i64,
    pub last_text: String,
    pub last_from_me: bool,
    pub is_group: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMsg {
    pub id: String,
    pub ts: i64,
    pub text: String,
    pub from_me: bool,
}

// ─── Tolerant parsing ───────────────────────────────────────────────

fn parse_ts(v: &Value) -> i64 {
    match v {
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(0.0);
            (if x > 1.0e11 { x / 1000.0 } else { x }) as i64
        }
        Value::String(s) => {
            let t = s.trim();
            if let Ok(n) = t.parse::<f64>() {
                return parse_ts(&json!(n));
            }
            if let Ok(d) = chrono::DateTime::parse_from_rfc3339(t) {
                return d.timestamp();
            }
            for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"] {
                if let Ok(n) = chrono::NaiveDateTime::parse_from_str(t.split('.').next().unwrap_or(t), fmt) {
                    use chrono::TimeZone;
                    if let Some(l) = chrono::Local.from_local_datetime(&n).earliest() {
                        return l.timestamp();
                    }
                }
            }
            0
        }
        _ => 0,
    }
}

fn first<'a>(o: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| o.get(*k)).filter(|v| !v.is_null())
}

fn first_str(o: &Value, keys: &[&str]) -> String {
    first(o, keys).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn first_bool(o: &Value, keys: &[&str]) -> bool {
    first(o, keys).map(|v| v.as_bool().unwrap_or_else(|| v.as_str().map(|s| s == "true").unwrap_or(false))).unwrap_or(false)
}

/// Pull the list of items out of whatever envelope the bridge used.
fn items(text: &str) -> Option<Vec<Value>> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    match v {
        Value::Array(a) => Some(a),
        Value::Object(ref o) => {
            for k in ["chats", "messages", "result", "data", "items", "results"] {
                if let Some(Value::Array(a)) = o.get(k) {
                    return Some(a.clone());
                }
            }
            Some(vec![v])
        }
        _ => None,
    }
}

/// `list_chats` text → chats. `None` = the text was not JSON we understand.
pub fn parse_chats(text: &str) -> Option<Vec<ChatInfo>> {
    let list = items(text)?;
    Some(
        list.iter()
            .filter_map(|c| {
                let jid = first_str(c, &["jid", "chat_jid", "id", "chatJID"]);
                if jid.is_empty() {
                    return None;
                }
                let is_group = jid.ends_with("@g.us") || first_bool(c, &["is_group", "isGroup", "group"]);
                let last_ts = first(c, &["last_message_time", "last_message_timestamp", "last_time", "last_active", "timestamp", "lastMessageTime"])
                    .map(parse_ts)
                    .unwrap_or(0);
                Some(ChatInfo {
                    name: first_str(c, &["name", "chat_name", "display_name", "title", "pushName"]),
                    last_text: first_str(c, &["last_message", "last_text", "text", "content", "lastMessage"]),
                    last_from_me: first_bool(c, &["last_is_from_me", "is_from_me", "from_me", "last_from_me", "isFromMe"]),
                    jid,
                    last_ts,
                    is_group,
                })
            })
            .collect(),
    )
}

/// `list_messages` text → messages, oldest first. `None` = not parseable.
pub fn parse_messages(text: &str) -> Option<Vec<ChatMsg>> {
    let list = items(text)?;
    let mut out: Vec<ChatMsg> = list
        .iter()
        .map(|m| ChatMsg {
            id: first_str(m, &["id", "message_id", "messageID", "ID"]),
            ts: first(m, &["timestamp", "time", "ts", "date", "Timestamp"]).map(parse_ts).unwrap_or(0),
            text: first_str(m, &["content", "text", "message", "body", "Content"]),
            from_me: first_bool(m, &["is_from_me", "from_me", "fromMe", "isFromMe"]),
        })
        .collect();
    out.sort_by_key(|m| m.ts);
    Some(out)
}

/// A short, redacted, single-line preview of message text.
pub fn gist(text: &str, max: usize) -> String {
    let clean = crate::pii_filter::sanitize(text);
    let one: String = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max {
        return one;
    }
    let cut: String = one.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

fn display_name(c: &ChatInfo) -> String {
    if !c.name.trim().is_empty() {
        return c.name.trim().to_string();
    }
    let num: String = c.jid.split('@').next().unwrap_or("").chars().filter(|x| x.is_ascii_digit()).collect();
    if num.len() >= 4 { format!("number ending {}", &num[num.len() - 4..]) } else { "someone".to_string() }
}

// ─── Schema-adaptive parameters ─────────────────────────────────────

/// Build tool arguments from desired (candidate names, value) pairs, using
/// only parameter names the tool declares. With no schema known, the first
/// candidate is used (the common convention).
pub fn adapt(desired: &[(&[&str], Value)], declared: &HashSet<String>) -> Value {
    let mut o = serde_json::Map::new();
    for (cands, val) in desired {
        let chosen = cands
            .iter()
            .find(|c| declared.contains(**c))
            .or_else(|| if declared.is_empty() { cands.first() } else { None });
        if let Some(name) = chosen {
            o.insert((*name).to_string(), val.clone());
        }
    }
    Value::Object(o)
}

// ─── Read-only bridge access ────────────────────────────────────────

/// Call a read-only bridge tool. Anything outside `READ_TOOLS` is refused
/// here, before `mcp_client` (which blocks the receipt/presence tools again).
pub async fn read_tool(client: &reqwest::Client, tool: &str, params: Value) -> Result<String, String> {
    if !READ_TOOLS.contains(&tool) {
        return Err(format!("'{tool}' is not an allowed read-only WhatsApp tool"));
    }
    let r = mcp_client::call_tool(McpServer::WhatsApp, tool, params, client, None).await;
    if !r.ok {
        return Err(r.error.unwrap_or_else(|| "bridge call failed".to_string()));
    }
    Ok(mcp_client::extract_text_full(&r))
}

#[allow(async_fn_in_trait)]
pub trait Bridge {
    async fn chats(&self) -> Result<Vec<ChatInfo>, String>;
    async fn messages(&self, jid: &str, limit: usize) -> Result<Vec<ChatMsg>, String>;
}

pub struct McpBridge<'a> {
    client: &'a reqwest::Client,
    props: HashMap<String, HashSet<String>>,
    pub tool_names: Vec<String>,
}

impl<'a> McpBridge<'a> {
    /// Learn the bridge's tool names and parameter names (`tools/list`).
    pub async fn connect(client: &'a reqwest::Client) -> Self {
        let mut props = HashMap::new();
        let mut tool_names = vec![];
        if let Ok(tools) = mcp_client::list_tools(McpServer::WhatsApp, client, None).await {
            for t in tools {
                let Some(name) = t.get("name").and_then(|n| n.as_str()) else { continue };
                tool_names.push(name.to_string());
                let keys: HashSet<String> = t
                    .pointer("/inputSchema/properties")
                    .and_then(|p| p.as_object())
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                props.insert(name.to_string(), keys);
            }
        }
        McpBridge { client, props, tool_names }
    }

    fn declared(&self, tool: &str) -> HashSet<String> {
        self.props.get(tool).cloned().unwrap_or_default()
    }
}

impl Bridge for McpBridge<'_> {
    async fn chats(&self) -> Result<Vec<ChatInfo>, String> {
        let params = adapt(
            &[
                (&["limit"], json!(CHATS_PER_POLL)),
                (&["sort_by", "sortBy"], json!("last_active")),
                (&["include_last_message", "includeLastMessage"], json!(true)),
            ],
            &self.declared("list_chats"),
        );
        let text = read_tool(self.client, "list_chats", params).await?;
        parse_chats(&text).ok_or_else(|| "list_chats answered in a format NEXUS does not understand".to_string())
    }

    async fn messages(&self, jid: &str, limit: usize) -> Result<Vec<ChatMsg>, String> {
        let params = adapt(
            &[(&["chat_jid", "jid", "chatJID"], json!(jid)), (&["limit"], json!(limit))],
            &self.declared("list_messages"),
        );
        let text = read_tool(self.client, "list_messages", params).await?;
        parse_messages(&text).ok_or_else(|| "list_messages answered in a format NEXUS does not understand".to_string())
    }
}

// ─── The poll ───────────────────────────────────────────────────────
//
// Three phases so the store lock is never held across a network call:
//   plan  (short lock)  decide which chats need their messages fetched
//   fetch (no lock)     read-only bridge calls
//   apply (short lock)  update statistics, store previews, decide alerts

#[derive(Debug, Clone, PartialEq)]
pub struct WaAlert {
    pub jid: String,
    pub name: String,
    pub score: u8,
    pub why: String,
    pub urgency: AlertUrgency,
    pub gist: String,
}

fn cursor_key(jid: &str) -> String {
    format!("wa_cursor:{jid}")
}

fn interactions(msgs: &[ChatMsg]) -> Vec<Interaction> {
    msgs.iter().map(|m| Interaction { ts: m.ts, from_me: m.from_me }).collect()
}

fn store_preview(store: &Store, jid: &str, name: &str, text: &str, ts: i64, score: u8, now: i64) {
    let value = json!({"name": name, "jid": jid, "text": gist(text, 140), "ts": ts, "score": score}).to_string();
    let _ = store.observe(
        &Observation {
            tier: Tier::Chat,
            key: format!("chat:{jid}"),
            value,
            source: "whatsapp:preview".into(),
            trust: Trust::Untrusted,
            pinned: false,
        },
        now,
    );
}

fn chat_cursor(store: &Store, jid: &str) -> i64 {
    store.meta_get(&cursor_key(jid)).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// Which chats need their messages looked at: unknown contacts (to learn
/// their history — a few per poll) and known ones with a new INCOMING message.
pub fn plan(store: &Store, chats: &[ChatInfo]) -> Vec<(String, usize)> {
    let mut want = vec![];
    let mut unknown = 0usize;
    for c in chats.iter().filter(|c| !c.is_group && !c.jid.is_empty()) {
        if people::get(store, &c.jid).is_none() {
            if unknown < MAX_BOOTSTRAP_PER_POLL {
                unknown += 1;
                want.push((c.jid.clone(), SAMPLE_MESSAGES));
            }
        } else if c.last_ts > chat_cursor(store, &c.jid) && !c.last_from_me {
            want.push((c.jid.clone(), 5));
        }
    }
    want
}

/// Apply fetched data. Never speaks about first-seen chats.
pub fn apply(store: &Store, chats: &[ChatInfo], messages: &HashMap<String, Vec<ChatMsg>>, now: i64) -> Vec<WaAlert> {
    let mut alerts: Vec<WaAlert> = vec![];
    for c in chats.iter().filter(|c| !c.is_group && !c.jid.is_empty()) {
        let name = display_name(c);
        let ckey = cursor_key(&c.jid);

        let Some(mut person) = people::get(store, &c.jid) else {
            // First sight: learn from history, say nothing. (Chats beyond the
            // per-poll cap were not fetched and are picked up next poll.)
            let Some(msgs) = messages.get(&c.jid) else { continue };
            let mut p = people::stats_from(&interactions(msgs), now);
            p.jid = c.jid.clone();
            p.name = name;
            people::save(store, &p, now);
            let newest = msgs.iter().map(|m| m.ts).max().unwrap_or(0).max(c.last_ts);
            store.meta_set(&ckey, &newest.to_string());
            continue;
        };

        let cursor = chat_cursor(store, &c.jid);
        if c.last_ts <= cursor {
            continue;
        }
        if name != "someone" && !name.starts_with("number ending") && person.name != name {
            person.name = name.clone();
        }

        if c.last_from_me {
            // You wrote last: keep the two-way and reply statistics fresh, no alert.
            person.msgs_out += 1;
            person.last_out_ts = c.last_ts;
            if person.last_in_ts > 0 && c.last_ts - person.last_in_ts <= people::REPLY_WITHIN_SECS {
                person.replied += 1;
            }
            people::save(store, &person, now);
            store.meta_set(&ckey, &c.last_ts.to_string());
            continue;
        }

        // A new incoming message: count it, look at the freshest.
        let recent = messages.get(&c.jid).cloned().unwrap_or_default();
        let newest_in: Vec<&ChatMsg> = recent.iter().filter(|m| !m.from_me && m.ts > cursor).collect();
        let fresh_count = newest_in.len().max(1) as u32;
        let text = newest_in.last().map(|m| m.text.clone()).unwrap_or_else(|| c.last_text.clone());
        person.msgs_in += fresh_count;
        person.their_sampled += fresh_count;
        person.last_in_ts = c.last_ts;
        let sc = people::score(&person, now);
        people::save(store, &person, now);
        store_preview(store, &c.jid, &person.name, &text, c.last_ts, sc.value, now);
        store.meta_set(&ckey, &c.last_ts.to_string());

        if person.muted {
            continue;
        }
        if let Some(urgency) = people::urgency_for(sc.value) {
            alerts.push(WaAlert {
                jid: c.jid.clone(),
                name: person.name.clone(),
                score: sc.value,
                why: sc.why,
                urgency,
                gist: gist(&text, 120),
            });
        }
    }
    alerts.sort_by(|a, b| b.score.cmp(&a.score));
    alerts
}

/// One full poll against any bridge. The store lock is taken twice, briefly,
/// and never across an `.await`.
pub async fn poll_once<B: Bridge>(
    store: &parking_lot::Mutex<Store>,
    bridge: &B,
    now: i64,
) -> Result<Vec<WaAlert>, String> {
    let chats = bridge.chats().await?;
    let wanted = plan(&store.lock(), &chats);
    let mut messages: HashMap<String, Vec<ChatMsg>> = HashMap::new();
    for (jid, limit) in wanted {
        if let Ok(m) = bridge.messages(&jid, limit).await {
            messages.insert(jid, m);
        }
    }
    let guard = store.lock();
    Ok(guard.batch(|s| apply(s, &chats, &messages, now)))
}

// ─── Wording ────────────────────────────────────────────────────────

fn capitalize(s: String) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

/// Spoken alert. The message text is NOT spoken (a private message read out
/// in a room is a privacy leak); "read that message" speaks it on request.
pub fn alert_text(alerts: &[WaAlert], address: Option<&str>) -> String {
    let tail = address.map(|a| format!(", {a}")).unwrap_or_default();
    match alerts {
        [] => String::new(),
        [one] => capitalize(format!("{} sent you a message{tail}. Say read that message to hear it.", one.name)),
        many => {
            let names: Vec<&str> = many.iter().take(3).map(|a| a.name.as_str()).collect();
            let list = match names.len() {
                1 => names[0].to_string(),
                2 => format!("{} and {}", names[0], names[1]),
                _ => format!("{}, {} and {}", names[0], names[1], names[2]),
            };
            capitalize(format!(
                "{} of your priority people messaged you{tail}: {list}. Say read that message to hear the latest.",
                many.len()
            ))
        }
    }
}

// ─── "Read that message" ────────────────────────────────────────────

static LAST_ALERT: Mutex<Option<(String, String, Instant)>> = Mutex::new(None);

fn note_alert(a: &WaAlert) {
    *LAST_ALERT.lock().unwrap_or_else(|e| e.into_inner()) = Some((a.jid.clone(), a.name.clone(), Instant::now()));
}

/// How long ago the last alert was spoken (within the 30-minute window).
pub fn last_alert_age() -> Option<Duration> {
    LAST_ALERT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|(_, _, at)| at.elapsed())
        .filter(|age| *age < LAST_ALERT_WINDOW)
}

/// The chat of the most recent alert (within 30 minutes).
pub fn last_alert_chat() -> Option<(String, String)> {
    LAST_ALERT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .filter(|(_, _, at)| at.elapsed() < LAST_ALERT_WINDOW)
        .map(|(j, n, _)| (j.clone(), n.clone()))
}

/// Read-aloud text for the incoming messages in `msgs` (latest 3), never
/// marking anything read. Pure.
pub fn read_aloud(name: &str, msgs: &[ChatMsg]) -> String {
    let incoming: Vec<&ChatMsg> = msgs.iter().filter(|m| !m.from_me && !m.text.trim().is_empty()).collect();
    if incoming.is_empty() {
        return format!("I don't see any recent messages from {name}.");
    }
    let last3: Vec<String> = incoming.iter().rev().take(3).rev().map(|m| gist(&m.text, 220)).collect();
    let body = if last3.len() == 1 { last3[0].clone() } else { last3.join(". Then: ") };
    format!("{name} says: {body}. I haven't marked it as read.")
}

/// Sidebar card with the recent conversation (both directions).
pub fn card_markdown(name: &str, msgs: &[ChatMsg]) -> String {
    let esc = |s: &str| {
        s.chars()
            .flat_map(|c| if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') { vec!['\\', c] } else { vec![c] })
            .collect::<String>()
    };
    let mut md = format!("## {}\n\n", esc(name));
    for m in msgs.iter().rev().take(10).rev() {
        let who = if m.from_me { "You" } else { name };
        md.push_str(&format!("- **{}**: {}\n", esc(who), esc(&gist(&m.text, 300))));
    }
    md.push_str("\n---\nNEXUS did not mark this chat as read — the sender will not see blue ticks from this.\n");
    md
}

// ─── People commands ("make Asha a VIP", "who are my priority people") ─

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonFlag {
    Vip,
    Mute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlagOutcome {
    Done(String),
    Ambiguous(Vec<String>),
    NotFound,
}

/// Set or clear VIP / mute for the person named `query`. VIP and mute are
/// mutually exclusive (turning one on turns the other off). Never guesses
/// between several matches.
pub fn set_flag(store: &Store, query: &str, flag: PersonFlag, on: bool, now: i64) -> FlagOutcome {
    let all = people::load_all(store);
    match people::find(&all, query) {
        people::Lookup::Found(p) => {
            let mut p = p.clone();
            match flag {
                PersonFlag::Vip => {
                    p.vip = on;
                    if on {
                        p.muted = false;
                    }
                }
                PersonFlag::Mute => {
                    p.muted = on;
                    if on {
                        p.vip = false;
                    }
                }
            }
            people::save_user_choice(store, &p, now);
            FlagOutcome::Done(p.name)
        }
        people::Lookup::Ambiguous(v) => FlagOutcome::Ambiguous(v.iter().map(|p| p.name.clone()).collect()),
        people::Lookup::NotFound => FlagOutcome::NotFound,
    }
}

/// The people NEXUS would interrupt you for, as one spoken sentence.
pub fn priority_line(store: &Store, now: i64) -> String {
    let ranked = people::ranked(&people::load_all(store), now);
    let top: Vec<String> = ranked
        .iter()
        .filter(|(p, s)| !p.muted && s.value >= people::MEDIUM_AT)
        .take(5)
        .map(|(p, _)| p.name.clone())
        .collect();
    match top.as_slice() {
        [] => "I haven't learned any priority people on WhatsApp yet.".into(),
        [one] => format!("Your priority person on WhatsApp is {one}."),
        _ => format!("Your priority people on WhatsApp are {}.", join_names(&top)),
    }
}

fn join_names(v: &[String]) -> String {
    match v.len() {
        0 => String::new(),
        1 => v[0].clone(),
        _ => format!("{} and {}", v[..v.len() - 1].join(", "), v[v.len() - 1]),
    }
}

/// Resolve who "read that message" / "read Asha's message" refers to.
pub fn resolve_chat(store: &Store, name: Option<&str>) -> Result<(String, String), String> {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        None => last_alert_chat().ok_or_else(|| "There's no recent WhatsApp alert to read.".to_string()),
        Some(n) => {
            let all = people::load_all(store);
            match people::find(&all, n) {
                people::Lookup::Found(p) => Ok((p.jid.clone(), p.name.clone())),
                people::Lookup::Ambiguous(v) => {
                    Err(format!("I know more than one {n}: {}. Which one?", join_names(&v.iter().map(|p| p.name.clone()).collect::<Vec<_>>())))
                }
                people::Lookup::NotFound => Err(format!("I don't know a WhatsApp contact called {n}.")),
            }
        }
    }
}

/// Fetch a chat's recent messages. Read-only; marks nothing as read.
pub async fn fetch_chat(client: &reqwest::Client, jid: &str) -> Result<Vec<ChatMsg>, String> {
    if !matches!(mcp_client::query_pairing_status(client).await, PairingState::Ready) {
        return Err("WhatsApp isn't connected right now.".into());
    }
    McpBridge::connect(client).await.messages(jid, 10).await
}

// ─── Self-test (run once the bridge is up) ──────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SelfTest {
    pub reachable: bool,
    pub paired: bool,
    pub tools: usize,
    pub has_read_tools: bool,
    /// Receipt/presence tools the bridge offers — present, but NEXUS refuses to call them.
    pub guarded_tools_present: Vec<String>,
    pub chats_parsed: usize,
    pub format_ok: bool,
    pub note: String,
}

pub async fn selftest(client: &reqwest::Client) -> SelfTest {
    let pairing = mcp_client::query_pairing_status(client).await;
    let reachable = !matches!(pairing, PairingState::Unavailable);
    let paired = matches!(pairing, PairingState::Ready);
    if !reachable {
        return SelfTest {
            reachable,
            paired,
            tools: 0,
            has_read_tools: false,
            guarded_tools_present: vec![],
            chats_parsed: 0,
            format_ok: false,
            note: "The WhatsApp bridge is not running on 127.0.0.1:8765.".into(),
        };
    }
    let bridge = McpBridge::connect(client).await;
    let has_read_tools = ["list_chats", "list_messages"].iter().all(|t| bridge.tool_names.iter().any(|n| n == t));
    let guarded: Vec<String> = bridge
        .tool_names
        .iter()
        .filter(|n| McpServer::WhatsApp.blocked_reason(n).is_some())
        .cloned()
        .collect();
    let (chats_parsed, format_ok, note) = if !paired {
        (0, false, "The bridge is running but not paired with your phone yet.".to_string())
    } else {
        match bridge.chats().await {
            Ok(c) => (c.len(), true, "Chat list understood.".to_string()),
            Err(e) => (0, false, e),
        }
    };
    SelfTest {
        reachable,
        paired,
        tools: bridge.tool_names.len(),
        has_read_tools,
        guarded_tools_present: guarded,
        chats_parsed,
        format_ok,
        note,
    }
}

// ─── Glue: the watcher loop ─────────────────────────────────────────

pub(crate) fn speak_address<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(app));
    let name = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|d| crate::memory::read_user_profile(&d))
        .and_then(|p| p.name);
    crate::persona::address(name.as_deref(), friend)
}

fn fire<R: Runtime>(app: &AppHandle<R>, alerts: &[WaAlert]) {
    let Some(top) = alerts.first() else { return };
    note_alert(top);
    let urgency = alerts.iter().map(|a| a.urgency).max_by_key(|u| super::mailtriage::urgency_rank(*u)).unwrap_or(AlertUrgency::Medium);
    let text = alert_text(alerts, speak_address(app).as_deref());
    crate::proactive_policy::submit(app, format!("wa_{}", top.jid), text, urgency);
}

pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        let mut next_due = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let Ok(dir) = app.path().app_data_dir() else { continue };
            // Off by default until the read-receipt check has been done.
            if !super::enabled(&dir) || !super::flag(&dir, "memcoreWhatsapp", false) || Instant::now() < next_due {
                continue;
            }
            if !matches!(mcp_client::query_pairing_status(&client).await, PairingState::Ready) {
                next_due = Instant::now() + Duration::from_secs(DOWN_POLL_SECS);
                continue;
            }
            let Some(store) = super::store_handle(&dir) else {
                next_due = Instant::now() + Duration::from_secs(DOWN_POLL_SECS);
                continue;
            };
            let bridge = McpBridge::connect(&client).await;
            let now = chrono::Utc::now().timestamp();
            let active = super::resume::user_idle_secs().map(|s| s < 300).unwrap_or(true);
            match poll_once(&store, &bridge, now).await {
                Ok(alerts) => {
                    if !alerts.is_empty() {
                        tracing::info!("whatsapp: {} priority message(s)", alerts.len());
                        fire(&app, &alerts);
                    }
                    next_due = Instant::now() + Duration::from_secs(if active { ACTIVE_POLL_SECS } else { IDLE_POLL_SECS });
                }
                Err(e) => {
                    tracing::debug!("whatsapp: poll failed: {e}");
                    next_due = Instant::now() + Duration::from_secs(DOWN_POLL_SECS);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const NOW: i64 = 1_800_000_000;

    fn chat(jid: &str, name: &str, last_ts: i64, text: &str, from_me: bool) -> ChatInfo {
        ChatInfo { jid: jid.into(), name: name.into(), last_ts, last_text: text.into(), last_from_me: from_me, is_group: jid.ends_with("@g.us") }
    }
    fn msg(id: &str, ts: i64, text: &str, from_me: bool) -> ChatMsg {
        ChatMsg { id: id.into(), ts, text: text.into(), from_me }
    }

    /// A bridge that replays canned data and records every call it receives.
    struct Fake {
        chats: std::sync::Mutex<Vec<ChatInfo>>,
        msgs: HashMap<String, Vec<ChatMsg>>,
        calls: std::sync::Mutex<Vec<String>>,
    }
    impl Fake {
        fn new(chats: Vec<ChatInfo>, msgs: Vec<(&str, Vec<ChatMsg>)>) -> Self {
            Fake {
                chats: std::sync::Mutex::new(chats),
                msgs: msgs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                calls: Default::default(),
            }
        }
    }
    impl Bridge for Fake {
        async fn chats(&self) -> Result<Vec<ChatInfo>, String> {
            self.calls.lock().unwrap().push("list_chats".into());
            Ok(self.chats.lock().unwrap().clone())
        }
        async fn messages(&self, jid: &str, limit: usize) -> Result<Vec<ChatMsg>, String> {
            self.calls.lock().unwrap().push(format!("list_messages:{jid}"));
            let all = self.msgs.get(jid).cloned().unwrap_or_default();
            let skip = all.len().saturating_sub(limit);
            Ok(all.into_iter().skip(skip).collect())
        }
    }

    fn store() -> parking_lot::Mutex<Store> {
        parking_lot::Mutex::new(Store::open_in_memory().unwrap())
    }

    /// 30 days of a close, two-way friendship: they write, you answer within minutes.
    fn close_friend_history() -> Vec<ChatMsg> {
        let mut v = vec![];
        for d in 1..=14 {
            let t = NOW - d * 86_400;
            v.push(msg(&format!("i{d}"), t, "hey", false));
            v.push(msg(&format!("o{d}"), t + 300, "hi!", true));
            v.push(msg(&format!("i{d}b"), t + 600, "dinner?", false));
            v.push(msg(&format!("o{d}b"), t + 900, "sure", true));
        }
        v
    }

    // ── parsing ──

    #[test]
    fn chats_parse_from_several_shapes_and_timestamp_formats() {
        let arr = r#"[{"jid":"91@s.whatsapp.net","name":"Asha","last_message_time":"2026-10-08T01:00:00+05:30","last_message":"hi","is_from_me":false},
                      {"chat_jid":"123@g.us","chat_name":"Family","last_message_time":1790000000000},
                      {"jid":"92@s.whatsapp.net","last_message_time":1790000000,"last_is_from_me":true}]"#;
        let c = parse_chats(arr).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].name.as_str(), c[0].last_text.as_str(), c[0].last_from_me), ("Asha", "hi", false));
        assert_eq!(c[0].last_ts, 1_791_401_400, "RFC3339 with offset");
        assert!(c[1].is_group && c[1].last_ts == 1_790_000_000, "group by @g.us; milliseconds normalised");
        assert!(c[2].last_from_me && c[2].name.is_empty());
        // Wrapped in an envelope.
        assert_eq!(parse_chats(r#"{"chats":[{"jid":"1@s.whatsapp.net"}]}"#).unwrap().len(), 1);
        // Not JSON / not a list of chats → None, never a panic.
        assert!(parse_chats("Chat: Asha (91…) last message: hi").is_none());
        assert!(parse_chats("").is_none());
        assert_eq!(parse_chats("[]").unwrap().len(), 0);
        assert!(parse_chats(r#"[{"no":"jid"}]"#).unwrap().is_empty());
    }

    #[test]
    fn messages_parse_sorted_oldest_first() {
        let m = parse_messages(
            r#"[{"id":"b","timestamp":1790000100,"content":"second","is_from_me":true},
                {"id":"a","timestamp":"1790000000","text":"first","from_me":false}]"#,
        )
        .unwrap();
        assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(!m[0].from_me && m[1].from_me && m[0].text == "first");
        assert!(parse_messages("nope").is_none());
    }

    #[test]
    fn schema_adaptation_uses_only_declared_parameters() {
        let decl = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<HashSet<String>>();
        let want: Vec<(&[&str], Value)> = vec![(&["limit"], json!(20)), (&["sort_by", "sortBy"], json!("last_active")), (&["include_last_message"], json!(true))];
        let got = adapt(&want, &decl(&["limit", "sortBy"]));
        assert_eq!(got, json!({"limit": 20, "sortBy": "last_active"}), "undeclared include_last_message is dropped");
        // No schema known → the conventional first names.
        assert_eq!(adapt(&want, &decl(&[])), json!({"limit": 20, "sort_by": "last_active", "include_last_message": true}));
        // Declared but nothing matches → empty, rather than inventing arguments.
        assert_eq!(adapt(&want, &decl(&["query"])), json!({}));
    }

    #[test]
    fn gist_redacts_and_clips() {
        let g = gist("call me on 9876543210 or mail me@example.com   please\nnow", 200);
        assert!(!g.contains("9876543210") && !g.contains("me@example.com") && !g.contains('\n'), "{g}");
        assert!(gist(&"a".repeat(500), 50).chars().count() <= 51);
    }

    // ── the guarantees ──

    #[tokio::test]
    async fn read_tool_refuses_anything_outside_the_allowlist() {
        let client = reqwest::Client::new();
        for t in ["mark_read", "mark_chat_read", "send_presence", "send_typing", "send_message", "delete_message", "send_reaction"] {
            let e = read_tool(&client, t, json!({})).await.unwrap_err();
            assert!(e.contains("not an allowed read-only"), "{t}: {e}");
        }
    }

    #[test]
    fn the_allowlist_contains_no_write_or_receipt_tool() {
        for t in READ_TOOLS {
            assert!(McpServer::WhatsApp.blocked_reason(t).is_none(), "{t}");
            assert!(!t.starts_with("send_") && !t.starts_with("mark_") && !t.contains("delete") && !t.contains("edit"), "{t}");
        }
    }

    // ── the poll ──

    #[tokio::test]
    async fn first_sight_learns_history_and_stays_silent_then_new_messages_alert() {
        let st = store();
        let jid = "91@s.whatsapp.net";
        let mut hist = close_friend_history();
        let fake = Fake::new(vec![chat(jid, "Asha", NOW - 600, "dinner?", false)], vec![(jid, hist.clone())]);

        // Poll 1: unknown contact → statistics only, no alert, no read of anything else.
        let a = poll_once(&st, &fake, NOW).await.unwrap();
        assert!(a.is_empty(), "first sight must be silent");
        let p = people::get(&st.lock(), jid).unwrap();
        assert!(p.msgs_in >= 20 && p.msgs_out >= 20 && p.replied >= 20, "{p:?}");
        assert!(people::score(&p, NOW).value >= people::HIGH_AT);

        // Poll 2: nothing new → silent (no repeat).
        assert!(poll_once(&st, &fake, NOW + 30).await.unwrap().is_empty());

        // Poll 3: a new incoming message → one alert with a redacted gist.
        hist.push(msg("new1", NOW + 100, "call me on 9876543210 asap", false));
        let fake = Fake::new(vec![chat(jid, "Asha", NOW + 100, "call me on 9876543210 asap", false)], vec![(jid, hist.clone())]);
        let alerts = poll_once(&st, &fake, NOW + 120).await.unwrap();
        assert_eq!(alerts.len(), 1);
        let a = &alerts[0];
        assert_eq!((a.name.as_str(), a.urgency), ("Asha", AlertUrgency::High));
        assert!(!a.gist.contains("9876543210"), "{}", a.gist);
        // The stored preview is untrusted and redacted.
        let preview = st.lock().recent(Tier::Chat, 5);
        assert_eq!(preview.len(), 1);
        assert!(!preview[0].value.contains("9876543210"));

        // The same message again → no second alert.
        assert!(poll_once(&st, &fake, NOW + 150).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn strangers_groups_muted_and_own_messages_never_alert() {
        let st = store();
        let (a, b, g, m) = ("1@s.whatsapp.net", "2@s.whatsapp.net", "3@g.us", "4@s.whatsapp.net");
        let stranger_hist: Vec<ChatMsg> = (1..=3).map(|i| msg(&format!("s{i}"), NOW - i * 86_400, "offer!", false)).collect();
        let fake = Fake::new(
            vec![chat(a, "Shop", NOW - 100, "sale", false), chat(g, "Family", NOW - 100, "hi all", false), chat(m, "Raj", NOW - 100, "hey", false), chat(b, "Asha", NOW - 100, "hi", true)],
            vec![(a, stranger_hist), (g, vec![]), (m, close_friend_history()), (b, close_friend_history())],
        );
        assert!(poll_once(&st, &fake, NOW).await.unwrap().is_empty()); // learn
        assert!(people::get(&st.lock(), g).is_none(), "groups are never tracked");

        // Raj is muted by the user; the shop and a group message arrive; Asha is written to by me.
        {
            let g_ = st.lock();
            let mut raj = people::get(&g_, m).unwrap();
            raj.muted = true;
            people::save(&g_, &raj, NOW);
        }
        let fake = Fake::new(
            vec![chat(a, "Shop", NOW + 50, "sale again", false), chat(g, "Family", NOW + 50, "hello", false), chat(m, "Raj", NOW + 50, "urgent", false), chat(b, "Asha", NOW + 50, "ok done", true)],
            vec![(a, vec![msg("s9", NOW + 50, "sale again", false)]), (m, vec![msg("r9", NOW + 50, "urgent", false)])],
        );
        let alerts = poll_once(&st, &fake, NOW + 60).await.unwrap();
        assert!(alerts.is_empty(), "stranger below threshold, group skipped, muted silent, own message silent: {alerts:?}");
        // …but the statistics kept learning.
        let asha = people::get(&st.lock(), b).unwrap();
        assert!(asha.last_out_ts == NOW + 50);
    }

    #[tokio::test]
    async fn bootstrap_is_capped_per_poll_and_finished_next_poll() {
        let st = store();
        let chats: Vec<ChatInfo> = (0..10).map(|i| chat(&format!("{i}@s.whatsapp.net"), &format!("P{i}"), NOW - 100, "x", false)).collect();
        let msgs: Vec<(&str, Vec<ChatMsg>)> = vec![];
        let fake = Fake::new(chats, msgs);
        poll_once(&st, &fake, NOW).await.unwrap();
        assert_eq!(people::load_all(&st.lock()).len(), MAX_BOOTSTRAP_PER_POLL);
        poll_once(&st, &fake, NOW + 30).await.unwrap();
        assert_eq!(people::load_all(&st.lock()).len(), 10);
    }

    #[tokio::test]
    async fn the_poll_only_ever_calls_read_methods() {
        let st = store();
        let fake = Fake::new(vec![chat("1@s.whatsapp.net", "A", NOW, "x", false)], vec![]);
        poll_once(&st, &fake, NOW).await.unwrap();
        assert!(fake.calls.lock().unwrap().iter().all(|c| c.starts_with("list_")));
    }

    // ── people commands ──

    fn seeded() -> parking_lot::Mutex<Store> {
        let st = store();
        {
            let g = st.lock();
            for (jid, name) in [("1@s.whatsapp.net", "Asha Rao"), ("2@s.whatsapp.net", "Asha Menon"), ("3@s.whatsapp.net", "Raj")] {
                let mut p = people::stats_from(&interactions(&close_friend_history()), NOW);
                p.jid = jid.into();
                p.name = name.into();
                people::save(&g, &p, NOW);
            }
        }
        st
    }

    #[test]
    fn vip_and_mute_are_exclusive_and_ambiguity_is_never_guessed() {
        let st = seeded();
        let g = st.lock();
        match set_flag(&g, "asha", PersonFlag::Vip, true, NOW) {
            FlagOutcome::Ambiguous(mut v) => {
                v.sort();
                assert_eq!(v, vec!["Asha Menon".to_string(), "Asha Rao".to_string()]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
        assert_eq!(set_flag(&g, "nobody", PersonFlag::Vip, true, NOW), FlagOutcome::NotFound);
        assert_eq!(set_flag(&g, "raj", PersonFlag::Vip, true, NOW), FlagOutcome::Done("Raj".into()));
        assert!(people::get(&g, "3@s.whatsapp.net").unwrap().vip);
        assert_eq!(set_flag(&g, "raj", PersonFlag::Mute, true, NOW), FlagOutcome::Done("Raj".into()));
        let p = people::get(&g, "3@s.whatsapp.net").unwrap();
        assert!(p.muted && !p.vip, "muting clears VIP");
        set_flag(&g, "raj", PersonFlag::Mute, false, NOW);
        assert!(!people::get(&g, "3@s.whatsapp.net").unwrap().muted);
        // Full name disambiguates.
        assert_eq!(set_flag(&g, "asha menon", PersonFlag::Vip, true, NOW), FlagOutcome::Done("Asha Menon".into()));
    }

    #[test]
    fn priority_line_names_only_unmuted_people_above_the_bar() {
        let st = seeded();
        let g = st.lock();
        set_flag(&g, "raj", PersonFlag::Mute, true, NOW);
        let line = priority_line(&g, NOW);
        assert!(line.contains("Asha Rao") && line.contains("Asha Menon") && !line.contains("Raj"), "{line}");
        assert!(priority_line(&store().lock(), NOW).contains("haven't learned any"));
    }

    #[test]
    fn resolve_chat_by_name_or_last_alert() {
        let st = seeded();
        let g = st.lock();
        assert_eq!(resolve_chat(&g, Some("raj")).unwrap().0, "3@s.whatsapp.net");
        assert!(resolve_chat(&g, Some("asha")).unwrap_err().contains("more than one"));
        assert!(resolve_chat(&g, Some("zed")).unwrap_err().contains("don't know"));
    }

    // ── wording & reading ──

    #[test]
    fn spoken_alerts_never_contain_the_message_text() {
        let a = WaAlert { jid: "j".into(), name: "Asha".into(), score: 90, why: "".into(), urgency: AlertUrgency::High, gist: "my password is hunter2".into() };
        let one = alert_text(&[a.clone()], Some("sir"));
        assert_eq!(one, "Asha sent you a message, sir. Say read that message to hear it.");
        assert!(!one.contains("hunter2"));
        let many = alert_text(&[a.clone(), WaAlert { name: "Raj".into(), ..a.clone() }, WaAlert { name: "Mom".into(), ..a.clone() }], None);
        assert!(many.starts_with("3 of your priority people messaged you: Asha, Raj and Mom."), "{many}");
        assert_eq!(alert_text(&[], None), "");
    }

    #[test]
    fn read_aloud_speaks_incoming_only_and_says_nothing_was_marked_read() {
        let msgs = vec![msg("1", 1, "old one", false), msg("2", 2, "my reply", true), msg("3", 3, "are you free tonight?", false)];
        let t = read_aloud("Asha", &msgs);
        assert!(t.starts_with("Asha says:") && t.contains("are you free tonight?") && !t.contains("my reply"), "{t}");
        assert!(t.ends_with("I haven't marked it as read."));
        assert!(read_aloud("Asha", &[msg("1", 1, "mine", true)]).contains("don't see any recent messages"));
        let md = card_markdown("Asha [x]", &msgs);
        assert!(md.contains("**You**: my reply") && md.contains("Asha \\[x\\]") && md.contains("did not mark this chat as read"), "{md}");
    }

    // ── the real call path, against a mock bridge ──

    static MOCK_GUARD: Mutex<()> = Mutex::new(());

    struct MockBridge {
        log: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }
    impl Drop for MockBridge {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            *mcp_client::TEST_WHATSAPP_URL.lock().unwrap() = None;
        }
    }

    fn start_mock_bridge() -> MockBridge {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log2, stop2) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop2.load(Ordering::Relaxed) {
                let Ok((mut sock, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(3));
                    continue;
                };
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = vec![0u8; 16384];
                let mut got = 0;
                loop {
                    let n = sock.read(&mut buf[got..]).unwrap_or(0);
                    got += n;
                    let text = String::from_utf8_lossy(&buf[..got]).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len: usize = text[..i]
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                            .unwrap_or(0);
                        if got >= i + 4 + len || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..got]).to_string();
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("");
                let v: Value = serde_json::from_str(body).unwrap_or(json!({}));
                let method = v["method"].as_str().unwrap_or("").to_string();
                let tool = v["params"]["name"].as_str().unwrap_or("").to_string();
                log2.lock().unwrap().push(if tool.is_empty() { method.clone() } else { format!("{method}:{tool}:{}", v["params"]["arguments"]) });
                let result = if method == "tools/list" {
                    json!({"tools":[
                        {"name":"list_chats","inputSchema":{"properties":{"limit":{},"sort_by":{},"include_last_message":{}}}},
                        {"name":"list_messages","inputSchema":{"properties":{"chat_jid":{},"limit":{}}}},
                        {"name":"mark_read","inputSchema":{"properties":{"message_ids":{}}}},
                        {"name":"send_presence","inputSchema":{"properties":{"presence":{}}}},
                        {"name":"send_message","inputSchema":{"properties":{"recipient":{},"message":{}}}}
                    ]})
                } else if tool == "list_chats" {
                    json!({"content":[{"type":"text","text": r#"[{"jid":"91@s.whatsapp.net","name":"Asha","last_message_time":1790000000,"last_message":"hi","is_from_me":false},{"jid":"7@g.us","name":"Family","last_message_time":1790000001}]"#}]})
                } else if tool == "list_messages" {
                    json!({"content":[{"type":"text","text": r#"[{"id":"m1","timestamp":1790000000,"content":"hi","is_from_me":false}]"#}]})
                } else if tool == "pairing_status" {
                    json!({"content":[{"type":"text","text":"{\"setup_state\":\"ready\"}"}]})
                } else {
                    json!({"content":[{"type":"text","text":"ok"}]})
                };
                let payload = json!({"jsonrpc":"2.0","id":1,"result":result}).to_string();
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                let _ = sock.write_all(reply.as_bytes());
            }
        });
        *mcp_client::TEST_WHATSAPP_URL.lock().unwrap() = Some(format!("http://{addr}/mcp"));
        MockBridge { log, stop }
    }

    #[tokio::test]
    async fn real_call_path_reads_only_adapts_parameters_and_never_asks_for_receipts() {
        let _g = MOCK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let mock = start_mock_bridge();
        let client = reqwest::Client::new();
        let bridge = McpBridge::connect(&client).await;
        assert!(bridge.tool_names.contains(&"mark_read".to_string()), "the mock bridge does offer receipt tools");

        let chats = bridge.chats().await.unwrap();
        assert_eq!(chats.len(), 2);
        let msgs = bridge.messages("91@s.whatsapp.net", 5).await.unwrap();
        assert_eq!(msgs.len(), 1);

        // Even a direct attempt through the central guard is refused and never reaches the bridge.
        let r = mcp_client::call_tool(McpServer::WhatsApp, "mark_read", json!({"message_ids":["m1"]}), &client, None).await;
        assert!(!r.ok && r.error.unwrap().contains("NEVER") == false);

        let log = mock.log.lock().unwrap().clone();
        assert!(log.iter().any(|l| l.starts_with("tools/list")));
        let calls: Vec<&String> = log.iter().filter(|l| l.starts_with("tools/call:")).collect();
        assert!(calls.iter().all(|l| l.contains(":list_chats:") || l.contains(":list_messages:")), "{calls:?}");
        assert!(!log.iter().any(|l| l.contains("mark_read") || l.contains("send_presence") || l.contains("send_typing") || l.contains("mark_chat_read")), "{log:?}");
        // Arguments follow the schema the bridge declared.
        let lc = calls.iter().find(|l| l.contains(":list_chats:")).unwrap();
        assert!(lc.contains("\"limit\":20") && lc.contains("\"sort_by\":\"last_active\"") && lc.contains("include_last_message"), "{lc}");
        let lm = calls.iter().find(|l| l.contains(":list_messages:")).unwrap();
        assert!(lm.contains("\"chat_jid\":\"91@s.whatsapp.net\"") && lm.contains("\"limit\":5"), "{lm}");
    }

    #[tokio::test]
    async fn selftest_reports_reachability_formats_and_guarded_tools() {
        let _g = MOCK_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        {
            // Nothing listening: honest "not running".
            *mcp_client::TEST_WHATSAPP_URL.lock().unwrap() = Some("http://127.0.0.1:1/mcp".into());
            let t = selftest(&reqwest::Client::new()).await;
            assert!(!t.reachable && t.note.contains("not running"), "{t:?}");
            *mcp_client::TEST_WHATSAPP_URL.lock().unwrap() = None;
        }
        let _mock = start_mock_bridge();
        let t = selftest(&reqwest::Client::new()).await;
        assert!(t.reachable && t.paired && t.has_read_tools && t.format_ok, "{t:?}");
        assert_eq!(t.chats_parsed, 2);
        assert!(t.guarded_tools_present.contains(&"mark_read".to_string()) && t.guarded_tools_present.contains(&"send_presence".to_string()), "{t:?}");
    }
}
