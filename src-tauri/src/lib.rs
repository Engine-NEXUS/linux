//! NEXUS — Tauri v2 main process.
//!
//! Wires up: window manager (click-through), global hotkey, autostart, tray,
//! wake-word engine, the WSS network bridge, deep-link (OAuth redirects),
//! and window positioning (bottom-center sidebar).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod window_manager;
// Smart Turn end-of-turn model — standalone prototype, not yet wired into capture.
#[allow(dead_code)]
pub(crate) mod turn_detect;
// Silero VAD (Handy-derived) — used by the STT capture loop when "vadSilero" is on.
pub(crate) mod vad;
// Phase 8: directed-speech gate for open-mic (ghost hot-mic) turns
pub(crate) mod directed;
// Phase 9: when proactive alerts may speak (breakpoints, urgency, meeting policy)
pub(crate) mod proactive_policy;
pub(crate) mod screen_context;
pub(crate) mod usage_counter;
pub(crate) mod api_keys;
pub(crate) mod github_profile;
pub(crate) mod screen_tour;
// Kokoro-82M local TTS (replaces Piper) — see docs/research/jarvis-landscape/09
pub(crate) mod tts_kokoro;
#[cfg(not(target_os = "linux"))]
mod hotkey;
// wakeword-oww (default): openWakeWord via tract-onnx (pure Rust, no C++ deps)
#[cfg(feature = "wakeword-oww")]
mod wakeword_oww;
#[cfg(feature = "wakeword-oww")]
mod wakeword {
    pub use crate::wakeword_oww::*;
}
// Offline wake-word replay harness (dev tool `wake_replay`, `nexus wake replay`).
#[cfg(all(feature = "wakeword-oww", not(feature = "mock-wake")))]
pub mod wake_eval;
// Phase D: Speaker verification module (voice profile enrollment + cosine similarity)
pub(crate) mod voice_profile;
pub(crate) mod acoustic_profile;
mod network;
mod tray;
pub(crate) mod commands;
mod command_executor;
mod app_registry;
pub mod intent_parser;
mod nlu_client;
mod nlu_local;
mod admin_config;
#[cfg(feature = "admin-brain")]
mod brain_client;
#[cfg(feature = "admin-brain")]
mod brain_monitor;
#[cfg(feature = "admin-brain")]
mod lazy_brain;
mod lazy_stt;
mod stt;
pub mod stt_groq;
pub(crate) mod stt_deepgram;
mod stt_learning;
mod stt_stream;
mod tts;
pub mod tts_edge;
mod tts_network;
mod contacts_book;
mod tts_cache_disk;
mod tts_health;
mod tts_system;
mod tts_wav;
pub(crate) mod tts_swap;
pub(crate) mod voice_catalog;
#[cfg(test)]
mod tts_bench;
mod pipeline_bench;
mod volume;
mod keep_awake;
mod agent_watch;
// Phase D: Speaker verification is now wired via voice_profile module.
// The OWW engine uses the existing embedding_model.onnx for speaker embeddings.
mod meeting_detect;
mod mic_permissions;
mod mpris;
// Linux platform helpers: PipeWire/ALSA capture-device selection and
// browser-meeting detection (both pure, both unit-tested).
#[cfg(target_os = "linux")]
mod audio_linux;
#[cfg(target_os = "linux")]
mod meeting_linux;
// Bounded subprocess runs for the Linux desktop tools (wpctl/pactl/amixer).
#[cfg(target_os = "linux")]
mod proc_linux;
pub(crate) mod media_player;
mod architect;
mod browser_url;
mod symbol_extractor;
mod dyn_windows;
mod diagnostics;
pub(crate) mod orchestrator;
pub(crate) mod github_cmd;
pub(crate) mod live;
pub(crate) mod router;
pub(crate) mod mcp_client;
pub(crate) mod auth_vault;
pub(crate) mod identity_state;
pub(crate) mod ghostwriter;
pub mod screen;
pub mod shortcuts;
pub(crate) mod telegram;
pub(crate) mod command_center;
pub(crate) mod ghost_center;
pub(crate) mod ghost_learning;
pub(crate) mod nlu_update;
pub(crate) mod power;
pub(crate) mod power_cmd;
pub(crate) mod howto;
pub(crate) mod power_close;
pub(crate) mod latency;
pub(crate) mod coder;
pub(crate) mod dictation;
pub(crate) mod dictation_text;
pub(crate) mod hand;
pub(crate) mod mode_intent;
// ── Phase B–D modules (recovered after a stale lib.rs rewrite) ──────────
mod agent_specs;
mod conversation;
mod browser_center;
pub(crate) mod center;
pub(crate) mod diary;
pub(crate) mod ghost;
pub(crate) mod google;
mod improve;
pub(crate) mod memcore;
pub(crate) mod memory;
pub(crate) mod persona;
mod missed_intent_logger;
mod pii_filter;
pub(crate) mod stage;
mod last_heard;
mod system_center;
mod vision;
mod webhook;
mod youtube_center;
// Windows.Media.Ocr — zero-RAM local screen text extraction (Feature 86).
// On Linux the same module hosts the local RapidOCR sidecar client.
pub(crate) mod ocr;
// Animation calibration (drag + scroll-wheel placement designer).
pub(crate) mod calibration;
#[cfg(target_os = "windows")]
mod dwm_corners;
#[cfg(target_os = "windows")]
mod sidebar_backdrop;
pub(crate) mod live_glass;
pub(crate) mod luminance_probe;
#[cfg(target_os = "linux")]
pub(crate) mod session;
// Linux display-backend bootstrap (GDK_BACKEND=x11 when XWayland is there).
// Called from main() before Tauri/GTK starts — see the module docs.
#[cfg(target_os = "linux")]
pub mod gdk_backend_linux;

