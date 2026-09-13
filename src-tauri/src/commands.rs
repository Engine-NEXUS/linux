//! IPC commands for setup window management and configuration.

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(feature = "wakeword-sherpa")]
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager, Runtime};
#[cfg(not(target_os = "windows"))]
use tauri_plugin_autostart::ManagerExt;

#[cfg(feature = "wakeword-sherpa")]
use crate::voice_profile;
use crate::app_registry;

// ─── Pending sidebar content ───────────────────────────────────────
//
// When the sidebar window is created on-demand, the WebView2 needs time
// to load sidebar.html and mount the React app before it can receive
// Tauri events. If we emit `sidebar:show` / `sidebar:backdrop` immediately
// after creating the window, those events are lost because no listener
// exists yet.
//
// Instead, we store the content + backdrop here. The frontend calls
// `get_pending_sidebar_content` on mount, which returns and clears the
// pending data. This is race-free regardless of how long the WebView
// takes to load.

#[derive(Clone)]
struct PendingSidebar {
    query: String,
    text: String,
    backdrop: Option<String>, // data:image/png;base64,... URI
    analysis: Option<serde_json::Value>, // structured repo analysis data
}

static PENDING_SIDEBAR: Mutex<Option<PendingSidebar>> = Mutex::new(None);

// ─── Pending PR list (race-free sidebar creation) ──────────────────────
// Same pattern as PENDING_SIDEBAR: the orchestrator stores the PR list
// result here before creating the sidebar window. The frontend calls
// `get_pending_pr_list` on mount to fetch (and clear) the pending data.
// This is race-free regardless of how long the WebView takes to load.
static PENDING_PR_LIST: Mutex<Option<serde_json::Value>> = Mutex::new(None);

/// Store a pending PR list result (called from the orchestrator before
/// creating the sidebar window). The frontend fetches this on mount.
pub fn set_pending_pr_list(pr_list_json: serde_json::Value) {
    let mut pending = PENDING_PR_LIST.lock().unwrap();
    *pending = Some(pr_list_json);
}

/// IPC: Fetch pending PR list data (called by the PR list sidebar on mount).
/// Returns the PR list JSON that was stored by the orchestrator, or null
/// if no data is pending. Clears the pending data after returning.
#[tauri::command]
pub fn get_pending_pr_list() -> Result<Option<serde_json::Value>, String> {
    let mut pending = PENDING_PR_LIST.lock().unwrap();
    let data = pending.take();
    if data.is_some() {
        tracing::info!("pr-list: pending data fetched by frontend");
    }
    Ok(data)
}

/// IPC: open the setup window (called from tray menu "Settings…" or first launch).
/// Creates the window on-demand if it doesn't exist (saves ~250 MB RAM at idle).
#[tauri::command]
pub fn open_setup_window<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let win = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::setup())?;
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// IPC: close/destroy the setup window and activate the main assistant orb.
/// Destroys the window (not just hide) to free ~250 MB of WebView2 processes.
/// If `first_run` is true, speaks the first-run greeting instead of waking.
#[tauri::command]
pub fn close_setup_window<R: Runtime>(
    app: tauri::AppHandle<R>,
    first_run: Option<bool>,
) -> Result<(), String> {
    let _ = crate::dyn_windows::destroy_window(&app, "setup");
    if let Some(main_win) = app.get_webview_window("main") {
        let _ = main_win.show();
        let _ = crate::window_manager::configure_non_activating_overlay(&main_win);
        let _ = main_win.set_ignore_cursor_events(false);
        if first_run.unwrap_or(false) {
            let _ = main_win.eval(
                "window.__NEXUS_FIRST_RUN_GREETING__ && window.__NEXUS_FIRST_RUN_GREETING__()",
            );
        } else {
            let _ = main_win.eval("window.__NEXUS_WAKE__ && window.__NEXUS_WAKE__()");
        }
    }
    Ok(())
}

/// IPC: save the server URL config (marks setup as complete).
/// Writes a JSON file to the app data dir so the app knows setup is done.
#[tauri::command]
pub fn save_server_config<R: Runtime>(
    app: tauri::AppHandle<R>,
    server_url: String,
    user_id: String,
    device_id: String,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config_path = dir.join("nexus-config.json");
    let config = serde_json::json!({
        "serverUrl": server_url,
        "userId": user_id,
        "deviceId": device_id,
    });
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, config.to_string()).map_err(|e| e.to_string())?;
    tracing::info!("server config saved to {:?}", config_path);
    Ok(())
}

/// Serialized server config returned by `get_server_config`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub server_url: String,
    pub user_id: String,
    pub device_id: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_url: option_env!("NEXUS_SERVER_URL")
                .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev")
                .to_string(),
            user_id: String::new(),
            device_id: String::new(),
        }
    }
}

/// IPC: Get the saved server config (or defaults if not yet configured).
/// The frontend calls this at startup to get the Worker URL, user ID,
/// and device ID — instead of relying on build-time env vars.
#[tauri::command]
pub fn get_server_config<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<ServerConfig, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config_path = dir.join("nexus-config.json");
    if !config_path.exists() {
        return Ok(ServerConfig::default());
    }
    let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let default_url = option_env!("NEXUS_SERVER_URL")
        .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");
    // Use saved URL, but fall back to default if the saved URL is empty
    // (a previous save_settings call may have written an empty string).
    let saved_url = json["serverUrl"].as_str().unwrap_or(default_url);
    let server_url = if saved_url.is_empty() { default_url } else { saved_url };
    Ok(ServerConfig {
        server_url: server_url.to_string(),
        user_id: json["userId"].as_str().unwrap_or("").to_string(),
        device_id: json["deviceId"].as_str().unwrap_or("").to_string(),
    })
}

// ─── Voice profile commands — only available when wakeword-sherpa is enabled ─
// Speaker enrollment uses sherpa-onnx for embedding extraction. When using the
// default wakeword-oww engine, verification is not yet wired (see AGENTS.md),
// so these commands are compiled out to avoid pulling in sherpa-onnx C++ deps.
#[cfg(feature = "wakeword-sherpa")]
pub use voice_profile_commands::*;
#[cfg(feature = "wakeword-sherpa")]
mod voice_profile_commands {
    use super::*;

    /// IPC: Get the current voice profile status (enrolled or not, number of clips, threshold).
    #[tauri::command]
    pub fn get_voice_profile_status<R: Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<voice_profile::VoiceProfileStatus, String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let profile_path = voice_profile::resolve_profile_path(&dir);

        let sound_alikes: Vec<String> = voice_profile::SOUND_ALIKES
            .iter()
            .map(|s| s.to_string())
            .collect();

        if !profile_path.exists() {
            return Ok(voice_profile::VoiceProfileStatus {
                enrolled: false,
                num_clips: 0,
                threshold: voice_profile::DEFAULT_THRESHOLD,
                created_at: 0,
                updated_at: 0,
                wake_variants: vec!["nexus".to_string()],
                sound_alikes,
            });
        }

