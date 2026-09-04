//! NEXUS — Tauri v2 main process.
//!
//! Wires up: window manager (click-through), global hotkey, autostart, tray,
//! wake-word engine, the WSS network bridge, deep-link (OAuth redirects),
//! and window positioning (bottom-center sidebar).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod window_manager;
mod hotkey;
// wakeword-oww (default): openWakeWord via tract-onnx (pure Rust, no C++ deps)
#[cfg(feature = "wakeword-oww")]
mod wakeword_oww;
#[cfg(feature = "wakeword-oww")]
mod wakeword {
    pub use crate::wakeword_oww::*;
}
mod network;
mod tray;
pub mod commands;
mod command_executor;
mod app_registry;
pub mod intent_parser;
mod nlu_client;
mod lazy_nlu;
mod lazy_stt;
mod stt;
mod stt_learning;
mod tts;
// Verification is not yet wired into wakeword_oww (see AGENTS.md known limitations).
mod meeting_detect;
mod mic_permissions;
mod mpris;
mod architect;
mod dyn_windows;
mod diagnostics;
#[cfg(target_os = "windows")]
mod dwm_corners;
#[cfg(target_os = "windows")]
mod sidebar_backdrop;

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

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tracing::info!("single-instance: secondary launch attempt with args: {:?}", args);
            // Handle deep-link redirects on Windows/Linux (passed as CLI arg)
            if let Some(url) = args.iter().find(|a| a.starts_with("nexus://")) {
                tracing::info!("single-instance: deep-link callback: {}", url);
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

            if is_background && !is_setup && !is_settings {
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
                if let Ok(win) = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::settings()) {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            } else {
                // Only wake the main window if we are NOT in the middle of setup
                let setup_active = app.get_webview_window("setup").is_some();
                if !setup_active {
                    if let Some(main_win) = app.get_webview_window("main") {
                        let _ = main_win.show();
                        let _ = crate::window_manager::configure_non_activating_overlay(&main_win);
                        let _ = main_win.eval("window.__NEXUS_WAKE__ && window.__NEXUS_WAKE__()");
                    }
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build());

    #[cfg(not(target_os = "linux"))]
    let mut builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_positioner::init())
        .setup(|app| {
            // macOS: hide from the Dock and Cmd+Tab switcher (accessory/background app).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Create the main window on-demand (only in the primary instance).
            // Removing it from tauri.conf.json prevents the single-instance
            // secondary launch from flashing a blank window before exiting.
            let _ = crate::dyn_windows::get_or_create_window(app.handle(), crate::dyn_windows::WindowConfig::main());

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
                            $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Seconds 0);
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
            let stt_state = stt::SttState { _placeholder: std::sync::Arc::new(tokio::sync::Mutex::new(())) };
            app.manage(stt_state);

            let tts_engine_arc = std::sync::Arc::new(tokio::sync::Mutex::new(None));
            let tts_cache_arc = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
            let tts_sample_rate_arc = std::sync::Arc::new(tokio::sync::Mutex::new(22050u32));
            let tts_state = tts::TtsState { engine: tts_engine_arc.clone(), cache: tts_cache_arc.clone(), sample_rate: tts_sample_rate_arc.clone() };
            app.manage(tts_state);
            // Piper TTS is lazy-loaded on first speak_text call (saves ~80 MB at idle).
            // See tts::ensure_engine_loaded().

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

            // Window overlay + click-through.
            window_manager::init(app.handle())?;

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

            // Position the orb at bottom-center, just above the taskbar/dock.
            if let Some(win) = app.get_webview_window("main") {
                let _ = window_manager::position_orb(&win);
            }

            // Pre-index installed apps for instant launch (background thread).
            app_registry::init();

            // NLU pre-warm is deferred — it will be started lazily on the first
            // unparseable command via lazy_nlu::ensure_nlu_running(). This saves
            // 50-100 MB RAM at idle. The deterministic parser handles most
            // commands without NLU.

            // Global hotkey → wake event.
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

            // Start STT idle monitor — kills the Python STT sidecar after 5 min
            // of inactivity to reclaim ~340 MB RAM.
            crate::lazy_stt::start_idle_monitor();

            // Listen for deep-link events (macOS emits these; Windows/Linux use single-instance).
            let handle = app.handle().clone();
            let _ = app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    let url_str = url.as_str();
                    if url_str.starts_with("nexus://oauth/") {
                        let _ = handle.emit("deep-link://oauth-callback", url_str);
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
                    let default_config = serde_json::json!({
                        "serverUrl": server_url,
                        "userId": user_id,
                        "deviceId": device_id,
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

            if should_open_setup {
                // Hide the orb during setup — it should not steal focus or
                // appear behind the setup wizard on first launch.
                if let Some(main_win) = app.get_webview_window("main") {
                    let _ = main_win.hide();
                }
                if let Ok(win) = crate::dyn_windows::get_or_create_window(app.handle(), crate::dyn_windows::WindowConfig::setup()) {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            } else {
                // ─── --background flag: silent tray-only startup ────────
                //
                // When launched by the scheduled task (auto-start), the app
                // passes --background. In this mode, the main orb window stays
                // hidden and the app runs silently in the system tray.
                // The user activates it via wake word, hotkey, or tray click.
                let is_background = std::env::args().any(|arg| arg == "--background");
                if is_background {
                    tracing::info!("startup: --background mode — orb hidden, tray only");
                    if let Some(main_win) = app.get_webview_window("main") {
                        let _ = main_win.hide();
                    }
                }
            }

            // Run connection diagnostics on startup.
            // This checks STT, TTS, Cloudflare Worker, GitHub, and Google
            // and logs a formatted status table to stdout.
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
                diagnostics::log_diagnostics(&worker_url, &user_id);
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_manager::set_click_through,
            window_manager::show_overlay,
            window_manager::hide_overlay,
            network::open_session,
            network::send_transcript,
            network::cancel_session,
            network::close_session,
            commands::open_setup_window,
            commands::close_setup_window,
            commands::save_server_config,
            commands::get_server_config,
            commands::meeting_active,
            commands::is_nexus_paused,
            commands::meeting_status,
            commands::set_meeting_detection,
            commands::open_settings_window,
            commands::close_settings_window,
            commands::get_settings,
            commands::save_settings,
            commands::set_autostart,
            commands::is_autostart_enabled,
            commands::check_mic_permission,
            commands::open_mic_settings,
            commands::clear_transcript,
            commands::refresh_app_registry,
            commands::show_sidebar,
            commands::show_sidebar_with_content,
            commands::show_sidebar_with_analysis,
            commands::hide_sidebar,
            commands::get_pending_sidebar_content,
            commands::pause_wakeword,
            commands::resume_wakeword,
            stt::transcribe_audio,
            stt::stt_status,
            tts::speak_text,
            tts::speak_cached,
            tts::stop_tts,
            stt_learning::log_failed_transcript,
            stt_learning::log_successful_transcript,
            stt_learning::get_learned_corrections,
            diagnostics::nexus_diagnostics,
            command_executor::execute_command,
            intent_parser::parse_transcript,
            architect::get_active_repo_url,
            architect::open_architect_window,
            architect::get_pending_architect_repo,
            architect::analyze_repo_phase1,
            architect::analyze_repo_deep,
            architect::query_impact,
            architect::enrich_phase1,
            architect::analyze_repo_fast,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NEXUS application");
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
