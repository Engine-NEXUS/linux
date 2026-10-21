//! Verified "open <name>'s chat in WhatsApp" for Ghost mode.
//!
//! The old drill pressed Ctrl+F, typed the name, pressed Enter and announced "X is open" without
//! checking anything: it typed into whatever window had focus (WhatsApp is a Store app that often
//! sits hidden in the tray) and claimed success on a miss. This module is the safe replacement:
//!
//!   1. the FOREGROUND PROCESS must be WhatsApp before any key is sent (otherwise `WrongWindow`);
//!   2. Ctrl+F, type the candidate name, then wait for a matching chat-list item via UI Automation
//!      (proof the contact exists) - polling, not a fixed sleep;
//!   3. click that item (no reliance on Enter selecting the first result);
//!   4. on a miss, try the next candidate (learned alias first, then kinship synonyms), and only
//!      then report `NotFound`.
//!
//! All decisions are pure and run against the [`WaEnv`] trait, so the whole flow is unit-tested
//! with a fake environment; [`RealEnv`] wires it to the OS.

/// One chat-list entry as seen through UI Automation.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatItem {
    pub name: String,
    pub cx: i32,
    pub cy: i32,
}

/// Result of an open attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum WaOpen {
    /// The chat was found and clicked; `matched` is the list item's accessible name.
    Opened { matched: String },
    /// Searched every candidate name; none showed up in the chat list.
    NotFound,
    /// WhatsApp is not the foreground window - nothing was typed.
    WrongWindow,
    /// A stop/cancel arrived mid-flow.
    Stopped,
    /// A key/click primitive failed.
    Failed(String),
    /// A chat was opened by keyboard but its header could not be read to confirm it is `guess`
    /// (no OCR, unreadable). Nothing may be typed into it.
    Unverified { guess: String },
}

/// Everything the flow needs from the OS (fake in tests).
pub trait WaEnv {
    /// Lowercase exe name of the foreground window ("whatsapp.root.exe").
    fn foreground_process(&self) -> Option<String>;
    fn hotkey(&self, keys: &[&str]) -> Result<(), String>;
    fn key(&self, key: &str) -> Result<(), String>;
    fn type_text(&self, text: &str) -> Result<(), String>;
    fn sleep_ms(&self, ms: u64);
    /// Chat-list items currently visible in the foreground window.
    fn list_items(&self) -> Vec<ChatItem>;
    fn click(&self, x: i32, y: i32) -> Result<(), String>;
    fn stop_requested(&self) -> bool;
    /// Monotonic milliseconds (real clock). Fakes keep 0: their time is the sum of their sleeps.
    fn now_ms(&self) -> u64 {
        0
    }
    /// UI Automation already proved blind to WhatsApp's chat list in this session (skip it).
    fn uia_blind(&self) -> bool {
        false
    }
    fn note_uia_blind(&self) {}
    fn note_uia_ok(&self) {}
    /// OCR text of the open chat's header band (None = cannot read it).
    fn header_text(&self) -> Option<String> {
        None
    }
}

/// True when the foreground process is the WhatsApp desktop app (Store build: WhatsApp.Root.exe,
/// older builds WhatsApp.exe). A browser tab titled "WhatsApp" is deliberately NOT accepted - the
/// keyboard flow is only validated against the desktop app.
pub fn is_whatsapp_process(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n == "whatsapp.exe" || n == "whatsapp.root.exe" || n.starts_with("whatsapp.")
}

const KIN_GROUPS: &[&[&str]] = &[
    // Most common saved names first: the search stops after MAX_CANDIDATES.
    &["mummy", "amma", "mom", "mommy", "mami", "mamma", "mama", "mum", "ammi", "mother"],
    &["daddy", "dad", "papa", "pappa", "appa", "abbu", "father"],
    &["didi", "akka", "sister", "sis"],
    &["bhai", "anna", "brother", "bro"],
];

/// True when `word` is a kinship word ("mummy", "dad", "didi"...) that WhatsApp users
/// save contacts under. Used by the parser so "open mummy chat" needs no WhatsApp tail.
pub fn is_kin_word(word: &str) -> bool {
    let w = word.trim().to_lowercase();
    KIN_GROUPS.iter().any(|g| g.contains(&w.as_str()))
}

