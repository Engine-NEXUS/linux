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
pub const VAULT_SERVICES: &[&str] = &[
    "google", "swiggy", "spotify", "vercel", "render", "telegram",
    "figma", "supabase", "stripe", "slack", "netlify", "github",
];

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

/// The ONE place the vault opens an OS credential entry.
///
/// Unit tests must never touch the user's real Windows Credential Manager:
/// since the `windows-native` backend became real (change 107), the vault
/// tests were writing fake accounts (`user1@gmail.com`, …) and overwriting
/// the Google account registry in the real store — a `cargo test` after the
/// user connected Gmail would have wiped their real account list. Under
/// `cfg(test)` every entry is an in-memory mock instead. Set
/// `NEXUS_REAL_KEYRING=1` to run an ignored live probe against the real store.
fn kr(user: &str) -> keyring::Result<keyring::Entry> {
    #[cfg(test)]
    {
        use std::sync::Once;
        static MOCK: Once = Once::new();
        MOCK.call_once(|| {
            if std::env::var("NEXUS_REAL_KEYRING").is_err() {
                keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
            }
        });
    }
    keyring::Entry::new("com.nexus.assistant", user)
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
    match kr(&keyring_key(service)) {
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
    let entry = kr(&keyring_key(service)).ok()?;
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
    if let Ok(entry) = kr(&keyring_key(service)) {
        if let Err(e) = entry.delete_credential() {
            tracing::debug!("vault: keychain delete for {service}: {e}");
        }
    }
}

// ─── API key storage (no expiry — plain credentials) ────────────────

static APIKEY_MEMORY: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

fn apikey_key(service: &str) -> String {
    format!("nexus-apikey-{service}")
}

/// Store an API key in memory + OS keychain (best-effort).
pub fn set_api_key(service: &str, key: &str) {
    if key.is_empty() {
        clear_api_key(service);
        return;
    }
    APIKEY_MEMORY.write().insert(service.to_string(), key.to_string());
    match kr(&apikey_key(service)) {
        Ok(entry) => {
            if let Err(e) = entry.set_password(key) {
                tracing::warn!("vault: keychain write failed for apikey {service}: {e}");
            }
        }
        Err(e) => tracing::warn!("vault: keychain entry failed for apikey {service}: {e}"),
    }
}

/// Retrieve an API key from memory or OS keychain, or None.
pub fn get_api_key(service: &str) -> Option<String> {
    if let Some(key) = APIKEY_MEMORY.read().get(service) {
        if !key.is_empty() {
            return Some(key.clone());
        }
    }
    let entry = kr(&apikey_key(service)).ok()?;
    let key = entry.get_password().ok()?;
    if key.is_empty() {
        None
    } else {
        APIKEY_MEMORY.write().insert(service.to_string(), key.clone());
        Some(key)
    }
}

/// Delete an API key from memory and OS keychain.
pub fn clear_api_key(service: &str) {
    APIKEY_MEMORY.write().remove(service);
    if let Ok(entry) = kr(&apikey_key(service)) {
        if let Err(e) = entry.delete_credential() {
            tracing::debug!("vault: keychain delete for apikey {service}: {e}");
        }
    }
}

/// One-time migration: move API keys from settings.json to OS keychain.
/// Runs on startup. Idempotent — skips services already in keychain.
#[allow(dead_code)]
pub fn migrate_api_keys_to_keychain(app_data_dir: &std::path::Path) {
    let path = app_data_dir.join("settings.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };
    for service in ["groq", "gemini", "cerebras"] {
        // Skip if already in keychain
        if get_api_key(service).is_some() {
            continue;
        }
        let camel = format!("{service}ApiKey");
        let snake = format!("{service}_api_key");
        if let Some(key) = json
            .get(&camel)
            .or_else(|| json.get(&snake))
            .and_then(|v| v.as_str())
        {
            if !key.is_empty() {
                set_api_key(service, key);
                tracing::info!("vault: migrated {service} API key to keychain");
            }
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
    match kr(&keyring_key(service)) {
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
    // The device bearer is mandatory for the gated Worker.  Keep the legacy
    // user_id during the staged rollout; worker_proxy owns the one removal
    // flag and places the trusted native value first.
    let url = crate::worker_proxy::build_worker_url(
        worker_url,
        "/oauth/google-token",
        None,
        Some(user_id),
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = crate::worker_proxy::apply_to(client.get(&url))
        .send()
        .await
        .map_err(|e| format!("google token fetch: {e}"))?;
    if resp.status() == 404 {
        return Err("Google not connected. Connect Google in setup.".into());
    }
    if resp.status() == 401 || resp.status() == 403 {
        return Err(crate::worker_proxy::DEVICE_NOT_AUTHORIZED_MESSAGE.into());
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

/// Fetch a fresh Swiggy access token from the Worker (OAuth + refresh
/// server-side, same shape as Google) and cache it in the vault.
async fn fetch_swiggy_token_from_worker(
    worker_url: &str,
    user_id: &str,
) -> Result<String, String> {
    // See the Google fetch above: this remains transition-compatible with the
    // deployed Worker while also authenticating a request to the gated one.
    let url = crate::worker_proxy::build_worker_url(
        worker_url,
        "/oauth/swiggy-token",
        None,
        Some(user_id),
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = crate::worker_proxy::apply_to(client.get(&url))
        .send()
        .await
        .map_err(|e| format!("swiggy token fetch: {e}"))?;
    if resp.status() == 404 {
        return Err("Swiggy not connected. Connect Swiggy in Settings, Connections tab.".into());
    }
    if resp.status() == 401 || resp.status() == 403 {
        return Err(crate::worker_proxy::DEVICE_NOT_AUTHORIZED_MESSAGE.into());
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

// ─── Multi-Email Google Account Registry Vault ────────────────────────────────

use crate::google::types::GoogleAccountProfile;

static GOOGLE_ACCOUNTS_CACHE: Lazy<RwLock<Option<Vec<GoogleAccountProfile>>>> =
    Lazy::new(|| RwLock::new(None));

fn google_accounts_keyring_key() -> &'static str {
    "nexus-google-accounts"
}

fn google_refresh_token_keyring_key(email: &str) -> String {
    format!("nexus-google-rt-{}", email.to_lowercase().trim())
}

fn google_access_token_keyring_key(email: &str) -> String {
    format!("nexus-google-at-{}", email.to_lowercase().trim())
}

/// Retrieve all registered Google account profiles from vault.
pub fn get_google_accounts() -> Vec<GoogleAccountProfile> {
    if let Some(cached) = GOOGLE_ACCOUNTS_CACHE.read().as_ref() {
        return cached.clone();
    }
    let accounts: Vec<GoogleAccountProfile> = (|| {
        let entry = kr(google_accounts_keyring_key()).ok()?;
        let json_str = entry.get_password().ok()?;
        serde_json::from_str(&json_str).ok()
    })()
    .unwrap_or_default();

    *GOOGLE_ACCOUNTS_CACHE.write() = Some(accounts.clone());
    accounts
}

/// Save or update a Google account profile (and optional refresh token).
pub fn save_google_account(profile: GoogleAccountProfile, refresh_token: Option<&str>) {
    let mut accounts = get_google_accounts();
    let email_lower = profile.email.to_lowercase().trim().to_string();

    let mut profile = profile;
    if accounts.is_empty() {
        profile.is_primary = true;
    } else if profile.is_primary {
        for a in accounts.iter_mut() {
            a.is_primary = false;
        }
    }

    if let Some(pos) = accounts.iter().position(|a| a.email.to_lowercase().trim() == email_lower) {
        accounts[pos] = profile;
    } else {
        accounts.push(profile);
    }

    *GOOGLE_ACCOUNTS_CACHE.write() = Some(accounts.clone());

    if let Ok(json_str) = serde_json::to_string(&accounts) {
        if let Ok(entry) = kr(google_accounts_keyring_key()) {
            let _ = entry.set_password(&json_str);
        }
    }

    if let Some(rt) = refresh_token {
        if !rt.trim().is_empty() {
            // One copy only. (A second plain copy under `nexus-google-rt-…`
            // used to be written here; nothing ever read it.)
            set_token(&format!("google_rt_{email_lower}"), rt, 365.0 * 86400.0 * 10.0);
        }
    }
}

/// Set the primary Google account by email.
pub fn set_primary_google_account(email: &str) -> Result<(), String> {
    let mut accounts = get_google_accounts();
    let email_lower = email.to_lowercase().trim().to_string();
    let mut found = false;
    for a in accounts.iter_mut() {
        if a.email.to_lowercase().trim() == email_lower {
            a.is_primary = true;
            found = true;
        } else {
            a.is_primary = false;
        }
    }
    if !found {
        return Err(format!("Account '{email}' not found in registry"));
    }
    *GOOGLE_ACCOUNTS_CACHE.write() = Some(accounts.clone());
    if let Ok(json_str) = serde_json::to_string(&accounts) {
        if let Ok(entry) = kr(google_accounts_keyring_key()) {
            let _ = entry.set_password(&json_str);
        }
    }
    Ok(())
}

/// Disconnect and remove a Google account and its tokens.
pub fn remove_google_account(email: &str) -> Result<(), String> {
    let mut accounts = get_google_accounts();
    let email_lower = email.to_lowercase().trim().to_string();
    let was_primary = accounts.iter().any(|a| a.email.to_lowercase().trim() == email_lower && a.is_primary);

    accounts.retain(|a| a.email.to_lowercase().trim() != email_lower);

    if was_primary && !accounts.is_empty() {
        accounts[0].is_primary = true;
    }
    *GOOGLE_ACCOUNTS_CACHE.write() = Some(accounts.clone());

    if let Ok(json_str) = serde_json::to_string(&accounts) {
        if let Ok(entry) = kr(google_accounts_keyring_key()) {
            let _ = entry.set_password(&json_str);
        }
    }

    // The refresh/access tokens that `get_google_refresh_token` /
    // `get_google_access_token` actually read live in the token vault under
    // `google_rt_<email>` / `google_at_<email>` (keychain `nexus-mcp-…`).
    // Removing an account used to clear only two OTHER key names, so a
    // "removed" account's refresh token stayed in Credential Manager and in
    // memory. `clear_token` wipes both.
    clear_token(&format!("google_rt_{email_lower}"));
    clear_token(&format!("google_at_{email_lower}"));
    // Legacy duplicate keys older builds wrote.
    if let Ok(entry) = kr(&google_refresh_token_keyring_key(&email_lower)) {
        let _ = entry.delete_credential();
    }
    if let Ok(entry) = kr(&google_access_token_keyring_key(&email_lower)) {
        let _ = entry.delete_credential();
    }

    Ok(())
}

/// Retrieve the stored refresh token for a given email.
pub fn get_google_refresh_token(email: &str) -> Option<String> {
    let key = format!("google_rt_{}", email.to_lowercase().trim());
    get_token(&key)
}

/// Store a fresh Google access token for a given account.
pub fn set_google_access_token(email: &str, access_token: &str, expires_in_secs: f64) {
    let key = format!("google_at_{}", email.to_lowercase().trim());
    set_token(&key, access_token, expires_in_secs);
}

/// Retrieve a non-expired cached access token for a given account email.
pub fn get_google_access_token(email: &str) -> Option<String> {
    let key = format!("google_at_{}", email.to_lowercase().trim());
    get_token(&key)
}

/// Resolve access token for a specific Google account (or primary account if None).
/// Falls back to default "google" vault token if multi-email registry is unpopulated.
pub fn get_token_for_google_account(account_email: Option<&str>) -> Option<String> {
    let accounts = get_google_accounts();
    let target_email = match account_email {
        Some(e) if !e.trim().is_empty() => e.to_lowercase().trim().to_string(),
        _ => {
            if let Some(primary) = accounts.iter().find(|a| a.is_primary).or_else(|| accounts.first()) {
                primary.email.to_lowercase()
            } else {
                return get_token("google");
            }
        }
    };

    if let Some(cached) = get_google_access_token(&target_email) {
        return Some(cached);
    }

    // Single-account backward-compatibility fallback
    get_token("google")
}

/// Test-only: serialise every test that touches the process-global Google
/// account registry (the in-memory cache plus the mock keyring entry).
/// Without it two tests race — one saves an account while the other asserts
/// on the list length — and the same registry leaks a phone number into the
/// memory-profile test, which reads it through `refresh_user_profile`.
#[cfg(test)]
pub(crate) fn google_registry_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A panicking test must not poison the lock for the next one.
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
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

    #[test]
    fn google_account_registry_roundtrip_and_primary_switch() {
        // The registry is process-global (memory cache + mock keyring): hold
        // the shared guard so a parallel test cannot add or remove accounts
        // while this one asserts on the list.
        let _g = google_registry_test_guard();
        let prof1 = GoogleAccountProfile {
            email: "user1@gmail.com".into(),
            name: "User One".into(),
            picture: Some("http://pic1.png".into()),
            is_primary: false,
            added_at_ms: 1000,
            scopes: vec!["email".into()],
            phone: Some("+9111".into()),
            address: None,
        };
        let prof2 = GoogleAccountProfile {
            email: "user2@gmail.com".into(),
            name: "User Two".into(),
            picture: None,
            is_primary: false,
            added_at_ms: 2000,
            scopes: vec!["email".into()],
            phone: None,
            address: None,
        };

        save_google_account(prof1.clone(), Some("rt_user1"));
        save_google_account(prof2.clone(), Some("rt_user2"));

        let accounts = get_google_accounts();
        assert_eq!(accounts.len(), 2);
        // First account added should automatically be primary
        assert!(accounts.iter().any(|a| a.email == "user1@gmail.com" && a.is_primary));

        set_primary_google_account("user2@gmail.com").unwrap();
        let updated = get_google_accounts();
        assert!(updated.iter().any(|a| a.email == "user2@gmail.com" && a.is_primary));
        assert!(updated.iter().any(|a| a.email == "user1@gmail.com" && !a.is_primary));

        set_google_access_token("user2@gmail.com", "at_user2_valid", 3600.0);
        assert_eq!(get_google_access_token("user2@gmail.com").as_deref(), Some("at_user2_valid"));

        assert_eq!(get_google_refresh_token("user2@gmail.com").as_deref(), Some("rt_user2"));

        remove_google_account("user1@gmail.com").unwrap();
        remove_google_account("user2@gmail.com").unwrap();
        assert!(get_google_accounts().is_empty());
        // Regression: a removed account must not keep a usable refresh or
        // access token (previously both survived in memory and the keychain).
        for email in ["user1@gmail.com", "user2@gmail.com"] {
            assert!(get_google_refresh_token(email).is_none(), "refresh token for {email} survived removal");
            assert!(get_google_access_token(email).is_none(), "access token for {email} survived removal");
        }
    }

    #[test]
    fn vault_tests_never_touch_the_real_credential_store() {
        // The default credential builder must be the in-memory mock under
        // cfg(test): an entry written through the vault is not readable via
        // a second, independent handle (a real store would return it).
        let a = kr("nexus-test-isolation-probe").unwrap();
        a.set_password("probe").unwrap();
        let b = kr("nexus-test-isolation-probe").unwrap();
        assert!(b.get_password().is_err(), "vault tests are talking to the REAL credential store");
    }

    #[test]
    fn google_profile_phone_address_roundtrip_and_legacy_compat() {
        // Same registry as the test above — serialise, do not race.
        let _g = google_registry_test_guard();
        // New fields survive the keychain round-trip.
        let prof = GoogleAccountProfile {
            email: "user3@gmail.com".into(),
            name: "User Three".into(),
            picture: None,
            is_primary: false,
            added_at_ms: 3000,
            scopes: vec!["email".into()],
            phone: Some("+9133".into()),
            address: Some("Hyderabad, IN".into()),
        };
        save_google_account(prof.clone(), None);
        let loaded: Vec<GoogleAccountProfile> = get_google_accounts()
            .into_iter()
            .filter(|a| a.email == "user3@gmail.com")
            .collect();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].phone.as_deref(), Some("+9133"));
        assert_eq!(loaded[0].address.as_deref(), Some("Hyderabad, IN"));
        let _ = remove_google_account("user3@gmail.com");

        // Pre-field JSON (no phone/address keys) must still parse —
        // otherwise every existing account vanishes after upgrade.
        let legacy = serde_json::json!({
            "email": "old@gmail.com",
            "name": "Old",
            "picture": null,
            "is_primary": false,
            "added_at_ms": 1,
            "scopes": []
        });
        let parsed: GoogleAccountProfile = serde_json::from_value(legacy).expect("legacy profile parses");
        assert_eq!(parsed.phone, None);
        assert_eq!(parsed.address, None);
    }
}
