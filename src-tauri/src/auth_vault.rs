//! Auth vault — one login per service group for all MCPs.
//!
//! Tokens live in the OS credential store (Windows Credential Manager /
//! macOS Keychain / Linux Secret Service) via the `keyring` crate, with an
//! in-memory cache for expiry. `settings.json` never holds secrets.
//!
//! Service groups (see docs/mcp/01-auth-vault-one-login.md):
//!   - "google"   — Gmail+Calendar+Contacts+Drive+Sheets+Meet (one consent).
//!                  Fetched from the Worker (OAuth + refresh server-side).
//!   - "swiggy"   — Swiggy OAuth (wired in a later phase).
//!   - "spotify"  — Spotify OAuth (later phase).
//!   - "vercel"   — Vercel token (later phase).
//!   - "render"   — Render OAuth/key (later phase).
//! Session/cookie services (WhatsApp, LinkedIn) report validity via their
//! bridges, not this vault.

use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

/// Vault services with token credentials.
pub const VAULT_SERVICES: &[&str] = &["google", "swiggy", "spotify", "vercel", "render", "telegram"];

/// Refresh 5 minutes before expiry (mirrors github_cmd.rs).
const REFRESH_SKEW_SECS: f64 = 300.0;

#[derive(Debug, Clone)]
struct CachedToken {
    token: String,
    expires_at: f64,
}

impl CachedToken {
    fn is_valid(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as f64;
        now < self.expires_at - REFRESH_SKEW_SECS
    }
}

static MEMORY: Lazy<RwLock<HashMap<String, CachedToken>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn keyring_key(service: &str) -> String {
    format!("nexus-mcp-{service}")
}

/// Stored keyring payload: "<token>|<expires_at_unix>".
fn encode(token: &str, expires_at: f64) -> String {
    format!("{token}|{expires_at}")
}

fn decode(payload: &str) -> Option<(String, f64)> {
    let (token, exp) = payload.rsplit_once('|')?;
    let expires_at: f64 = exp.parse().ok()?;
    if token.is_empty() {
        return None;
    }
    Some((token.to_string(), expires_at))
}

/// Store a token (memory + OS keychain, best-effort).
/// Keychain failures only warn — memory cache keeps the session working.
pub fn set_token(service: &str, token: &str, expires_in_secs: f64) {
    let now = chrono::Utc::now().timestamp() as f64;
    let cached = CachedToken {
        token: token.to_string(),
        expires_at: now + expires_in_secs,
    };
    MEMORY
        .write()
        .insert(service.to_string(), cached.clone());
    match keyring::Entry::new("com.nexus.assistant", &keyring_key(service)) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(&encode(token, cached.expires_at)) {
                tracing::warn!("vault: keychain write failed for {service}: {e}");
            }
        }
        Err(e) => tracing::warn!("vault: keychain entry failed for {service}: {e}"),
    }
}

/// Get a valid token, or `None` (missing/expired).
/// Expired entries are evicted (memory + keychain) so later reads don't
/// keep decoding dead credentials — use `get_valid_token` when a refresher
/// may exist for the service.
pub fn get_token(service: &str) -> Option<String> {
    // Scoped read: the guard must drop before any write below
    // (parking_lot RwLock is not reentrant — holding read while taking
    // write deadlocks the thread).
    let live_copy = MEMORY
        .read()
        .get(service)
        .filter(|c| c.is_valid())
        .map(|c| c.token.clone());
    if let Some(token) = live_copy {
        return Some(token);
    }
    // Memory copy is missing or dead: drop it now (keychain re-warms
    // below if it holds a fresher copy; otherwise the entry is gone).
    MEMORY.write().remove(service);
    // Fall back to the OS keychain (warm after restart).
    let entry = keyring::Entry::new("com.nexus.assistant", &keyring_key(service)).ok()?;
    let payload = entry.get_password().ok()?;
    let (token, expires_at) = decode(&payload)?;
    let cached = CachedToken { token, expires_at };
    if cached.is_valid() {
        let token = cached.token.clone();
        MEMORY.write().insert(service.to_string(), cached);
        Some(token)
    } else {
        // Dead credential: evict everywhere so status reads "missing"
        // (reconnectable) instead of "expired" (stuck).
        MEMORY.write().remove(service);
        let _ = entry.delete_credential();
        None
    }
}

/// Forget a service (logout / disconnect).
pub fn clear_token(service: &str) {
    MEMORY.write().remove(service);
    if let Ok(entry) = keyring::Entry::new("com.nexus.assistant", &keyring_key(service)) {
        if let Err(e) = entry.delete_credential() {
            tracing::debug!("vault: keychain delete for {service}: {e}");
        }
    }
}

/// Status for the dashboard: "live" | "expired" | "missing".
pub fn token_status(service: &str) -> &'static str {
    if let Some(cached) = MEMORY.read().get(service) {
        if cached.is_valid() {
            return "live";
        }
        return "expired";
    }
    match keyring::Entry::new("com.nexus.assistant", &keyring_key(service)) {
        Ok(entry) => match entry.get_password().ok().and_then(|p| decode(&p)) {
            Some((_, exp)) => {
                let now = chrono::Utc::now().timestamp() as f64;
                if now < exp - REFRESH_SKEW_SECS {
                    "live"
                } else {
                    "expired"
                }
            }
            None => "missing",
        },
        Err(_) => "missing",
    }
}