/// Candidate search strings, best first: learned aliases, the spoken name, then the other words of
/// its kinship group (so "mummy" also tries "amma"/"mom"). Deduplicated, lowercase.
/// A miss costs ~2 s per candidate; ten of them made "not found" take 20-30 s.
pub const MAX_CANDIDATES: usize = 3;

pub fn name_candidates(contact: &str, learned: &[String]) -> Vec<String> {
    let spoken = contact.trim().to_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().to_lowercase();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    for l in learned {
        push(l);
    }
    push(&spoken);
    if let Some(group) = KIN_GROUPS.iter().find(|g| g.contains(&spoken.as_str())) {
        for w in group.iter() {
            push(w);
        }
    }
    out.truncate(MAX_CANDIDATES);
    out
}

/// Pick the chat-list item that best matches `want`: every token (2+ chars) of `want` must appear
/// in the item's name; exact > starts-with > contains. Pure.
pub fn pick_match<'a>(items: &'a [ChatItem], want: &str) -> Option<&'a ChatItem> {
    let want = want.trim().to_lowercase();
    let tokens: Vec<&str> = want.split_whitespace().filter(|t| t.len() >= 2).collect();
    if tokens.is_empty() {
        return None;
    }
    let mut best: Option<(u8, &ChatItem)> = None;
    for it in items {
        let name = it.name.to_lowercase();
        if name.is_empty() || !tokens.iter().all(|t| name.contains(t)) {
            continue;
        }
        let score = if name == want {
            3
        } else if name.starts_with(&want) {
            2
        } else {
            1
        };
        if best.map(|(s, _)| score > s).unwrap_or(true) {
            best = Some((score, it));
        }
    }
    best.map(|(_, i)| i)
}

/// Total time one "open the chat" attempt may spend (sleeps + UI Automation calls). The old loop
/// counted polls, not time: with the chat list unreadable every poll blocked 2.5 s, so one name
/// could take about a minute (2026-10-15 log: 57 s, then "WhatsApp lost focus").
pub const TOTAL_BUDGET_MS: u64 = 9_000;
/// Per name: how long to wait for the chat list to show a match.
pub const CANDIDATE_BUDGET_MS: u64 = 3_500;
/// A chat-list read slower than this that finds nothing means UI Automation cannot see the list.
pub const SLOW_LIST_MS: u64 = 1_000;

/// Does the OCR text of a chat header contain one of the wanted names (every token of it)? Pure.
pub fn header_matches(ocr_text: &str, wanted: &[&str]) -> bool {
    let norm = |t: &str| -> String {
        t.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect::<String>()
    };
    let hay = norm(ocr_text);
    wanted.iter().any(|w| {
        let toks: Vec<String> = norm(w).split_whitespace().filter(|t| t.chars().count() >= 2).map(String::from).collect();
        !toks.is_empty() && toks.iter().all(|t| hay.contains(t.as_str()))
    })
}

