//! Global hotkey (Ctrl/Cmd+Space) → state-dependent action.
//!
//! On press:
//!   - If any window (the unified sidebar hosting Assistant/Command
//!     Hub/Architect/PR-List, settings, setup) is visible → close it
//!     only (do NOT wake).
//!   - If no window is visible → wake the assistant (do NOT touch windows).
//!   - If the assistant is speaking → barge-in: the frontend wake handler
//!     stops TTS and starts listening (handled in main.tsx startListening).
//!
//! This means:
//!   - Pressing the hotkey twice (with sidebar visible) first closes the
//!     sidebar, then wakes NEXUS on the second press.
//!   - Wake-word activation does NOT close the sidebar (handled separately
//!     in `wakeword_oww.rs`, which never emits `sidebar:hide`).
//!   - The hotkey never does both at once — it's one or the other based on
//!     the current sidebar visibility state.
//!
//! NOTE: The global-shortcut plugin is not available on Linux.
//! This entire module is compiled only on Windows and macOS.

#![cfg(not(target_os = "linux"))]

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const HOTKEYS: &[&str] = &[
    "CommandOrControl+Space",
    "CommandOrControl+Shift+S",
    "CommandOrControl+Alt+X",
];

pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    for &hk in HOTKEYS {
        let sc: Shortcut = match hk.parse() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to parse hotkey '{hk}': {e}");
                continue;
            }
        };

        let handle = app.clone();
        if let Err(e) = app.global_shortcut().on_shortcut(sc, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                // Ctrl+Shift+S → open settings sidebar directly
                if hk == "CommandOrControl+Shift+S" {
                    tracing::info!("hotkey ({}) → opening settings sidebar", hk);
                    let app_clone = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::commands::show_settings_sidebar(app_clone).await;
                    });
                    return;
                }

                // Ctrl+Alt+X → stage kill-switch: destroy + disable the
                // fullscreen overlay for the session (blackout escape hatch
                // even if the renderer is wedged and can't hear IPC).
                if hk == "CommandOrControl+Alt+X" {
                    tracing::warn!("hotkey ({}) → stage kill-switch", hk);
                    let app_clone = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::orchestrator::request_barge_in("hotkey-stage-kill");
                        let _ = crate::stage::stage_hide_kill(app_clone).await;
                    });
                    return;
                }

                // Ctrl+Space → Symmetric Agent Call / Cancel:
                // If the agent is currently active (capturing, thinking, speaking,
                // or running a ghost session) → CANCEL the calling agent.
                // If the agent is idle → CALL the agent (start capture, wake orb).
                // Windows (settings, sidebar, setup) are discarded by Escape, never Ctrl+Space.
                let is_capturing = crate::wakeword_oww::stt_capturing();
                let is_speaking = handle.try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
                    .map(|ms| ms.tts_playing.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false);
                let has_request = crate::orchestrator::has_active_request();
                let is_ghost = crate::ghost::session_active();

                let agent_active = is_capturing || is_speaking || has_request || is_ghost;

                if agent_active {
                    tracing::info!(
                        "hotkey ({}) → agent is active (capturing={}, speaking={}, thinking={}, ghost={}), CANCELING calling agent",
                        hk, is_capturing, is_speaking, has_request, is_ghost
                    );

                    // 1. Stop TTS audio immediately
                    let _ = crate::tts::stop_tts();
                    let _ = tauri::Emitter::emit(&handle, "tts:stop", ());
                    if let Some(ms) = handle.try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>() {
                        ms.set_tts_playing(false);
                    }

                    // 2. Cancel in-flight orchestrator requests
                    crate::orchestrator::cancel_active();

                    // 3. Abort STT capture
                    crate::wakeword_oww::abort_stt_capture();

                    // 4. If ghost session was active, abort it
                    if is_ghost {
                        let app_clone = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = crate::ghost::ghost_abort(app_clone).await;
                        });
                    }

                    // 5. Emit cancellation event to stage overlay to immediately hide orb & reset to idle
                    let _ = tauri::Emitter::emit(&handle, "orb:cancel", ());

                    return;
                }

                // Agent is idle → CALL the agent
                tracing::info!("hotkey ({}) → agent is idle, CALLING agent", hk);
                println!("[ORB-HOTKEY] Hotkey ({hk}) → agent idle, calling agent & waking orb to listening");

                // Cancel any stale request slot just in case
                crate::orchestrator::request_barge_in("hotkey-wake");
                let _ = tauri::Emitter::emit(&handle, "tts:stop", ());
                let _ = crate::tts::stop_tts();

                // Only pre-start local STT sidecar if cloud STT won't be used
                // (saves ~340 MB RAM when Groq cloud STT is active).
                let groq_key = crate::commands::read_groq_api_key(&handle);
                let local_only = crate::commands::read_local_stt_only(&handle);
                if groq_key.is_empty() || local_only {
                    std::thread::spawn(|| {
                        crate::lazy_stt::ensure_stt_running();
                    });
                } else {
                    tracing::info!("hotkey: Groq cloud STT configured, skipping local sidecar pre-start (saves RAM)");
                }

                // Start Rust-side STT capture (same as wake word path).
                crate::wakeword_oww::start_stt_capture();
                crate::window_manager::wake_orb_src(&handle, "hotkey");
            }
        }) {
            tracing::warn!("Failed to register handler for hotkey '{hk}': {e}");
        } else {
            tracing::info!("Registered global hotkey handler: {hk}");
        }
    }

    Ok(())
}