        let profile = voice_profile::VoiceProfile::load(&profile_path)
            .map_err(|e| e.to_string())?;
        Ok(voice_profile::VoiceProfileStatus {
            enrolled: true,
            num_clips: profile.num_clips,
            threshold: profile.threshold,
            created_at: profile.created_at,
            updated_at: profile.updated_at,
            wake_variants: profile.wake_variants,
            sound_alikes,
        })
    }

    /// Resolve the sherpa resource directory (handles dev + production paths).
    pub fn resolve_sherpa_dir(resource_dir: &Path) -> Option<PathBuf> {
        // Production: resource_dir/resources/sherpa (Tauri v2 on Windows: resource_dir() = exe_dir)
        let sherpa = resource_dir.join("resources").join("sherpa");
        if sherpa.exists() {
            return Some(sherpa);
        }
        // Fallback: resource_dir/sherpa (some Tauri versions may return resources/ directly)
        let sherpa_alt = resource_dir.join("sherpa");
        if sherpa_alt.exists() {
            return Some(sherpa_alt);
        }
        if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
            let dev = PathBuf::from(manifest).join("resources").join("sherpa");
            if dev.exists() {
                return Some(dev);
            }
        }
        // Fallback: exe_dir/../resources/sherpa
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let p = parent.join("../resources/sherpa");
                if p.exists() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Run ASR on enrollment clips to capture wake-word variants.
    /// Returns the list of ASR transcripts (one per clip, may be empty/garbage).
    pub fn transcribe_enrollment_clips(
        sherpa_dir: &Path,
        clips: &[Vec<f32>],
    ) -> Result<Vec<String>, String> {
        use sherpa_onnx::{
            OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineTransducerModelConfig,
        };

        let kws_dir = sherpa_dir.join("kws");

        // Prefer int8 (quantized) models, fall back to fp32
        let encoder = kws_dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let encoder = if encoder.exists() { encoder } else { kws_dir.join("encoder-epoch-12-avg-2-chunk-16-left-64.onnx") };
        let decoder = kws_dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let decoder = if decoder.exists() { decoder } else { kws_dir.join("decoder-epoch-12-avg-2-chunk-16-left-64.onnx") };
        let joiner = kws_dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx");
        let joiner = if joiner.exists() { joiner } else { kws_dir.join("joiner-epoch-12-avg-2-chunk-16-left-64.onnx") };
        let tokens = kws_dir.join("tokens.txt");

        for (name, path) in [
            ("encoder", &encoder),
            ("decoder", &decoder),
            ("joiner", &joiner),
            ("tokens", &tokens),
        ] {
            if !path.exists() {
                return Err(format!("ASR model file '{}' not found at: {}", name, path.display()));
            }
        }

        let config = OnlineRecognizerConfig {
            model_config: OnlineModelConfig {
                transducer: OnlineTransducerModelConfig {
                    encoder: Some(encoder.to_string_lossy().to_string()),
                    decoder: Some(decoder.to_string_lossy().to_string()),
                    joiner: Some(joiner.to_string_lossy().to_string()),
                },
                tokens: Some(tokens.to_string_lossy().to_string()),
                num_threads: 1,
                provider: Some("cpu".to_string()),
                ..Default::default()
            },
            decoding_method: Some("greedy_search".to_string()),
            enable_endpoint: false,
            ..Default::default()
        };

        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| "Failed to create OnlineRecognizer for enrollment".to_string())?;

        let mut variants = Vec::with_capacity(clips.len());

        for (i, clip) in clips.iter().enumerate() {
            if clip.is_empty() {
                variants.push(String::new());
                continue;
            }

            let stream = recognizer.create_stream();

            // Feed the clip + 0.5s tail padding
            stream.accept_waveform(16000, clip);
            let tail = vec![0.0f32; 8000];
            stream.accept_waveform(16000, &tail);
            stream.input_finished();

            while recognizer.is_ready(&stream) {
                recognizer.decode(&stream);
            }

            let text = if let Some(result) = recognizer.get_result(&stream) {
                result.text.trim().to_lowercase()
            } else {
                String::new()
            };

            tracing::info!("Enrollment clip {} ASR transcript: \"{}\"", i + 1, text);
            variants.push(text);

            recognizer.reset(&stream);
        }

        Ok(variants)
    }

    /// IPC: Enroll a voice profile from multiple audio clips.
    /// Each clip is a Vec<f32> of 16kHz mono audio samples.
    /// Also runs ASR on each clip to capture wake-word variants.
    /// Re-enrollment APPENDS new variants to existing ones (does not wipe).
    #[tauri::command]
    pub fn enroll_voice<R: Runtime>(
        app: tauri::AppHandle<R>,
        clips: Vec<Vec<f32>>,
        threshold: Option<f32>,
    ) -> Result<Vec<String>, String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let profile_path = voice_profile::resolve_profile_path(&dir);

        let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
        let sherpa_dir = resolve_sherpa_dir(&resource_dir)
            .ok_or_else(|| "Sherpa resource directory not found".to_string())?;

        let speaker_model = sherpa_dir.join("speaker_model.onnx");

        let mut verifier = voice_profile::SpeakerVerifier::new(speaker_model, profile_path)
            .map_err(|e| e.to_string())?;

        // Run ASR on each clip to capture wake-word variants
        let asr_variants = transcribe_enrollment_clips(&sherpa_dir, &clips)
            .map_err(|e| {
                tracing::warn!("Enrollment ASR failed (continuing without variants): {e}");
                // Don't fail enrollment if ASR fails — just use empty variants
                vec![String::new(); clips.len()]
            })
            .unwrap_or_else(|_| vec![String::new(); clips.len()]);

        let threshold = threshold.unwrap_or(voice_profile::DEFAULT_THRESHOLD);
        verifier
            .enroll(&clips, threshold, asr_variants.clone())
            .map_err(|e| e.to_string())?;

        // Return the captured variants so the UI can show them
        let captured: Vec<String> = verifier
            .profile()
            .map(|p| p.wake_variants.clone())
            .unwrap_or_default();

        Ok(captured)
    }

    /// IPC: Delete the voice profile (disables speaker verification).
    #[tauri::command]
    pub fn delete_voice_profile<R: Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let profile_path = voice_profile::resolve_profile_path(&dir);

        if profile_path.exists() {
            std::fs::remove_file(&profile_path).map_err(|e| e.to_string())?;
            tracing::info!("Voice profile deleted");
        }
        Ok(())
    }
}

// ─── Meeting / privacy mode commands ─────────────────────────────────

/// IPC: Check whether TTS should be suppressed right now.
///
/// The frontend calls this before speaking to decide whether to
/// produce audible TTS or show a silent visual response instead.
///
/// Uses `should_suppress_tts()` (not `is_meeting_active()`) so that
/// disabling auto-detection in settings takes effect immediately.
/// `is_meeting_active()` only reports the raw detection flag, which the
/// polling loop clears up to 2s later — long enough for the user to
/// disable detection and still have their next response muted.
#[tauri::command]
pub fn meeting_active<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<bool, String> {
    let state = app
        .try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
        .ok_or_else(|| "meeting state not managed".to_string())?;
    Ok(state.should_suppress_tts())
}

/// IPC: Check if NEXUS is paused (manual pause via tray).
#[tauri::command]
pub fn is_nexus_paused<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<bool, String> {
    let state = app
        .try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
        .ok_or_else(|| "meeting state not managed".to_string())?;
    Ok(state.is_paused())
}

/// IPC: Get the full meeting/privacy mode status.
#[tauri::command]
pub fn meeting_status<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<MeetingStatus, String> {
    let state = app
        .try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
        .ok_or_else(|| "meeting state not managed".to_string())?;
    Ok(MeetingStatus {
        meeting_active: state.is_meeting_active(),
        paused: state.is_paused(),
        tts_playing: state.tts_playing.load(std::sync::atomic::Ordering::Relaxed),
        detection_enabled: state.detection_enabled.load(std::sync::atomic::Ordering::Relaxed),
    })
}

