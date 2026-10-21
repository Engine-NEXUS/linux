//! Calendar helpers (plan P5): spoken agenda, and turning "add dentist to my
//! calendar tomorrow at 5pm" into an event. Pure and unit-tested; the HTTP
//! calls live in `google_io.rs`.
//!
//! Creating an event is never silent: the draft is read back and only saved
//! after a yes (the same offer mechanism the timetable uses).

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};

use crate::google::types::{CalendarEvent, NewEvent};

use super::timetable;

#[derive(Debug, Clone, PartialEq)]
pub struct AgendaItem {
    pub summary: String,
    pub all_day: bool,
    pub start_epoch: i64,
    pub end_epoch: i64,
    /// "3 PM", "5:30 PM" or "all day".
    pub when_label: String,
}

fn fmt_local_time(dt: &DateTime<Local>) -> String {
    timetable::fmt_time((dt.hour() * 60 + dt.minute()) as u16)
}

/// Events (already sorted by Google) → display items in local time.
pub fn items_from_events(events: &[CalendarEvent]) -> Vec<AgendaItem> {
    events
        .iter()
        .filter(|e| e.status != "cancelled")
        .filter_map(|e| {
            let all_day = e.start_iso.len() == 10;
            let start = crate::google::calendar::CalendarService::parse_iso_to_epoch(&e.start_iso)?;
            let end = crate::google::calendar::CalendarService::parse_iso_to_epoch(&e.end_iso).unwrap_or(start);
            let when_label = if all_day {
                "all day".to_string()
            } else {
                fmt_local_time(&Local.timestamp_opt(start, 0).single()?)
            };
            Some(AgendaItem {
                summary: e.summary.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect(),
                all_day,
                start_epoch: start,
                end_epoch: end,
                when_label,
            })
        })
        .collect()
}

/// RFC 3339 bounds (local midnight → next local midnight) for today + offset.
pub fn day_bounds(now: DateTime<Local>, day_offset: i64) -> (String, String) {
    let date = now.date_naive() + Duration::days(day_offset);
    let start = local_midnight(date);
    let end = local_midnight(date + Duration::days(1));
    (start.to_rfc3339(), end.to_rfc3339())
}

fn local_midnight(date: NaiveDate) -> DateTime<Local> {
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default())
        .earliest()
        .unwrap_or_else(Local::now)
}

/// "You have 2 events today: Dentist at 3 PM; and Team sync at 5:30 PM."
pub fn agenda_speech(items: &[AgendaItem], day_word: &str, address: Option<&str>) -> String {
    let tail = address.map(|a| format!(", {a}")).unwrap_or_default();
    if items.is_empty() {
        return format!("Nothing on your calendar {day_word}{tail}.");
    }
    let parts: Vec<String> = items
        .iter()
        .take(4)
        .map(|i| if i.all_day { format!("{}, all day", i.summary) } else { format!("{} at {}", i.summary, i.when_label) })
        .collect();
    let more = items.len().saturating_sub(4);
    let list = match parts.len() {
        1 => parts[0].clone(),
        n => format!("{}; and {}", parts[..n - 1].join("; "), parts[n - 1]),
    };
    format!(
        "You have {} event{} {day_word}{tail}: {list}{}.",
        items.len(),
        if items.len() == 1 { "" } else { "s" },
        if more > 0 { format!("; plus {more} more") } else { String::new() }
    )
}

fn md_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|') { vec!['\\', c] } else { vec![c] })
        .collect()
}

pub fn card_markdown(title: &str, items: &[AgendaItem]) -> String {
    let mut md = format!("## {}\n\n", md_escape(title));
    if items.is_empty() {
        md.push_str("Nothing scheduled.\n");
    }
    for i in items {
        md.push_str(&format!("- **{}** — {}\n", md_escape(&i.summary), i.when_label));
    }
    md
}

// ─── "add X to my calendar tomorrow at 5pm" ──────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct EventDraft {
    pub summary: String,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    NoTitle,
    NoTime,
    InThePast,
}

impl DraftError {
    pub fn message(self) -> &'static str {
        match self {
            DraftError::NoTitle => "What should I call it, sir? Say add, then the event, to my calendar, with the day and time.",
            DraftError::NoTime => "I need the day and time, sir. For example, add dentist to my calendar tomorrow at 5 PM.",
            DraftError::InThePast => "That time has already passed today, sir.",
        }
    }
}

