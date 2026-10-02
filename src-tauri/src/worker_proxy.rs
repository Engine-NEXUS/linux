//! Allowlisted Worker proxy for the WebView.
//!
//! Security context (2026-10-02): the setup wizard and settings sidebar call the
//! Worker directly with `fetch` — `/oauth/auth-url`, `/oauth/exchange`,
//! `/oauth/status`, `/oauth/disconnect`, `/apikeys/*`. Now that the Worker is
//! deny-by-default and authenticates by device credential, those calls would all
//! fail. The obvious shortcut — handing the bearer token to the renderer — is
//! the same mistake as the one being fixed: it puts a credential in the WebView.
//!
//! So the renderer asks Rust to make the call, and Rust attaches the credential
//! it holds in the OS keyring. The renderer never sees a token.
//!
//! ## Why the allowlist matters
//!
//! This is a proxy, so without a tight allowlist it would be a
//! confused-deputy primitive: the renderer could reach anything the device can
//! reach, including `/models/nlu/publish` (admin) or `/` (the intent endpoint).
//! Every (method, path) pair is therefore enumerated here. Anything not listed
//! is refused with a clear error rather than forwarded.

use serde_json::Value;
use tauri::Runtime;

/// (method, exact path) pairs the renderer may invoke.
///
/// Note `user_id` is deliberately absent: the Worker now derives the account
/// from the device credential, so the renderer does not send one and cannot ask
/// for another user's tokens.
const ALLOWED: &[(&str, &str)] = &[
    ("GET", "/oauth/auth-url"),
    ("POST", "/oauth/exchange"),
    ("GET", "/oauth/status"),
    ("DELETE", "/oauth/disconnect"),
    ("GET", "/oauth/github-token"),
    ("POST", "/apikeys/add"),
    ("DELETE", "/apikeys/remove"),
    ("GET", "/apikeys/list"),
];

fn is_allowed(method: &str, path: &str) -> bool {
    ALLOWED.iter().any(|(m, p)| *m == method && *p == path)
}

/// Perform an allowlisted Worker request on behalf of the renderer.
///
/// `query` is appended verbatim to the path, so callers are responsible for
/// percent-encoding. It cannot change the path: any `query` that tries to smuggle
/// a path separator is rejected below.
#[tauri::command]
pub async fn worker_request<R: Runtime>(
    app: tauri::AppHandle<R>,
    method: String,
    path: String,
    query: Option<String>,
    body: Option<Value>,
) -> Result<Value, String> {
    let method_upper = method.to_uppercase();

    if !is_allowed(&method_upper, &path) {
        return Err(format!(
            "worker_request: {method_upper} {path} is not on the allowlist"
        ));
    }

    // A query string must not be able to escape the path it is attached to.
    if let Some(q) = &query {
        if q.contains("..") || q.contains("//") || q.contains('#') {
            return Err("worker_request: suspicious query string".into());
        }
    }

    let (worker_url, _user_id, _device_id) = crate::network::get_session_info()
        .ok_or_else(|| "worker_request: no worker session configured".to_string())?;

    let mut url = format!("{}{}", worker_url.trim_end_matches('/'), path);
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        url.push('?');
        url.push_str(&q);
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("worker_request http client: {e}"))?;

    let req = client.request(
        reqwest::Method::from_bytes(method_upper.as_bytes())
            .map_err(|e| format!("bad method: {e}"))?,
        &url,
    );

    let req = match body {
        Some(b) if method_upper == "POST" => req.json(&b),
        _ => req,
    };

    let resp = crate::device_auth::apply_to(req)
        .send()
        .await
        .map_err(|e| format!("worker_request failed: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if status == reqwest::StatusCode::UNAUTHORIZED
        || status == reqwest::StatusCode::FORBIDDEN
    {
        return Err(
            "Device not authorized with the Worker. Restart NEXUS to re-register."
                .into(),
        );
    }

    if text.trim().is_empty() {
        return Ok(serde_json::json!({ "ok": status.is_success() }));
    }

    serde_json::from_str::<Value>(&text).map_err(|e| {
        format!(
            "worker_request: {status} returned non-JSON ({e}): {}",
            &text.chars().take(200).collect::<String>()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_covers_exactly_the_oauth_and_apikey_calls() {
        assert_eq!(ALLOWED.len(), 8);
        for (m, p) in [
            ("GET", "/oauth/auth-url"),
            ("POST", "/oauth/exchange"),
            ("GET", "/oauth/status"),
            ("DELETE", "/oauth/disconnect"),
            ("GET", "/oauth/github-token"),
            ("POST", "/apikeys/add"),
            ("DELETE", "/apikeys/remove"),
            ("GET", "/apikeys/list"),
        ] {
            assert!(is_allowed(m, p), "{m} {p} should be allowed");
        }
    }

    #[test]
    fn admin_publish_route_is_never_reachable() {
        assert!(!is_allowed("POST", "/models/nlu/publish"));
    }

    #[test]
    fn intent_endpoint_is_not_a_generic_proxy_target() {
        // `/` is the main intent POST. The renderer already has a first-class
        // Tauri command for that; it must not be reachable via this proxy too.
        assert!(!is_allowed("POST", "/"));
    }

    #[test]
    fn registration_and_bootstrap_stay_private_to_rust() {
        // These must not be renderer-reachable: registration mints credentials,
        // and `/health` + `/config/check` + model download are already public.
        assert!(!is_allowed("POST", "/api/register"));
        assert!(!is_allowed("GET", "/health"));
        assert!(!is_allowed("GET", "/config/check"));
        assert!(!is_allowed("GET", "/models/nlu/download"));
    }

    #[test]
    fn method_must_match_exactly() {
        assert!(!is_allowed("POST", "/oauth/status"));
        assert!(!is_allowed("GET", "/oauth/exchange"));
        assert!(!is_allowed("PATCH", "/oauth/status"));
    }

    #[test]
    fn trailing_slash_or_prefix_does_not_bypass_the_allowlist() {
        assert!(!is_allowed("GET", "/oauth/status/"));
        assert!(!is_allowed("GET", "/oauth/status/../publish"));
        assert!(!is_allowed("GET", "/oauth/statusx"));
    }
}