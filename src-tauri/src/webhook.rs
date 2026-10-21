//! Local webhook triggers (C5): external systems fire assistant actions.
//!
//! A minimal HTTP listener on 127.0.0.1:39220 (localhost ONLY — never
//! 0.0.0.0). `POST /trigger` with `Authorization: Bearer <token>` and
//! JSON `{"transcript": "..."}` routes through the normal orchestrator
//! pipeline. Use cases: Task Scheduler reminders, home automation,
//! scripts (`curl -X POST localhost:39220/trigger ...`).
//!
//! Security: localhost bind + bearer token (keychain-stored, rotatable)
//! + 8KB body cap + read timeouts. No new dependencies (std TcpListener).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub const WEBHOOK_PORT: u16 = 39220;
const MAX_BODY: usize = 8192;
const TOKEN_SERVICE: &str = "webhook";

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Get-or-create the webhook bearer token (keychain-stored).
pub fn webhook_token() -> String {
    if let Some(tok) = crate::auth_vault::get_api_key(TOKEN_SERVICE) {
        if !tok.is_empty() {
            return tok;
        }
    }
    // std-only entropy: time + pid + counter + stack address, SipHashed.
    // Localhost-only bearer — no network attacker model.
    let mut h = DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut h);
    (&raw const TOKEN_COUNTER as usize).hash(&mut h);
    let mut seed = h.finish();
    let tok: String = (0..32)
        .map(|_| {
            // xorshift64* PRNG step
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            let v = (seed.wrapping_mul(0x2545F4914F6CDD1D) >> 60) as usize % 16;
            const HEX: &[u8] = b"0123456789abcdef";
            HEX[v] as char
        })
        .collect();
    crate::auth_vault::set_api_key(TOKEN_SERVICE, &tok);
    tok
}

/// Rotate the webhook token. Returns the new token.
pub fn rotate_webhook_token() -> String {
    crate::auth_vault::clear_api_key(TOKEN_SERVICE);
    webhook_token()
}

/// Parsed webhook request. Pure + unit-tested.
#[derive(Debug, PartialEq)]
pub struct WebhookRequest {
    pub method: String,
    pub path: String,
    pub auth: String,
    pub body: String,
}

/// Parse a raw HTTP request (headers + body already joined).
/// Returns None on malformed input or oversize body.
pub fn parse_request(raw: &str) -> Option<WebhookRequest> {
    let (head, body) = raw.split_once("\r\n\r\n")?;
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.split('?').next()?.to_string();
    let mut auth = String::new();
    for line in lines {
        if let Some(v) = line.strip_prefix("Authorization:") {
            auth = v.trim().to_string();
        }
    }
    if body.len() > MAX_BODY {
        return None;
    }
    Some(WebhookRequest {
        method,
        path,
        auth,
        body: body.to_string(),
    })
}

/// Constant-time bearer comparison (no timing oracle on localhost, but cheap).
fn bearer_ok(header: &str, token: &str) -> bool {
    let expect = format!("Bearer {}", token);
    if header.len() != expect.len() {
        return false;
    }
    header
        .bytes()
        .zip(expect.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    }
}

fn respond(stream: &mut std::net::TcpStream, code: u16, body: &str) {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        reason(code),
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
}