/// Run the verified open. Never sends a key unless WhatsApp is the foreground process.
///
/// Rungs, each time-boxed (whole attempt <= `TOTAL_BUDGET_MS`):
///   1. UI Automation chat list: type the name, find the matching item, click it (verified by name);
///   2. keyboard only (no UI Automation): type the name, Down, Enter, then READ the chat header with
///      on-device OCR to confirm it is the right person. Used when UI Automation cannot see the list
///      (checked once, BEFORE typing: an unfiltered list always has items) and remembered for the session.
pub fn open_chat(env: &dyn WaEnv, contact: &str, learned: &[String]) -> WaOpen {
    let candidates = name_candidates(contact, learned);
    if candidates.is_empty() {
        return WaOpen::NotFound;
    }
    let mut wanted: Vec<&str> = candidates.iter().map(|c| c.as_str()).collect();
    wanted.push(contact);
    let mut spent: u64 = 0;
    let mut blind = env.uia_blind();
    let mut baseline_checked = blind;
    for (i, cand) in candidates.iter().enumerate() {
        if env.stop_requested() {
            return WaOpen::Stopped;
        }
        let fg_ok = env
            .foreground_process()
            .map(|p| is_whatsapp_process(&p))
            .unwrap_or(false);
        if !fg_ok {
            return WaOpen::WrongWindow;
        }
        if spent >= TOTAL_BUDGET_MS {
            break;
        }
        if let Err(e) = env.hotkey(&["ctrl", "f"]) {
            return WaOpen::Failed(e);
        }
        env.sleep_ms(450);
        spent += 450;
        if i > 0 {
            // The previous query is still in the box: replace it.
            if let Err(e) = env.hotkey(&["ctrl", "a"]) {
                return WaOpen::Failed(e);
            }
        }
        if !baseline_checked {
            // Can UI Automation see the chat list at all? An unfiltered list always has items, so an
            // empty read here means "blind", not "no such contact".
            baseline_checked = true;
            let t0 = env.now_ms();
            let base = env.list_items();
            spent += env.now_ms().saturating_sub(t0);
            if base.is_empty() {
                blind = true;
                env.note_uia_blind();
            } else {
                env.note_uia_ok();
            }
        }
        if let Err(e) = env.type_text(cand) {
            return WaOpen::Failed(e);
        }
        if !blind {
            let started = spent;
            while spent.saturating_sub(started) < CANDIDATE_BUDGET_MS && spent < TOTAL_BUDGET_MS {
                env.sleep_ms(150);
                spent += 150;
                if env.stop_requested() {
                    return WaOpen::Stopped;
                }
                let t0 = env.now_ms();
                let items = env.list_items();
                let took = env.now_ms().saturating_sub(t0);
                spent += took;
                if let Some(hit) = pick_match(&items, cand) {
                    let matched = hit.name.clone();
                    if let Err(e) = env.click(hit.cx, hit.cy) {
                        return WaOpen::Failed(e);
                    }
                    env.sleep_ms(300);
                    return WaOpen::Opened { matched };
                }
                if items.is_empty() && took >= SLOW_LIST_MS {
                    // The list became unreadable mid-flow: stop wasting seconds on it.
                    blind = true;
                    env.note_uia_blind();
                    break;
                }
            }
        }
        if blind {
            // Keyboard rung: the result list needs a moment to filter, then Down selects the first
            // result and Enter opens it. The header is read back to confirm WHO it is.
            env.sleep_ms(600);
            if let Err(e) = env.key("down") {
                return WaOpen::Failed(e);
            }
            env.sleep_ms(150);
            if let Err(e) = env.key("enter") {
                return WaOpen::Failed(e);
            }
            env.sleep_ms(800);
            spent += 1_550;
            match env.header_text() {
                Some(t) if header_matches(&t, &wanted) => return WaOpen::Opened { matched: cand.clone() },
                Some(_) => continue, // some other chat opened: try the next name
                None => return WaOpen::Unverified { guess: cand.clone() },
            }
        }
    }
    // Leave the app tidy: close the search box.
    let _ = env.key("escape");
    WaOpen::NotFound
}

/// Short name for speech: the list item's accessible name is often
/// "Mummy, last message..., 9:41 PM, 2 unread" - speak only the part before the first comma.
pub fn spoken_name(item_name: &str) -> String {
    let first = item_name.split(',').next().unwrap_or(item_name).trim();
    if first.is_empty() {
        item_name.trim().to_string()
    } else {
        first.to_string()
    }
}

/// Learned alias for a spoken name, stored as the core fact `wa_alias_<name>`
/// ("remember that wa alias mummy is Amma"). Returns the stored saved-contact name(s).
pub fn learned_aliases(app_data_dir: &std::path::Path, contact: &str) -> Vec<String> {
    let key = format!("wa_alias_{}", crate::memory::normalize_key(contact));
    crate::memory::core_value(app_data_dir, &key).into_iter().collect()
}