/// IPC: Enable or disable automatic meeting detection.
#[tauri::command]
pub fn set_meeting_detection<R: Runtime>(
    app: tauri::AppHandle<R>,
    enabled: bool,
) -> Result<(), String> {
    let state = app
        .try_state::<std::sync::Arc<crate::meeting_detect::MeetingState>>()
        .ok_or_else(|| "meeting state not managed".to_string())?;
    state.detection_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    tracing::info!("meeting detection: {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

/// Serialized meeting status returned by `meeting_status`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MeetingStatus {
    pub meeting_active: bool,
    pub paused: bool,
    pub tts_playing: bool,
    pub detection_enabled: bool,
}

// ─── Response Sidebar window ─────────────────────────────────────────

/// Pending backdrop for the settings sidebar — stored when the window is
/// first shown, fetched by the frontend on mount (same pattern as
/// PENDING_SIDEBAR for the response sidebar). This handles the race
/// condition where the backdrop event is emitted before the React app
/// has mounted and registered its event listener.
static PENDING_SETTINGS_BACKDROP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// IPC: Show the response sidebar window (positioned at bottom-right).
/// Called when a server response is incoming (n8n/Ollama/Hermes).
/// Creates the sidebar window on-demand if it doesn't exist (saves ~250 MB RAM at idle).
///
/// MUST be async — WebviewWindowBuilder::build() dispatches to the main thread,
/// and a synchronous command runs on a blocking thread that can't yield, causing
/// a deadlock. Async commands run on the tokio runtime which can properly yield.
#[tauri::command]
pub async fn show_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let win = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::sidebar())?;
    // Check if window is already visible to avoid capturing ourselves
    let already_visible = win.is_visible().unwrap_or(false);
    show_sidebar_inner(&app, &win, already_visible)?;
    Ok(())
}

/// IPC: Show the sidebar AND set its content.
///
/// The sidebar window is created on-demand. Since the React app needs time
/// to load before it can receive Tauri events, we store the content + backdrop
/// in a static. The frontend calls `get_pending_sidebar_content` on mount,
/// which returns and clears the pending data. This is race-free.
///
/// If the window already exists (React already loaded), we also emit the
/// `sidebar:show` event as a fast path — the event listener will handle it
/// immediately without needing to poll the pending content.
#[tauri::command]
pub async fn show_sidebar_with_content<R: Runtime>(
    app: tauri::AppHandle<R>,
    query: String,
    text: String,
) -> Result<(), String> {
    let window_existed = app.get_webview_window("sidebar").is_some();
    let win = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::sidebar())?;

    // ── Key fix: pre-position the window BEFORE capturing the backdrop ──
    // A freshly-created window sits at physical (0, 0). On Windows, a window
    // at (0, 0) may be partially off-screen or on the wrong monitor, which
    // causes `win.current_monitor()` inside `capture_backdrop` to return None,
    // silently aborting the capture. We run the same positioning math used by
    // `show_sidebar_inner` here first, so the window is in its final position
    // on the correct monitor before we call `BitBlt`. This is safe to do even
    // before `win.show()` — `set_position` works on hidden windows.
    if let Ok(Some(monitor)) = win.current_monitor().or_else(|_| {
        // Fallback: if not yet on a monitor, try primary monitor
        win.primary_monitor()
    }) {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let sidebar_w = 400i32;
        let sidebar_h = 1000i32;
        let phys_w = (sidebar_w as f64 * scale) as i32;
        let phys_h = (sidebar_h as f64 * scale) as i32;
        #[cfg(target_os = "windows")]
        let taskbar = (48.0 * scale) as i32;
        #[cfg(not(target_os = "windows"))]
        let taskbar = (48.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    }

    // Capture the backdrop before showing (only for fresh windows or hidden ones).
    // If the window is already visible, skip capture to avoid capturing ourselves.
    let backdrop = if window_existed && win.is_visible().unwrap_or(false) {
        None
    } else {
        capture_backdrop(&app, &win)
    };

    // Store the pending content so the frontend can fetch it on mount.
    // This handles the fresh-window case where events would be missed.
    {
        let mut pending = PENDING_SIDEBAR.lock().unwrap();
        *pending = Some(PendingSidebar {
            query: query.clone(),
            text: text.clone(),
            backdrop: backdrop.clone(),
            analysis: None,
        });
    }

    // Show the window (no-op if already visible).
    show_sidebar_inner(&app, &win, backdrop.is_some())?;

    // If the window already existed (React already loaded), also emit the
    // event as a fast path. The frontend listener will handle it immediately.
    if window_existed {
        let payload = serde_json::json!({
            "query": query,
            "text": text,
        });
        let _ = app.emit("sidebar:show", payload);
        // Also emit the backdrop if we captured one.
        if let Some(uri) = backdrop {
            let _ = app.emit("sidebar:backdrop", uri);
        }
    }

    tracing::info!("sidebar: shown with content (query={} chars, text={} chars, window_existed={})", query.len(), text.len(), window_existed);
    Ok(())
}

/// IPC: Show the sidebar with structured analysis data (rich dashboard).
/// Like `show_sidebar_with_content` but also stores the analysis JSON so
/// the frontend can render the AnalysisDashboard with pie charts.
#[tauri::command]
pub async fn show_sidebar_with_analysis<R: Runtime>(
    app: tauri::AppHandle<R>,
    query: String,
    text: String,
    analysis: serde_json::Value,
) -> Result<(), String> {
    let window_existed = app.get_webview_window("sidebar").is_some();
    let win = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::sidebar())?;

    // Pre-position the window before capturing backdrop
    if !window_existed {
        let _ = win.set_position(tauri::LogicalPosition::new(0.0, 0.0));
    }

    let backdrop = if window_existed && win.is_visible().unwrap_or(false) {
        None
    } else {
        capture_backdrop(&app, &win)
    };

    {
        let mut pending = PENDING_SIDEBAR.lock().unwrap();
        *pending = Some(PendingSidebar {
            query: query.clone(),
            text: text.clone(),
            backdrop: backdrop.clone(),
            analysis: Some(analysis.clone()),
        });
    }

    show_sidebar_inner(&app, &win, backdrop.is_some())?;

    // Fast path: if the window already exists, also emit events
    if window_existed {
        let _ = app.emit("sidebar:show", serde_json::json!({
            "query": query,
            "text": text,
        }));
        let _ = app.emit("sidebar:analysis", serde_json::json!({
            "query": query,
            "text": text,
            "analysis": analysis,
        }));
        if let Some(uri) = backdrop {
            let _ = app.emit("sidebar:backdrop", uri);
        }
    }

    tracing::info!("sidebar: shown with analysis (query={} chars, text={} chars, window_existed={})", query.len(), text.len(), window_existed);
    Ok(())
}

/// IPC: Fetch pending sidebar content (called by the frontend on mount).
/// Returns the content + backdrop + analysis that was stored by
/// `show_sidebar_with_content` or `show_sidebar_with_analysis`, or null
/// if no content is pending. Clears the pending data after returning.
#[tauri::command]
pub fn get_pending_sidebar_content() -> Result<Option<serde_json::Value>, String> {
    let mut pending = PENDING_SIDEBAR.lock().unwrap();
    let data = pending.take();
    match data {
        Some(p) => {
            tracing::info!("sidebar: pending content fetched (query={} chars, text={} chars, has_backdrop={}, has_analysis={})", p.query.len(), p.text.len(), p.backdrop.is_some(), p.analysis.is_some());
            Ok(Some(serde_json::json!({
                "query": p.query,
                "text": p.text,
                "backdrop": p.backdrop,
                "analysis": p.analysis,
            })))
        }
        None => Ok(None),
    }
}