/// Fetch a fresh Google access token from the Worker (which owns OAuth +
/// refresh server-side) and cache it in the vault. Assumed 1-hour life —
/// the Worker refreshes on its end; we re-fetch when our copy is stale.
async fn fetch_google_token_from_worker(
    worker_url: &str,
    user_id: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/oauth/google-token?user_id={}",
        worker_url.trim_end_matches('/'),
        user_id
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("google token fetch: {e}"))?;
    if resp.status() == 404 {
        return Err("Google not connected. Connect Google in setup.".into());
    }
    if !resp.status().is_success() {
        return Err(format!("google token error {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("token json: {e}"))?;
    data["token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "token field missing in worker response".to_string())
}

/// Get a valid Google token: vault first, Worker on miss/expiry.
pub async fn get_valid_google_token(
    worker_url: &str,
    user_id: &str,
) -> Result<String, String> {
    if let Some(token) = get_token("google") {
        return Ok(token);
    }
    let token = fetch_google_token_from_worker(worker_url, user_id).await?;
    set_token("google", &token, 3600.0);
    Ok(token)
}

/// Fetch a fresh Swiggy access token from the Worker (OAuth + refresh
/// server-side, same shape as Google) and cache it in the vault.
async fn fetch_swiggy_token_from_worker(
    worker_url: &str,
    user_id: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/oauth/swiggy-token?user_id={}",
        worker_url.trim_end_matches('/'),
        user_id
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("swiggy token fetch: {e}"))?;
    if resp.status() == 404 {
        return Err("Swiggy not connected. Connect Swiggy in Settings, Connections tab.".into());
    }
    if !resp.status().is_success() {
        return Err(format!("swiggy token error {}", resp.status()));
    }
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("token json: {e}"))?;
    data["token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "token field missing in worker response".to_string())
}

/// Refresh registry: services that can mint a fresh token without the user.
/// Google and Swiggy refresh via the Worker (OAuth + refresh server-side).
/// Spotify / Vercel / Render remain manual (vault_set_token) until their
/// OAuth phases land — refresh returns None and the caller guides reconnect.
async fn refresh_service_token(
    service: &str,
    worker_url: Option<&str>,
    user_id: Option<&str>,
) -> Option<String> {
    match service {
        "google" => match (worker_url, user_id) {
            (Some(url), Some(uid)) => {
                match fetch_google_token_from_worker(url, uid).await {
                    Ok(token) => {
                        set_token(service, &token, 3600.0);
                        Some(token)
                    }
                    Err(e) => {
                        tracing::warn!("vault: google refresh failed: {e}");
                        None
                    }
                }
            }
            _ => None,
        },
        "swiggy" => match (worker_url, user_id) {
            (Some(url), Some(uid)) => {
                match fetch_swiggy_token_from_worker(url, uid).await {
                    Ok(token) => {
                        set_token(service, &token, 3600.0);
                        Some(token)
                    }
                    Err(e) => {
                        tracing::warn!("vault: swiggy refresh failed: {e}");
                        None
                    }
                }
            }
            _ => None,
        },
        _ => None,
    }
}

/// Valid token with refresh: memory/keychain first, refresher on miss,
/// eviction when nothing can mint. This is the call dispatch paths must
/// use — never raw `get_token`, which can't heal.
pub async fn get_valid_token(
    service: &str,
    worker_url: Option<&str>,
    user_id: Option<&str>,
) -> Option<String> {
    if let Some(token) = get_token(service) {
        return Some(token);
    }
    refresh_service_token(service, worker_url, user_id).await
}

/// Resolve a bearer token for an MCP server: vault (with refresh) or None.
/// `None` means anonymous — the caller must speak reconnect guidance.
pub async fn resolve_server_token(server: crate::mcp_client::McpServer) -> Option<String> {
    let key = server.vault_key()?;
    let session = crate::network::get_session_info();
    match session {
        Some((url, uid, _)) => get_valid_token(key, Some(&url), Some(&uid)).await,
        None => get_valid_token(key, None, None).await,
    }
}

/// Last-known statuses for edge-triggered idle alerts (service → status).
/// Only transitions are reported — never repeat nags.
static LAST_KNOWN: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// Idle credential monitor: every 90s, re-read each vault service status.
/// On live→expired/missing transitions, log + emit `vault:changed` so the
/// Connections tab refreshes itself. No TTS while idle (nobody's listening);
/// the user hears about it on next use via the pre-flight guidance, and
/// sees the badge in Connections. This is the "inform when idle" half;
/// dispatch pre-flight is the "inform on use" half.
pub fn spawn_monitor<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Emitter;
    // Seed baseline silently (no alert for pre-existing state).
    {
        let mut known = LAST_KNOWN.write();
        for s in VAULT_SERVICES {
            known.insert(s.to_string(), token_status(s).to_string());
        }
    }
    std::thread::Builder::new()
        .name("vault-monitor".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(90));
            let mut changed: Vec<String> = Vec::new();
            {
                let mut known = LAST_KNOWN.write();
                for s in VAULT_SERVICES {
                    let now = token_status(s).to_string();
                    let prev = known.get(*s).cloned().unwrap_or_default();
                    if prev == "live" && (now == "expired" || now == "missing") {
                        changed.push(s.to_string());
                    }
                    known.insert(s.to_string(), now);
                }
            }
            for service in changed {
                tracing::warn!(
                    "vault: {service} went stale while idle — reconnect in Settings → Connections"
                );
                let _ = app.emit("vault:changed", service.clone());
            }
            // WhatsApp session rotation (≈20 days, WhatsApp-side): probe the
            // bridge's pairing_status gently (1 probe / 90s ≪ the 5/min
            // /pair rate limit). When the session drops, the Connections
            // badge refreshes and the next send gets a fresh QR card —
            // the user learns about rotation BEFORE a message fails.
            if let Err(e) = probe_whatsapp_pairing() {
                tracing::debug!("vault: whatsapp pairing probe: {e}");
            }
        })
        .ok();
}

