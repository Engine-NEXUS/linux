//! Ghost drill runner — scripted keyboard-first tasks inside a ghost session.
//!
//! Phase 1: the WhatsApp drill (open → search → type; send stays behind
//! the existing confirm flow). Every step checks, in order:
//!   1. `ghost::stop_requested()` — voice "stop" / Esc / API abort.
//!   2. `ghost::session_active()` — when the session ends (Esc or voice
//!      exit), the drill must stop typing immediately. Mouse use is
//!      never a stop (no takeover detection).
//!
//! Keyboard primitives here are synchronous and atomic (press+release in
//! one call), so abort-between-steps can never strand a modifier —
//! verified by code inspection of `keyboard.rs` (no key is ever held
//! across an await/step boundary). No key-release tracker is needed
//! until Phase 2 introduces held motion states.
//!
//! Overlap note (honest scoping): the drill runs synchronously inside
//! its Tauri command. Continued user speech still captures + queues via
//! the independent Rust STT thread, and follow-ups execute after — true
//! parallel act∥listen lanes are Phase 3, not this file.

use tauri::{AppHandle, Runtime};

/// Step guard: bail with "Stopped" unless the session is live and no
/// stop was requested. Also pauses (not aborts) on exclusive-fullscreen
/// foreground — games/players swallow synthetic input, so acting would
/// click into the void.
fn guard_step() -> Result<(), String> {
    if crate::ghost::stop_requested() {
        return Err("Stopped, sir.".to_string());
    }
    if !crate::ghost::session_active() {
        return Err("You took over — stopping, sir.".to_string());
    }
    #[cfg(target_os = "windows")]
    if let Some(reason) = super::mouse::foreground_blocked() {
        return Err(reason);
    }
    Ok(())
}