/// Capture the desktop region behind the sidebar window (Windows only).
/// Must be called BEFORE `win.show()` so we don't capture the sidebar itself.
/// Returns the blurred backdrop as a `data:image/png;base64,...` URI, or None.
fn capture_backdrop<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    win: &tauri::WebviewWindow<R>,
) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let monitor = win.current_monitor().ok()??;
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        // Read the ACTUAL window size — the wide sidebar is 900px, not 600px.
        let logical = win.inner_size().map(|s| s.to_logical::<f64>(scale)).unwrap_or(
            tauri::LogicalSize::new(400.0, 1000.0)
        );
        let sidebar_w = logical.width;
        let sidebar_h = logical.height;
        let phys_w = (sidebar_w * scale) as i32;
        let phys_h = (sidebar_h * scale) as i32;
        let taskbar = (48.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);

        match crate::sidebar_backdrop::capture_and_blur(x, y, phys_w, phys_h, 32.0) {
            Some(data_uri) => {
                tracing::info!("sidebar: backdrop captured ({} bytes)", data_uri.len());
                Some(data_uri)
            }
            None => {
                tracing::warn!("sidebar: backdrop capture failed (x={x}, y={y}, w={phys_w}, h={phys_h})");
                None
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Shared inner logic for showing the sidebar window.
/// `backdrop_already_captured`: if true, skip the backdrop capture (it was
/// already done by the caller and stored in the pending content). This prevents
/// re-capturing the window itself when the window is already visible.
fn show_sidebar_inner<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    win: &tauri::WebviewWindow<R>,
    backdrop_already_captured: bool,
) -> Result<(), String> {

    // Force the sidebar to the correct size in case the window was
    // created with a different size from a previous build.
    if let Ok(scale) = win.scale_factor() {
        let _ = win.set_size(tauri::PhysicalSize::new(
            (400.0 * scale) as i32,
            (1000.0 * scale) as i32,
        ));
    }

    // Position at bottom-right of the screen, above the taskbar.
    // Read the ACTUAL window size (logical) instead of hardcoding —
    // the wide sidebar is 900px and would otherwise be pushed off-screen.
    use tauri::PhysicalPosition;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        // Get the window's actual logical size; fall back to 600x1000 if unavailable
        let logical = win.inner_size().map(|s| s.to_logical::<f64>(scale)).unwrap_or(
            tauri::LogicalSize::new(400.0, 1000.0)
        );
        let sidebar_w = logical.width;
        let sidebar_h = logical.height;
        let phys_w = (sidebar_w * scale) as i32;
        let phys_h = (sidebar_h * scale) as i32;

        #[cfg(target_os = "macos")]
        let taskbar = (70.0 * scale) as i32;
        #[cfg(target_os = "windows")]
        let taskbar = (48.0 * scale) as i32;
        #[cfg(target_os = "linux")]
        let taskbar = (36.0 * scale) as i32;
        let gap = (12.0 * scale) as i32;

        let x = screen.width as i32 - phys_w - gap;
        // Clamp Y so the window doesn't go off-screen if taller than the monitor
        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }

    // ─── "Fake blur" backdrop capture (Windows only) ───────────────
    // Only capture if the caller hasn't already done so. This prevents
    // capturing the sidebar itself when the window is already visible
    // (e.g. when show_sidebar is called from the frontend's useEffect).
    if !backdrop_already_captured {
        if let Some(data_uri) = capture_backdrop(_app, win) {
            let _ = _app.emit("sidebar:backdrop", data_uri);
        }
    }

    win.show().map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        use tauri::PhysicalPosition;
        if let Ok(Some(monitor)) = win.current_monitor() {
            let scale = monitor.scale_factor();
            let screen = monitor.size();
            let sidebar_w = 400i32;
            let sidebar_h = 1000i32;
            let phys_w = (sidebar_w as f64 * scale) as i32;
            let phys_h = (sidebar_h as f64 * scale) as i32;
            let taskbar = (36.0 * scale) as i32;
            let gap = (12.0 * scale) as i32;
            let x = screen.width as i32 - phys_w - gap;
            let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);
            let _ = win.set_position(PhysicalPosition::new(x, y));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // No window-vibrancy re-apply here — see the detailed comment in
        // lib.rs's setup hook for why this window intentionally never calls
        // apply_blur/apply_acrylic/apply_mica. The window's transparency
        // comes from tao's own material-free DWM registration (done once at
        // window creation) and does not need re-applying on every show.
        //
        // Corner rounding is a plain window-shape attribute (not a material)
        // so it's safe/cheap to re-assert on every show in case it was lost.
        crate::dwm_corners::round_corners(&win);

        // ── Live blur: 1 FPS + change detection ──────────────────────
        // The previous 200ms (5 FPS) loop created a "buffering video"
        // effect — each frame required the full capture→blur→JPEG→base64
        //→event→repaint pipeline (~50-100ms), making 5 FPS look stuttery.
        //
        // Now: capture every 1s, hash the raw BGRA, and only run the
        // expensive blur→encode→emit pipeline when the background actually
        // changed (hash differs). When nothing moves behind the sidebar,
        // CPU stays at ~0. When something moves, the blur updates within
        // ~1s with a gentle CSS crossfade (see sidebar.css ::before).
        static LIVE_BLUR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static LAST_FRAME_HASH: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
        if !LIVE_BLUR_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            LIVE_BLUR_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            // Reset hash so the first frame after show always emits
            *LAST_FRAME_HASH.lock().unwrap() = None;

            let win_clone = win.clone();
            let app_clone = _app.clone();
            tauri::async_runtime::spawn(async move {
                // Wait for window to fully appear
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                while win_clone.is_visible().unwrap_or(false) {
                    // 1 FPS — 1000ms between captures
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

                    if !win_clone.is_visible().unwrap_or(false) {
                        break;
                    }

                    if let Ok(Some(monitor)) = win_clone.current_monitor() {
                        let scale = monitor.scale_factor();
                        let screen = monitor.size();
                        let sidebar_w = 400i32;
                        let sidebar_h = 1000i32;
                        let phys_w = (sidebar_w as f64 * scale) as i32;
                        let phys_h = (sidebar_h as f64 * scale) as i32;
                        let taskbar = (48.0 * scale) as i32;
                        let gap = (12.0 * scale) as i32;
                        let x = screen.width as i32 - phys_w - gap;
                        let y = (screen.height as i32 - phys_h - taskbar - gap).max(0);

                        // Step 1: cheap raw capture for hashing (~1ms)
                        let raw_bgra = match crate::sidebar_backdrop::capture_region_bgra_public(x, y, phys_w, phys_h) {
                            Some(bgra) => bgra,
                            None => continue,
                        };

                        // Step 2: hash and compare to previous frame
                        let current_hash = crate::sidebar_backdrop::frame_hash(&raw_bgra);
                        let mut prev_hash_guard = LAST_FRAME_HASH.lock().unwrap();
                        let should_emit = match *prev_hash_guard {
                            Some(prev) => prev != current_hash,
                            None => true, // First frame after show — always emit
                        };
                        *prev_hash_guard = Some(current_hash);
                        drop(prev_hash_guard);

                        // Step 3: only run the expensive pipeline if changed
                        if should_emit {
                            // We already have the raw BGRA — blur + encode it
                            // without re-capturing (reuse the bytes we have).
                            if let Some(data_uri) = crate::sidebar_backdrop::blur_bgra_to_jpeg(&raw_bgra, phys_w, phys_h, 32.0) {
                                let _ = app_clone.emit("sidebar:backdrop", data_uri);
                            }
                        }
                    }
                }

                // Clean up: reset hash so next show captures fresh
                *LAST_FRAME_HASH.lock().unwrap() = None;
                LIVE_BLUR_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        // Re-apply vibrancy after show — the effect can be lost if the
        // window was hidden for a long time or the app was backgrounded.
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
        let _ = apply_vibrancy(
            &win,
            NSVisualEffectMaterial::Sidebar,
            Some(NSVisualEffectState::Active),
            Some(20.0),
        );
    }

    Ok(())
}

/// IPC: Hide the response sidebar window.
/// Called after the server response has been spoken.
/// Also destroys the architect-sidebar if it's open.
#[tauri::command]
pub fn hide_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    // Destroy sidebar windows to free ~250 MB of WebView2 processes each.
    let _ = crate::dyn_windows::destroy_window(&app, "sidebar");
    let _ = crate::dyn_windows::destroy_window(&app, "architect-sidebar");
    Ok(())
}