// ── Linux semantic tier + policy gate ─────────────────────────────────────
//
// The Linux counterpart of UI Automation: the accessibility tree (AT-SPI),
// the Chromium accessibility tree (CDP), and the identity-based policy gate
// that every semantic operation passes through. `semantic` is the one entry
// point; `policy` is the decision function; `pointer` owns the pre-capture
// privacy gate.
#[cfg(target_os = "linux")]
pub(crate) mod atspi;
#[cfg(target_os = "linux")]
mod lazy_atspi;
#[cfg(target_os = "linux")]
pub(crate) mod cdp;
#[cfg(target_os = "linux")]
mod lazy_cdp;
#[cfg(target_os = "linux")]
pub(crate) mod semantic;
#[cfg(target_os = "linux")]
pub(crate) mod policy;
#[cfg(target_os = "linux")]
pub(crate) mod pointer;
#[cfg(target_os = "linux")]
mod lazy_ocr;

use tauri::{Emitter, Listener, Manager};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_deep_link::DeepLinkExt;
use tracing_subscriber::EnvFilter;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Shared app state held across async tasks.
pub struct AppState {
    pub events: tauri::AppHandle,
}

// ─── WebView2 stale profile cleanup (Windows) ─────────────────────────────
//
// See the comment in run() for why this is a separate function called
// BEFORE tauri::Builder::default().
#[cfg(target_os = "windows")]
fn cleanup_webview2_profile() {
    // The WebView2 data directory is at %LOCALAPPDATA%\<identifier>\EBWebView.
    // The identifier is "com.nexus.assistant" (from tauri.conf.json).
    let local_appdata = match std::env::var("LOCALAPPDATA") {
        Ok(v) => v,
        Err(_) => return,
    };
    let webview_dir = std::path::PathBuf::from(&local_appdata)
        .join("com.nexus.assistant")
        .join("EBWebView");

    if !webview_dir.exists() {
        return; // Nothing to clean — fresh install or already cleaned.
    }

    // Step 1: Kill orphaned msedgewebview2.exe processes from a PREVIOUS
    // NEXUS instance. These processes reference our EBWebView directory
    // (--user-data-dir=...com.nexus.assistant\EBWebView) and hold file
    // locks that prevent deletion. The CURRENT instance hasn't created
    // any WebView2 processes yet (we're before the Tauri builder), so
    // any such process MUST be an orphan from a previous run.
    //
    // We use `taskkill /F /FI` with a window-title filter won't work, so
    // we use PowerShell to find processes by command-line match and kill
    // them. This is the most reliable approach on Windows.
    let ps_script = r#"
        $target = 'com.nexus.assistant\EBWebView'
        $procs = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
            Where-Object { $_.CommandLine -like "*$target*" }
        if ($procs) {
            $procs | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
            Start-Sleep -Milliseconds 500
            Write-Output "KILLED:$($procs.Count)"
        } else {
            Write-Output "NONE"
        }
        "#;

    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", ps_script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    // Step 2: Attempt to delete the EBWebView directory. Retry up to 3
    // times with 200ms between attempts — the killed processes may take
    // a moment to release their file handles.
    for attempt in 1..=3u8 {
        match std::fs::remove_dir_all(&webview_dir) {
            Ok(()) => {
                tracing::info!("cleared WebView2 profile (attempt {}): {}", attempt, webview_dir.display());
                return;
            }
            Err(e) if e.raw_os_error() == Some(32) => {
                // os error 32 = sharing violation (files still locked)
                tracing::debug!("WebView2 cleanup attempt {} failed (locked): {}", attempt, e);
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) if e.raw_os_error() == Some(2) => {
                // os error 2 = not found (another thread already deleted it)
                return;
            }
            Err(e) => {
                tracing::warn!("WebView2 cleanup error (attempt {}): {}", attempt, e);
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    }

    // Step 3: If deletion still fails (stubborn locks), rename the
    // directory instead. WebView2 will create a fresh one, and the
    // stale rename target can be cleaned up by the OS or a future run.
    let stale_dir = webview_dir.with_extension("stale");
    // Remove any previous stale dir first
    let _ = std::fs::remove_dir_all(&stale_dir);
    match std::fs::rename(&webview_dir, &stale_dir) {
        Ok(()) => {
            tracing::info!(
                "WebView2 profile locked — renamed to stale: {} → {}",
                webview_dir.display(),
                stale_dir.display()
            );
        }
        Err(e) => {
            tracing::error!(
                "WebView2 profile cleanup FAILED — could not delete or rename {}: {e}",
                webview_dir.display()
            );
            tracing::error!(
                "This will likely cause 'localhost refused to connect' on this launch."
            );
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,nexus=debug")))
        .with_target(false)
        .init();

    // ─── WebView2 stale profile cleanup ───────────────────────────────
    //
    // This MUST happen BEFORE tauri::Builder::default() because Tauri
    // creates WebView2 windows (and their child msedgewebview2.exe
    // processes) during builder initialization — BEFORE .setup() runs.
    // If we try to delete EBWebView in .setup(), the current instance's
    // own WebView2 is already holding the directory locked (os error 32).
    //
    // Root cause of "localhost refused to connect":
    //   WebView2 persists session state (Preferences, Sessions, etc.) in
    //   %LOCALAPPDATA%/<identifier>/EBWebView. If a dev build
    //   (localhost:5173) was ever run, the stale dev URL survives in
    //   Preferences and is restored on every subsequent launch — even
    //   release builds — causing ERR_CONNECTION_REFUSED.
    //
    // Fix: delete the entire EBWebView directory before Tauri starts so
    // WebView2 creates a fresh profile with the bundled frontend.
    // Also kill any orphaned msedgewebview2.exe processes from a previous
    // NEXUS instance that may still hold the directory locked.
    #[cfg(target_os = "windows")]
    cleanup_webview2_profile();

    crate::power::enable_process_ecoqos();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!("single-instance: secondary launch attempt with args: {:?}", args);
            // Handle deep-link redirects on Windows/Linux (passed as CLI arg)
            if let Some(url) = args.iter().find(|a| a.starts_with("nexus://")) {
                tracing::info!("single-instance: deep-link callback: {}", url);
                if url == "nexus://settings" {
                    // Open settings sidebar via deep link
                    let app_clone = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::commands::show_settings_sidebar(app_clone).await;
                    });
                    return;
                }
                let _ = app.emit("deep-link://oauth-callback", url.clone());
                // OAuth callback — just emit the event and return.
                // Do NOT try to show/wake the main window here; the WebView2
                // environment may not be accessible from this callback context
                // (causes HRESULT 0x8007139F "group or resource not in correct
                // state"). The frontend listens for the event and handles UI.
                return;
            }

            // Check if secondary launch requested setup wizard or settings window
            let is_setup = args.iter().any(|a| a == "--setup" || a == "-s");
            let is_settings = args.iter().any(|a| a == "--settings");
            let is_background = args.iter().any(|a| a == "--background");
            // `--wake` is the summon path on Linux, where the global hotkey is
            // compiled out: a desktop shortcut / custom keybinding runs
            // `nexus --wake`, which lands here and wakes the running instance.
            let is_wake = args.iter().any(|a| a == "--wake");

            if is_background && !is_setup && !is_settings && !is_wake {
                // Silent background launch — don't show the orb, just ensure the
                // tray is running. This handles the scheduled-task auto-start case.
                tracing::info!("single-instance: --background launch, staying hidden");
                return;
            }

            if is_setup {
                if let Ok(win) = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::setup()) {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            } else if is_settings {
                // Open the settings sidebar (liquid-glass, 720x1000)
                let app_clone = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = crate::commands::show_settings_sidebar(app_clone).await;
                });
            } else {
                // Only wake the orb if we are NOT in the middle of setup
                let setup_active = app.get_webview_window("setup").is_some();
                if !setup_active {
                    crate::window_manager::wake_orb_src(app, if is_wake { "wake-flag" } else { "relaunch" });
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_positioner::init());

    // global-shortcut plugin is not available on Linux
    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());
    }

    builder.setup(|app| {
        // Stage FIRST (docs/features/123, P3): WebView2's cold start (~8 s) now overlaps the rest of setup
        // (preflight audit, memory, registries) instead of following it.
        // ─── Stage: the always-on home for the orb + loading indicator ──
            // Single-Stage Shell step 2 (AGENTS.md 2026-09-25 planned this,
            // never executed until now): the voice orb and loading spinner
            // no longer get their own small OS windows ("main",
            // "loading-indicator") — they're positioned divs inside the one
            // fullscreen `stage` overlay, which must therefore be shown at
            // boot instead of the old on-demand/ghost-only model. Called
            // synchronously (not spawned) so the window exists in time for
            // `mic_permissions::init` to find it by label a few lines down.
            if let Err(e) = crate::stage::stage_show_sync(app.handle()) {
                tracing::error!("stage: failed to show at boot: {e}");
            }

        // Immediate 5-layer preflight system audit — prints health matrix to stdout
        let audit_dir = app.path().app_data_dir().ok();
        diagnostics::run_full_system_audit(audit_dir.as_deref());

        // If a previous run died while NEXUS held the system volume at the TTS level, put the
        // user's original volume back before anything speaks.
        if let Some(dir) = audit_dir.clone() {
            volume::init(dir.clone());
            // One-time memory cleanup (junk episodes/turns/poisoned brief; originals backed up).
            std::thread::spawn(move || memcore::hygiene::run_startup_cleanup(&dir));
        }

        // macOS: hide from the Dock and Cmd+Tab switcher (accessory/background app).
        #[cfg(target_os = "macos")]
        app.set_activation_policy(tauri::ActivationPolicy::Accessory);


            // ─── Stage hitbox loop + blackout watchdog ──────────────────
            // Both loops are permanent and self-guard (they idle while the
            // stage is hidden/disabled and re-arm on stage_show). Spawning
            // them once here wires:
            //   • click-through holes over registered hitboxes (without
            //     this the fullscreen stage swallows ALL desktop clicks)
            //   • ghost ring cursor ride-along (observe_cursor share)
            //   • blackout detection → destroy → auto-fix backoff
            crate::stage::spawn_hitbox_loop(app.handle().clone());
            crate::stage::spawn_stage_watchdog(app.handle().clone());

            // WebView2 profile cleanup is done BEFORE tauri::Builder::default()
            // in run() — see cleanup_webview2_profile() above. Doing it here
            // in .setup() is too late: Tauri has already created WebView2
            // windows and their child processes hold the EBWebView directory
            // locked (os error 32).

            // Register the nexus:// deep-link scheme (Windows + Linux runtime registration).
            // macOS uses Info.plist CFBundleURLTypes (already configured).
            #[cfg(desktop)]
            {
                let _ = app.deep_link().register("nexus");
            }

            // ─── Autostart: respect settings.autostart ───────────────────
            //
            // On Windows, we use a Scheduled Task with "At log on" trigger
            // instead of the HKCU\...\Run registry key. This launches NEXUS
            // IMMEDIATELY when the user logs on — no 10-30s desktop-settle
            // delay. The task launches with --background for silent tray start.
            //
            // On macOS/Linux, we use tauri-plugin-autostart (LaunchAgent /
            // systemd user units are already zero-delay on those platforms).
            //
            // The autostart setting is read from settings.json. If the file
            // doesn't exist yet (first run), we default to enabled.
            let autostart_enabled = {
                let dir = app.path().app_data_dir();
                let settings_path = dir
                    .as_ref()
                    .ok()
                    .map(|d| d.join("settings.json"));
                let mut enabled = true; // default: enabled
                if let Some(ref path) = settings_path {
                    if path.exists() {
                        if let Ok(content) = std::fs::read_to_string(path) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                                if let Some(v) = json.get("autostart").and_then(|v| v.as_bool()) {
                                    enabled = v;
                                }
                            }
                        }
                    }
                }
                enabled
            };

            #[cfg(target_os = "windows")]
            {
                let exe_path = std::env::current_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                if !exe_path.is_empty() {
                  // Task registration shells out to PowerShell (~2 s measured at boot): off the setup
                  // path, so the first word is not delayed by it (docs/features/123, cause E).
                  let _ = std::thread::Builder::new().name("autostart-task".into()).spawn(move || {
                    // Always remove old HKCU\Run entry (from the previous autostart plugin)
                    // to avoid double-launching.
                    let _ = std::process::Command::new("reg")
                        .args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                               "/v", "NEXUS", "/f"])
                        .creation_flags(0x08000000) // CREATE_NO_WINDOW
                        .status();

                    if autostart_enabled {
                        // Create/update the scheduled task with --background flag
                        let ps_script = format!(
                            r#"$exe = '{}';
                            $user = [Security.Principal.WindowsIdentity]::GetCurrent().Name;
                            $action = New-ScheduledTaskAction -Execute $exe -Argument '--background';
                            $trigger = New-ScheduledTaskTrigger -AtLogOn -User $user;
                            $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -Priority 4 -ExecutionTimeLimit (New-TimeSpan -Seconds 0);
                            $result = Register-ScheduledTask -TaskName 'NEXUS' -Action $action -Trigger $trigger -Settings $settings -User $user -Force;
                            if ($result) {{ Write-Output 'NEXUS_TASK_OK' }} else {{ Write-Output 'NEXUS_TASK_FAIL' }}"#,
                            exe_path
                        );

                        let result = std::process::Command::new("powershell")
                            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
                            .creation_flags(0x08000000) // CREATE_NO_WINDOW
                            .output();

                        match result {
                            Ok(out) if out.status.success()
                                && String::from_utf8_lossy(&out.stdout).contains("NEXUS_TASK_OK") =>
                            {
                                tracing::info!(
                                    "autostart: scheduled task 'NEXUS' created (AtLogOn, --background)"
                                );
                            }
                            Ok(out) => {
                                tracing::warn!(
                                    "autostart: Register-ScheduledTask failed: stdout={} stderr={}",
                                    String::from_utf8_lossy(&out.stdout).trim(),
                                    String::from_utf8_lossy(&out.stderr).trim()
                                );
                            }
                            Err(e) => {
                                tracing::warn!("autostart: failed to run PowerShell: {e}");
                            }
                        }
                    } else {
                        // Autostart disabled — remove the scheduled task if it exists
                        let _ = std::process::Command::new("powershell")
                            .args(["-NoProfile", "-NonInteractive", "-Command",
                                "Unregister-ScheduledTask -TaskName 'NEXUS' -Confirm:$false -ErrorAction SilentlyContinue"])
                            .creation_flags(0x08000000)
                            .output();
                        tracing::info!("autostart: disabled (scheduled task removed)");
                    }
                  });
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                // macOS/Linux: use tauri-plugin-autostart (LaunchAgent / systemd)
                let autostart = app.autolaunch();
                if autostart_enabled {
                    let _ = autostart.enable();
                    tracing::info!("autostart: enabled (LaunchAgent)");
                } else {
                    let _ = autostart.disable();
                    tracing::info!("autostart: disabled");
                }
            }

            // Tray menu.
            tray::setup(app.handle())?;

            // ─── Meeting / privacy mode state ──────────────────────────
            let meeting_state = std::sync::Arc::new(meeting_detect::MeetingState::new());
            app.manage(meeting_state.clone());

            // ─── STT / TTS Local Engine State ──────────────────────────
            let stt_state = stt::SttState::new();
            app.manage(stt_state);

            // ─── Architect Cancellation Registry ──────────────────────
            // Required by analyze_repo_deep and cancel_architect_analysis.
            // Without this, Phase 2 deep scan invoke fails silently.
            app.manage(architect::ArchitectCancels::new());

            let tts_state = tts::TtsState::new();
            let prewarm_cache = tts_state.cache.clone();
            app.manage(tts_state);
            // Phase 2 TTS: edge-tts (cloud) primary, Kokoro (local) fallback.
            // No local engine to pre-warm — edge-tts is cloud (0 MB RAM).
            // Only the cached ack phrases are pre-synthesized at boot.

            // ─── STT Self-Learning State ──────────────────────────────
            app.manage(stt_learning::SttLearningState::new());

            // Wire the meeting state into the wake engine so the audio callback
            // can check `should_suppress_wake()` on every chunk.
            #[cfg(feature = "wakeword-oww")]
            wakeword_oww::set_meeting_state(meeting_state.clone());

            // Spawn the meeting detection polling loop (WASAPI on Windows,
            // process-name detection on macOS/Linux).
            let state_for_loop = meeting_state.clone();
            tauri::async_runtime::spawn(async move {
                meeting_detect::run_detection_loop(state_for_loop).await;
            });

            // Sleep/wake detection via time-jump monitoring.
            // thread::sleep uses the monotonic clock (stops while the system is
            // asleep); SystemTime is the wall clock (jumps forward across sleep).
            // A gap much larger than the sleep interval means the machine just
            // resumed from sleep/hibernate.
            //
            // The sleep-wake watcher remains for future use (e.g. re-init
            // audio device after sleep, refresh app registry, etc.).
            {
                let _state = meeting_state.clone();
                let app_for_resume = app.handle().clone();
                std::thread::Builder::new()
                    .name("sleep-wake-watch".into())
                    .spawn(move || loop {
                        let before = std::time::SystemTime::now();
                        std::thread::sleep(std::time::Duration::from_secs(10));
                        let gap = std::time::SystemTime::now()
                            .duration_since(before)
                            .unwrap_or_default();
                        if gap > std::time::Duration::from_secs(60) {
                            tracing::info!("system resumed from sleep (gap {gap:?})");
                            memcore::briefing::on_resume(app_for_resume.clone(), gap.as_secs());
                        }
                    })
                    .ok();
            }

            // Listen for TTS events from the frontend.
            // When NEXUS starts speaking, suppress wake detection to prevent
            // self-triggering (NEXUS hears its own TTS voice).
            // When TTS ends, resume after a short grace period.
            {
                let state_for_tts = meeting_state.clone();
                let app_for_tts = app.handle().clone();
                app.handle().listen("tts-started", move |_event| {
                    state_for_tts.set_tts_playing(true);
                    tracing::debug!("meeting: TTS started — suppressing wake detection");
                });

                let state_for_tts_end = meeting_state.clone();
                app.handle().listen("tts-ended", move |_event| {
                    // Don't immediately resume — wait 500ms for audio to settle
                    let state = state_for_tts_end.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        state.set_tts_playing(false);
                        tracing::debug!("meeting: TTS ended — resuming wake detection");
                    });
                    let _ = app_for_tts;
                });
            }

            // NOTE: Sidebar/setup/settings/architect windows are NO LONGER created
            // at startup. They are created on-demand by dyn_windows.rs when first
            // needed, and destroyed when closed. This saves ~1 GB of RAM at idle
            // (each WebView2 window spawns ~7 processes = ~250 MB).
            //
            // Platform-specific effects (DWM corners, macOS vibrancy) are applied
            // inside dyn_windows::get_or_create_window() at creation time.

            // WebView2 permission handler — auto-approves mic/camera for our
            // own app origins so the permission dialog never re-appears.
            mic_permissions::init(app);

            // Compute + cache the orb's bottom-center rect (just above the
            // taskbar/dock) so the stage frontend's pending-pull fallback
            // has something correct even if it mounts before this emit
            // reaches a live listener.
            window_manager::emit_orb_rect(app.handle());

            // Pre-index installed apps for instant launch (background thread).
            app_registry::init();

            // Start the foreground window tracker for architect repo detection.
            // This caches the last non-NEXUS foreground window title so that
            // `get_active_repo_url()` can detect the user's browser/GitHub
            // app even after the NEXUS orb steals focus during STT.
            architect::start_foreground_tracker();

            // Boot-time housekeeping (Phase B–D modules).
            commands::note_boot();
            if let Ok(app_data) = app.path().app_data_dir() {
                usage_counter::init(&app_data);
                tts::init_disk_cache(&app_data); // persisted stock voice lines (instant acks, no re-render per boot)
                diary::log_boot_rollup(&app_data);
                // Last session's resume points, captured BEFORE the recorder
                // writes anything for this session (boot briefing reads them).
                memcore::briefing::snapshot_at_boot(&app_data);
                // What was open when the PC last went off becomes the pending
                // "shall I reopen it?" offer; only then may the 1-minute
                // snapshots start overwriting the rolling one.
                memcore::session::at_boot(&app_data, chrono::Utc::now().timestamp());
                memcore::session::start_recorder(app_data.clone());
                memcore::session::install_session_end_hook(app_data.clone());
                agent_specs::ensure_example(&app_data);
                let improve_dir = app_data.clone();
                std::thread::spawn(move || {
                    let clusters = improve::run_miner(&improve_dir);
                    if !clusters.is_empty() {
                        tracing::info!("improve: boot miner found {} cluster(s)", clusters.len());
                    }
                });
            }
            webhook::spawn_listener(app.handle().clone());
            keep_awake::start_working_poller(); // screen stays awake while a ghost drill/request is running
            agent_watch::start_idle_monitor(); // kills the Node sidecar after 10 min with no active watch
            google::sentinel::start_sentinel_poller(app.handle().clone());
            proactive_policy::start_ticker(app.handle().clone()); // releases deferred alerts at breakpoints
            memcore::resume::start_recorder(app.handle().clone()); // 30 s foreground-window resume points
            memcore::briefing::start_boot_briefing(app.handle().clone()); // once-a-day spoken "where you left off"
            memcore::scheduler::start(app.handle().clone()); // timetable slot reminders
            memcore::mailwatch::start(app.handle().clone()); // Gmail history.list watcher (idle until Google is connected)
            memcore::leavewatch::start(app.handle().clone()); // leave-by reminders (needs calendar + Routes key + saved home)
            memcore::leases::start(app.handle().clone()); // temporary action leases (auto-unblock)
            memcore::wa::start(app.handle().clone()); // WhatsApp priority watcher (read-only; off unless memcoreWhatsapp)

            // ─── Pre-warm TTS + STT + NLU at startup (Phase 1) ──────────
            // Eliminates ~31.7s of cold-start latency on the first voice command.
            //
            // RAM cost: +600 MB idle (TTS ~350 MB + STT ~150 MB + NLU ~100 MB)
            // Latency saved: ~31.7s on first command (TTS 5.7s + STT 8s + NLU 18s)
            //
            // Priority order: TTS first (needed for "On it sir" ack),
            // then STT (needed for transcription), then NLU (needed for
            // ambiguous commands — deterministic parser handles most).
            //
            // TTS pre-warm: pre-synthesize ack phrases using edge-tts (cloud).
            // This generates the 5 cached phrases ("On it sir", etc.) so
            // speak_cached() plays in <5ms. No local engine to load —
            // edge-tts is cloud (0 MB RAM). Falls back to the local Kokoro voice if offline.
            let prewarm_cache2 = prewarm_cache.clone();
            let prewarm_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tracing::info!("tts: startup cache pre-generation starting...");
                // the equipped persona's cloud voice (was hardcoded Ava: acks and replies disagreed after a restart)
                let voice = commands::read_edge_tts_voice(&prewarm_app);
                tts::pregenerate_cache(&prewarm_cache2, &voice).await;
                tracing::info!("tts: startup cache pre-generation complete — ack phrases ready");
            });

            // Start TTS network monitor — re-probes every 30s while down
            // (cloud-restored watchdog, P4), every 60s while up, and
            // unloads Kokoro after 10 minutes of stable network.
            tts_network::start_network_monitor(app.handle().clone());

            // Offline voice (Feature 83): make sure the equipped persona's Kokoro voice is installed.
            // First run downloads the shared model once (background, cloud keeps speaking meanwhile);
            // afterwards it is a 0.5 MB voice file. Delayed so it never competes with boot, and only
            // attempted while online (the network watchdog re-runs it when the cloud returns).
            {
                let h = app.handle().clone();
                if let Some(p) = voice_catalog::find_by_key(&commands::read_selected_voice(&h)) {
                    tts_kokoro::set_preferred_voice(p.kokoro_voice);
                }
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                    if tts_network::check_network_now().await {
                        if let Some(engine) = tts::kokoro_engine_handle() {
                            tts_swap::sync_selected_voice(&h, engine);
                        }
                    }
                });
            }

            // STT pre-warm removed in Phase 2.
            // Primary STT is now Groq cloud (0 MB RAM, ~247ms latency).
            // Local faster-whisper starts lazily only as a fallback when
            // Groq is unavailable (no key, network error, rate limit).
            // This saves ~150 MB idle RAM.

            // NLU pre-warm removed in Phase 2.
            // The deterministic Rust parser handles 90-95% of commands
            // in <5ms with only 2 MB RAM. The BERT-Mini Python sidecar
            // starts lazily only when an ambiguous command is encountered.
            // This saves ~100 MB idle RAM.

            // Global hotkey → wake event (not available on Linux).
            #[cfg(not(target_os = "linux"))]
            hotkey::init(app.handle())?;

            // Wake-word engine — runs on a DEDICATED OS THREAD, not tokio.
            // tract-onnx model optimization is CPU-heavy blocking work that
            // can take 30-120s on a cold boot. Running it on tokio's async
            // runtime (which is single-threaded in NEXUS) would block ALL
            // other async tasks (meeting detection, network bridge, sidecar
            // health check) for the entire duration.
            //
            // The hotkey still works immediately (registered above) — the
            // user can press Ctrl+Space while the wake engine loads.
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("wake-engine".into())
                .spawn(move || {
                    if let Err(e) = wakeword::run(handle) {
                        tracing::error!("wake-word engine stopped: {e}");
                    }
                })
                .ok();

            // Network bridge (HTTP) sends transcripts to the Cloudflare Worker.
            // No sidecar, no server, no WebSocket — fully serverless.
            // The Worker URL is baked into the installer via NEXUS_SERVER_URL.

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = network::run(handle).await {
                    tracing::error!("network bridge stopped: {e}");
                }
            });

            // Telegram remote (owner-only, ₹0 phone control). Starts only
            // when a bot token is in the vault AND telegramChatId is set —
            // otherwise logs once and stays off.
            crate::telegram::spawn_bridge(app.handle().clone());

            // 9Router provider health probe — checks free-tier model menus
            // (Groq/Gemini/Cerebras IDs die silently; Sept 2026 llama 404).
            // Non-blocking, logs warnings only. Zero inference cost.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::router::probe_provider_health(&handle).await;
            });

            // Start STT idle monitor — kills the Python STT sidecar after 5 min
            // of inactivity to reclaim ~340 MB RAM.
            crate::lazy_stt::start_idle_monitor();

            // Pre-warm local STT ~20s after boot (background, non-blocking) so
            // the first voice command answers with zero cold-start delay.
            // No-op for Groq cloud users (saves RAM).
            if let Ok(dir) = app.path().app_data_dir() {
                crate::lazy_stt::spawn_prewarm(dir);
            }

            // NLU sidecar is on-demand fallback only (never pre-warmed at boot
            // to enforce strict <150MB RAM limit in online mode).

            // Vault idle monitor: watches credential expiry while the user
            // is away (90s cadence, edge-triggered). Alerts land in the log
            // + `vault:changed` event; the Connections tab refreshes itself.
            crate::auth_vault::spawn_monitor(app.handle().clone());

            // Listen for deep-link events (macOS emits these; Windows/Linux use single-instance).
            let handle = app.handle().clone();
            let _ = app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_str = url.as_str();
                    if url_str.starts_with("nexus://oauth/") {
                        let _ = handle.emit("deep-link://oauth-callback", url_str);
                    } else if url_str == "nexus://settings" {
                        // Deep link to open settings sidebar
                        let app_clone = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = crate::commands::show_settings_sidebar(app_clone).await;
                        });
                    }
                }
            });

            // Check if this is first launch (no config file yet).
            // Auto-generate a unique user ID and device ID (UUID v4) and use
            // the server URL baked into the installer. The user never has to
            // manually enter these — they're system-generated.
            //
            // The server URL is determined at build time:
            //   - Default: ws://127.0.0.1:41098/ws (local dev / same-machine sidecar)
            //   - Installer override: set NEXUS_SERVER_URL env var before building
            //     the installer to bake in the user's remote server URL.
            let store_path = app.path().app_data_dir().ok();
            let mut should_open_setup = std::env::args().any(|arg| arg == "--setup" || arg == "-s");
            if let Some(dir) = store_path {
                let config_path = dir.join("nexus-config.json");
                if !config_path.exists() {
                    should_open_setup = true;
                    let user_id = format!("user_{}", network::uuid_v4());
                    let device_id = format!("device_{}", network::uuid_v4());
                    let server_url = option_env!("NEXUS_SERVER_URL")
                        .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");
                    // Feature 88: client UUIDs are PROVISIONAL hints. The
                    // canonical profile_id is issued by the Worker claim
                    // handshake (setup Accounts step) — never here.
                    let default_config = serde_json::json!({
                        "serverUrl": server_url,
                        "userId": user_id,
                        "deviceId": device_id,
                        "identity": "provisional",
                    });
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(&config_path, default_config.to_string());
                    tracing::info!(
                        "auto-created config at {:?} — user={}, device={}, server={}",
                        config_path, user_id, device_id, server_url
                    );
                }
            }

            // Auto-open the network session from saved config so that
            // diagnostics, architect, and transcript commands have the
            // user_id available before the frontend calls open_session.
            // This fixes "Not configured" diagnostics and GitHub token
            // lookup failures when the user hasn't spoken yet.
            if let Some(dir) = app.path().app_data_dir().ok() {
                let config_path = dir.join("nexus-config.json");
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        let default_url = option_env!("NEXUS_SERVER_URL")
                            .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");
                        let url = json["serverUrl"].as_str().unwrap_or(default_url);
                        let url = if url.is_empty() { default_url } else { url };
                        let uid = json["userId"].as_str().unwrap_or("");
                        let did = json["deviceId"].as_str().unwrap_or("");
                        if !uid.is_empty() {
                            network::open_session_from_config(url, uid, did);
                        }
                    }
                }
            }

            // NLU model update check — family devices pull admin-trained
            // BERT-Mini updates from the Worker (R2 + KV manifest).
            // Runs in background; session was just auto-opened above.
            // No-op if the Worker has no manifest or R2 is unconfigured.
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Small delay: let the app finish first-paint before a
                // potential ~35 MB model download saturates the connection.
                tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                nlu_update::spawn_update_check(update_handle);
            });

            // TEMPORARY dev-persist (remove before release): if the user
            // enabled "keep calibrator open", auto-open the calibration
            // HUD + wakeup preview shortly after boot so live cross-checks
            // survive rebuilds/restarts. Session Cancel still closes it.
            if calibration::dev_persist_enabled(app.handle()) {
                let persist_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    if let Err(e) = calibration::show_calibration_hud(persist_handle).await {
                        tracing::warn!("calibration: dev-persist auto-open failed: {e}");
                    }
                });
            }

            if should_open_setup {
                // The orb starts hidden by default (frontend `visible`
                // state defaults to false) — nothing to hide here now that
                // it's a stage-hosted div instead of its own window; it
                // simply never shows itself during setup since nothing
                // tells it to wake.
                if let Ok(win) = crate::dyn_windows::get_or_create_window(app.handle(), crate::dyn_windows::WindowConfig::setup()) {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            } else {
                // ─── --background flag: silent tray-only startup ────────
                //
                // When launched by the scheduled task (auto-start), the app
                // passes --background. The orb stays hidden (default
                // frontend state) and the app runs silently in the system
                // tray. The user activates it via wake word, hotkey, or
                // tray click.
                let is_background = std::env::args().any(|arg| arg == "--background");
                if is_background {
                    tracing::info!("startup: --background mode — orb hidden, tray only");
                }

                // ─── --wake flag: summon the orb ─────────────────────────
                //
                // The Linux desktop shortcut / custom keybinding runs
                // `nexus --wake` (the global hotkey is compiled out there).
                // When NEXUS is already running, the single-instance callback
                // above forwards the flag and wakes it. A COLD start has to
                // wake itself — and only once the stage renderer is alive,
                // otherwise the wake event fires before React mounts and the
                // orb stays idle.
                if std::env::args().any(|arg| arg == "--wake") {
                    tracing::info!("startup: --wake flag — summoning the orb");
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if !crate::stage::wait_until_ready(20_000).await {
                            tracing::warn!("startup: --wake — the stage never beat; summoning anyway");
                        }
                        crate::window_manager::wake_orb_src(&handle, "wake-flag");
                    });
                }
            }

            // Run connection diagnostics on startup.
            // This checks STT, TTS, Cloudflare Worker, GitHub, and Google
            // and logs a formatted status table to stdout.
            // Compute the data dir BEFORE the spawn — capturing the generic
            // AppHandle in a plain thread closure drags non-Send wry
            // internals across the boundary; an owned PathBuf is Send.
            let diag_data_dir = app.path().app_data_dir().ok();
            std::thread::spawn(move || {
                // Wait 5s for the network session to be established.
                std::thread::sleep(std::time::Duration::from_secs(5));
                let (worker_url, user_id) = match network::get_session_info() {
                    Some((url, uid, _)) => (url, uid),
                    None => {
                        // Try reading from config file
                        let config_path = std::env::var("APPDATA")
                            .ok()
                            .map(|d| std::path::Path::new(&d)
                                .join("com.nexus.assistant")
                                .join("nexus-config.json"));
                        match config_path.and_then(|p| std::fs::read_to_string(p).ok()) {
                            Some(content) => {
                                let server_url = extract_json_string(&content, "serverUrl")
                                    .unwrap_or_default();
                                let uid = extract_json_string(&content, "userId")
                                    .unwrap_or_default();
                                (server_url, uid)
                            }
                            None => (String::new(), String::new()),
                        }
                    }
                };
                diagnostics::log_diagnostics(&worker_url, &user_id, diag_data_dir.as_deref());
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_manager::set_orb_interactive,
            window_manager::set_orb_position,
            window_manager::get_pending_orb_rect,
            window_manager::get_pending_orb_position,
            window_manager::get_pending_loading_rect,
            network::open_session,
            network::send_transcript,
            network::cancel_session,
            network::close_session,
            orchestrator::orchestrator_process,
            orchestrator::orchestrator_cancel,
            orchestrator::orchestrator_done,
            orchestrator::orchestrator_status,
            orchestrator::orchestrator_show_loading,
            orchestrator::orchestrator_hide_loading,
            orchestrator::orchestrator_github_execute,
            orchestrator::orchestrator_github_clear_token,
            orchestrator::orchestrator_mcp_confirm,
            mcp_client::mcp_status,
            mcp_client::mcp_connect_state,
            stage::stage_show,
            stage::stage_hide,
            stage::stage_heartbeat,
            stage::stage_take_pending_transcript,
            last_heard::last_stt_transcript,
            last_heard::stt_resubmit_transcript,
            stage::stage_set_hitboxes,
            stage::stage_hide_kill,
            ghost::ghost_enter,
            ghost::ghost_exit,
            ghost::ghost_abort,
            ghost::get_pending_ghost_session,
            ghost::ghost_drill_running,
            calibration::show_calibration_hud,
            calibration::preview_companion_hud,
            calibration::calibration_set_target,
            calibration::calibration_report_position,
            calibration::calibration_report_size,
            calibration::calibration_nudge,
            calibration::calibration_apply_drafts,
            calibration::calibration_default_target,
            calibration::calibration_save,
            calibration::calibration_cancel,
            calibration::calibration_dev_persist,
            auth_vault::vault_status,
            auth_vault::vault_set_token,
            auth_vault::vault_clear_token,
            commands::open_setup_window,
            commands::close_setup_window,
            commands::save_server_config,
            commands::get_server_config,
            commands::claim_profile,
            commands::get_identity_status,
            commands::refresh_identity_status,
            commands::disconnect_device,
            commands::meeting_active,
            commands::is_nexus_paused,
            commands::meeting_status,
            commands::set_meeting_detection,
            commands::open_settings_window,
            commands::close_settings_window,
            hand::hand_frame,
            hand::hand_error,
            hand::hand_info,
            hand::hand_stats,
            hand::hand_config,
            hand::hand_start,
            hand::hand_stop,
            hand::hand_status,
            commands::open_orb_studio_window,
            commands::close_orb_studio_window,
            commands::set_orb_morphology,
            commands::set_orb_capsule_style,
            commands::test_wake_orb,
            commands::get_settings,
            commands::save_settings,
            commands::settings_patch,
            commands::get_health_status,
            commands::export_settings,
            commands::import_settings,
            commands::memory_recall,
            commands::memory_forget,
            commands::memcore_status,
            commands::memcore_list,
            commands::memcore_egress_log,
            commands::memcore_clear_activity,
            commands::offer_pending,
            commands::memcore_timetable,
            commands::memcore_timetable_delete,
            commands::memcore_mail_list,
            commands::memcore_mail_mute,
            commands::memcore_people,
            commands::open_maps_window,
            commands::memcore_person_flag,
            commands::whatsapp_selftest,
            commands::diary_summary,
            commands::webhook_token,
            commands::webhook_rotate_token,
            commands::improvement_report,
            commands::vision_quota_status,
            commands::vision_key_status,
            commands::vision_test_key,
            commands::list_tts_voices,
            commands::get_pending_settings_backdrop,
            commands::set_autostart,
            commands::is_autostart_enabled,
            commands::check_mic_permission,
            commands::open_mic_settings,
            commands::clear_transcript,
            commands::refresh_app_registry,
            commands::show_sidebar,
            commands::show_sidebar_with_content,
            commands::show_sidebar_with_analysis,
            commands::show_sidebar_with_confirmation,
            commands::hide_sidebar,
            commands::get_pending_sidebar_content,
            commands::show_sidebar_view,
            commands::set_sidebar_dock,
            commands::get_pending_sidebar_view,
            commands::get_pending_spatial,
            commands::get_pending_annotation,
            commands::show_pr_list_sidebar,
            commands::hide_pr_list_sidebar,
            commands::get_pending_pr_list,
            commands::debug_trace,
            commands::show_settings_sidebar,
            commands::hide_settings_sidebar,
            commands::pause_wakeword,
            commands::resume_wakeword,
            commands::mic_self_test,
            commands::start_stt_capture,
            directed::directed_gate,
            usage_counter::get_usage_stats,
            api_keys::api_keys_list,
            api_keys::api_key_set,
            api_keys::api_key_delete,
            api_keys::api_keys_count,
            github_profile::github_profile,
            github_profile::github_profile_clear,
            proactive_policy::proactive_snooze,
            commands::stop_stt_capture,
            commands::stt_capture_had_speech,
            commands::google_get_accounts,
            commands::google_connect_account,
            commands::google_connect_extended,
            commands::google_set_primary_account,
            commands::google_disconnect_account,
            commands::memory_seed_setup_identity,
            commands::google_save_custom_credentials,
            commands::google_credentials_status,
            stt::transcribe_audio,
            stt::stt_filter_stats,
            stt_stream::stt_stream_start,
            stt_stream::stt_stream_push_chunk,
            stt_stream::stt_stream_stop,
            tts::speak_text,
            tts::speak_cached,
            tts::stop_tts,
            tts::restore_tts_volume,
            shortcuts::exec::shortcut_do,
            tts::preview_voice,
            tts::set_voice_preference,
            tts::list_voice_personas,
            tts::get_voice_status,
            tts::get_voice_sync_state,
            tts::get_voice_transport,
            stt_learning::log_failed_transcript,
            stt_learning::log_successful_transcript,
            stt_learning::get_learned_corrections,
            missed_intent_logger::get_missed_intents,
            command_executor::execute_command,
            intent_parser::parse_transcript,
            architect::get_active_repo_url,
            architect::open_architect_window,
            architect::open_architect_with_auto_detect,
            architect::get_pending_architect_repo,
            architect::analyze_repo_phase1,
            architect::analyze_repo_deep,
            architect::analyze_repo_fast,
            architect::enrich_phase1,
            architect::cancel_architect_analysis,
            architect::query_impact,
            // Agent-completion watcher (Claude Code, Gemini CLI, Codex, Aider, Hermes, opencode)
            agent_watch::agent_watch_list_adapters,
            agent_watch::agent_watch_unavailable_reason,
            agent_watch::agent_watch_start,
            agent_watch::agent_watch_stop,
            agent_watch::agent_watch_status,
            agent_watch::agent_watch_list_watches,
            // Live mode commands
            live::live_type_text,
            live::live_press_key,
            live::live_press_hotkey,
            live::live_whatsapp_open,
            live::live_whatsapp_search,
            live::live_whatsapp_send,
            live::live_whatsapp_type_message,
            live::live_browser_new_tab,
            live::live_browser_navigate,
            live::live_browser_search,
            live::live_open_site,
            live::live_focus_app,
            live::live_cancel,
            live::live_get_state,
            // Ghost desktop drill commands
            live::live_ghost_whatsapp,
            live::live_ghost_click,
            live::live_ghost_calibrate,
            // Phase D: OWW voice profile commands (speaker verification)
            commands::get_voice_profile_status,
            commands::enroll_voice,
            commands::delete_voice_profile,
            // Selective hitbox registry (live_glass) & Adaptive Luminance commands.
            // NOTE: the DWM Acrylic path was removed per ADR-05 (docs/architecture/06).
            live_glass::register_glass_hitboxes,
            luminance_probe::get_screen_luminance,
        ])
        .build(tauri::generate_context!())
        .expect("error while building NEXUS application")
        .run(|_app_handle, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => api.prevent_exit(),
            // A real exit must never leave the system volume at the TTS level, nor the
            // screen/system forced awake.
            tauri::RunEvent::Exit => {
                volume::force_restore_volume();
                keep_awake::force_clear();
            }
            _ => {}
        });
}

/// Simple JSON string value extractor (avoids pulling serde_json for one field).
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let idx = json.find(&pattern)?;
    let after = &json[idx + pattern.len()..];
    let colon = after.find(':')?;
    let after_colon = &after[colon + 1..];
    let quote_start = after_colon.find('"')?;
    let after_quote = &after_colon[quote_start + 1..];
    let quote_end = after_quote.find('"')?;
    Some(after_quote[..quote_end].to_string())
}