/// Remember what a spoken name turned out to be saved as (`wa_alias_<name>` = "Mom"). Written after
/// every verified open, so NEXUS learns without being told; a name that already IS the saved name
/// (no difference) is not stored. Returns true when something was written.
pub fn learn_alias(app_data_dir: &std::path::Path, spoken: &str, saved_as: &str) -> bool {
    let saved = saved_as.trim();
    if saved.is_empty() || spoken.trim().eq_ignore_ascii_case(saved) {
        return false;
    }
    let key = format!("wa_alias_{}", crate::memory::normalize_key(spoken));
    if crate::memory::core_value(app_data_dir, &key).as_deref() == Some(saved) {
        return false;
    }
    crate::memory::remember(app_data_dir, &key, saved)
}

/// Saved phone number for a spoken name, stored as the core fact `wa_phone_<name>`
/// ("remember that wa phone mummy is 98xxxxxx"). Local only; never sent to a cloud model.
pub fn learned_phone(app_data_dir: &std::path::Path, contact: &str) -> Option<String> {
    let key = format!("wa_phone_{}", crate::memory::normalize_key(contact));
    crate::memory::core_value(app_data_dir, &key).and_then(|v| normalize_phone(&v))
}

/// Digits (optionally a leading `+`) of a saved number; 8-15 digits or `None`. Pure.
pub fn normalize_phone(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if !(8..=15).contains(&digits.len()) {
        return None;
    }
    Some(digits)
}

/// `whatsapp://send?phone=<digits>`: opens that chat directly, no search box, no UI scraping.
pub fn deep_link(phone_digits: &str) -> String {
    format!("whatsapp://send?phone={phone_digits}")
}

// ─────────────────────────────── real environment ───────────────────────────────

#[cfg(target_os = "windows")]
pub struct RealEnv;