// ─── PR List sidebar window ───────────────────────────────────────────

/// IPC: Show the PR list sidebar window.
/// Creates a 500x1000 transparent, always-on-top, non-activating window
/// on the right edge of the screen. Shows the PR list with Merge and
/// Analyse buttons. The window is created on-demand and destroyed when
/// closed (frees ~250 MB of WebView2 processes).
///
/// Carbon copy of the response sidebar: captures the desktop backdrop,
/// blurs it, emits `sidebar:backdrop`, and runs a 1 FPS live blur loop
/// so the PR list has the same liquid-glass appearance.
#[tauri::command]
pub async fn show_pr_list_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let win = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::pr_list_sidebar(),
    )?;

    // Position at the right edge of the screen, vertically centered
    use tauri::PhysicalPosition;
    let mut capture_x = 0i32;
    let mut capture_y = 0i32;
    let mut capture_w = 500i32;
    let mut capture_h = 1000i32;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let win_w = 500i32;
        let win_h = 1000i32;
        let phys_w = (win_w as f64 * scale) as i32;
        let phys_h = (win_h as f64 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = screen.width as i32 - phys_w - gap;
        let y = (screen.height as i32 - phys_h) / 2; // vertically centered
        let _ = win.set_position(PhysicalPosition::new(x, y));
        capture_x = x;
        capture_y = y;
        capture_w = phys_w;
        capture_h = phys_h;
    }

    // ─── "Fake blur" backdrop capture (Windows only) ───────────────
    // Capture the desktop behind the window BEFORE showing it, so we
    // don't capture the sidebar itself. This matches the response sidebar.
    #[cfg(target_os = "windows")]
    {
        if let Some(data_uri) = crate::sidebar_backdrop::capture_and_blur(
            capture_x, capture_y, capture_w, capture_h, 32.0,
        ) {
            tracing::info!("pr-list: backdrop captured ({} bytes)", data_uri.len());
            let _ = app.emit("sidebar:backdrop", data_uri);
        } else {
            tracing::warn!("pr-list: backdrop capture failed");
        }
    }

    win.show().map_err(|e| e.to_string())?;

    // ─── Live blur loop (Windows only) ──────────────────────────────
    // 1 FPS capture + change detection, matching the response sidebar.
    #[cfg(target_os = "windows")]
    {
        static PR_LIVE_BLUR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static PR_LAST_FRAME_HASH: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
        if !PR_LIVE_BLUR_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            PR_LIVE_BLUR_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            *PR_LAST_FRAME_HASH.lock().unwrap() = None;

            let win_clone = win.clone();
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                while win_clone.is_visible().unwrap_or(false) {
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    if !win_clone.is_visible().unwrap_or(false) {
                        break;
                    }

                    if let Ok(Some(monitor)) = win_clone.current_monitor() {
                        let scale = monitor.scale_factor();
                        let screen = monitor.size();
                        let win_w = 500i32;
                        let win_h = 1000i32;
                        let phys_w = (win_w as f64 * scale) as i32;
                        let phys_h = (win_h as f64 * scale) as i32;
                        let gap = (12.0 * scale) as i32;
                        let x = screen.width as i32 - phys_w - gap;
                        let y = (screen.height as i32 - phys_h) / 2;

                        let raw_bgra = match crate::sidebar_backdrop::capture_region_bgra_public(x, y, phys_w, phys_h) {
                            Some(bgra) => bgra,
                            None => continue,
                        };

                        let current_hash = crate::sidebar_backdrop::frame_hash(&raw_bgra);
                        let mut prev_hash_guard = PR_LAST_FRAME_HASH.lock().unwrap();
                        let should_emit = match *prev_hash_guard {
                            Some(prev) => prev != current_hash,
                            None => true,
                        };
                        *prev_hash_guard = Some(current_hash);
                        drop(prev_hash_guard);

                        if should_emit {
                            if let Some(data_uri) = crate::sidebar_backdrop::blur_bgra_to_jpeg(&raw_bgra, phys_w, phys_h, 32.0) {
                                let _ = app_clone.emit("sidebar:backdrop", data_uri);
                            }
                        }
                    }
                }

                *PR_LAST_FRAME_HASH.lock().unwrap() = None;
                PR_LIVE_BLUR_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }
    }

    Ok(())
}

/// IPC: Hide (destroy) the PR list sidebar window.
#[tauri::command]
pub fn hide_pr_list_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let _ = crate::dyn_windows::destroy_window(&app, "pr-list-sidebar");
    Ok(())
}

/// IPC: Show the settings sidebar (liquid-glass, 520x1000, always-on-top).
/// Creates the window on-demand, positions it at the LEFT edge of the
/// screen (vertically centered) so the orb is visible on the right while
/// adjusting position. Captures the desktop backdrop for the blur effect
/// and starts the live-blur loop. Same pattern as the other sidebars.
#[tauri::command]
pub async fn show_settings_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let win = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::settings_sidebar(),
    )?;

    // Update size in case the window already existed with a different size
    let _ = win.set_size(tauri::PhysicalSize::new(
        (520.0 * win.scale_factor().unwrap_or(1.0)) as i32,
        (1000.0 * win.scale_factor().unwrap_or(1.0)) as i32,
    ));

    // Show the main orb window so the user can see it move while dragging
    // the position sliders in the Display tab.
    if let Some(orb) = app.get_webview_window("main") {
        let _ = orb.show();
    }

    // Position at the LEFT edge of the screen, vertically centered.
    // This keeps the right side of the screen free so the orb is visible
    // while the user adjusts its position via the sliders.
    use tauri::PhysicalPosition;
    let mut capture_x = 0i32;
    let mut capture_y = 0i32;
    let mut capture_w = 520i32;
    let mut capture_h = 1000i32;
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let win_w = 520i32;
        let win_h = 1000i32;
        let phys_w = (win_w as f64 * scale) as i32;
        let phys_h = (win_h as f64 * scale) as i32;
        let gap = (12.0 * scale) as i32;
        let x = gap; // LEFT edge
        let y = (screen.height as i32 - phys_h) / 2;
        let _ = win.set_position(PhysicalPosition::new(x, y));
        capture_x = x;
        capture_y = y;
        capture_w = phys_w;
        capture_h = phys_h;
    }

    // "Fake blur" backdrop capture (Windows only) — same as other sidebars.
    // Store in PENDING_SETTINGS_BACKDROP so the frontend can fetch it on
    // mount (the event would be lost if emitted before React loads).
    #[cfg(target_os = "windows")]
    {
        if let Some(data_uri) = crate::sidebar_backdrop::capture_and_blur(
            capture_x, capture_y, capture_w, capture_h, 32.0,
        ) {
            tracing::info!("settings-sidebar: backdrop captured ({} bytes)", data_uri.len());
            // Store for frontend to fetch on mount
            *PENDING_SETTINGS_BACKDROP.lock().unwrap() = Some(data_uri.clone());
            // Also emit (works if window already existed and React is loaded)
            let _ = app.emit("sidebar:backdrop", data_uri);
        } else {
            tracing::warn!("settings-sidebar: backdrop capture failed");
        }
    }

    win.show().map_err(|e| e.to_string())?;

    // Live blur loop (Windows only) — 1 FPS capture + change detection.
    #[cfg(target_os = "windows")]
    {
        static SETTINGS_LIVE_BLUR_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        static SETTINGS_LAST_FRAME_HASH: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);
        if !SETTINGS_LIVE_BLUR_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
            SETTINGS_LIVE_BLUR_ACTIVE.store(true, std::sync::atomic::Ordering::SeqCst);
            *SETTINGS_LAST_FRAME_HASH.lock().unwrap() = None;

            let win_clone = win.clone();
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                while win_clone.is_visible().unwrap_or(false) {
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                    if !win_clone.is_visible().unwrap_or(false) {
                        break;
                    }

                    if let Ok(Some(monitor)) = win_clone.current_monitor() {
                        let scale = monitor.scale_factor();
                        let screen = monitor.size();
                        let win_w = 520i32;
                        let win_h = 1000i32;
                        let phys_w = (win_w as f64 * scale) as i32;
                        let phys_h = (win_h as f64 * scale) as i32;
                        let gap = (12.0 * scale) as i32;
                        let x = gap; // LEFT edge
                        let y = (screen.height as i32 - phys_h) / 2;

                        let raw_bgra = match crate::sidebar_backdrop::capture_region_bgra_public(x, y, phys_w, phys_h) {
                            Some(bgra) => bgra,
                            None => continue,
                        };

                        let current_hash = crate::sidebar_backdrop::frame_hash(&raw_bgra);
                        let mut prev_hash_guard = SETTINGS_LAST_FRAME_HASH.lock().unwrap();
                        let should_emit = match *prev_hash_guard {
                            Some(prev) => prev != current_hash,
                            None => true,
                        };
                        *prev_hash_guard = Some(current_hash);
                        drop(prev_hash_guard);

                        if should_emit {
                            if let Some(data_uri) = crate::sidebar_backdrop::blur_bgra_to_jpeg(&raw_bgra, phys_w, phys_h, 32.0) {
                                let _ = app_clone.emit("sidebar:backdrop", data_uri);
                            }
                        }
                    }
                }

                *SETTINGS_LAST_FRAME_HASH.lock().unwrap() = None;
                SETTINGS_LIVE_BLUR_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
            });
        }
    }

    Ok(())
}