/// One cheap pairing_status probe against the WhatsApp bridge, for the
/// idle monitor. Never trips the breaker (inner call) and never audits
/// (status poll, not a user action).
fn probe_whatsapp_pairing() -> Result<bool, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("rt: {e}"))?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .connect_timeout(Duration::from_secs(3))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let state = crate::mcp_client::query_pairing_status(&client).await;
        match state {
            crate::mcp_client::PairingState::Ready => Ok(true),
            other => {
                tracing::info!("vault: whatsapp pairing state while idle: {other:?}");
                Ok(false)
            }
        }
    })
}

/// Per-service status for the Connect dashboard / diagnostics.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VaultEntry {
    pub service: String,
    pub status: String,
}

#[tauri::command]
pub async fn vault_status() -> Vec<VaultEntry> {
    VAULT_SERVICES
        .iter()
        .map(|s| VaultEntry {
            service: s.to_string(),
            status: token_status(s).to_string(),
        })
        .collect()
}

/// Store a user-supplied credential (API token / OAuth token) for a vault
/// service. Used by the Connections tab for token-based services
/// (Vercel, Render key, Spotify). `expires_in_secs`: 0 = no known expiry
/// (treated as 10 years so it stays "live" until deleted).
#[tauri::command]
pub async fn vault_set_token(
    service: String,
    token: String,
    expires_in_secs: Option<f64>,
) -> Result<(), String> {
    if !VAULT_SERVICES.contains(&service.as_str()) {
        return Err(format!("unknown vault service: {service}"));
    }
    let token = token.trim();
    if token.is_empty() {
        return Err("empty token".to_string());
    }
    let ttl = match expires_in_secs {
        Some(s) if s > 0.0 => s,
        _ => 10.0 * 365.0 * 24.0 * 3600.0,
    };
    set_token(&service, token, ttl);
    Ok(())
}

/// Delete a vault credential (per-service logout). Does not touch the
/// provider side — for OAuth services also use provider Disconnect.
#[tauri::command]
pub async fn vault_clear_token(service: String) -> Result<(), String> {
    if !VAULT_SERVICES.contains(&service.as_str()) {
        return Err(format!("unknown vault service: {service}"));
    }
    clear_token(&service);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_roundtrip_and_expiry() {
        set_token("test-svc", "tok123", 3600.0);
        assert_eq!(get_token("test-svc").as_deref(), Some("tok123"));
        assert_eq!(token_status("test-svc"), "live");
        set_token("test-svc", "old", -10.0);
        // Memory copy expired; keychain copy (if writable) also expired.
        assert!(get_token("test-svc").is_none());
        clear_token("test-svc");
        assert_eq!(token_status("test-svc"), "missing");
    }

    #[test]
    fn payload_codec() {
        let enc = encode("abc", 1234.5);
        assert_eq!(decode(&enc), Some(("abc".to_string(), 1234.5)));
        assert!(decode("no-sep").is_none());
    }

    #[test]
    fn expired_token_evicts_and_reports_missing() {
        // Dead credentials must not linger: reads evict, status flips to
        // missing (reconnectable) instead of stuck-expired.
        set_token("test-evict", "dead", -10.0);
        assert!(get_token("test-evict").is_none());
        eprintln!("after-get memory-present={} status={}",
            MEMORY.read().contains_key("test-evict"), token_status("test-evict"));
        assert_eq!(token_status("test-evict"), "missing");
        clear_token("test-evict");
    }

    #[tokio::test]
    async fn valid_token_without_refresher_returns_none_when_dead() {
        // swiggy/spotify/vercel have no auto-refresh yet: dead stays dead
        // (caller speaks reconnect guidance) instead of replaying.
        set_token("test-norefresh", "dead", -10.0);
        let got = get_valid_token("test-norefresh", None, None).await;
        assert!(got.is_none());
        clear_token("test-norefresh");
    }
}