const WEEKDAYS: [(&str, u32); 14] = [
    ("monday", 0), ("mon", 0), ("tuesday", 1), ("tue", 1), ("tues", 1), ("wednesday", 2), ("wed", 2),
    ("thursday", 3), ("thu", 3), ("thurs", 3), ("friday", 4), ("fri", 4), ("saturday", 5), ("sunday", 6),
];

/// Day words → how many days from `today` (0 = today). Returns the offset and
/// the indexes of the words that were consumed.
fn find_day(words: &[String], today_weekday0: u32) -> Option<(i64, Vec<usize>)> {
    for (i, w) in words.iter().enumerate() {
        match w.as_str() {
            "today" | "tonight" => return Some((0, vec![i])),
            "tomorrow" => return Some((1, vec![i])),
            _ => {}
        }
        if w == "day" && words.get(i + 1).map(|x| x == "after").unwrap_or(false) && words.get(i + 2).map(|x| x == "tomorrow").unwrap_or(false) {
            return Some((2, vec![i, i + 1, i + 2]));
        }
        if let Some((_, target)) = WEEKDAYS.iter().find(|(n, _)| n == w) {
            let mut ahead = ((*target + 7 - today_weekday0) % 7) as i64;
            if ahead == 0 {
                ahead = 7;
            }
            let mut used = vec![i];
            if i > 0 && (words[i - 1] == "next" || words[i - 1] == "this") {
                used.push(i - 1);
            }
            return Some((ahead, used));
        }
    }
    None
}

/// "at 5pm", "at 5:30 pm", "at 17:00", "noon". Returns minutes and consumed word indexes.
fn find_time(words: &[String]) -> Option<(u16, Vec<usize>)> {
    for (i, w) in words.iter().enumerate() {
        if w == "noon" {
            return Some((720, vec![i]));
        }
        if w == "midnight" {
            return Some((0, vec![i]));
        }
        if w == "at" {
            // "at 5", "at 5pm", "at 5 pm", "at 5:30 pm"
            let first = words.get(i + 1)?;
            if let Some(m) = timetable::parse_time(first) {
                // Bare hour with a following am/pm word.
                if let Some(ap) = words.get(i + 2).filter(|x| matches!(x.as_str(), "am" | "pm")) {
                    if let Some(m2) = timetable::parse_time(&format!("{first}{ap}")) {
                        return Some((m2, vec![i, i + 1, i + 2]));
                    }
                }
                return Some((m, vec![i, i + 1]));
            }
        }
    }
    None
}

fn find_duration(words: &[String]) -> (i64, Vec<usize>) {
    for (i, w) in words.iter().enumerate() {
        if w == "for" {
            if let (Some(n), Some(unit)) = (words.get(i + 1).and_then(|x| x.parse::<i64>().ok()), words.get(i + 2)) {
                if unit.starts_with("hour") {
                    return (n.clamp(1, 12) * 60, vec![i, i + 1, i + 2]);
                }
                if unit.starts_with("min") {
                    return (n.clamp(5, 720), vec![i, i + 1, i + 2]);
                }
            }
        }
    }
    (60, vec![])
}

const CALENDAR_MARKERS: &[&str] = &[
    "to my calendar", "on my calendar", "in my calendar", "to the calendar", "on the calendar",
    "to my google calendar", "on my google calendar", "in my google calendar",
];
const ADD_VERBS: &[&str] = &["add", "put", "schedule", "create", "book", "block", "set up"];

/// True for "add dentist to my calendar tomorrow at 5pm" style requests.
pub fn is_calendar_add(text: &str) -> bool {
    let t = text.to_lowercase();
    CALENDAR_MARKERS.iter().any(|m| t.contains(m)) && ADD_VERBS.iter().any(|v| t.starts_with(v) || t.contains(&format!(" {v} ")))
}