/// IPC: Hide (destroy) the settings sidebar window.
#[tauri::command]
pub fn hide_settings_sidebar<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let _ = crate::dyn_windows::destroy_window(&app, "settings-sidebar");
    Ok(())
}

/// IPC: Fetch the pending settings backdrop (called by the frontend on mount).
/// Returns the blurred desktop image as a data URI, or null if none pending.
/// Clears the pending data after returning. This handles the race condition
/// where the backdrop is captured before the React app has mounted.
#[tauri::command]
pub fn get_pending_settings_backdrop() -> Result<Option<String>, String> {
    let mut pending = PENDING_SETTINGS_BACKDROP.lock().unwrap();
    Ok(pending.take())
}

// ─── Loading indicator window ────────────────────────────────────────

/// IPC: Show the loading indicator window.
/// Creates a small 80x80 transparent click-through window at the
/// top-right corner of the screen. Shows the loading.json Lottie
/// animation while NEXUS is processing a request (after "On it sir").
/// The window is permanently click-through — mouse events pass through
/// to whatever is behind it.
///
/// IMPORTANT: This command is async so it runs on a thread pool, NOT the
/// main thread. A synchronous command would block the main thread during
/// WebView2 window creation, which prevents Tauri events (like the Worker
/// "result" event) from being delivered to the frontend — causing the
/// response to never appear.
#[tauri::command]
pub async fn show_loading_indicator<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    // Create the window — WebviewWindowBuilder::build() dispatches to the
    // main thread internally, so this is safe to call from a thread pool.
    let win = crate::dyn_windows::get_or_create_window(
        &app,
        crate::dyn_windows::WindowConfig::loading_indicator(),
    )?;

    // Position at the top-right corner — 7px from right, 9px from top.
    if let Ok(Some(monitor)) = win.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size();
        let win_size = 80i32;
        let phys_win = (win_size as f64 * scale) as i32;
        let inset_x = (7.0 * scale) as i32;
        let inset_y = (9.0 * scale) as i32;
        let x = screen.width as i32 - phys_win - inset_x;
        let y = inset_y;
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
        tracing::debug!("loading-indicator positioned at ({x}, {y}) [scale={scale}]");
    }

    // Permanently click-through — mouse events pass through to windows behind.
    win.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
    win.show().map_err(|e| e.to_string())?;
    tracing::info!("loading-indicator window shown");
    Ok(())
}

/// IPC: Hide/destroy the loading indicator window.
/// Called when the Worker response arrives. Destroys the window to
/// free ~250 MB of WebView2 processes.
#[tauri::command]
pub async fn hide_loading_indicator<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let _ = crate::dyn_windows::destroy_window(&app, "loading-indicator");
    Ok(())
}

// ─── Settings window + persistence ───────────────────────────────────

/// IPC: Open the settings window.
/// Creates the window on-demand if it doesn't exist (saves ~250 MB RAM at idle).
#[tauri::command]
pub fn open_settings_window<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let win = crate::dyn_windows::get_or_create_window(&app, crate::dyn_windows::WindowConfig::settings())?;
    win.show().map_err(|e| e.to_string())?;
    win.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

/// IPC: Close/hide the settings window.
#[tauri::command]
pub fn close_settings_window<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    // Destroy to free ~250 MB of WebView2 processes.
    let _ = crate::dyn_windows::destroy_window(&app, "settings");
    Ok(())
}

/// Serialized settings returned by `get_settings` and accepted by `save_settings`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusSettings {
    pub autostart: bool,
    pub hotkey: String,
    pub auto_hide_delay: u32,
    pub wake_word_enabled: bool,
    pub wake_phrase: String,
    pub wake_sensitivity: String,
    pub speaker_verification: bool,
    pub meeting_mode_auto: bool,
    pub suppress_tts_in_meetings: bool,
    pub local_stt_only: bool,
    pub server_url: String,
    pub user_id: String,
    pub device_id: String,
    pub tts_voice: String,
    pub speech_rate: f64,
    #[serde(default = "default_tts_provider")]
    pub tts_provider: String,
    /// TTS auto-volume: NEXUS sets system volume to this level (0-100)
    /// before speaking, then restores the original volume after.
    /// Default 75. Set to 0 to disable auto-volume.
    #[serde(default = "default_tts_volume")]
    pub tts_volume: u8,
    /// Groq API key for cloud STT (Whisper Large v3 Turbo).
    /// Free at console.groq.com — 2,000 requests/day per user.
    /// When empty, NEXUS uses local faster-whisper STT.
    #[serde(default)]
    pub groq_api_key: String,
    /// edge-tts voice name (Microsoft Neural, free, no API key).
    /// Default: en-US-AvaNeural. See full list at Microsoft Speech docs.
    #[serde(default = "default_edge_tts_voice")]
    pub edge_tts_voice: String,
    /// Orb horizontal position as percentage (0.0 = left, 0.5 = center, 1.0 = right).
    /// Default 0.5 (center). Saved to settings.json, persists across restarts.
    #[serde(default = "default_orb_horizontal_pct")]
    pub orb_horizontal_pct: f64,
    /// Orb vertical position as percentage (0.0 = top, 1.0 = bottom).
    /// Default 1.0 (bottom). Saved to settings.json, persists across restarts.
    #[serde(default = "default_orb_vertical_pct")]
    pub orb_vertical_pct: f64,
    /// Orb window size in pixels (100-300).
    /// Default 200. Saved to settings.json, persists across restarts.
    #[serde(default = "default_orb_size")]
    pub orb_size: u32,
    /// Gemini API key for Google Gemini models.
    /// When empty, Gemini features are unavailable.
    #[serde(default)]
    pub gemini_api_key: String,
}

fn default_tts_provider() -> String {
    "kokoro".to_string()
}

fn default_tts_volume() -> u8 {
    75
}

fn default_edge_tts_voice() -> String {
    "en-US-AvaNeural".to_string()
}

fn default_orb_horizontal_pct() -> f64 {
    0.5
}

fn default_orb_vertical_pct() -> f64 {
    1.0
}

fn default_orb_size() -> u32 {
    200
}

