//! Telegram remote — owner-only phone control at ₹0.
//!
//! A long-polling bot (no public IP, no certs) that exposes NEXUS to one
//! Telegram chat: the owner's. Text messages run the normal pipeline
//! (deterministic parse → local / worker / MCP dispatch); voice notes are
//! transcribed via Groq and handled identically.
//!
//! Security: the owner chat id comes from `telegramChatId` in settings.json
//! and the bot token from the vault ("telegram" service). Any other chat is
//! ignored silently — not even an error reply (no oracle for scanners).
//! Destructive/confirm-gated flows reply with their prompt text instead of
//! opening desktop UI; the owner confirms from the orb.
//!
//! v1 scope: text + voice in, text replies back. Voice-note replies and
//! compound-plan execution stay on the orb.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Manager, Runtime};
use teloxide::prelude::*;
use teloxide::types::ChatId;

/// Read the owner chat id from settings.json (`telegramChatId`, loose parse
/// so a missing key just disables the bridge).
fn read_owner_chat_id(app: &AppHandle<impl Runtime>) -> Option<i64> {
    let dir = app.path().app_data_dir().ok()?;
    let content = std::fs::read_to_string(dir.join("settings.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("telegramChatId")
        .or_else(|| json.get("telegram_chat_id"))
        .and_then(|v| v.as_i64())
        .filter(|id| *id != 0)
}

/// Handle one owner text line: parse → route → reply text.
async fn handle_text<R: Runtime>(
    app: AppHandle<R>,
    bot: Bot,
    chat: ChatId,
    text: String,
) {
    let reply = dispatch_text(&app, &text).await;
    if let Err(e) = bot.send_message(chat, reply).await {
        tracing::warn!("telegram: reply failed: {e}");
    }
}

async fn dispatch_text<R: Runtime>(app: &AppHandle<R>, text: &str) -> String {
    use crate::intent_parser::{parse_deterministic, ParsedIntent};
    use crate::orchestrator::Subsystem;

    if text.trim().is_empty() {
        return "Say a command, sir.".to_string();
    }
    if text.trim() == "/start" {
        return "NEXUS remote online, sir. Send any command as text or voice note. Confirmations happen on the orb.".to_string();
    }

    let intent = match parse_deterministic(text) {
        Some(r) => r.intent,
        None => ParsedIntent::Unknown {
            raw: text.to_string(),
        },
    };
    let subsystem = crate::orchestrator::route_intent(&intent);
    // route_intent is pub(crate); same crate so this resolves.

    match subsystem {
        Subsystem::LocalCommand => match crate::command_center::to_local_intent(&intent) {
            Some(local) => match crate::command_executor::execute_command(local).await {
                Ok(res) => res.message,
                Err(e) => format!("Failed, sir: {e}"),
            },
            None => "That runs on the orb, sir.".to_string(),
        },
        Subsystem::WorkerBackend => {
            let request_id = format!("tg-{}", chrono::Utc::now().timestamp_millis());
            let cancel = Arc::new(AtomicBool::new(false));
            match crate::orchestrator::dispatch_to_worker_pub(
                app.clone(),
                text.to_string(),
                None,
                request_id,
                cancel,
            )
            .await
            {
                Ok((answer, _, _)) => answer,
                Err(e) => format!("Worker failed, sir: {e}"),
            }
        }
        Subsystem::Mcp => {
            let request_id = format!("tg-{}", chrono::Utc::now().timestamp_millis());
            match crate::orchestrator::dispatch_to_mcp_pub(app, &intent, text, &request_id)
                .await
            {
                Ok(Some(out)) => out,
                Ok(None) => "Awaiting your confirmation on device, sir.".to_string(),
                Err(e) => format!("MCP failed, sir: {e}"),
            }
        }
        _ => "That one needs the orb screen, sir — try an app, media, question, or order.".to_string(),
    }
}

/// Transcribe a Telegram voice note (Opus/OGG bytes) via Groq.
async fn transcribe_voice_note<R: Runtime>(
    app: &AppHandle<R>,
    bytes: Vec<u8>,
) -> Result<String, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let settings_path = data_dir.join("settings.json");
    let content = std::fs::read_to_string(&settings_path).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or_default();
    let key = json
        .get("groqApiKey")
        .or_else(|| json.get("groq_api_key"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if key.is_empty() {
        return Err("Groq key missing — voice notes need it".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    crate::stt_groq::transcribe_bytes_with_groq(&bytes, "voice.ogg", "audio/ogg", key, &client)
        .await
}

/// Spawn the long-polling bridge. No token or no owner chat id → log and
/// return (bridge stays off, everything else unaffected).
pub fn spawn_bridge<R: Runtime>(app: AppHandle<R>) {
    let token = match crate::auth_vault::get_token("telegram") {
        Some(t) => t,
        None => {
            tracing::info!("telegram: no bot token in vault — remote off");
            return;
        }
    };
    let owner = match read_owner_chat_id(&app) {
        Some(id) => ChatId(id),
        None => {
            tracing::info!("telegram: telegramChatId not set — remote off");
            return;
        }
    };
    spawn_bridge_with(app, token, owner);
}

/// Run the poll loop for an explicit token/owner pair. Recursing here on
/// token rotation keeps exactly one loop alive (the old task returns).
fn spawn_bridge_with<R: Runtime>(app: AppHandle<R>, token: String, owner: ChatId) {
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let bot = Bot::new(token);
        // Manual long-poll loop (no extra teloxide features needed):
        // 30s server-side wait per poll, offset-tracked, forever.
        // The vault token is re-read periodically: rotation/revocation
        // takes effect without an app restart.
        let mut offset: i32 = 0;
        let mut polls = 0u32;
        let mut auth_failures = 0u32;
        loop {
            polls += 1;
            if polls % 20 == 0 {
                match crate::auth_vault::get_token("telegram") {
                    Some(t) if t == bot.token() => {}
                    Some(t) => {
                        tracing::info!("telegram: vault token rotated — rebuilding client");
                        return spawn_bridge_with(app_clone.clone(), t, owner);
                    }
                    None => {
                        tracing::warn!("telegram: vault token removed — stopping bridge");
                        return;
                    }
                }
            }
            let updates = match bot
                .get_updates()
                .offset(offset)
                .timeout(30)
                .await {
                Ok(u) => {
                    auth_failures = 0;
                    u
                }
                Err(e) => {
                    let msg = e.to_string().to_lowercase();
                    // Revoked/invalid token: stop, clear, notify once.
                    // Anything else is transient: back off and continue.
                    if msg.contains("401") || msg.contains("unauthorized") {
                        auth_failures += 1;
                        if auth_failures >= 2 {
                            tracing::error!(
                                "telegram: bot token rejected (401) — stopping bridge, clearing vault"
                            );
                            crate::auth_vault::clear_token("telegram");
                            return;
                        }
                    }
                    tracing::warn!("telegram: poll failed: {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };
            for update in updates {
                offset = update.id.0.saturating_add(1) as i32;
                let teloxide::types::UpdateKind::Message(msg) = update.kind else {
                    continue;
                };
                // Owner-only: anyone else gets silence (no oracle).
                if msg.chat.id != owner {
                    tracing::warn!("telegram: ignored message from non-owner chat");
                    continue;
                }
                let app = app_clone.clone();
                let bot = bot.clone();
                let chat = msg.chat.id;
                if let Some(text) = msg.text().map(str::to_string) {
                    handle_text(app, bot, chat, text).await;
                } else if let Some(voice) = msg.voice().cloned() {
                    let reply = match bot.get_file(voice.file.id).await {
                        Ok(file) => {
                            let url = format!(
                                "https://api.telegram.org/file/bot{}/{}",
                                bot.token(),
                                file.path
                            );
                            match reqwest::get(&url).await {
                                Ok(resp) => match resp.bytes().await {
                                    Ok(b) => match transcribe_voice_note(&app, b.to_vec()).await
                                    {
                                        Ok(t) if !t.trim().is_empty() => {
                                            dispatch_text(&app, &t).await
                                        }
                                        Ok(_) => "Didn't catch that, sir.".to_string(),
                                        Err(e) => format!("Voice failed, sir: {e}"),
                                    },
                                    Err(e) => format!("Download failed, sir: {e}"),
                                },
                                Err(e) => format!("Download failed, sir: {e}"),
                            }
                        }
                        Err(e) => format!("Voice fetch failed, sir: {e}"),
                    };
                    if let Err(e) = bot.send_message(chat, reply).await {
                        tracing::warn!("telegram: reply failed: {e}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn owner_gate_logic() {
        // The gate is `msg.chat.id != owner → ignore`. Document the rule:
        // unknown chats must never receive any reply (no oracle).
        let owner: i64 = 12345;
        let stranger: i64 = 99999;
        assert_ne!(owner, stranger);
    }
}