#[cfg(target_os = "windows")]
impl WaEnv for RealEnv {
    fn foreground_process(&self) -> Option<String> {
        crate::browser_url::foreground_process_name()
    }
    fn hotkey(&self, keys: &[&str]) -> Result<(), String> {
        super::keyboard::press_hotkey(keys)
    }
    fn key(&self, key: &str) -> Result<(), String> {
        super::keyboard::press_key(key)
    }
    fn type_text(&self, text: &str) -> Result<(), String> {
        super::keyboard::type_text(text)
    }
    fn sleep_ms(&self, ms: u64) {
        crate::keep_awake::heartbeat(); // a drill that waits is alive (the 10 s watchdog cleared the lease mid-drill)
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    fn list_items(&self) -> Vec<ChatItem> {
        crate::screen::list_actionables()
            .into_iter()
            .filter(|e| e.kind == "ListItem")
            .map(|e| ChatItem {
                cx: e.x + e.w / 2,
                cy: e.y + e.h / 2,
                name: e.name,
            })
            .collect()
    }
    fn click(&self, x: i32, y: i32) -> Result<(), String> {
        super::mouse::click_at(x, y, crate::ghost::stop_requested)
    }
    fn stop_requested(&self) -> bool {
        crate::ghost::stop_requested() || !crate::ghost::session_active()
    }
    fn now_ms(&self) -> u64 {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
    fn uia_blind(&self) -> bool {
        UIA_BLIND_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed) > self.now_ms()
    }
    fn note_uia_blind(&self) {
        // Skip UI Automation for WhatsApp for 10 minutes: every blind read costs its 2.5 s budget.
        UIA_BLIND_UNTIL_MS.store(self.now_ms() + 10 * 60_000, std::sync::atomic::Ordering::Relaxed);
        tracing::warn!("[WA] UI Automation cannot read WhatsApp's chat list - using the keyboard route for 10 min");
    }
    fn note_uia_ok(&self) {
        UIA_BLIND_UNTIL_MS.store(0, std::sync::atomic::Ordering::Relaxed);
    }
    fn header_text(&self) -> Option<String> {
        // The open chat's header is the top band of the right-hand pane (the list takes the left ~30 %).
        let (x, y, w, h) = crate::browser_url::foreground_window_rect()?;
        let left = x + w * 30 / 100;
        let band_h = (h / 6).clamp(90, 220);
        crate::ocr::capture_region_text(left, y, w - w * 30 / 100, band_h)
    }
}

/// Until when (ms on `RealEnv::now_ms`) UI Automation is skipped for WhatsApp.
#[cfg(target_os = "windows")]
static UIA_BLIND_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Bring the WhatsApp desktop app to the foreground (launch it if it only lives in the tray).
/// Returns true only when the FOREGROUND PROCESS is WhatsApp - a window title match is not enough.
#[cfg(target_os = "windows")]
pub fn ensure_whatsapp_foreground(max_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let mut last_launch: Option<std::time::Instant> = None;
    while (start.elapsed().as_millis() as u64) < max_ms {
        if crate::browser_url::foreground_process_name()
            .map(|p| is_whatsapp_process(&p))
            .unwrap_or(false)
        {
            return true;
        }
        // (Re)activate through the registry (focus-or-launch): a hidden tray instance is shown by
        // the Store activation; re-issue it every 2.5 s while we wait.
        let due = last_launch.map(|t| t.elapsed().as_millis() > 2500).unwrap_or(true);
        if due {
            let _ = crate::command_executor::resolve_and_open_app("WhatsApp");
            last_launch = Some(std::time::Instant::now());
        }
        let _ = super::window::focus_app_by_title("WhatsApp");
        crate::keep_awake::heartbeat();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    crate::browser_url::foreground_process_name()
        .map(|p| is_whatsapp_process(&p))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        fg: RefCell<Option<String>>,
        /// items shown after the Nth typed query (by query text), else empty
        results: Vec<(String, Vec<ChatItem>)>,
        typed: RefCell<Vec<String>>,
        keys: RefCell<Vec<String>>,
        clicks: RefCell<Vec<(i32, i32)>>,
        current_query: RefCell<String>,
        stop: bool,
    }

    impl Fake {
        fn new(fg: Option<&str>, results: Vec<(&str, Vec<ChatItem>)>) -> Self {
            Fake {
                fg: RefCell::new(fg.map(|s| s.to_string())),
                results: results.into_iter().map(|(q, v)| (q.to_string(), v)).collect(),
                typed: RefCell::new(vec![]),
                keys: RefCell::new(vec![]),
                clicks: RefCell::new(vec![]),
                current_query: RefCell::new(String::new()),
                stop: false,
            }
        }
    }

    impl WaEnv for Fake {
        fn foreground_process(&self) -> Option<String> {
            self.fg.borrow().clone()
        }
        fn hotkey(&self, keys: &[&str]) -> Result<(), String> {
            self.keys.borrow_mut().push(keys.join("+"));
            Ok(())
        }
        fn key(&self, key: &str) -> Result<(), String> {
            self.keys.borrow_mut().push(key.to_string());
            Ok(())
        }
        fn type_text(&self, text: &str) -> Result<(), String> {
            self.typed.borrow_mut().push(text.to_string());
            *self.current_query.borrow_mut() = text.to_string();
            Ok(())
        }
        fn sleep_ms(&self, _ms: u64) {}
        fn list_items(&self) -> Vec<ChatItem> {
            let q = self.current_query.borrow().clone();
            if q.is_empty() {
                // The unfiltered chat list: everything the fake knows.
                return self.results.iter().flat_map(|(_, v)| v.clone()).collect();
            }
            self.results
                .iter()
                .find(|(k, _)| *k == q)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        }
        fn click(&self, x: i32, y: i32) -> Result<(), String> {
            self.clicks.borrow_mut().push((x, y));
            Ok(())
        }
        fn stop_requested(&self) -> bool {
            self.stop
        }
    }

    fn item(name: &str, x: i32, y: i32) -> ChatItem {
        ChatItem { name: name.to_string(), cx: x, cy: y }
    }

    #[test]
    fn nothing_is_typed_unless_whatsapp_is_foreground() {
        for fg in [None, Some("code.exe"), Some("brave.exe"), Some("notepad.exe")] {
            let env = Fake::new(fg, vec![("mummy", vec![item("Mummy", 10, 10)])]);
            assert_eq!(open_chat(&env, "mummy", &[]), WaOpen::WrongWindow);
            assert!(env.typed.borrow().is_empty(), "typed into {fg:?}");
            assert!(env.keys.borrow().is_empty(), "keys sent to {fg:?}");
            assert!(env.clicks.borrow().is_empty());
        }
    }

    #[test]
    fn opens_the_matching_chat_by_clicking_it() {
        let env = Fake::new(
            Some("whatsapp.root.exe"),
            vec![("mummy", vec![item("Raj, hi, 9:00 PM", 5, 5), item("Mummy, ok, 9:41 PM, 2 unread", 100, 220)])],
        );
        let r = open_chat(&env, "mummy", &[]);
        assert_eq!(r, WaOpen::Opened { matched: "Mummy, ok, 9:41 PM, 2 unread".into() });
        assert_eq!(*env.clicks.borrow(), vec![(100, 220)]);
        assert_eq!(env.keys.borrow()[0], "ctrl+f");
        assert!(!env.keys.borrow().contains(&"enter".to_string()), "must click, not press Enter blindly");
    }

    #[test]
    fn falls_back_to_a_kinship_synonym_then_reports_the_match() {
        let env = Fake::new(
            Some("whatsapp.root.exe"),
            vec![("amma", vec![item("Amma", 50, 60)])],
        );
        let r = open_chat(&env, "mummy", &[]);
        assert_eq!(r, WaOpen::Opened { matched: "Amma".into() });
        // The second query replaced the first (Ctrl+A) instead of appending to it.
        assert!(env.keys.borrow().contains(&"ctrl+a".to_string()));
    }

    #[test]
    fn learned_alias_is_tried_first() {
        let env = Fake::new(
            Some("whatsapp.root.exe"),
            vec![("amma jaan", vec![item("Amma Jaan", 1, 2)]), ("mummy", vec![item("Mummy Dearest", 3, 4)])],
        );
        let r = open_chat(&env, "mummy", &["Amma Jaan".to_string()]);
        assert_eq!(r, WaOpen::Opened { matched: "Amma Jaan".into() });
        assert_eq!(env.typed.borrow()[0], "amma jaan");
    }

    #[test]
    fn unknown_contact_is_reported_honestly_and_never_clicks() {
        // The chat list is readable (other chats exist) but nobody matches: NotFound, no Enter, no click.
        let env = Fake::new(Some("whatsapp.root.exe"), vec![("raj", vec![item("Raj", 1, 1)])]);
        assert_eq!(open_chat(&env, "zorblax", &[]), WaOpen::NotFound);
        assert!(env.clicks.borrow().is_empty());
        assert!(env.keys.borrow().contains(&"escape".to_string()), "search box left open");
        assert!(!env.keys.borrow().contains(&"enter".to_string()));
    }

    #[test]
    fn a_stop_aborts_before_any_key() {
        let mut env = Fake::new(Some("whatsapp.root.exe"), vec![]);
        env.stop = true;
        assert_eq!(open_chat(&env, "mummy", &[]), WaOpen::Stopped);
        assert!(env.keys.borrow().is_empty());
    }

    #[test]
    fn process_check_accepts_only_the_desktop_app() {
        assert!(is_whatsapp_process("WhatsApp.Root.exe"));
        assert!(is_whatsapp_process("whatsapp.exe"));
        assert!(!is_whatsapp_process("brave.exe"), "a WhatsApp Web tab is not the desktop app");
        assert!(!is_whatsapp_process("code.exe"));
        assert!(!is_whatsapp_process(""));
    }

    #[test]
    fn candidates_are_ordered_and_deduplicated() {
        let c = name_candidates("Mummy", &["Amma Jaan".into(), "mummy".into()]);
        assert_eq!(c[0], "amma jaan");
        assert_eq!(c[1], "mummy");
        assert!(c.contains(&"amma".to_string()));
        assert_eq!(c.iter().filter(|x| x.as_str() == "mummy").count(), 1);
        // A real name is never expanded.
        assert_eq!(name_candidates("Chris", &[]), vec!["chris".to_string()]);
        assert_eq!(name_candidates("thanmatee akka", &[]), vec!["thanmatee akka".to_string()]);
    }

    #[test]
    fn matching_requires_every_token_and_prefers_exact() {
        let items = vec![
            item("Thanmatee, hello", 1, 1),
            item("Thanmatee Akka", 2, 2),
            item("Akka Store", 3, 3),
        ];
        let hit = pick_match(&items, "thanmatee akka").unwrap();
        assert_eq!(hit.name, "Thanmatee Akka");
        assert!(pick_match(&items, "someone else").is_none());
        assert!(pick_match(&items, "").is_none());
    }

    #[test]
    fn spoken_name_drops_the_message_preview() {
        assert_eq!(spoken_name("Mummy, ok see you, 9:41 PM, 2 unread"), "Mummy");
        assert_eq!(spoken_name("Amma"), "Amma");
    }

    #[test]
    fn a_miss_tries_at_most_three_names() {
        let c = name_candidates("mummy", &["amma jaan".into()]);
        assert!(c.len() <= MAX_CANDIDATES, "{c:?}");
        assert_eq!(c[0], "amma jaan");
        assert_eq!(c[1], "mummy");
        let none = name_candidates("mummy", &[]);
        assert_eq!(none.len(), MAX_CANDIDATES);
    }

    #[test]
    fn phone_numbers_are_normalised_or_rejected() {
        assert_eq!(normalize_phone("+91 98765 43210").as_deref(), Some("919876543210"));
        assert_eq!(normalize_phone("98765-43210").as_deref(), Some("9876543210"));
        assert_eq!(normalize_phone("12345"), None);
        assert_eq!(normalize_phone("not a number"), None);
        assert_eq!(normalize_phone("1234567890123456"), None);
        assert_eq!(deep_link("919876543210"), "whatsapp://send?phone=919876543210");
    }

    // ── Field test 2026-10-15: UI Automation could not read WhatsApp's chat list ──────────────────

    /// A WhatsApp whose chat list UI Automation cannot read (the real one in the log), with a header OCR.
    struct Blind {
        fg: Option<String>,
        header: Option<String>,
        keys: RefCell<Vec<String>>,
        typed: RefCell<Vec<String>>,
        clicks: RefCell<Vec<(i32, i32)>>,
        list_calls: RefCell<u32>,
        clock: RefCell<u64>,
        blind_flag: RefCell<bool>,
        /// Each list read costs this long (the 2.5 s UI Automation budget in the real log).
        list_cost_ms: u64,
    }

    impl Blind {
        fn new(header: Option<&str>) -> Self {
            Blind {
                fg: Some("whatsapp.root.exe".into()),
                header: header.map(String::from),
                keys: RefCell::new(vec![]),
                typed: RefCell::new(vec![]),
                clicks: RefCell::new(vec![]),
                list_calls: RefCell::new(0),
                clock: RefCell::new(0),
                blind_flag: RefCell::new(false),
                list_cost_ms: 2_500,
            }
        }
    }

    impl WaEnv for Blind {
        fn foreground_process(&self) -> Option<String> {
            self.fg.clone()
        }
        fn hotkey(&self, keys: &[&str]) -> Result<(), String> {
            self.keys.borrow_mut().push(keys.join("+"));
            Ok(())
        }
        fn key(&self, key: &str) -> Result<(), String> {
            self.keys.borrow_mut().push(key.to_string());
            Ok(())
        }
        fn type_text(&self, text: &str) -> Result<(), String> {
            self.typed.borrow_mut().push(text.to_string());
            Ok(())
        }
        fn sleep_ms(&self, ms: u64) {
            *self.clock.borrow_mut() += ms;
        }
        fn list_items(&self) -> Vec<ChatItem> {
            *self.list_calls.borrow_mut() += 1;
            *self.clock.borrow_mut() += self.list_cost_ms;
            vec![] // UI Automation times out and returns nothing
        }
        fn click(&self, x: i32, y: i32) -> Result<(), String> {
            self.clicks.borrow_mut().push((x, y));
            Ok(())
        }
        fn stop_requested(&self) -> bool {
            false
        }
        fn now_ms(&self) -> u64 {
            *self.clock.borrow()
        }
        fn uia_blind(&self) -> bool {
            *self.blind_flag.borrow()
        }
        fn note_uia_blind(&self) {
            *self.blind_flag.borrow_mut() = true;
        }
        fn header_text(&self) -> Option<String> {
            self.header.clone()
        }
    }

    #[test]
    fn an_unreadable_chat_list_costs_one_read_not_a_minute_and_the_keyboard_route_opens_the_chat() {
        let env = Blind::new(Some("Mummy  online  Search  Video call"));
        let r = open_chat(&env, "mummy", &[]);
        assert_eq!(r, WaOpen::Opened { matched: "mummy".into() });
        assert_eq!(*env.list_calls.borrow(), 1, "ONE blind read (the baseline), never a poll loop of 2.5 s reads");
        assert!(*env.clock.borrow() < 6_000, "whole attempt well under 6 s, was {} ms", env.clock.borrow());
        assert!(env.clicks.borrow().is_empty(), "no UI Automation click without a list");
        let keys = env.keys.borrow().clone();
        assert_eq!(keys, vec!["ctrl+f", "down", "enter"], "{keys:?}");
        assert!(*env.blind_flag.borrow(), "the session remembers UI Automation is blind");
    }

    #[test]
    fn once_blind_later_attempts_skip_ui_automation_entirely() {
        let env = Blind::new(Some("Amma"));
        *env.blind_flag.borrow_mut() = true;
        let r = open_chat(&env, "mummy", &["Amma".to_string()]);
        assert_eq!(r, WaOpen::Opened { matched: "amma".into() });
        assert_eq!(*env.list_calls.borrow(), 0, "no 2.5 s UI Automation read at all");
    }

    #[test]
    fn a_header_that_is_someone_else_is_never_confirmed() {
        let env = Blind::new(Some("Raj Kumar  last seen today"));
        let r = open_chat(&env, "mummy", &[]);
        assert_eq!(r, WaOpen::NotFound, "wrong chat must not be reported as opened");
    }

    #[test]
    fn when_the_header_cannot_be_read_the_result_is_unverified_not_opened() {
        let env = Blind::new(None);
        let r = open_chat(&env, "mummy", &[]);
        assert!(matches!(r, WaOpen::Unverified { .. }), "{r:?}");
    }

    #[test]
    fn the_total_time_is_bounded_even_when_every_name_misses() {
        let env = Blind::new(Some("Someone Else"));
        let _ = open_chat(&env, "mummy", &["amma jaan".to_string()]);
        assert!(*env.clock.borrow() <= TOTAL_BUDGET_MS + 3_000, "bounded, was {} ms", env.clock.borrow());
    }

    #[test]
    fn header_matching_needs_every_token_and_ignores_punctuation() {
        assert!(header_matches("Mummy ❤  online", &["mummy"]));
        assert!(header_matches("THANMATEE  AKKA", &["thanmatee akka"]));
        assert!(!header_matches("Thanmatee", &["thanmatee akka"]), "both tokens required");
        assert!(header_matches("Amma Jaan", &["mummy", "amma"]), "any wanted name may match");
        assert!(!header_matches("", &["mummy"]));
        assert!(!header_matches("Raj", &[]));
        assert!(!header_matches("Raj", &[""]));
    }

    #[test]
    fn a_verified_open_teaches_the_saved_name_once() {
        let dir = std::env::temp_dir().join(format!("nexus_wa_learn_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(learned_aliases(&dir, "mummy").is_empty());
        assert!(learn_alias(&dir, "mummy", "Mom"), "first time: written");
        assert_eq!(learned_aliases(&dir, "mummy"), vec!["Mom".to_string()]);
        assert!(!learn_alias(&dir, "mummy", "Mom"), "same answer again: nothing to write");
        assert!(!learn_alias(&dir, "Chris", "chris"), "already the saved name: nothing to learn");
        assert!(!learn_alias(&dir, "mummy", "  "), "empty names are ignored");
        // and the next open tries the learned name FIRST
        let c = name_candidates("mummy", &learned_aliases(&dir, "mummy"));
        assert_eq!(c[0], "mom");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