impl Default for NexusSettings {
    fn default() -> Self {
        Self {
            autostart: true,
            hotkey: "Ctrl+Space".to_string(),
            auto_hide_delay: 8,
            wake_word_enabled: true,
            wake_phrase: "NEXUS".to_string(),
            wake_sensitivity: "medium".to_string(),
            speaker_verification: false,
            meeting_mode_auto: true,
            suppress_tts_in_meetings: true,
            local_stt_only: true,
            server_url: option_env!("NEXUS_SERVER_URL")
                .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev")
                .to_string(),
            user_id: String::new(),
            device_id: String::new(),
            tts_voice: "af_sky".to_string(),
            speech_rate: 1.15,
            tts_provider: "kokoro".to_string(),
            tts_volume: 75,
            groq_api_key: String::new(),
            edge_tts_voice: "en-US-AvaNeural".to_string(),
            orb_horizontal_pct: 0.5,
            orb_vertical_pct: 1.0,
            orb_size: 200,
            gemini_api_key: String::new(),
        }
    }
}

/// TTS voice metadata returned by `list_tts_voices`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsVoiceInfo {
    pub id: String,
    pub name: String,
    pub gender: String,
    pub provider: String,
    pub language: String,
}

/// IPC: List all available TTS voices (Edge TTS cloud + Piper local).
/// Returns a curated list of en-US voices. The full Edge TTS catalog has
/// 400+ voices across 140+ locales — we surface the most useful en-US ones
/// to keep the settings UI manageable.
#[tauri::command]
pub fn list_tts_voices() -> Result<Vec<TtsVoiceInfo>, String> {
    let edge_voices = [
        ("en-US-AvaNeural",        "Ava",       "Female"),
        ("en-US-AndrewNeural",     "Andrew",    "Male"),
        ("en-US-AndrewMultilingualNeural", "Andrew ML", "Male"),
        ("en-US-EmmaNeural",       "Emma",      "Female"),
        ("en-US-BrianNeural",      "Brian",     "Male"),
        ("en-US-ChristopherNeural","Christopher","Male"),
        ("en-US-EricNeural",       "Eric",      "Male"),
        ("en-US-GuyNeural",        "Guy",       "Male"),
        ("en-US-JennyNeural",      "Jenny",     "Female"),
        ("en-US-MichelleNeural",   "Michelle",  "Female"),
        ("en-US-RogerNeural",      "Roger",     "Male"),
        ("en-US-SteffanNeural",    "Steffan",   "Male"),
        ("en-US-AriaNeural",       "Aria",      "Female"),
        ("en-US-DavisNeural",      "Davis",     "Male"),
        ("en-US-NancyNeural",      "Nancy",     "Female"),
        ("en-US-SaraNeural",       "Sara",      "Female"),
        ("en-US-NoraNeural",       "Nora",      "Female"),
        ("en-US-AmberNeural",      "Amber",     "Female"),
        ("en-US-AshleyNeural",     "Ashley",    "Female"),
        ("en-US-BrandonNeural",    "Brandon",   "Male"),
        ("enUS-CoraNeural",        "Cora",      "Female"),
        ("en-US-ElizabethNeural",  "Elizabeth", "Female"),
        ("en-US-MonicaNeural",     "Monica",    "Female"),
        ("en-US-SoniaNeural",      "Sonia",     "Female"),
    ];

    let mut voices: Vec<TtsVoiceInfo> = edge_voices
        .iter()
        .map(|(id, name, gender)| TtsVoiceInfo {
            id: id.to_string(),
            name: name.to_string(),
            gender: gender.to_string(),
            provider: "edge-tts".to_string(),
            language: "en-US".to_string(),
        })
        .collect();

    // Add local Piper voice (always available offline)
    voices.push(TtsVoiceInfo {
        id: "piper-amy".to_string(),
        name: "Amy (Offline)".to_string(),
        gender: "Female".to_string(),
        provider: "piper".to_string(),
        language: "en-US".to_string(),
    });

    Ok(voices)
}

/// IPC: Get the current settings (merged with defaults for missing fields).
#[tauri::command]
pub fn get_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<NexusSettings, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = dir.join("settings.json");
    if !path.exists() {
        return Ok(NexusSettings::default());
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut settings: NexusSettings = serde_json::from_str(&content)
        .unwrap_or_default();
    // Merge server config if present
    let config_path = dir.join("nexus-config.json");
    if config_path.exists() {
        if let Ok(config) = std::fs::read_to_string(&config_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&config) {
                let default_url = option_env!("NEXUS_SERVER_URL")
                    .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");
                if let Some(url) = json.get("serverUrl").and_then(|v| v.as_str()) {
                    // Don't overwrite with empty string — keep default
                    settings.server_url = if url.is_empty() { default_url.to_string() } else { url.to_string() };
                }
                if let Some(uid) = json.get("userId").and_then(|v| v.as_str()) {
                    settings.user_id = uid.to_string();
                }
                if let Some(did) = json.get("deviceId").and_then(|v| v.as_str()) {
                    settings.device_id = did.to_string();
                }
            }
        }
    }
    Ok(settings)
}

/// IPC: Save settings to disk.
#[tauri::command]
pub fn save_settings<R: Runtime>(
    app: tauri::AppHandle<R>,
    settings: NexusSettings,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("settings.json");
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;

    // Also save server config separately — but NEVER overwrite an existing
    // identity (user_id / device_id) with empty values. This prevents
    // identity loss if settings.json is stale or nexus-config.json was
    // temporarily missing.
    let config_path = dir.join("nexus-config.json");
    let default_url = option_env!("NEXUS_SERVER_URL")
        .unwrap_or("https://nexus-worker.chitkullakshya.workers.dev");

    // Read existing config to preserve identity if needed
    let existing = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok());

    let (server_url, user_id, device_id) = if let Some(ref existing) = existing {
        let preserved_url = if settings.server_url.is_empty() {
            existing["serverUrl"].as_str().unwrap_or(default_url).to_string()
        } else {
            settings.server_url.clone()
        };
        let preserved_uid = if settings.user_id.is_empty() {
            existing["userId"].as_str().unwrap_or("").to_string()
        } else {
            settings.user_id.clone()
        };
        let preserved_did = if settings.device_id.is_empty() {
            existing["deviceId"].as_str().unwrap_or("").to_string()
        } else {
            settings.device_id.clone()
        };
        (preserved_url, preserved_uid, preserved_did)
    } else {
        // No existing config — use what we have, with URL fallback
        let url = if settings.server_url.is_empty() { default_url.to_string() } else { settings.server_url.clone() };
        (url, settings.user_id.clone(), settings.device_id.clone())
    };

    let config = serde_json::json!({
        "serverUrl": server_url,
        "userId": user_id,
        "deviceId": device_id,
    });
    std::fs::write(&config_path, config.to_string()).map_err(|e| e.to_string())?;
    tracing::info!("settings saved to {:?}", path);
    Ok(())
}

