//! Feature 88 (C1) — canonical laptop identity: claim handshake + status.
//!
//! Identity model (docs/research/worker-identity/01-...architecture):
//! - The WORKER issues `profile_id` / `device_id` / device token.
//! - The client's locally-generated UUIDs are provisional hints only.
//! - `nexus-config.json` carries additive fields:
//!     "identity": "provisional" | "canonical"
//!     "profileId": "prof_..."      (canonical only)
//!     "legacyUserId": "user_..."   (pre-identity id, migration window)
//!     "lastKnownStatus": "pending" | "approved" | "suspended" | "revoked"
//!     "deviceName": "...", "os": "windows"
//! - The device token lives ONLY in the OS keyring (`auth_vault`,
//!   service "device_token") — never in plaintext config.
//! - Lifecycle: provisional → claim → pending → (admin) approved.
//!   Rejected states surface a distinct reason; local features keep working.
//! - A network failure NEVER regenerates identity — the provisional hint
//!   persists until the claim succeeds (no duplicate-profile flood).

use serde::{Deserialize, Serialize};
use tauri::Manager;

pub const DEVICE_TOKEN_SERVICE: &str = "device_token";
pub const IDENTITY_PROVISIONAL: &str = "provisional";
pub const IDENTITY_CANONICAL: &str = "canonical";

// ---- Config model ----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct IdentityConfig {
    #[serde(rename = "identity", default)]
    pub identity: String,
    #[serde(rename = "profileId", default)]
    pub profile_id: String,
    #[serde(rename = "legacyUserId", default)]
    pub legacy_user_id: String,
    #[serde(rename = "lastKnownStatus", default)]
    pub last_known_status: String,
    #[serde(rename = "deviceName", default)]
    pub device_name: String,
}

/// Read the identity block from nexus-config.json. Missing file or fields
/// → defaults (provisional, empty ids). Never errors on legacy configs.
pub fn read_identity_config(dir: &std::path::Path) -> IdentityConfig {
    let path = dir.join("nexus-config.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return IdentityConfig::default();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return IdentityConfig::default();
    };
    IdentityConfig {
        identity: json["identity"].as_str().unwrap_or(IDENTITY_PROVISIONAL).to_string(),
        profile_id: json["profileId"].as_str().unwrap_or("").to_string(),
        legacy_user_id: json["userId"].as_str().unwrap_or("").to_string(),
        last_known_status: json["lastKnownStatus"].as_str().unwrap_or("").to_string(),
        device_name: json["deviceName"].as_str().unwrap_or("").to_string(),
    }
}

/// Write canonical identity fields back into nexus-config.json,
/// preserving every existing key (serverUrl, userId, deviceId...).
pub fn write_identity_config(
    dir: &std::path::Path,
    patch: &IdentityConfig,
) -> Result<(), String> {
    let path = dir.join("nexus-config.json");
    let existing = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let mut obj = existing.as_object().cloned().unwrap_or_default();
    if !patch.identity.is_empty() { obj.insert("identity".into(), serde_json::Value::String(patch.identity.clone())); }
    if !patch.profile_id.is_empty() { obj.insert("profileId".into(), serde_json::Value::String(patch.profile_id.clone())); }
    if !patch.legacy_user_id.is_empty() { obj.insert("legacyUserId".into(), serde_json::Value::String(patch.legacy_user_id.clone())); }
    if !patch.last_known_status.is_empty() { obj.insert("lastKnownStatus".into(), serde_json::Value::String(patch.last_known_status.clone())); }
    if !patch.device_name.is_empty() { obj.insert("deviceName".into(), serde_json::Value::String(patch.device_name.clone())); }
    std::fs::write(&path, serde_json::Value::Object(obj).to_string()).map_err(|e| e.to_string())
}

// ---- Claim handshake ----

/// Build the claim request body. Pure.
pub fn build_claim_body(provisional_user_id: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "device_name": hostname(),
        "os": std::env::consts::OS,
        "app_version": env!("CARGO_PKG_VERSION"),
    });
    if !provisional_user_id.is_empty() {
        body["provisional_user_id"] = serde_json::Value::String(provisional_user_id.to_string());
    }
    body
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string())
}