pub fn parse_event_request(text: &str, now: DateTime<Local>) -> Result<EventDraft, DraftError> {
    let lower = text.trim().to_lowercase();
    let lower = lower.trim_end_matches(['.', '!', '?']).to_string();
    // Remove the calendar marker and the leading verb, keep everything else.
    let mut rest = lower.clone();
    for m in CALENDAR_MARKERS {
        rest = rest.replace(m, " ");
    }
    let words: Vec<String> = rest
        .split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '.'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_matches('.').to_string())
        .filter(|w| !w.is_empty())
        .collect();

    let mut consumed = vec![false; words.len()];
    if let Some(first) = words.first() {
        if ADD_VERBS.iter().any(|v| v.split(' ').next() == Some(first.as_str())) {
            consumed[0] = true;
            if first == "set" && words.get(1).map(|w| w == "up").unwrap_or(false) {
                consumed[1] = true;
            }
        }
    }
    let day = find_day(&words, now.weekday().num_days_from_monday());
    if let Some((_, used)) = &day {
        for i in used {
            consumed[*i] = true;
        }
    }
    let time = find_time(&words);
    if let Some((_, used)) = &time {
        for i in used {
            consumed[*i] = true;
        }
    }
    let (minutes, dur_used) = find_duration(&words);
    for i in dur_used {
        consumed[i] = true;
    }

    const FILLER: &[&str] = &["a", "an", "the", "my", "event", "meeting", "called", "named", "titled", "please", "on", "at", "for", "next", "this", "me"];
    let title_words: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(i, w)| !consumed[*i] && !FILLER.contains(&w.as_str()))
        .map(|(_, w)| w.as_str())
        .collect();
    // Keep "meeting"/"event" when they are the only content ("add a meeting ...").
    let summary = if title_words.is_empty() {
        let only_meeting = words.iter().enumerate().any(|(i, w)| !consumed[i] && (w == "meeting" || w == "event"));
        if only_meeting { "Meeting".to_string() } else { return Err(DraftError::NoTitle) }
    } else {
        let t = title_words.join(" ");
        let mut c = t.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or(t)
    };

    let (offset, minute) = match (day, time) {
        (Some((d, _)), Some((m, _))) => (d, m),
        (None, Some((m, _))) => (0, m),
        _ => return Err(DraftError::NoTime),
    };
    let date = now.date_naive() + Duration::days(offset);
    let start_naive = date.and_time(NaiveTime::from_hms_opt((minute / 60) as u32, (minute % 60) as u32, 0).ok_or(DraftError::NoTime)?);
    let start = Local.from_local_datetime(&start_naive).earliest().ok_or(DraftError::NoTime)?;
    if start <= now {
        return Err(DraftError::InThePast);
    }
    Ok(EventDraft { summary: summary.chars().take(80).collect(), start, end: start + Duration::minutes(minutes) })
}

pub fn to_new_event(d: &EventDraft) -> NewEvent {
    NewEvent {
        summary: d.summary.clone(),
        description: Some("Added by NEXUS".to_string()),
        start_iso: d.start.to_rfc3339(),
        end_iso: d.end.to_rfc3339(),
        location: None,
    }
}

/// "tomorrow at 5 PM" / "Friday at 5 PM" — for reading a draft back.
pub fn when_phrase(start: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (start.date_naive() - now.date_naive()).num_days();
    let day = match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        2..=6 => start.format("%A").to_string(),
        _ => start.format("%A %e %B").to_string().split_whitespace().collect::<Vec<_>>().join(" "),
    };
    format!("{day} at {}", fmt_local_time(&start))
}