/// Read the Groq API key from settings.json (non-IPC helper for stt.rs).
/// Returns empty string if no key is set or settings file doesn't exist.
pub fn read_groq_api_key<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    let dir = app.path().app_data_dir();
    let Ok(dir) = dir else { return String::new(); };
    let path = dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else { return String::new(); };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return String::new(); };
    // Read groqApiKey (frontend camelCase) with fallback to groq_api_key
    // (snake_case, used by manual settings.json edits or older versions).
    json.get("groqApiKey")
        .or_else(|| json.get("groq_api_key"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Read the localSttOnly flag from settings.json (non-IPC helper for stt.rs).
/// Returns true if the user has enabled "Local STT only" (privacy mode —
/// audio never leaves the device). Returns false if not set or file missing.
pub fn read_local_stt_only<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    let dir = app.path().app_data_dir();
    let Ok(dir) = dir else { return false; };
    let path = dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else { return false; };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return false; };
    json.get("localSttOnly")
        .or_else(|| json.get("local_stt_only"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Read the edge-tts voice from settings.json (non-IPC helper for tts.rs).
/// Returns default voice if not set.
pub fn read_edge_tts_voice(app: &tauri::AppHandle) -> String {
    let dir = app.path().app_data_dir();
    let Ok(dir) = dir else { return "en-US-AvaNeural".to_string(); };
    let path = dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else { return "en-US-AvaNeural".to_string(); };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else { return "en-US-AvaNeural".to_string(); };
    json.get("edgeTtsVoice")
        .or_else(|| json.get("edge_tts_voice"))
        .and_then(|v| v.as_str())
        .unwrap_or("en-US-AvaNeural")
        .to_string()
}

// ─── Autostart management ───────────────────────────────────────────
//
// Windows: uses a Scheduled Task named "NEXUS" with AtLogOn trigger.
// macOS/Linux: uses tauri-plugin-autostart (LaunchAgent / systemd).

/// IPC: Enable or disable auto-start at login.
/// On Windows, creates/removes the "NEXUS" Scheduled Task.
/// On macOS/Linux, calls the autostart plugin enable/disable.
#[tauri::command]
pub fn set_autostart<R: Runtime>(
    app: tauri::AppHandle<R>,
    enabled: bool,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = &app; // unused on Windows — autostart uses PowerShell directly
        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;

        if enabled {
            // Create/update scheduled task with --background flag for silent start
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
                    tracing::info!("autostart: scheduled task 'NEXUS' created (AtLogOn, --background)");
                    Ok(true)
                }
                Ok(out) => {
                    tracing::warn!(
                        "autostart: Register-ScheduledTask failed: stdout={} stderr={}",
                        String::from_utf8_lossy(&out.stdout).trim(),
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                    Err("Failed to create scheduled task".to_string())
                }
                Err(e) => Err(format!("PowerShell error: {e}")),
            }
        } else {
            // Remove the scheduled task
            let result = std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "Unregister-ScheduledTask -TaskName 'NEXUS' -Confirm:$false"])
                .creation_flags(0x08000000)
                .output();
            match result {
                Ok(out) if out.status.success() => {
                    tracing::info!("autostart: scheduled task 'NEXUS' removed");
                    Ok(false)
                }
                Ok(out) => {
                    // Task may not exist — that's fine, it's already disabled
                    tracing::info!(
                        "autostart: Unregister-ScheduledTask completed (stderr={})",
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                    Ok(false)
                }
                Err(e) => Err(format!("PowerShell error: {e}")),
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let autostart = app.autolaunch();
        if enabled {
            autostart.enable().map_err(|e| e.to_string())?;
            tracing::info!("autostart: enabled (LaunchAgent)");
        } else {
            autostart.disable().map_err(|e| e.to_string())?;
            tracing::info!("autostart: disabled");
        }
        Ok(enabled)
    }
}

/// IPC: Check if auto-start is currently enabled.
#[tauri::command]
pub fn is_autostart_enabled<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let _ = app; // unused on Windows
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "Get-ScheduledTask -TaskName 'NEXUS' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty State"])
            .creation_flags(0x08000000)
            .output();
        match result {
            Ok(out) => {
                let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let enabled = state == "Ready" || state == "Running";
                Ok(enabled)
            }
            Err(e) => {
                tracing::warn!("is_autostart_enabled: {e}");
                Ok(false)
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let autostart = app.autolaunch();
        Ok(autostart.is_enabled().unwrap_or(false))
    }
}

// ─── Microphone permission ──────────────────────────────────────────
//
// On Windows, desktop (Win32) apps don't need per-app mic permission like UWP
// apps do. However, the GLOBAL mic privacy toggle (Settings → Privacy →
// Microphone → "Allow apps to access your microphone") can block all apps.
// When that's off, cpal returns an empty device list or fails to build a
// stream. We probe the default input device to detect this state.

/// IPC: Check if the microphone is accessible.
///
/// Probes cpal's default input device. Returns:
///   "granted"   — device found and stream can be built
///   "denied"    — no input devices or stream build failed (likely mic privacy off)
///   "no_device" — no input devices at all (no mic connected)
#[tauri::command]
pub fn check_mic_permission() -> String {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    // Check if there are any input devices at all
    let devices = match host.input_devices() {
        Ok(d) => d.collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("check_mic_permission: input_devices() failed: {e}");
            return "denied".to_string();
        }
    };

    if devices.is_empty() {
        tracing::warn!("check_mic_permission: no input devices found");
        return "no_device".to_string();
    }

    // Try the default device
    match host.default_input_device() {
        Some(device) => {
            let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
            tracing::info!("check_mic_permission: probing device '{}'", dev_name);

            // Try to build a minimal stream config to verify access
            let default_config = device.default_input_config();
            match default_config {
                Ok(_config) => {
                    // If we can get a default config, the device is accessible.
                    // Actually building a stream would require a callback and
                    // play() call, which is heavy for a permission check.
                    // Getting the default config is sufficient — if mic privacy
                    // is off, this returns an error on Windows.
                    tracing::info!("check_mic_permission: granted (device '{}')", dev_name);
                    "granted".to_string()
                }
                Err(e) => {
                    tracing::warn!("check_mic_permission: default_input_config failed: {e}");
                    "denied".to_string()
                }
            }
        }
        None => {
            tracing::warn!("check_mic_permission: no default input device");
            "no_device".to_string()
        }
    }
}

/// IPC: Open the OS microphone privacy settings.
///
/// On Windows, opens `ms-settings:privacy-microphone`.
/// On macOS, opens System Settings → Microphone.
/// On Linux, this is a no-op (PipeWire/PulseAudio handle permissions differently).
#[tauri::command]
pub fn open_mic_settings() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "ms-settings:privacy-microphone"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
            .map_err(|e| e.to_string())?;
        tracing::info!("opened Windows mic privacy settings");
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        // No standard mic privacy settings on most Linux distros
        tracing::info!("open_mic_settings: no-op on Linux");
    }

    Ok(())
}

/// IPC: Clear the conversation transcript (frontend store).
/// This is a no-op on the Rust side — the frontend handles it.
/// The command exists so the settings UI can call it via IPC.
#[tauri::command]
pub fn clear_transcript() -> Result<(), String> {
    tracing::info!("transcript cleared (frontend-side)");
    Ok(())
}

/// Force a manual app registry refresh (e.g. after installing a new app).
/// Scans the OS for installed apps and updates the cache immediately.
#[tauri::command]
pub fn refresh_app_registry() -> Result<String, String> {
    tracing::info!("manual app registry refresh requested");
    app_registry::force_refresh();
    Ok("App registry refreshed".to_string())
}

/// IPC: Pause the wake-word engine's cpal stream.
/// Called by the frontend before acquiring the microphone via getUserMedia()
/// to avoid OS-level mic lock contention (Intel Smart Sound Technology).
#[tauri::command]
pub fn pause_wakeword() -> Result<(), String> {
    crate::wakeword_oww::pause_stream();
    Ok(())
}

/// IPC: Resume the wake-word engine's cpal stream.
/// Called by the frontend after releasing the microphone so the wake-word
/// engine can resume listening for "NEXUS".
#[tauri::command]
pub fn resume_wakeword() -> Result<(), String> {
    crate::wakeword_oww::resume_stream();
    Ok(())
}

/// Start Rust-side STT capture from the cpal stream.
/// Called by the frontend when the user wakes NEXUS or when retrying
/// after an empty transcript. The cpal stream captures audio directly —
/// no getUserMedia, no baton pass. This fixes the Intel SST driver issue
/// where getUserMedia returns silence but cpal is still working.
#[tauri::command]
pub fn start_stt_capture() -> Result<(), String> {
    crate::wakeword_oww::start_stt_capture();
    Ok(())
}
