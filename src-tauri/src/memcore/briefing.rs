//! Boot briefing (plan P3): after the laptop starts, NEXUS says where the
//! user left off and what is still open — once a day, only when it has
//! something true to say, never during a meeting.
//!
//! The text is built deterministically from local data (resume points, mail
//! deadlines the user asked to watch, requests NEXUS could not finish). No
//! cloud call is involved, so nothing about it leaves the device. It never
//! invents a quantity: "how much is left" is only ever a count of open
//! items or a deadline taken verbatim from an email.

use std::sync::Mutex;

use super::agenda::AgendaItem;
use super::mailtriage::{self, Category, StoredMail};
use super::resume::{self, Kind, Sample};
use super::store::Tier;

/// Resume points older than this are not "where you left off" any more.
pub const MAX_RESUME_AGE_SECS: i64 = 7 * 24 * 3600;
/// The hello waits for the stage renderer (so orb + caption exist) but never longer than this.
pub const READY_WAIT_SECS: u64 = 60;
/// A short settle after the stage is alive (boot is still busy for a moment).
pub const SETTLE_SECS: u64 = 3;
/// The user counts as present with input in the last 5 minutes (they just typed a password at logon;
/// unknown = assume present). The old 60 s band made an active user wait up to 15 min for nothing.
pub const PRESENT_WITHIN_SECS: u64 = 300;
/// A plain "Good morning, sir." on every logon / app start, but not twice within this window
/// (a developer restarting the app should not be greeted every minute).
pub const HELLO_MIN_GAP_SECS: i64 = 2 * 3600;
/// Per-part cap while gathering the details of the briefing: the hello never waits for them.
const GATHER_CAP_SECS: u64 = 3;

/// Is a hello due, given when the last one was spoken (unix seconds)?
pub fn hello_due(last_hello: Option<i64>, now: i64) -> bool {
    last_hello.map(|t| now - t >= HELLO_MIN_GAP_SECS || now < t).unwrap_or(true)
}

/// The briefing text without its leading time-of-day greeting (when a hello was already spoken).
pub fn without_greeting(spoken: &str, hour: u32, name: Option<&str>, friend: bool) -> String {
    let g = greeting(hour, name, friend);
    spoken.strip_prefix(g.as_str()).unwrap_or(spoken).trim_start().to_string()
}

/// The three hellos (user decision): morning 05:00-11:59, afternoon 12:00-16:59, evening otherwise.
pub fn hello_line(hour: u32, name: Option<&str>, friend: bool) -> String {
    greeting(hour, name, friend)
}
/// How long the boot briefing waits for the user to show up before giving up for today.
pub const PRESENCE_WAIT_SECS: u64 = 20 * 60;
const PRESENCE_POLL_SECS: u64 = 10;

pub fn user_present(idle_secs: Option<u64>) -> bool {
    idle_secs.map(|s| s < PRESENT_WITHIN_SECS).unwrap_or(true)
}