/// Parse the claim response. Pure + unit-tested.
/// Returns (profile_id, device_id, one-time device_token).
pub fn parse_claim_response(data: &serde_json::Value) -> Result<(String, String, String), String> {
    let profile_id = data["profile_id"].as_str().unwrap_or("").to_string();
    let device_id = data["device_id"].as_str().unwrap_or("").to_string();
    let token = data["device_token"].as_str().unwrap_or("").to_string();
    if profile_id.is_empty() || device_id.is_empty() {
        return Err(format!(
            "claim response missing identity fields (profile_id={}, device_id={})",
            profile_id.len(),
            device_id.len()
        ));
    }
    Ok((profile_id, device_id, token))
}

/// POST /v1/profiles/claim and persist the canonical identity on success.
/// Called from `claim_profile` IPC and the pending-poll loop. The client's
/// provisional id is NEVER regenerated here — a failed claim is retried
/// later with the SAME hint.
pub async fn claim<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<IdentityStatus, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cfg = read_identity_config(&dir);
    let server_url = read_server_url(&dir)?;

    let body = build_claim_body(&cfg.legacy_user_id);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("claim http client: {e}"))?;

    let url = format!("{}/v1/profiles/claim", server_url.trim_end_matches('/'));
    tracing::info!("identity: claiming profile at {}", url);
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("claim request: {e}"))?;

    let status = resp.status();
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("claim json: {e}"))?;

    if !status.is_success() {
        let code = data["error"].as_str().unwrap_or("unknown");
        // rate_limited / pending_cap → stay provisional; retry later.
        tracing::warn!("identity: claim refused ({status}): {code}");
        return Ok(IdentityStatus {
            state: identity_state_for_config(&cfg).to_string(),
            profile_id: cfg.profile_id.clone(),
            device_name: cfg.device_name.clone(),
            reason: Some(code.to_string()),
        });
    }

    let (profile_id, _device_id, token) = parse_claim_response(&data)?;
    crate::auth_vault::set_api_key(DEVICE_TOKEN_SERVICE, &token);
    write_identity_config(&dir, &IdentityConfig {
        identity: IDENTITY_CANONICAL.to_string(),
        profile_id: profile_id.clone(),
        legacy_user_id: cfg.legacy_user_id.clone(),
        last_known_status: data["status"].as_str().unwrap_or("pending").to_string(),
        device_name: body["device_name"].as_str().unwrap_or("").to_string(),
    })?;
    tracing::info!("identity: claimed profile {} (status pending — awaiting admin approval)", profile_id);

    Ok(IdentityStatus {
        state: "pending".to_string(),
        profile_id,
        device_name: body["device_name"].as_str().unwrap_or("").to_string(),
        reason: None,
    })
}

fn read_server_url(dir: &std::path::Path) -> Result<String, String> {
    let path = dir.join("nexus-config.json");
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    let url = json["serverUrl"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        return Err("no serverUrl configured".into());
    }
    Ok(url)
}

// ---- Status ----

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityStatus {
    /// provisional | pending | approved | suspended | revoked | denied
    pub state: String,
    pub profile_id: String,
    pub device_name: String,
    pub reason: Option<String>,
}

fn identity_state_for_config(cfg: &IdentityConfig) -> &'static str {
    if cfg.identity == IDENTITY_CANONICAL && !cfg.profile_id.is_empty() {
        match cfg.last_known_status.as_str() {
            "approved" => "approved",
            "suspended" => "suspended",
            "revoked" => "revoked",
            _ => "pending",
        }
    } else {
        "provisional"
    }
}

/// Derive the current identity status purely from local state.
pub fn identity_status(dir: &std::path::Path) -> IdentityStatus {
    let cfg = read_identity_config(dir);
    IdentityStatus {
        state: identity_state_for_config(&cfg).to_string(),
        profile_id: cfg.profile_id.clone(),
        device_name: cfg.device_name.clone(),
        reason: None,
    }
}