/// Handle one connection. Returns a transcript to process, if triggered.
fn handle_conn(
    mut stream: std::net::TcpStream,
    token: &str,
) -> Option<String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok()?;
    // Read headers first, then the exact body per Content-Length.
    let mut buf = vec![0u8; 4096];
    let mut raw = Vec::new();
    let mut header_end = None;
    while header_end.is_none() {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > MAX_BODY + 2048 {
            respond(&mut stream, 400, r#"{"error":"too large"}"#);
            return None;
        }
        header_end = find_header_end(&raw);
    }
    let hlen = header_end?;
    let head = String::from_utf8_lossy(&raw[..hlen]).to_string();
    let content_len: usize = head
        .lines()
        .find_map(|l| {
            l.strip_prefix("Content-Length:")
                .or_else(|| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
        })
        .unwrap_or(0);
    if content_len > MAX_BODY {
        respond(&mut stream, 400, r#"{"error":"too large"}"#);
        return None;
    }
    while raw.len() < hlen + content_len {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
    }
    let body = String::from_utf8_lossy(&raw[hlen..]).to_string();
    let req = parse_request(&format!("{}\r\n\r\n{}", head, body))?;

    if req.method == "GET" && req.path == "/health" {
        respond(&mut stream, 200, r#"{"ok":true,"service":"nexus-webhook"}"#);
        return None;
    }
    if req.method != "POST" || req.path != "/trigger" {
        respond(&mut stream, 404, r#"{"error":"not found"}"#);
        return None;
    }
    if !bearer_ok(&req.auth, token) {
        respond(&mut stream, 401, r#"{"error":"unauthorized"}"#);
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(&req.body).ok()?;
    let transcript = json.get("transcript")?.as_str()?.trim().to_string();
    if transcript.is_empty() || transcript.len() > 500 {
        respond(&mut stream, 400, r#"{"error":"bad transcript"}"#);
        return None;
    }
    respond(&mut stream, 202, r#"{"ok":true,"queued":true}"#);
    Some(transcript)
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
}

/// Spawn the localhost webhook listener. Fire-and-forget thread; logs only.
pub fn spawn_listener<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Manager;
    std::thread::Builder::new()
        .name("webhook-rx".into())
        .spawn(move || {
            let listener = match TcpListener::bind(("127.0.0.1", WEBHOOK_PORT)) {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("webhook: bind failed (port busy?): {}", e);
                    return;
                }
            };
            let token = webhook_token();
            tracing::info!("webhook: listening on 127.0.0.1:{} (localhost only)", WEBHOOK_PORT);
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let peer_local = stream
                    .peer_addr()
                    .map(|a| a.ip().is_loopback())
                    .unwrap_or(false);
                if !peer_local {
                    continue;
                }
                if let Some(transcript) = handle_conn(stream, &token) {
                    tracing::info!(
                        "webhook: trigger {:?}...",
                        transcript.chars().take(60).collect::<String>()
                    );
                    if let Ok(dir) = app.path().app_data_dir() {
                        crate::diary::log_event(&dir, "webhook", &transcript);
                    }
                    let app_c = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::orchestrator::process_transcript(
                            app_c,
                            transcript,
                            None,
                            None,
                        )
                        .await;
                    });
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_trigger() {
        let raw = "POST /trigger HTTP/1.1\r\nAuthorization: Bearer abc\r\nContent-Length: 3\r\n\r\n{\"a\":1}";
        let r = parse_request(raw).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/trigger");
        assert_eq!(r.auth, "Bearer abc");
    }

    #[test]
    fn test_parse_health() {
        let raw = "GET /health HTTP/1.1\r\nHost: x\r\n\r\n";
        let r = parse_request(raw).unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/health");
    }

    #[test]
    fn test_parse_strips_query() {
        let raw = "GET /health?x=1 HTTP/1.1\r\n\r\n";
        assert_eq!(parse_request(raw).unwrap().path, "/health");
    }

    #[test]
    fn test_parse_malformed_none() {
        assert!(parse_request("garbage").is_none());
        assert!(parse_request("GET").is_none());
    }

    #[test]
    fn test_bearer_ok() {
        assert!(bearer_ok("Bearer abc123", "abc123"));
        assert!(!bearer_ok("Bearer abc123", "abc124"));
        assert!(!bearer_ok("Bearer short", "much-longer-token"));
        assert!(!bearer_ok("", "abc"));
    }

    #[test]
    fn test_find_header_end() {
        let raw = b"GET / HTTP/1.1\r\n\r\nbody";
        assert_eq!(find_header_end(raw), Some(18));
        assert_eq!(find_header_end(b"incomplete"), None);
    }
}
