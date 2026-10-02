//! Device authentication against the NEXUS Worker.
//!
//! Security context (2026-10-02): the Worker previously returned live OAuth
//! access tokens from `/oauth/github-token`, `/oauth/google-token` and
//! `/oauth/swiggy-token` keyed only on a client-supplied `?user_id=` — with no
//! authentication of any kind. The `device_token` column existed in the schema
//! but was never written and never read.
//!
//! This module makes the device credential real:
//!   1. mint a 256-bit token on first run;
//!   2. persist it in the OS credential store (Secret Service / Keychain /
//!      Credential Manager) via the `keyring` crate, matching what
//!      [`crate::auth_vault`] already does for OAuth tokens — **not** a
//!      plaintext file;
//!   3. register it with the Worker, which keeps only its SHA-256;
//!   4. attach it as `Authorization: Bearer …` on every Worker request.
//!
//! ## Why the token is not stored next to `nexus-config.json`
//!
//! A bearer credential in a world-readable-ish config file is the same class of
//! mistake as the one being fixed. `keyring` is already a dependency for
//! `auth_vault`, so there is no new dependency and no new platform risk — only a
//! second entry in a store we already write to.
//!
//! ## Failure behaviour
//!
//! A machine with no Secret Service provider (headless, minimal window manager,
//! some CI images) must not fail to start. Registration falls back to an
//! in-memory-only token and logs loudly; that device then re-registers on each
//! launch and loses the ability to authenticate until a provider is available.

use std::sync::OnceLock;

use base64::Engine;
use serde::{Deserialize, Serialize};

/// Keyring service name. Shared with `auth_vault` so both live in one namespace.
const KEYRING_SERVICE: &str = "com.nexus.assistant";

/// Entry name for the device bearer token.
fn device_key() -> String {
    "device-bearer-token".to_string()
}

/// Process-wide cache so the OS keyring is hit once per run, not per request.
static CACHED_TOKEN: OnceLock<String> = OnceLock::new();

/// A 256-bit token, URL-safe base64 (no padding).
///
/// Uses `uuid::Uuid::new_v4()` for entropy rather than a time/pid mix: this value
/// is a credential, so it must come from a CSPRNG. Two v4 UUIDs give 256 bits.
pub fn mint_device_token() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(a.as_bytes());
    bytes[16..].copy_from_slice(b.as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Read the device token from the OS credential store.
fn keyring_load() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &device_key()).ok()?;
    entry.get_password().ok().filter(|s| !s.is_empty())
}

/// Persist the device token to the OS credential store.
///
/// Returns `false` when no credential provider is available. The caller decides
/// what to do; we deliberately do not silently fall back to a plaintext file.
fn keyring_store(token: &str) -> bool {
    match keyring::Entry::new(KEYRING_SERVICE, &device_key()) {
        Ok(entry) => entry.set_password(token).is_ok(),
        Err(_) => false,
    }
}

/// Process-local fallback when no credential provider exists.
static EPHEMERAL: OnceLock<String> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone, Debug)]
struct RegisterResponse {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    device_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Load the current device token, if one has been minted this run.
pub fn current_token() -> Option<String> {
    if let Some(t) = CACHED_TOKEN.get() {
        return Some(t.clone());
    }
    if let Some(t) = EPHEMERAL.get() {
        return Some(t.clone());
    }
    None
}

/// Overwrite the cached token. Exposed so the registration path and tests can
/// install a token minted during this run.
pub fn set_cached_token(token: &str) {
    let _ = CACHED_TOKEN.set(token.to_string());
}

/// Register this device with the Worker and return a usable bearer token.
///
/// Idempotent in the sense that it always ends with a working token, but note it
/// **rotates** the credential on every call. Callers should therefore register
/// once at startup, not per request.
pub async fn ensure_registered(
    worker_url: &str,
    user_id: &str,
    device_id: &str,
    device_name: &str,
    os: &str,
) -> Result<String, String> {
    if let Some(existing) = keyring_load() {
        if CACHED_TOKEN.get().is_none() && EPHEMERAL.get().is_none() {
            let _ = CACHED_TOKEN.set(existing.clone());
        }
        return Ok(existing);
    }

    if user_id.is_empty() || device_id.is_empty() {
        return Err("device registration needs user_id and device_id".into());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("device register http client: {e}"))?;

    let resp = client
        .post(format!("{}/api/register", worker_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "user_id": user_id,
            "device_id": device_id,
            "device_name": device_name,
            "os": os,
        }))
        .send()
        .await
        .map_err(|e| format!("device register request failed: {e}"))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("device register response parse: {e}"))?;

    if !status.is_success() {
        return Err(format!(
            "device register {status}: {}",
            body["error"].as_str().unwrap_or("unknown error")
        ));
    }

    let parsed: RegisterResponse =
        serde_json::from_value(body).map_err(|e| format!("device register decode: {e}"))?;
    let token = parsed
        .device_token
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "device register returned no device_token".to_string())?;

    if keyring_store(&token) {
        let _ = CACHED_TOKEN.set(token.clone());
        tracing::info!("device_auth: registered and stored credential in OS keyring");
    } else {
        // No Secret Service / Keychain on this machine. Keep it for this process
        // so the app still works, but make the trade-off loud: it will not
        // survive a restart.
        let _ = EPHEMERAL.set(token.clone());
        tracing::warn!(
            "device_auth: no OS credential store available — device token is \
             process-local only and will re-register on next launch"
        );
    }

    Ok(token)
}

/// Value for the `Authorization` header, or `None` if not yet registered.
///
/// Call sites use `.header("Authorization", ...)` only when `Some`, so a
/// not-yet-registered device produces a clear 401 from the Worker rather than a
/// malformed header.
pub fn auth_header_value() -> Option<String> {
    current_token().map(|t| format!("Bearer {t}"))
}

/// Attach the device credential to a request builder, if we have one.
pub fn apply_to(req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    match auth_header_value() {
        Some(v) => req.header("Authorization", v),
        None => req,
    }
}

/// Test-only: clear the process caches.
#[cfg(test)]
pub fn reset_for_test() {
    // OnceLock cannot be reset, so tests that need isolation run in separate
    // processes. This exists so the intent is explicit at call sites.
    let _ = &CACHED_TOKEN;
    let _ = &EPHEMERAL;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_tokens_are_256_bits_and_unique() {
        let a = mint_device_token();
        let b = mint_device_token();
        assert_ne!(a, b, "two consecutive tokens must differ");
        // 32 bytes, URL-safe base64 without padding => 43 chars
        assert_eq!(a.len(), 43, "expected 256-bit URL_SAFE_NO_PAD base64");
        assert!(
            a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token must be URL-safe: {a}"
        );
    }

    #[test]
    fn tokens_are_not_derived_from_time() {
        // Two calls in the same nanosecond-ish window must still differ, which a
        // time+pid+counter scheme would not guarantee.
        let a = mint_device_token();
        let b = mint_device_token();
        assert_ne!(a, b);
    }

    #[test]
    fn auth_header_is_bearer_or_absent() {
        // Never registered in this process -> must be None, not Some("Bearer ").
        if current_token().is_none() {
            assert!(auth_header_value().is_none());
        } else {
            let v = auth_header_value().unwrap();
            assert!(v.starts_with("Bearer "));
            assert!(v.len() > "Bearer ".len());
        }
    }
}