/// GET /v1/profiles/me → refresh local lastKnownStatus. Returns the state.
pub async fn refresh_status<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<IdentityStatus, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cfg = read_identity_config(&dir);
    if cfg.identity != IDENTITY_CANONICAL || cfg.profile_id.is_empty() {
        return Ok(identity_status(&dir));
    }
    let token = crate::auth_vault::get_api_key(DEVICE_TOKEN_SERVICE).unwrap_or_default();
    if token.is_empty() {
        // Lost credential → cannot authenticate; surface provisional.
        return Ok(IdentityStatus {
            state: "provisional".to_string(),
            profile_id: cfg.profile_id.clone(),
            device_name: cfg.device_name.clone(),
            reason: Some("device_token_missing".to_string()),
        });
    }
    let server_url = read_server_url(&dir)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("me http client: {e}"))?;
    let url = format!(
        "{}/v1/profiles/me?profile_id={}&device_id={}",
        server_url.trim_end_matches('/'),
        cfg.profile_id,
        network_device_id(&dir)
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("me request: {e}"))?;
    let status_code = resp.status();
    let data: serde_json::Value = resp.json().await.map_err(|e| format!("me json: {e}"))?;
    if !status_code.is_success() {
        // 403 with {error: pending|suspended|revoked|...}
        let code = data["error"].as_str().unwrap_or("pending").to_string();
        write_identity_config(&dir, &IdentityConfig {
            last_known_status: code.clone(),
            ..IdentityConfig::default()
        })?;
        return Ok(IdentityStatus {
            state: code,
            profile_id: cfg.profile_id.clone(),
            device_name: cfg.device_name.clone(),
            reason: None,
        });
    }
    let known = data["status"].as_str().unwrap_or("approved").to_string();
    write_identity_config(&dir, &IdentityConfig {
        last_known_status: known.clone(),
        ..IdentityConfig::default()
    })?;
    Ok(IdentityStatus {
        state: known,
        profile_id: cfg.profile_id.clone(),
        device_name: cfg.device_name.clone(),
        reason: None,
    })
}

fn network_device_id(dir: &std::path::Path) -> String {
    dir.join("nexus-config.json")
        .to_str()
        .and_then(|_| std::fs::read_to_string(dir.join("nexus-config.json")).ok())
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|j| j["deviceId"].as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

// ---- Pending-approval poll loop ----

/// Self-revoke the current device (reinstall path). The device token is
/// invalidated server-side; local canonical identity is reset to
/// provisional so a later claim can register cleanly.
pub async fn disconnect<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let cfg = read_identity_config(&dir);
    if cfg.identity != IDENTITY_CANONICAL || cfg.profile_id.is_empty() {
        return Err("not connected".into());
    }
    let token = crate::auth_vault::get_api_key(DEVICE_TOKEN_SERVICE).unwrap_or_default();
    if token.is_empty() {
        return Err("device token missing".into());
    }
    let server_url = read_server_url(&dir)?;
    let device_id = network_device_id(&dir);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("disconnect http client: {e}"))?;
    let url = format!("{}/v1/devices/current", server_url.trim_end_matches('/'));
    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "profile_id": cfg.profile_id,
            "device_id": device_id,
        }))
        .send()
        .await
        .map_err(|e| format!("disconnect request: {e}"))?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("disconnect refused: {body}"));
    }
    let _ = crate::auth_vault::clear_api_key(DEVICE_TOKEN_SERVICE);
    reset_identity_config(&dir)?;
    tracing::info!("identity: device self-revoked — profile identity reset to provisional");
    Ok(())
}

/// Clear the canonical identity block entirely (used on self-revoke):
/// removes profileId/legacyUserId/lastKnownStatus/deviceName and stamps
/// identity=provisional. Preserves every other config key.
fn reset_identity_config(dir: &std::path::Path) -> Result<(), String> {
    let path = dir.join("nexus-config.json");
    let existing = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let mut obj = existing.as_object().cloned().unwrap_or_default();
    for key in ["profileId", "legacyUserId", "lastKnownStatus", "deviceName"] {
        obj.remove(key);
    }
    obj.insert("identity".into(), serde_json::Value::String(IDENTITY_PROVISIONAL.into()));
    std::fs::write(&path, serde_json::Value::Object(obj).to_string()).map_err(|e| e.to_string())
}

const POLL_INTERVAL_SECS: u64 = 600; // 10 min
const POLLS_PER_SESSION: u32 = 18;   // ~3h cap, then stop silently

