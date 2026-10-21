//! Slot reminders (plan P4): every 30 s, fire the reminder for any timetable
//! slot whose start time has arrived and that has not fired today.
//!
//! Delivery goes through `proactive_policy::submit`, so a reminder waits out
//! meetings and speech instead of cutting in. "Fired today" is stored in the
//! sealed store, so a restart inside the 10-minute grace window does not
//! repeat it. Activities with something to open ask "Shall I start?" and
//! register a pending offer; the rest ("Gym", "Lunch") are only announced.

use tauri::{AppHandle, Manager, Runtime};

use super::offer::{self, Offer};
use super::timetable::{self, Slot};

pub const TICK_SECS: u64 = 30;
/// "Later" re-asks after this long.
pub const SNOOZE_SECS: u64 = 10 * 60;

/// What the reminder says. Pure.
pub fn reminder_text(title: &str, asks_to_start: bool, address: Option<&str>) -> String {
    let who = address.map(|a| format!(", {a}")).unwrap_or_default();
    if asks_to_start {
        format!("It's time for {title}{who}. Shall I start?")
    } else {
        format!("It's time for {title}{who}.")
    }
}

/// Fire one slot's reminder now.
pub fn fire<R: Runtime>(app: &AppHandle<R>, slot: &Slot) {
    let activity = timetable::activity_key(&slot.title);
    let asks = timetable::plan_sites(&activity).is_some();
    let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(app));
    let name = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|d| crate::memory::read_user_profile(&d))
        .and_then(|p| p.name);
    let address = crate::persona::address(name.as_deref(), friend);
    let text = reminder_text(&slot.title, asks, address.as_deref());
    if asks {
        // Not armed until the question is actually spoken (see speak_line).
        offer::set(Offer::Start { title: slot.title.clone(), activity }, false);
        crate::proactive_policy::submit(
            app,
            format!("offer_slot_{}", slot.id),
            text,
            crate::google::types::AlertUrgency::Medium,
        );
    } else {
        crate::proactive_policy::submit(
            app,
            format!("slot_{}", slot.id),
            text,
            crate::google::types::AlertUrgency::Medium,
        );
    }
}

/// "Later": ask again in ten minutes (the slot stays fired for today).
pub fn snooze_start<R: Runtime>(app: &AppHandle<R>, title: String, activity: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(SNOOZE_SECS)).await;
        let friend = crate::persona::is_friend(&crate::commands::read_persona_mode(&app));
        let address = crate::persona::address(None, friend).filter(|_| !friend);
        let text = reminder_text(&title, true, address.as_deref());
        offer::set(Offer::Start { title: title.clone(), activity }, false);
        crate::proactive_policy::submit(
            &app,
            format!("offer_snooze_{}", timetable::activity_key(&title)),
            text,
            crate::google::types::AlertUrgency::Medium,
        );
    });
}

/// Start the 30 s scheduler. Cheap no-op without slots.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    use chrono::{Datelike, Timelike};
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(TICK_SECS)).await;
            let Ok(dir) = app.path().app_data_dir() else { continue };
            if !super::enabled(&dir) || !super::flag(&dir, "memcoreTimetable", true) {
                continue;
            }
            let now = chrono::Local::now();
            let weekday0 = now.weekday().num_days_from_monday() as u8;
            let minute = (now.hour() * 60 + now.minute()) as u16;
            let today = now.format("%Y-%m-%d").to_string();
            let due: Vec<Slot> = super::with_store(&dir, |s| {
                let slots = timetable::load_slots(s);
                if slots.is_empty() {
                    return vec![];
                }
                let due: Vec<Slot> = timetable::due(&slots, weekday0, minute, &|id| {
                    s.meta_get(&timetable::fired_key(id)).as_deref() == Some(today.as_str())
                })
                .into_iter()
                .cloned()
                .collect();
                for d in &due {
                    s.meta_set(&timetable::fired_key(&d.id), &today);
                }
                due
            })
            .unwrap_or_default();
            for slot in due {
                tracing::info!("timetable: reminder for '{}'", slot.title);
                fire(&app, &slot);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_wording() {
        assert_eq!(reminder_text("DSA", true, Some("sir")), "It's time for DSA, sir. Shall I start?");
        assert_eq!(reminder_text("Gym", false, Some("sir")), "It's time for Gym, sir.");
        assert_eq!(reminder_text("DSA", true, None), "It's time for DSA. Shall I start?");
        assert_eq!(reminder_text("DSA", true, Some("Lakshya")), "It's time for DSA, Lakshya. Shall I start?");
    }
}