/// What overlaps a draft, read back before confirming.
pub fn conflict_speech(items: &[AgendaItem], draft: &EventDraft) -> Option<String> {
    let (s, e) = (draft.start.timestamp(), draft.end.timestamp());
    let hits: Vec<&AgendaItem> = items
        .iter()
        .filter(|i| !i.all_day && s < i.end_epoch && e > i.start_epoch)
        .collect();
    match hits.as_slice() {
        [] => None,
        [one] => Some(format!("Note, that overlaps with {} at {}.", one.summary, one.when_label)),
        many => Some(format!("Note, that overlaps with {} other events.", many.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Wednesday 2026-10-07 10:00 local.
    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 7, 10, 0, 0).earliest().unwrap()
    }

    fn event(summary: &str, start: &str, end: &str) -> CalendarEvent {
        CalendarEvent {
            id: "e".into(),
            summary: summary.into(),
            description: None,
            start_iso: start.into(),
            end_iso: end.into(),
            location: None,
            status: "confirmed".into(),
        }
    }

    #[test]
    fn parses_natural_requests() {
        let d = parse_event_request("add dentist to my calendar tomorrow at 5pm", now()).unwrap();
        assert_eq!(d.summary, "Dentist");
        assert_eq!(d.start.format("%Y-%m-%d %H:%M").to_string(), "2026-10-08 17:00");
        assert_eq!((d.end - d.start).num_minutes(), 60);

        let d = parse_event_request("Schedule exam revision on my calendar on friday at 4 pm for 2 hours", now()).unwrap();
        assert_eq!(d.summary, "Exam revision");
        assert_eq!(d.start.format("%Y-%m-%d %H:%M").to_string(), "2026-10-09 16:00");
        assert_eq!((d.end - d.start).num_minutes(), 120);

        let d = parse_event_request("put team sync today at 5:30 pm on my calendar", now()).unwrap();
        assert_eq!((d.summary.as_str(), d.start.format("%H:%M").to_string().as_str()), ("Team sync", "17:30"));

        let d = parse_event_request("add hackathon deadline to my calendar next monday at noon", now()).unwrap();
        assert_eq!(d.start.format("%Y-%m-%d %H:%M").to_string(), "2026-10-12 12:00");
        assert_eq!(d.summary, "Hackathon deadline");
    }

    #[test]
    fn weekday_equal_to_today_means_next_week() {
        let d = parse_event_request("add gym to my calendar on wednesday at 6pm", now()).unwrap();
        assert_eq!(d.start.format("%Y-%m-%d").to_string(), "2026-10-14");
    }

    #[test]
    fn reports_what_is_missing() {
        assert_eq!(parse_event_request("add dentist to my calendar", now()).unwrap_err(), DraftError::NoTime);
        assert_eq!(parse_event_request("add to my calendar tomorrow at 5pm", now()).unwrap_err(), DraftError::NoTitle);
        assert_eq!(parse_event_request("add dentist to my calendar today at 9am", now()).unwrap_err(), DraftError::InThePast);
        assert!(parse_event_request("add a meeting to my calendar tomorrow at 3pm", now()).unwrap().summary == "Meeting");
    }

    #[test]
    fn detects_calendar_add_requests_only() {
        assert!(is_calendar_add("add dentist to my calendar tomorrow at 5pm"));
        assert!(is_calendar_add("schedule a call on my calendar friday at 3"));
        assert!(!is_calendar_add("open google calendar"));
        assert!(!is_calendar_add("what's on my calendar today"));
        assert!(!is_calendar_add("add milk to my shopping list"));
    }

    #[test]
    fn event_payload_has_an_offset() {
        let d = parse_event_request("add dentist to my calendar tomorrow at 5pm", now()).unwrap();
        let e = to_new_event(&d);
        assert!(e.start_iso.contains('T') && (e.start_iso.contains('+') || e.start_iso.contains('-') || e.start_iso.ends_with('Z')), "{}", e.start_iso);
        assert!(crate::google::calendar::CalendarService::parse_iso_to_epoch(&e.start_iso).is_some());
    }

    #[test]
    fn agenda_wording() {
        let evs = vec![
            event("Dentist", "2026-10-07T15:00:00+00:00", "2026-10-07T16:00:00+00:00"),
            event("Holiday", "2026-10-07", "2026-10-08"),
        ];
        let items = items_from_events(&evs);
        assert_eq!(items.len(), 2);
        assert!(items[1].all_day && items[1].when_label == "all day");
        let s = agenda_speech(&items, "today", Some("sir"));
        assert!(s.starts_with("You have 2 events today, sir: Dentist at ") && s.contains("Holiday, all day"), "{s}");
        assert_eq!(agenda_speech(&[], "tomorrow", Some("sir")), "Nothing on your calendar tomorrow, sir.");
        let one = agenda_speech(&items[..1], "today", None);
        assert!(one.starts_with("You have 1 event today: Dentist at "), "{one}");
        let cancelled = CalendarEvent { status: "cancelled".into(), ..evs[0].clone() };
        assert!(items_from_events(&[cancelled]).is_empty());
    }

    #[test]
    fn conflicts_and_phrases() {
        let d = parse_event_request("add dentist to my calendar tomorrow at 5pm", now()).unwrap();
        let busy = AgendaItem {
            summary: "Team sync".into(),
            all_day: false,
            start_epoch: d.start.timestamp() - 1800,
            end_epoch: d.start.timestamp() + 1800,
            when_label: "4:30 PM".into(),
        };
        assert!(conflict_speech(&[busy.clone()], &d).unwrap().contains("Team sync"));
        let free = AgendaItem { start_epoch: d.end.timestamp() + 10, end_epoch: d.end.timestamp() + 3000, ..busy };
        assert!(conflict_speech(&[free], &d).is_none());
        assert_eq!(when_phrase(d.start, now()), "tomorrow at 5 PM");
    }

    #[test]
    fn day_bounds_span_a_local_day() {
        let (a, b) = day_bounds(now(), 0);
        let (sa, sb) = (
            crate::google::calendar::CalendarService::parse_iso_to_epoch(&a).unwrap(),
            crate::google::calendar::CalendarService::parse_iso_to_epoch(&b).unwrap(),
        );
        let span = sb - sa;
        assert!((82_800..=90_000).contains(&span), "{span}"); // 23-25 h around DST changes
    }
}