/// WhatsApp ghost drill: enter session (if not already active) →
/// announce → open → search contact → type message. When called from an
/// already-open ghost session, the session is NOT exited at the end —
/// the user stays in control and follow-ups keep flowing; the draft
/// result line just tells them what happened.
/// Send is NEVER included: `live_whatsapp_send` (confirm-gated) owns it.
pub async fn whatsapp_drill<R: Runtime>(
    app: AppHandle<R>,
    contact: &str,
    message: &str,
) -> Result<String, String> {
    // Already in a live session (voice-initiated)? Keep it open after.
    let keep_session = crate::ghost::session_active();
    let contact = contact.trim();
    let message = message.trim();
    if contact.is_empty() {
        return Err("ghost drill: empty contact".to_string());
    }
    // Empty message = chat-open intent: open + find contact, skip typing.
    let chat_open = message.is_empty();
    // Blocklist applies to contacts too (e.g. "my bank account").
    if let crate::live::safety::SafetyVerdict::Blocked(msg) =
        crate::live::safety::safety_check("whatsapp_search", Some(contact))
    {
        return Err(msg);
    }

    // Enter session only when none is live (voice-initiated calls reuse
    // the open session — no duplicate announce, no Esc re-arm).
    if !keep_session {
        crate::ghost::ghost_enter(crate::ghost::ghost_wry::g_wry(app.clone())).await?;
        crate::ghost::announce(crate::ghost::ghost_wry::g_wry_ref(&app), "Taking the mouse, sir. Tap Esc any time to cancel.");
    }
    crate::ghost::drill_begin();

    let run: Result<(String, bool), String> = {
        // Synchronous task run in a blocking thread — every step is a
        // blocking OS action (enigo, sleeps), never an await. Boxing the
        // closure keeps the async wrapper Send (the enigo/Win32 internals
        // are not Send; live-bug chain found at compile time).
        let contact = contact.to_string();
        let message = message.to_string();
        let data_dir = {
            use tauri::Manager;
            app.path().app_data_dir().ok()
        };
        tauri::async_runtime::spawn_blocking(move || -> Result<(String, bool), String> {
            guard_step()?;
            #[allow(unused_assignments)]
            let mut opened_name = contact.clone();
            #[allow(unused_assignments)]
            let mut unverified = false;
            #[cfg(target_os = "windows")]
            {
                use super::wa_open::{self, WaOpen};
                let learned = data_dir
                    .as_deref()
                    .map(|d| wa_open::learned_aliases(d, &contact))
                    .unwrap_or_default();
                // 0. A KNOWN NUMBER opens the exact chat by link: no search box, no UI scraping, no focus
                // tricks (the one deterministic route; numbers come from the contacts book / a saved fact).
                if let Some(dir) = data_dir.as_deref() {
                    if let Some(phone) = resolve_phone(dir, &contact, &learned)? {
                        let _ = open::that(wa_open::deep_link(&phone));
                        if wa_open::ensure_whatsapp_foreground(8000) {
                            std::thread::sleep(std::time::Duration::from_millis(400));
                            guard_step()?;
                            if !chat_open {
                                super::whatsapp::type_message(&message)?;
                            }
                            return Ok((contact, false));
                        }
                        // The link did not bring the app up: fall through to the search route.
                    }
                }
                // 1. WhatsApp must be the FOREGROUND PROCESS before a single key is sent
                // (it is a Store app that often sits hidden in the tray; the old fixed 1 s
                // sleep + window-title match typed into whatever else had focus).
                if !wa_open::ensure_whatsapp_foreground(8000) {
                    // Last resort: the Win-search drill, then give it a bit longer.
                    let _ = super::launcher::open_app_via_search("WhatsApp");
                    if !wa_open::ensure_whatsapp_foreground(5000) {
                        return Err("I couldn't bring WhatsApp to the front, sir. I didn't type anything.".to_string());
                    }
                }
                guard_step()?;
                // 2. Verified open: Ctrl+F, type, wait for the matching chat item, click it.
                match wa_open::open_chat(&wa_open::RealEnv, &contact, &learned) {
                    WaOpen::Opened { matched } => {
                        opened_name = wa_open::spoken_name(&matched);
                        // Learn: remember what this person is saved as, so the next "open <name>" starts
                        // from the exact saved name instead of guessing synonyms.
                        if let Some(d) = data_dir.as_deref() {
                            wa_open::learn_alias(d, &contact, &opened_name);
                        }
                    }
                    WaOpen::Unverified { guess } => {
                        if !chat_open {
                            // Never type a message into a chat that could not be confirmed.
                            return Err(format!(
                                "I opened a chat, but I couldn't confirm it is {contact}'s, sir. I didn't type anything."
                            ));
                        }
                        opened_name = guess;
                        unverified = true;
                    }
                    WaOpen::NotFound => {
                        return Err(format!(
                            "I couldn't find {contact} in WhatsApp, sir. If they are saved under another name, say: remember that wa alias {contact} is, and the saved name."
                        ));
                    }
                    WaOpen::WrongWindow => {
                        return Err("WhatsApp lost focus, sir. I didn't type anything.".to_string());
                    }
                    WaOpen::Stopped => return Err("Stopped, sir.".to_string()),
                    WaOpen::Failed(e) => return Err(e),
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = &data_dir;
                super::whatsapp::search_contact(&contact)?;
            }
            guard_step()?;
            // 3. Type the message (clipboard paste, visible on screen).
            // Chat-open variant never types.
            if !chat_open {
                super::whatsapp::type_message(&message)?;
            }
            Ok((opened_name, unverified))
        })
        .await
        .map_err(|e| format!("ghost drill join: {e}"))?
    };

    // Drain only on a clean run: no stop, no takeover, session intact.
    // Anything else means the context changed — queued items go stale,
    // and resuming behind a user takeover is rejected by design.
    let clean =
        run.is_ok() && !crate::ghost::stop_requested() && crate::ghost::session_active();
    crate::ghost::drill_end();

    // Always leave the session — success, stop, takeover, or error.
    // Voice-initiated runs keep the session open (user stays in control;
    // follow-ups keep flowing). Only standalone runs exit here.
    if !keep_session {
        let _ = crate::ghost::ghost_exit(crate::ghost::ghost_wry::g_wry(app.clone())).await;
    }

    match run {
        Ok((opened_name, unverified)) => {
            // One spoken line: a voice-initiated run (session already live) is spoken by the
            // orchestrator from the returned text; announcing here too said everything twice.
            let done_msg = if chat_open && unverified {
                format!("I opened a chat for {opened_name}, sir, but I couldn't confirm it's the right one.")
            } else if chat_open {
                format!("{opened_name} is open, sir.")
            } else {
                "Draft ready, sir. Say send when ready.".to_string()
            };
            if !keep_session {
                crate::ghost::announce(crate::ghost::ghost_wry::g_wry_ref(&app), &done_msg);
            }
            if clean && !keep_session {
                // Standalone run: queued mid-drill follow-ups drain now,
                // in order, through the normal pipeline (Phase 3 overlap).
                // spawn_blocking: the drain path re-enters the drill (and
                // enigo/Win32 internals are not Send), so the future must
                // not live on the async runtime directly.
                let drain_app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok();
                    if let Some(rt) = rt {
                        rt.block_on(async {
                            crate::orchestrator::drain_ghost_followups(&drain_app).await;
                        });
                    }
                });
            } else if clean {
                // Voice-initiated (session stays open): the caller's own
                // hot-mic loop re-opens the mic and processes follow-ups
                // as normal turns — draining here would recurse.
                tracing::debug!("ghost drill: session kept, follow-ups via hot-mic");
            } else {
                crate::ghost::drop_followups();
            }
            Ok(done_msg)
        }
        Err(e) => {
            crate::ghost::drop_followups();
            if !keep_session {
                crate::ghost::announce(crate::ghost::ghost_wry::g_wry_ref(&app), &e);
            }
            Err(e)
        }
    }
}

/// The WhatsApp number for a spoken name: a saved `wa_phone_<name>` fact, else the contacts book
/// (`contacts.json`, filled by "import my contacts") through the learned alias and the usual
/// kinship synonyms. `Err` = several different people match (the user must say which).
#[cfg(target_os = "windows")]
fn resolve_phone(dir: &std::path::Path, contact: &str, learned: &[String]) -> Result<Option<String>, String> {
    use super::wa_open;
    use crate::contacts_book::{self, Lookup};
    if let Some(p) = wa_open::learned_phone(dir, contact) {
        return Ok(Some(p));
    }
    let book = contacts_book::load(dir);
    if book.is_empty() {
        return Ok(None);
    }
    match contacts_book::lookup(&book, &wa_open::name_candidates(contact, learned)) {
        Lookup::One { number, .. } => Ok(wa_open::normalize_phone(&number)),
        Lookup::Ambiguous(names) => Err(format!(
            "I have more than one {contact} in your contacts, sir: {}. Say the full name.",
            names.join(", ")
        )),
        Lookup::None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    // Live drill needs a desktop + WhatsApp: verified manually.
    // Unit-testable surface: input validation lives in the drill head;
    // exercise it through the safety layer (no OS calls involved).
    use crate::live::safety;

    #[test]
    fn test_drill_blocks_denied_contact() {
        assert!(matches!(
            safety::safety_check("whatsapp_search", Some("my bank account")),
            safety::SafetyVerdict::Blocked(_)
        ));
    }

    #[test]
    fn test_drill_allows_normal_contact() {
        assert_eq!(
            safety::safety_check("whatsapp_search", Some("mummy")),
            safety::SafetyVerdict::Allowed
        );
    }
}