/// Bounded poll loop: while the profile is pending, refresh every 10 min.
/// Stops once approved/denied (the transcript 403 path also triggers a
/// refresh on demand). Never nags — the frontend owns the UI.
pub fn spawn_pending_poll<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        for _ in 0..POLLS_PER_SESSION {
            tokio::time::sleep(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
            let dir = match app.path().app_data_dir() {
                Ok(d) => d,
                Err(_) => return,
            };
            let current = identity_status(&dir);
            if current.state != "pending" && current.state != "provisional" {
                return;
            }
            match refresh_status(&app).await {
                Ok(st) if st.state != "pending" => return,
                Ok(_) => continue,
                Err(e) => {
                    tracing::debug!("identity poll: {e}");
                    continue;
                }
            }
        }
    });
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    /// Process-unique temp dir (repo convention — no external test deps).
    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "nexus_id_test_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn test_missing_config_defaults_to_provisional() {
        let dir = tmp("missing");
        let cfg = read_identity_config(&dir);
        assert_eq!(cfg.identity, "");
        assert!(cfg.profile_id.is_empty());
        let st = identity_status(&dir);
        assert_eq!(st.state, "provisional");
    }

    #[test]
    fn test_legacy_config_reads_provisional_with_legacy_id() {
        let dir = tmp("legacy");
        let config = serde_json::json!({
            "serverUrl": "https://w.test",
            "userId": "user_old1",
            "deviceId": "device_old1",
        });
        std::fs::write(dir.join("nexus-config.json"), config.to_string()).unwrap();
        let cfg = read_identity_config(&dir);
        assert_eq!(cfg.legacy_user_id, "user_old1");
        assert_eq!(identity_status(&dir).state, "provisional");
    }

    #[test]
    fn test_canonical_pending_roundtrip() {
        let dir = tmp("canonical");
        let base = serde_json::json!({
            "serverUrl": "https://w.test",
            "userId": "user_old1",
            "deviceId": "device_old1",
            "extraField": "preserved",
        });
        std::fs::write(dir.join("nexus-config.json"), base.to_string()).unwrap();
        write_identity_config(&dir, &IdentityConfig {
            identity: IDENTITY_CANONICAL.to_string(),
            profile_id: "prof_Kx7".to_string(),
            legacy_user_id: "user_old1".to_string(),
            last_known_status: "pending".to_string(),
            device_name: "XPS".to_string(),
        })
        .unwrap();
        let cfg = read_identity_config(&dir);
        assert_eq!(cfg.profile_id, "prof_Kx7");
        assert_eq!(cfg.last_known_status, "pending");
        assert_eq!(cfg.device_name, "XPS");
        assert_eq!(identity_status(&dir).state, "pending");
        // serverUrl + extra fields preserved
        let raw = std::fs::read_to_string(dir.join("nexus-config.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(json["serverUrl"], "https://w.test");
        assert_eq!(json["extraField"], "preserved");
    }

    #[test]
    fn test_status_transitions() {
        let dir = tmp("transitions");
        for (known, expected) in [
            ("approved", "approved"),
            ("suspended", "suspended"),
            ("revoked", "revoked"),
            ("", "pending"),
            ("garbage", "pending"),
        ] {
            let base = serde_json::json!({
                "serverUrl": "https://w.test",
                "userId": "user_old1",
                "deviceId": "device_old1",
                "identity": IDENTITY_CANONICAL,
                "profileId": "prof_Kx7",
                "lastKnownStatus": known,
            });
            std::fs::write(dir.join("nexus-config.json"), base.to_string()).unwrap();
            assert_eq!(identity_status(&dir).state, expected, "known={known}");
        }
    }

    #[test]
    fn test_build_claim_body_includes_hostname_and_hint() {
        let body = build_claim_body("user_hint1");
        assert_eq!(body["provisional_user_id"], "user_hint1");
        assert!(body["device_name"].as_str().is_some());
        assert_eq!(body["os"], std::env::consts::OS);
        let empty = build_claim_body("");
        assert!(empty.get("provisional_user_id").is_none());
    }

    #[test]
    fn test_parse_claim_response() {
        let ok = serde_json::json!({
            "profile_id": "prof_A", "device_id": "dev_B", "device_token": "tok",
            "status": "pending", "protocol_version": "1",
        });
        let (p, d, t) = parse_claim_response(&ok).unwrap();
        assert_eq!((p.as_str(), d.as_str(), t.as_str()), ("prof_A", "dev_B", "tok"));

        let missing = serde_json::json!({ "profile_id": "prof_A" });
        assert!(parse_claim_response(&missing).is_err());

        let error = serde_json::json!({ "error": "rate_limited" });
        assert!(parse_claim_response(&error).is_err());
    }
}
