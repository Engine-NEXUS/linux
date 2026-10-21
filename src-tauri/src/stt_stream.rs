//! Live partial-transcript stream client — connects to Deepgram Nova-2
//! cloud WebSocket (when configured) or local STT server's `/stream` endpoint.
//! Re-emits growing partial lines as `stt:partial` events for on-screen live captions.
//!
//! Hard constraint: this is PURELY ADDITIVE and runs entirely parallel to
//! the existing batch STT pipeline (`stt.rs`'s `transcribe_audio` / `transcribe_samples`),
//! which this module never touches. Every operation here swallows its own errors — a dropped or
//! failed stream must never affect command execution.

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use once_cell::sync::Lazy;
use tauri::Emitter;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Fallback host/port for local STT server.
const STREAM_URL: &str = "ws://127.0.0.1:39217/stream";

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;

/// The live connection's write half, if one is currently open. `None`
/// means "no stream" — every command below treats that as a silent no-op.
static SINK: Lazy<Mutex<Option<WsSink>>> = Lazy::new(|| Mutex::new(None));

/// IPC: open the live partial-transcript stream for this turn.
#[tauri::command]
pub async fn stt_stream_start<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let deepgram_key = crate::commands::read_api_key(&app, "deepgram");
    if !deepgram_key.is_empty() {
        let ws_url = "wss://api.deepgram.com/v1/listen?model=nova-2&smart_format=true&encoding=linear16&sample_rate=16000&channels=1&interim_results=true";
        if let Ok(mut req) = ws_url.into_client_request() {
            if let Ok(auth_header) = format!("Token {deepgram_key}").parse() {
                req.headers_mut().insert("Authorization", auth_header);
                match tokio_tungstenite::connect_async(req).await {
                    Ok((ws, _)) => {
                        let (sink, mut read) = ws.split();
                        *SINK.lock().await = Some(sink);
                        let app_clone = app.clone();
                        tauri::async_runtime::spawn(async move {
                            while let Some(msg) = read.next().await {
                                let Ok(Message::Text(text)) = msg else { continue };
                                let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
                                    continue;
                                };
                                if let Some(transcript) = payload
                                    .get("channel")
                                    .and_then(|ch| ch.get("alternatives"))
                                    .and_then(|alts| alts.as_array())
                                    .and_then(|arr| arr.first())
                                    .and_then(|alt| alt.get("transcript"))
                                    .and_then(|t| t.as_str())
                                {
                                    let trimmed = transcript.trim();
                                    if !trimmed.is_empty() {
                                        let _ = app_clone.emit("stt:partial", serde_json::json!({ "text": trimmed }));
                                    }
                                }
                            }
                            tracing::debug!("stt_stream: deepgram read loop ended");
                        });
                        tracing::debug!("stt_stream: connected to Deepgram Nova-2 stream");
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::debug!("stt_stream: deepgram connect failed ({e}) — falling back");
                    }
                }
            }
        }
    }

    match tokio_tungstenite::connect_async(STREAM_URL).await {
        Ok((ws, _response)) => {
            let (sink, mut read) = ws.split();
            *SINK.lock().await = Some(sink);
            tauri::async_runtime::spawn(async move {
                while let Some(msg) = read.next().await {
                    let Ok(Message::Text(text)) = msg else { continue };
                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if let Some(partial) = payload.get("text").and_then(|v| v.as_str()) {
                        let _ = app.emit("stt:partial", serde_json::json!({ "text": partial }));
                    }
                }
                tracing::debug!("stt_stream: read loop ended");
            });
            tracing::debug!("stt_stream: connected to local stream");
        }
        Err(e) => {
            tracing::debug!("stt_stream: connect failed ({e}) — live caption disabled this turn");
            *SINK.lock().await = None;
        }
    }
    Ok(())
}

/// IPC: push one chunk of raw 16-bit LE mono PCM @ 16kHz.
#[tauri::command]
pub async fn stt_stream_push_chunk(samples: Vec<i16>) -> Result<(), String> {
    let mut guard = SINK.lock().await;
    let Some(sink) = guard.as_mut() else {
        return Ok(());
    };
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in &samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    if sink.send(Message::from(bytes)).await.is_err() {
        // Connection died mid-turn — stop trying until the next start().
        *guard = None;
    }
    Ok(())
}

/// IPC: end the live stream (turn finished, aborted, or barge-in).
#[tauri::command]
pub async fn stt_stream_stop() -> Result<(), String> {
    let mut guard = SINK.lock().await;
    if let Some(mut sink) = guard.take() {
        let _ = sink.send(Message::Binary(vec![].into())).await;
        let _ = sink.send(Message::from(r#"{"cmd":"stop"}"#.to_string())).await;
        let _ = sink.close().await;
    }
    Ok(())
}
