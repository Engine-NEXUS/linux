//! HTTP bridge to the Cloudflare Worker (TEXT-ONLY protocol, serverless).
//!
//! The NEXUS client sends transcript text via HTTP POST to the Worker.
//! The Worker classifies intent, calls external APIs, and returns text.
//! No WebSocket, no sidecar, no server needed.
//!
//! The frontend drives it via IPC:
//!   - `open_session`     → load config (Worker URL, user_id, device_id).
//!   - `send_transcript`  → HTTP POST to Worker, emit events as they arrive.
//!   - `cancel_session`   → reset state (no connection to tear down).
//!   - `close_session`    → reset state.
//!
//! Events emitted to the frontend (same as the old WebSocket protocol):
//!   `{ "type": "state", "state": "thinking" }`
//!   `{ "type": "ack", "data": "On it, sir." }`
//!   `{ "type": "result", "data": "PR #76 is approved" }`
//!   `{ "type": "done" }`

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Runtime};
use parking_lot::Mutex;
use serde::Serialize;

const ACK_PHRASES: &[&str] = &[
    "On it, sir.",
    "Right away, sir.",
    "Checking that now, sir.",
    "Working on it, sir.",
    "Let me look into that, sir.",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerEvent {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dialog_state: Option<serde_json::Value>,
}

impl ServerEvent {
    fn state(s: &str) -> Self {
        Self { kind: "state".into(), state: Some(s.into()), data: None, message: None, analysis: None, dialog_state: None }
    }
    fn ack(text: &str) -> Self {
        Self { kind: "ack".into(), state: None, data: Some(text.into()), message: None, analysis: None, dialog_state: None }
    }
    fn result(text: &str) -> Self {
        Self { kind: "result".into(), state: None, data: Some(text.into()), message: None, analysis: None, dialog_state: None }
    }
    fn result_with_analysis(text: &str, analysis: serde_json::Value) -> Self {
        Self { kind: "result".into(), state: None, data: Some(text.into()), message: None, analysis: Some(analysis), dialog_state: None }
    }
    fn result_with_dialog(text: &str, dialog_state: serde_json::Value) -> Self {
        Self { kind: "result".into(), state: None, data: Some(text.into()), message: None, analysis: None, dialog_state: Some(dialog_state) }
    }
    fn result_with_analysis_and_dialog(text: &str, analysis: serde_json::Value, dialog_state: serde_json::Value) -> Self {
        Self { kind: "result".into(), state: None, data: Some(text.into()), message: None, analysis: Some(analysis), dialog_state: Some(dialog_state) }
    }
    #[allow(dead_code)]
    fn done() -> Self {
        Self { kind: "done".into(), state: None, data: None, message: None, analysis: None, dialog_state: None }
    }
    fn error(msg: &str) -> Self {
        Self { kind: "error".into(), state: None, data: None, message: Some(msg.into()), analysis: None, dialog_state: None }
    }
}

struct Session {
    worker_url: String,
    user_id: String,
    device_id: String,
    cancelled: bool,
}

static SESSION: once_cell::sync::Lazy<Arc<Mutex<Option<Session>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

/// Generate a RFC 4122 v4 UUID string without pulling a uuid crate.
pub(crate) fn uuid_v4() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let ctr = COUNTER.fetch_add(1, Ordering::Relaxed);

    let mut bytes = [0u8; 16];
    let a = nanos.wrapping_mul(0x2545F4914F6CDD1D).wrapping_add(ctr);
    let b = pid.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(nanos ^ ctr);
    bytes[0..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..16].copy_from_slice(&b.to_le_bytes());

    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// IPC: open a session — just stores the Worker URL + identity.
/// No WebSocket connection is made. The Worker is called on-demand per transcript.
#[tauri::command]
pub async fn open_session<R: Runtime>(
    _app: AppHandle<R>,
    url: String,
    _token: String,
    user_id: String,
    device_id: String,
) -> Result<String, String> {
    // Normalize: the URL might be a ws:// URL from old config. Convert to https://.
    let worker_url = url
        .replace("ws://", "http://")
        .replace("wss://", "https://")
        .replace("/ws", "");  // strip /ws path if present from old config

    let session_id = uuid_v4();

    *SESSION.lock() = Some(Session {
        worker_url,
        user_id,
        device_id,
        cancelled: false,
    });

    Ok(session_id)
}

/// Open a session from saved config (non-IPC, for startup auto-init).
/// Called at startup so diagnostics and architect can use the session
/// before the frontend calls open_session.
pub fn open_session_from_config(worker_url: &str, user_id: &str, device_id: &str) {
    if worker_url.is_empty() || user_id.is_empty() {
        return;
    }
    let normalized = worker_url
        .replace("ws://", "http://")
        .replace("wss://", "https://")
        .replace("/ws", "");
    *SESSION.lock() = Some(Session {
        worker_url: normalized,
        user_id: user_id.to_string(),
        device_id: device_id.to_string(),
        cancelled: false,
    });
    tracing::info!("network: session auto-opened from config (user={})", user_id);
}

/// Public accessor for the current Worker session config.
/// Used by architect.rs to POST enrichment requests directly to the Worker
/// without going through the full transcript flow.
pub fn get_session_info() -> Option<(String, String, String)> {
    let guard = SESSION.lock();
    guard.as_ref().map(|s| {
        (s.worker_url.clone(), s.user_id.clone(), s.device_id.clone())
    })
}

/// IPC: send transcript text to the Worker via HTTP POST.
/// Emits state/ack/result/done events to the frontend as the request progresses.
#[tauri::command]
pub async fn send_transcript<R: Runtime>(
    app: AppHandle<R>,
    text: String,
    dialog_context: Option<serde_json::Value>,
) -> Result<(), String> {
    let session_info = {
        let mut guard = SESSION.lock();
        match guard.as_mut() {
            Some(s) => {
                s.cancelled = false;
                (s.worker_url.clone(), s.user_id.clone(), s.device_id.clone())
            }
            None => return Err("no session open".into()),
        }
    };
    let (worker_url, user_id, device_id) = session_info;

    // 1. Emit "thinking" state
    let _ = app.emit("assistant:server", ServerEvent::state("thinking"));

    // 2. Emit ack immediately (the client speaks this locally via TTS)
    let ack = ACK_PHRASES[uuid_v4().as_bytes()[0] as usize % ACK_PHRASES.len()];
    let _ = app.emit("assistant:server", ServerEvent::ack(ack));

    // 3. Build the request payload
    let session_id = uuid_v4();
    let task = if let Some(ctx) = &dialog_context {
        serde_json::json!({
            "type": "general",
            "request": text,
            "dialog_context": ctx,
        })
    } else {
        serde_json::json!({
            "type": "general",
            "request": text,
        })
    };
    let payload = serde_json::json!({
        "request_id": session_id,
        "requester": {
            "id": user_id,
            "device_id": device_id,
        },
        "task": task,
    });

    // 4. HTTP POST to the Worker
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    eprintln!("[NEXUS] sending transcript to worker: url={} text={}", worker_url, text.chars().take(80).collect::<String>());

    let resp = crate::device_auth::apply_to(client.post(&worker_url))
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            let msg = format!("worker request: {e}");
            eprintln!("[NEXUS] worker request failed: {} — is_connect={}", msg, e.is_connect());
            msg
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let _ = app.emit("assistant:server", ServerEvent::error(&format!(
            "Worker error {status}: {body}"
        )));
        return Ok(());
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| {
            let msg = format!("worker json: {e}");
            eprintln!("[NEXUS] {}", msg);
            msg
        })?;

    eprintln!("[NEXUS] worker response received: reply_text len={}", data["reply_text"].as_str().map(|s| s.len()).unwrap_or(0));

    // 5. Check if cancelled while we were waiting
    {
        let guard = SESSION.lock();
        if let Some(s) = guard.as_ref() {
            if s.cancelled {
                return Ok(());
            }
        }
    }

    // 6. Extract reply text and emit result
    let reply_text = data["reply_text"]
        .as_str()
        .or(data["text"].as_str())
        .or(data["content"].as_str())
        .or(data["response"].as_str())
        .unwrap_or("I couldn't process that request.");

    // Check if the Worker included structured analysis data and/or dialog state
    let analysis = data.get("analysis");
    let dialog_state = data.get("dialog_state");
    if let (Some(a), Some(d)) = (analysis, dialog_state) {
        let _ = app.emit("assistant:server", ServerEvent::result_with_analysis_and_dialog(reply_text, a.clone(), d.clone()));
    } else if let Some(a) = analysis {
        let _ = app.emit("assistant:server", ServerEvent::result_with_analysis(reply_text, a.clone()));
    } else if let Some(d) = dialog_state {
        let _ = app.emit("assistant:server", ServerEvent::result_with_dialog(reply_text, d.clone()));
    } else {
        let _ = app.emit("assistant:server", ServerEvent::result(reply_text));
    }

    // 7. Do NOT emit "done" immediately — the frontend will emit "done"
    // after TTS finishes speaking the result. Emitting "done" here would
    // cause the frontend's done handler to call stopTts(), cancelling the
    // response before the user hears it.

    Ok(())
}

/// IPC: cancel the current turn.
#[tauri::command]
pub async fn cancel_session() -> Result<(), String> {
    if let Some(s) = SESSION.lock().as_mut() {
        s.cancelled = true;
    }
    Ok(())
}

/// IPC: close the session (reset state).
#[tauri::command]
pub async fn close_session() -> Result<(), String> {
    *SESSION.lock() = None;
    Ok(())
}

/// Background monitor (no-op for HTTP mode — no persistent connection to maintain).
pub async fn run<R: Runtime>(_app: AppHandle<R>) -> Result<(), String> {
    std::future::pending::<()>().await;
    Ok(())
}