/// An explicit "where did I leave off" ignores the window the user is in now.
const CURRENT_WINDOW_SECS: i64 = 90;

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeEntry {
    pub sample: Sample,
    pub last_seen: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MailItem {
    pub sender: String,
    pub subject: String,
    /// Deadline text exactly as found in the email ("Friday 5 PM").
    pub deadline: Option<String>,
    /// Inbox-triage category ("exam", "database", …) for mail the watcher filed.
    pub category: Option<String>,
}

/// What was open when the PC last went off and can be reopened ("shall I reopen it?").
#[derive(Debug, Clone, PartialEq)]
pub struct SessionOffer {
    /// "VS Code on ULTRON, Brave and 2 folders".
    pub summary: String,
    /// Unix seconds the PC was last seen in that state.
    pub left_at: i64,
}

#[derive(Debug, Clone)]
pub struct Input {
    pub now: i64,
    /// Local hour of day, 0-23.
    pub hour: u32,
    pub name: Option<String>,
    pub friend: bool,
    /// Newest first.
    pub resume: Vec<ResumeEntry>,
    pub mail: Vec<MailItem>,
    /// Requests NEXUS could not finish, newest first.
    pub unfinished: Vec<String>,
    /// Today's calendar (empty if not connected / unreachable).
    pub events: Vec<AgendaItem>,
    /// The previous session, when there is something to reopen.
    pub session: Option<SessionOffer>,
    /// Connection problems worth mentioning (new or changed only).
    pub health_spoken: Option<String>,
    /// Card section listing every current connection problem.
    pub health_card: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Briefing {
    pub spoken: String,
    pub card_md: String,
    /// Open items (mail deadlines + unfinished requests).
    pub items: usize,
    /// The spoken text ends with "Shall I reopen them?" - the caller must register the offer.
    pub offers_restore: bool,
}

fn clip(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let cut: String = t.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

fn md_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

pub fn ago_phrase(now: i64, ts: i64) -> String {
    let d = (now - ts).max(0);
    match d {
        0..=89 => "just now".into(),
        90..=3599 => format!("about {} minutes ago", (d / 60).max(2)),
        3600..=86_399 => {
            let h = (d + 1800) / 3600;
            if h == 1 { "about an hour ago".into() } else { format!("about {h} hours ago") }
        }
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", d / 86_400),
    }
}

fn greeting(hour: u32, name: Option<&str>, friend: bool) -> String {
    let base = match hour {
        5..=11 => "Good morning",
        12..=16 => "Good afternoon",
        _ => "Good evening",
    };
    format!("{base}{}.", crate::persona::greet_address(name, friend))
}

fn site_label(url: &str) -> Option<&'static str> {
    let u = url.to_lowercase();
    [
        ("leetcode.com", "LeetCode"),
        ("github.com", "GitHub"),
        ("youtube.com", "YouTube"),
        ("stackoverflow.com", "Stack Overflow"),
        ("docs.google.com", "Google Docs"),
        ("mail.google.com", "Gmail"),
    ]
    .iter()
    .find(|(host, _)| u.contains(host))
    .map(|(_, label)| *label)
}

/// Shell/terminal window titles are paths or executables ("C:\WINDOWS\system32\cmd.exe"),
/// which read out loud as noise.
fn title_is_noise(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    t.contains(":\\") || t.ends_with(".exe") || t.starts_with("administrator:")
}

/// A window whose title is just the app's own name ("OpenCode" in OpenCode.exe) carries no
/// information; speaking it gave "in Opencode on “OpenCode”".
fn title_repeats_app(title: &str, friendly: &str, process: &str) -> bool {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let t = norm(title);
    if t.is_empty() {
        return true;
    }
    let stem = process.to_lowercase();
    let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
    t == norm(friendly) || t == norm(stem)
}

/// "watching “…” on YouTube", "working on LeetCode: “…”", "in VS Code on “main.rs”".
pub fn describe(s: &Sample) -> String {
    let title = clip(&s.title, 60);
    match resume::kind(s) {
        Kind::Video => {
            let t = title.trim_end_matches(" - YouTube").to_string();
            format!("watching “{t}” on YouTube")
        }
        Kind::Browser => match s.url.as_deref().and_then(site_label) {
            Some(site) => format!("on {site}: “{title}”"),
            None => format!("reading “{title}” in {}", resume::friendly_app(&s.app)),
        },
        Kind::App => {
            let app = resume::friendly_app(&s.app);
            if title.is_empty() || title_is_noise(&title) || title_repeats_app(&title, &app, &s.app) {
                format!("in {app}")
            } else {
                format!("in {app} on “{title}”")
            }
        }
    }
}

pub fn mail_line(m: &MailItem) -> String {
    let who = clip(&m.sender, 40);
    let subj = clip(&m.subject, 60);
    if let Some(cat) = m.category.as_deref().and_then(Category::from_str) {
        return format!("{} from {who}: “{subj}”", cat.phrase());
    }
    match m.deadline.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => format!("{who} emailed about “{subj}”, deadline {d}"),
        None => format!("{who} emailed about “{subj}”"),
    }
}

/// Build the briefing, or `None` when there is nothing true to say.
pub fn build(input: &Input) -> Option<Briefing> {
    let latest = input
        .resume
        .iter()
        .find(|r| input.now - r.last_seen <= MAX_RESUME_AGE_SECS);
    let mut open: Vec<String> = input.mail.iter().map(mail_line).collect();
    open.extend(
        input
            .unfinished
            .iter()
            .map(|t| format!("I couldn’t finish “{}”", clip(t, 70))),
    );
    if latest.is_none()
        && open.is_empty()
        && input.events.is_empty()
        && input.session.is_none()
        && input.health_spoken.is_none()
    {
        return None;
    }

    // ── spoken (short) ──
    let mut spoken = greeting(input.hour, input.name.as_deref(), input.friend);
    if let Some(s) = &input.session {
        // The whole previous session is more useful than the single last window.
        // Active voice, no repeated greeting (the time-of-day greeting above
        // already signals "you're back" — "Welcome back." was redundant).
        spoken.push_str(&format!(
            " You left {} open {}.",
            s.summary,
            ago_phrase(input.now, s.left_at)
        ));
    } else if let Some(r) = latest {
        spoken.push_str(&format!(
            " You were {}, {}.",
            describe(&r.sample),
            ago_phrase(input.now, r.last_seen)
        ));
    }
    if let Some(first) = input.events.first() {
        let n = input.events.len();
        let when = if first.all_day { "all day".to_string() } else { format!("at {}", first.when_label) };
        spoken.push_str(&if n == 1 {
            format!(" You have one event today: {} {when}.", first.summary)
        } else {
            format!(" You have {n} events today; the first is {} {when}.", first.summary)
        });
    }
    match open.len() {
        0 => {}
        1 => spoken.push_str(&format!(" One thing is open: {}.", open[0])),
        n => {
            spoken.push_str(&format!(" {n} things are open. First: {}.", open[0]));
            if n > 1 {
                spoken.push_str(" Say “show my briefing” for the rest.");
            }
        }
    }
    if let Some(h) = &input.health_spoken {
        spoken.push(' ');
        spoken.push_str(h);
    }
    // The question comes last so the answer window opens right after it.
    if input.session.is_some() {
        spoken.push_str(" Shall I reopen them?");
    }

    // ── card (full) ──
    let mut md = String::from("## Welcome back\n\n");
    if let Some(s) = &input.session {
        md.push_str("### Open when you left\n");
        md.push_str(&format!("- {} — {}\n\n", md_escape(&s.summary), ago_phrase(input.now, s.left_at)));
    }
    let recents: Vec<&ResumeEntry> = input
        .resume
        .iter()
        .filter(|r| input.now - r.last_seen <= MAX_RESUME_AGE_SECS)
        .take(3)
        .collect();
    if !recents.is_empty() {
        md.push_str("### Where you left off\n");
        for r in recents {
            md.push_str(&format!(
                "- {} — {}\n",
                md_escape(&describe(&r.sample)),
                ago_phrase(input.now, r.last_seen)
            ));
        }
        md.push('\n');
    }
    if !input.events.is_empty() {
        md.push_str("### Today's calendar\n");
        for e in &input.events {
            md.push_str(&format!("- {} — {}\n", md_escape(&e.summary), e.when_label));
        }
        md.push('\n');
    }
    if !open.is_empty() {
        md.push_str(&format!("### Open ({})\n", open.len()));
        for o in &open {
            md.push_str(&format!("- {}\n", md_escape(o)));
        }
        md.push('\n');
    }
    if let Some(h) = &input.health_card {
        md.push_str(h);
    }
    md.push_str(
        "---\nOpen items come from emails you asked me to watch and requests I couldn’t finish. \
         Your timetable and email priorities will appear here once they are set up.\n",
    );

    Some(Briefing { spoken, card_md: md, items: open.len(), offers_restore: input.session.is_some() })
}

// ─── Glue ────────────────────────────────────────────────────────────

/// The last session's resume points, captured at startup BEFORE the recorder
/// writes anything for this session.
static BOOT_SNAPSHOT: Mutex<Option<Vec<ResumeEntry>>> = Mutex::new(None);

fn entries_from_store(dir: &std::path::Path, limit: usize) -> Vec<ResumeEntry> {
    super::with_store(dir, |s| {
        s.recent(Tier::Resume, limit)
            .into_iter()
            .filter_map(|h| {
                resume::decode(&h.value).map(|sample| ResumeEntry { sample, last_seen: h.last_seen })
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Call once at startup, before `resume::start_recorder`.
pub fn snapshot_at_boot(app_data_dir: &std::path::Path) {
    let entries = entries_from_store(app_data_dir, 20);
    if let Ok(mut g) = BOOT_SNAPSHOT.lock() {
        *g = Some(entries);
    }
}

/// Important mail the inbox watcher filed in the last 3 days (Medium and up),
/// most urgent first, at most 5.
pub fn important_mail(dir: &std::path::Path, now_secs: i64) -> Vec<MailItem> {
    let mut rows: Vec<StoredMail> = super::with_store(dir, |s| {
        s.recent(Tier::Mail, 60)
            .into_iter()
            .filter_map(|h| serde_json::from_str::<StoredMail>(&h.value).ok())
            .collect()
    })
    .unwrap_or_default();
    rows.retain(|m| {
        now_secs - m.ts_ms / 1000 <= 3 * 24 * 3600
            && mailtriage::urgency_rank(mailtriage::urgency_from(&m.urgency)) >= mailtriage::urgency_rank(crate::google::types::AlertUrgency::Medium)
    });
    rows.sort_by(|a, b| {
        mailtriage::urgency_rank(mailtriage::urgency_from(&b.urgency))
            .cmp(&mailtriage::urgency_rank(mailtriage::urgency_from(&a.urgency)))
            .then(b.ts_ms.cmp(&a.ts_ms))
    });
    rows.into_iter()
        .take(5)
        .map(|m| MailItem { sender: m.sender, subject: m.subject, deadline: None, category: Some(m.category) })
        .collect()
}

fn gather<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    dir: &std::path::Path,
    resume: Vec<ResumeEntry>,
    events: Vec<AgendaItem>,
    session: Option<SessionOffer>,
    health: super::health::Report,
) -> Input {
    use chrono::Timelike;
    let now = chrono::Utc::now().timestamp();
    let mut mail: Vec<MailItem> = crate::memory::load_mail_watches(dir)
        .into_iter()
        .filter(|w| w.status == crate::google::types::WatchStatus::Active)
        .map(|w| MailItem { sender: w.sender, subject: w.subject, deadline: w.initial_deadline_raw, category: None })
        .collect();
    mail.extend(important_mail(dir, now));
    Input {
        now,
        hour: chrono::Local::now().hour(),
        name: crate::memory::read_user_profile(dir).and_then(|p| p.name),
        friend: crate::persona::is_friend(&crate::commands::read_persona_mode(app)),
        resume,
        mail,
        unfinished: crate::conversation::unfinished_requests(dir, now),
        events,
        session,
        health_spoken: super::health::spoken_line(&health.to_speak),
        health_card: super::health::card_section(&health.all),
    }
}

/// The previous session, if it can still be (partly) reopened. Clears the pending
/// snapshot when everything in it is already open or it is too old. Blocking.
pub fn session_offer(dir: &std::path::Path, now: i64) -> Option<SessionOffer> {
    use super::{restore, session};
    let snap = session::load_pending(dir)?;
    if now - snap.ts > session::MAX_AGE_SECS {
        session::clear_pending(dir);
        return None;
    }
    let plan = restore::plan(&snap, &restore::running_exes(), &|p| std::path::Path::new(p).exists());
    if plan.is_empty() {
        session::clear_pending(dir);
        return None;
    }
    Some(SessionOffer { summary: restore::describe(&plan.items), left_at: snap.ts })
}

/// Today's calendar for the briefing, best effort (never blocks it for long).
async fn today_events() -> Vec<AgendaItem> {
    match tokio::time::timeout(std::time::Duration::from_secs(5), super::google_io::agenda_for(0)).await {
        Ok(Ok(items)) => items,
        _ => vec![],
    }
}

/// Wait until the stage renderer is alive (orb + caption exist, so sound is never invisible), then a
/// short settle. Never longer than `READY_WAIT_SECS`. Returns the seconds waited.
async fn wait_until_ready() -> u64 {
    let start = std::time::Instant::now();
    while !crate::stage::is_ready() && start.elapsed().as_secs() < READY_WAIT_SECS {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    tokio::time::sleep(std::time::Duration::from_secs(SETTLE_SECS)).await;
    start.elapsed().as_secs()
}

/// Wait (bounded) until the line just handed to the speech policy has been spoken and is over.
async fn wait_until_quiet(max_secs: u64) {
    let start = std::time::Instant::now();
    // Give the policy + heads-up beat time to start the line before we look for its end.
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    while crate::orchestrator::has_active_request() && start.elapsed().as_secs() < max_secs {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
}

/// Speak the plain time-of-day hello unless one was spoken recently. `tag` names the trigger in the log
/// and in the alert id. Returns whether a hello was submitted.
fn submit_hello<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &std::path::Path, tag: &str) -> bool {
    use chrono::Timelike;
    let now_ts = chrono::Utc::now().timestamp();
    let last_hello = super::with_store(dir, |s| s.meta_get("last_hello_ts").and_then(|v| v.parse::<i64>().ok())).flatten();
    if !hello_due(last_hello, now_ts) {
        println!("[BOOT] no hello ({tag}): one was spoken less than {}h ago", HELLO_MIN_GAP_SECS / 3600);
        return false;
    }
    super::with_store(dir, |s| s.meta_set("last_hello_ts", &now_ts.to_string()));
    let name = crate::memory::read_user_profile(dir).and_then(|p| p.name);
    let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(app));
    let hour = chrono::Local::now().hour();
    println!("[BOOT] hello ({tag}, hour {hour})");
    crate::proactive_policy::submit(
        app,
        format!("boot_hello_{tag}_{now_ts}"),
        hello_line(hour, name.as_deref(), friend),
        crate::google::types::AlertUrgency::High,
    );
    true
}

/// How long the PC must have been asleep before waking it counts as "coming back".
pub const RESUME_HELLO_MIN_AWAY_SECS: u64 = 20 * 60;

/// The machine just woke from sleep after `away_secs`. After a real absence, greet once the user is
/// back at the keyboard (they have typed their password / PIN). No personal content: just the hello.
pub fn on_resume<R: tauri::Runtime>(app: tauri::AppHandle<R>, away_secs: u64) {
    use tauri::Manager;
    if away_secs < RESUME_HELLO_MIN_AWAY_SECS {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let Ok(dir) = app.path().app_data_dir() else { return };
        if !super::enabled(&dir) || !super::flag(&dir, "memcoreBriefing", true) {
            return;
        }
        // Silent while the lock screen is up: wait (bounded) for input, which only the owner can give.
        let mut waited = 0u64;
        let baseline = resume::user_idle_secs();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            waited += 2;
            let idle = resume::user_idle_secs();
            let typed_recently = idle.map(|s| s < 10).unwrap_or(false);
            let changed = matches!((baseline, idle), (Some(b), Some(i)) if i < b);
            if typed_recently || changed {
                break;
            }
            if waited >= 180 {
                println!("[BOOT] resume hello skipped: nobody came back within 3 min");
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        submit_hello(&app, &dir, "resume");
    });
}

/// The spoken hello at logon / app start, then the once-a-day briefing. The hello is the cached
/// "Good morning/afternoon/evening, sir." line (no network, no voice change) the moment the stage is
/// alive; the details are gathered in parallel with a hard per-part cap and follow it.
pub fn start_boot_briefing<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let waited = wait_until_ready().await;
        println!("[BOOT] stage ready after {waited}s (incl. {SETTLE_SECS}s settle)");
        let Ok(dir) = app.path().app_data_dir() else { return };
        if !super::enabled(&dir) || !super::flag(&dir, "memcoreBriefing", true) {
            println!("[BOOT] skip: memcore or memcoreBriefing disabled");
            return;
        }
        // Autostart runs at logon: do not talk to an empty room. Wait (up to
        // PRESENCE_WAIT_SECS) until there has been keyboard/mouse input lately.
        let mut presence_waited = 0u64;
        while !user_present(resume::user_idle_secs()) {
            if presence_waited >= PRESENCE_WAIT_SECS {
                println!("[BOOT] skip: gave up waiting for presence after {PRESENCE_WAIT_SECS}s");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(PRESENCE_POLL_SECS)).await;
            presence_waited += PRESENCE_POLL_SECS;
        }
        // 1. The hello: every logon, short, cached, immediate.
        let hello_spoken = submit_hello(&app, &dir, "boot");

        // 2. The details, once a day.
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let already = super::with_store(&dir, |s| s.meta_get("last_briefing_day").as_deref() == Some(today.as_str()))
            .unwrap_or(true);
        if already {
            println!("[BOOT] skip briefing: already briefed today ({today})");
            return;
        }
        let snapshot = BOOT_SNAPSHOT.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
        let cap = std::time::Duration::from_secs(GATHER_CAP_SECS);
        let events_fut = async {
            if super::flag(&dir, "memcoreMail", true) {
                tokio::time::timeout(cap, today_events()).await.unwrap_or_default()
            } else {
                vec![]
            }
        };
        let session_fut = async {
            if super::flag(&dir, "memcoreSession", true) {
                let d = dir.clone();
                let now = chrono::Utc::now().timestamp();
                tokio::time::timeout(cap, tauri::async_runtime::spawn_blocking(move || session_offer(&d, now)))
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .flatten()
            } else {
                None
            }
        };
        let health_fut = async {
            if super::flag(&dir, "memcoreHealth", true) {
                // Health never delays the hello; it only decorates the details, so it gets a cap too.
                tokio::time::timeout(cap * 2, super::health::run(&app, &dir)).await.unwrap_or_default()
            } else {
                super::health::Report::default()
            }
        };
        let (events, session, health) = tokio::join!(events_fut, session_fut, health_fut);
        let input = gather(&app, &dir, snapshot, events, session, health);
        let Some(briefing) = build(&input) else {
            println!("[BOOT] skip briefing: nothing to say (no resume/mail/events/session/health)");
            return;
        };
        super::with_store(&dir, |s| s.meta_set("last_briefing_day", &today));
        println!(
            "[BOOT] briefing: session_offer={} health_spoken={} open_items={}",
            input.session.is_some(),
            input.health_spoken.is_some(),
            briefing.items
        );
        let spoken = if hello_spoken {
            // The hello already greeted; let it finish before the details start.
            wait_until_quiet(20).await;
            without_greeting(&briefing.spoken, input.hour, input.name.as_deref(), input.friend)
        } else {
            briefing.spoken.clone()
        };
        // The question is only armed once it has been spoken (see `speak_line`: ids that
        // start with "offer_" arm the reply window).
        let id = if briefing.offers_restore {
            super::offer::set(super::offer::Offer::ReopenSession, false);
            "offer_boot_session"
        } else {
            "boot_briefing"
        };
        crate::proactive_policy::submit(&app, id.to_string(), spoken, crate::google::types::AlertUrgency::High);
    });
}

/// "Where did I leave off / show my briefing": built now from live data,
/// skipping the window the user is in at this moment.
pub async fn on_demand<R: tauri::Runtime>(app: &tauri::AppHandle<R>, dir: &std::path::Path) -> Option<Briefing> {
    let now = chrono::Utc::now().timestamp();
    let events = today_events().await;
    let resume: Vec<ResumeEntry> = entries_from_store(dir, 20)
        .into_iter()
        .filter(|r| now - r.last_seen > CURRENT_WINDOW_SECS)
        .collect();
    build(&gather(app, dir, resume, events, None, super::health::Report::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(app: &str, title: &str, url: Option<&str>) -> Sample {
        Sample { app: app.into(), title: title.into(), url: url.map(String::from), activity: None }
    }

    fn input() -> Input {
        Input {
            now: 1_000_000,
            hour: 9,
            name: Some("Lakshya".into()),
            friend: false,
            resume: vec![],
            mail: vec![],
            unfinished: vec![],
            events: vec![],
            session: None,
            health_spoken: None,
            health_card: None,
        }
    }

    #[test]
    fn nothing_to_say_means_silence() {
        assert!(build(&input()).is_none());
        let mut i = input();
        i.resume = vec![ResumeEntry { sample: sample("code.exe", "a.rs", None), last_seen: i.now - MAX_RESUME_AGE_SECS - 1 }];
        assert!(build(&i).is_none(), "a resume point older than 7 days is not 'where you left off'");
    }

    #[test]
    fn says_where_you_left_off_with_a_real_age() {
        let mut i = input();
        i.resume = vec![ResumeEntry {
            sample: sample("brave.exe", "Two Sum", Some("https://leetcode.com/problems/two-sum")),
            last_seen: i.now - 3 * 3600,
        }];
        let b = build(&i).unwrap();
        assert!(b.spoken.starts_with("Good morning, sir."), "{}", b.spoken);
        assert!(b.spoken.contains("on LeetCode: “Two Sum”"), "{}", b.spoken);
        assert!(b.spoken.contains("about 3 hours ago"), "{}", b.spoken);
        assert_eq!(b.items, 0);
    }

    #[test]
    fn video_and_app_phrasing() {
        let v = sample("brave.exe", "Binary Search in 10 min - YouTube", Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert_eq!(describe(&v), "watching “Binary Search in 10 min” on YouTube");
        assert_eq!(describe(&sample("code.exe", "main.rs", None)), "in VS Code on “main.rs”");
        assert_eq!(
            describe(&sample("windowsterminal.exe", r"C:\WINDOWS\system32\cmd.exe ", None)),
            "in Terminal",
            "a shell path is noise, not a title"
        );
        assert_eq!(describe(&sample("brave.exe", "Healthiest nuts", Some("https://www.womenshealthmag.com/x"))), "reading “Healthiest nuts” in Brave");
    }

    #[test]
    fn open_items_are_counted_never_invented() {
        let mut i = input();
        i.mail = vec![MailItem { sender: "Prof. Rao".into(), subject: "Assignment 3".into(), deadline: Some("Friday 5 PM".into()), category: None }];
        i.unfinished = vec!["send the report to Asha".into()];
        let b = build(&i).unwrap();
        assert_eq!(b.items, 2);
        assert!(b.spoken.contains("2 things are open"), "{}", b.spoken);
        assert!(b.spoken.contains("deadline Friday 5 PM"), "{}", b.spoken);
        assert!(b.card_md.contains("### Open (2)") && b.card_md.contains("I couldn’t finish"), "{}", b.card_md);
        assert!(!b.spoken.contains('%'), "no made-up progress numbers: {}", b.spoken);
    }

    #[test]
    fn friend_mode_uses_the_first_name_and_late_hours_say_good_evening() {
        let mut i = input();
        i.friend = true;
        i.hour = 2;
        i.mail = vec![MailItem { sender: "GitHub".into(), subject: "Run failed".into(), deadline: None, category: None }];
        let b = build(&i).unwrap();
        assert!(b.spoken.starts_with("Good evening, Lakshya."), "{}", b.spoken);
        i.name = None;
        assert!(build(&i).unwrap().spoken.starts_with("Good evening."));
    }

    #[test]
    fn card_lists_up_to_three_recent_activities_and_escapes_markdown() {
        let mut i = input();
        i.resume = (0..5)
            .map(|n| ResumeEntry { sample: sample("code.exe", &format!("file{n}[x].rs"), None), last_seen: i.now - 600 * (n + 1) })
            .collect();
        let b = build(&i).unwrap();
        assert_eq!(b.card_md.matches("- in VS Code").count(), 3, "{}", b.card_md);
        assert!(b.card_md.contains("file0\\[x\\].rs"), "{}", b.card_md);
    }

    #[test]
    fn long_titles_are_clipped_for_speech() {
        let mut i = input();
        i.resume = vec![ResumeEntry { sample: sample("code.exe", &"x".repeat(300), None), last_seen: i.now - 600 }];
        assert!(build(&i).unwrap().spoken.chars().count() < 200);
    }

    #[test]
    fn triaged_mail_and_calendar_make_the_briefing() {
        let mut i = input();
        i.mail = vec![MailItem {
            sender: "Supabase".into(),
            subject: "Your project will be paused".into(),
            deadline: None,
            category: Some("database".into()),
        }];
        i.events = vec![
            AgendaItem { summary: "Dentist".into(), all_day: false, start_epoch: 0, end_epoch: 0, when_label: "3 PM".into() },
            AgendaItem { summary: "Team sync".into(), all_day: false, start_epoch: 0, end_epoch: 0, when_label: "5 PM".into() },
        ];
        let b = build(&i).unwrap();
        assert!(b.spoken.contains("You have 2 events today; the first is Dentist at 3 PM."), "{}", b.spoken);
        assert!(b.spoken.contains("a database service warning from Supabase"), "{}", b.spoken);
        assert!(b.card_md.contains("### Today's calendar") && b.card_md.contains("Team sync"), "{}", b.card_md);
        // A calendar alone is enough to say something.
        let mut only = input();
        only.events = i.events.clone();
        assert!(build(&only).is_some());
    }

    #[test]
    fn important_mail_is_recent_ranked_and_medium_plus() {
        use crate::google::types::AlertUrgency;
        use super::super::mailtriage::{to_stored, MailMeta, Triage};
        let dir = std::env::temp_dir().join(format!("nexus_brief_mail_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let now = chrono::Utc::now().timestamp();
        let mk = |id: &str, subject: &str, cat: Category, u: AlertUrgency, age_h: i64| {
            let m = MailMeta {
                id: id.into(),
                thread_id: "t".into(),
                from_name: "Sender".into(),
                from_email: "s@x.com".into(),
                subject: subject.into(),
                snippet: String::new(),
                labels: vec![],
                date_ms: (now - age_h * 3600) * 1000,
            };
            (Triage { category: cat, urgency: u, reason: "r".into() }, m)
        };
        super::super::with_store(&dir, |s| {
            for (t, m) in [
                mk("1", "Medium recent", Category::Github, AlertUrgency::Medium, 1),
                mk("2", "High older", Category::Exam, AlertUrgency::High, 5),
                mk("3", "Too old", Category::Exam, AlertUrgency::High, 24 * 5),
                mk("4", "Low", Category::Github, AlertUrgency::Low, 1),
            ] {
                s.observe(
                    &super::super::store::Observation {
                        tier: Tier::Mail,
                        key: format!("mail:{}", m.id),
                        value: serde_json::to_string(&to_stored(&t, &m, "me")).unwrap(),
                        source: "gmail:me".into(),
                        trust: super::super::store::Trust::Untrusted,
                        pinned: false,
                    },
                    now,
                )
                .unwrap();
            }
        });
        let got = important_mail(&dir, now);
        assert_eq!(got.iter().map(|m| m.subject.as_str()).collect::<Vec<_>>(), vec!["High older", "Medium recent"]);
        assert_eq!(got[0].category.as_deref(), Some("exam"));
    }

    #[test]
    fn exactly_three_hellos_by_local_hour() {
        for (h, want) in [
            (5, "Good morning, sir."), (11, "Good morning, sir."),
            (12, "Good afternoon, sir."), (16, "Good afternoon, sir."),
            (17, "Good evening, sir."), (23, "Good evening, sir."), (0, "Good evening, sir."), (4, "Good evening, sir."),
        ] {
            assert_eq!(hello_line(h, Some("Lakshya"), false), want, "hour {h}");
        }
        assert_eq!(hello_line(9, Some("Lakshya"), true), "Good morning, Lakshya.");
    }

    #[test]
    fn a_hello_is_due_on_the_first_start_and_after_the_gap_only() {
        let now = 1_000_000;
        assert!(hello_due(None, now));
        assert!(!hello_due(Some(now - 60), now));
        assert!(!hello_due(Some(now - HELLO_MIN_GAP_SECS + 1), now));
        assert!(hello_due(Some(now - HELLO_MIN_GAP_SECS), now));
        assert!(hello_due(Some(now + 500), now), "a timestamp from the future (clock change) must not mute hellos forever");
    }

    #[test]
    fn the_briefing_drops_its_greeting_once_a_hello_was_spoken() {
        let mut i = input();
        i.mail = vec![MailItem { sender: "GitHub".into(), subject: "Run failed".into(), deadline: None, category: None }];
        let b = build(&i).unwrap();
        let g = greeting(i.hour, i.name.as_deref(), i.friend);
        assert!(b.spoken.starts_with(&g));
        let rest = without_greeting(&b.spoken, i.hour, i.name.as_deref(), i.friend);
        assert!(!rest.starts_with("Good"), "{rest}");
        assert_eq!(format!("{g} {rest}"), b.spoken);
    }

    #[test]
    fn presence_gate() {
        assert!(user_present(Some(5)));
        assert!(user_present(Some(59)));
        assert!(user_present(Some(299)), "a user who typed a password a few minutes ago is present");
        assert!(!user_present(Some(300)));
        assert!(!user_present(Some(3600)));
        assert!(user_present(None), "unknown idle time must not block the briefing forever");
    }

    #[test]
    fn ago_phrases() {
        assert_eq!(ago_phrase(1000, 990), "just now");
        assert_eq!(ago_phrase(10_000, 10_000 - 600), "about 10 minutes ago");
        assert_eq!(ago_phrase(100_000, 100_000 - 3600), "about an hour ago");
        assert_eq!(ago_phrase(200_000, 200_000 - 90_000), "yesterday");
        assert_eq!(ago_phrase(1_000_000, 1_000_000 - 3 * 86_400), "3 days ago");
    }
}

#[cfg(test)]
mod greeting_text_tests {
    use super::*;

    fn sample(app: &str, title: &str) -> Sample {
        Sample {
            app: app.into(),
            title: title.into(),
            url: None,
            activity: None,
        }
    }

    #[test]
    fn a_title_that_only_repeats_the_app_name_is_dropped() {
        // The 2026-10-10 boot line: "You were in Opencode on “OpenCode”".
        assert_eq!(describe(&sample("OpenCode.exe", "OpenCode")), "in OpenCode");
        assert_eq!(describe(&sample("notepad.exe", "Notepad")), "in Notepad");
        assert_eq!(describe(&sample("Claude.exe", "claude")), "in Claude");
    }

    #[test]
    fn informative_titles_are_kept() {
        assert_eq!(
            describe(&sample("code.exe", "main.rs — ULTRON")),
            "in VS Code on “main.rs — ULTRON”"
        );
    }

    #[test]
    fn friendly_names_are_proper_cased() {
        assert_eq!(resume::friendly_app("opencode.exe"), "OpenCode");
        assert_eq!(resume::friendly_app("devenv.exe"), "Visual Studio");
        assert_eq!(resume::friendly_app("someapp.exe"), "Someapp");
    }
}

#[cfg(test)]
mod session_greeting_tests {
    use super::*;

    fn base() -> Input {
        Input {
            now: 1_000_000,
            hour: 19,
            name: None,
            friend: false,
            resume: vec![],
            mail: vec![],
            unfinished: vec![],
            events: vec![],
            session: None,
            health_spoken: None,
            health_card: None,
        }
    }

    #[test]
    fn the_last_session_replaces_the_single_window_line_and_ends_with_the_question() {
        let mut i = base();
        i.resume = vec![ResumeEntry {
            sample: Sample { app: "opencode.exe".into(), title: "OpenCode".into(), url: None, activity: None },
            last_seen: i.now - 79_200,
        }];
        i.session = Some(SessionOffer { summary: "VS Code on ULTRON, Brave and 2 folders".into(), left_at: i.now - 79_200 });
        let b = build(&i).unwrap();
        assert!(b.offers_restore);
        assert!(b.spoken.starts_with("Good evening, sir. You left VS Code on ULTRON, Brave and 2 folders open"), "{}", b.spoken);
        assert!(!b.spoken.contains("Welcome back"), "the time-of-day greeting already covers this: {}", b.spoken);
        assert!(!b.spoken.contains("You were in"), "the one-window line is redundant: {}", b.spoken);
        assert!(b.spoken.ends_with("Shall I reopen them?"), "{}", b.spoken);
        assert!(b.card_md.contains("### Open when you left"), "{}", b.card_md);
    }

    #[test]
    fn health_problems_come_before_the_question_and_can_make_the_greeting_alone() {
        let mut i = base();
        i.health_spoken = Some("Heads up: the WhatsApp bridge isn't running.".into());
        i.health_card = Some("### Connections
- WhatsApp bridge is not running on port 8765.

".into());
        let b = build(&i).expect("a connection problem alone is worth a sentence");
        assert!(!b.offers_restore);
        assert!(b.spoken.contains("Heads up: the WhatsApp bridge isn't running."), "{}", b.spoken);
        assert!(b.card_md.contains("### Connections"), "{}", b.card_md);

        i.session = Some(SessionOffer { summary: "Brave".into(), left_at: i.now - 3600 });
        let b2 = build(&i).unwrap();
        let q = b2.spoken.find("Shall I reopen them?").unwrap();
        assert!(b2.spoken.find("Heads up").unwrap() < q, "{}", b2.spoken);
    }

    #[test]
    fn nothing_to_say_is_still_silence() {
        assert!(build(&base()).is_none());
    }
}
