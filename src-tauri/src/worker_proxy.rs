//! Allowlisted NEXUS Worker proxy for the WebView.
//!
//! OAuth setup used to call the Worker directly from the renderer. The
//! renderer must not receive the device bearer credential, so this module owns
//! the small, explicit set of Worker routes it may request and attaches that
//! credential natively.

use serde_json::Value;

/// Leave this `false` until the gated Worker deployment is live.
///
/// TODO(worker-device-gate-deploy): set this to `true` only after the gated
/// Worker is deployed and supported clients no longer need legacy `user_id`.
pub(crate) const REMOVE_LEGACY_USER_ID_AFTER_GATED_WORKER_DEPLOYMENT: bool = false;

/// Honest, user-facing explanation for a rejected device credential.
pub(crate) const DEVICE_NOT_AUTHORIZED_MESSAGE: &str =
    "This device isn't authorized with the NEXUS server";

/// The renderer's entire Worker surface. Exact matches prevent this proxy
/// from becoming a confused-deputy path to intent, admin, registration, or
/// token-minting endpoints.
const ALLOWED: &[(&str, &str)] = &[
    ("GET", "/oauth/auth-url"),
    ("POST", "/oauth/exchange"),
    ("GET", "/oauth/status"),
    ("DELETE", "/oauth/disconnect"),
    ("POST", "/apikeys/add"),
    ("DELETE", "/apikeys/remove"),
    ("GET", "/apikeys/list"),
];

pub(crate) fn is_allowed(method: &str, path: &str) -> bool {
    ALLOWED.iter().any(|(allowed_method, allowed_path)| {
        *allowed_method == method && *allowed_path == path
    })
}

/// Reject query shapes that could change the URL structure after the fixed
/// allowlisted path has been selected. The currently supported OAuth query
/// values are URL-safe identifiers and PKCE challenges, so percent escapes are
/// not needed here either.
pub(crate) fn is_safe_query(query: &str) -> bool {
    !query.contains("..")
        && !query.contains("//")
        && !query.contains('#')
        && !query.contains('\\')
        && !query.contains('%')
        && !query.starts_with('?')
        && !query.chars().any(char::is_control)
}

fn percent_encode_query_value(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

fn query_key_is_user_id(part: &str) -> bool {
    let key = part.split_once('=').map(|(key, _)| key).unwrap_or(part);
    key.eq_ignore_ascii_case("user_id")
}

/// Remove a renderer-provided user id. During the transition, the native
/// session identity is inserted first below; after deployment, no `user_id`
/// reaches the Worker at all.
fn sanitize_renderer_query(query: Option<&str>) -> Result<Option<String>, String> {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Ok(None);
    };
    if !is_safe_query(query) {
        return Err("worker_request: suspicious query string".into());
    }

    let retained: Vec<&str> = query
        .split('&')
        .filter(|part| !part.is_empty() && !query_key_is_user_id(part))
        .collect();
    if retained.is_empty() {
        Ok(None)
    } else {
        Ok(Some(retained.join("&")))
    }
}

/// Build a Worker URL without ever letting the renderer choose the legacy
/// account identity. Native-only token fetches reuse this helper as well, so
/// the transition flag has exactly one owner.
pub(crate) fn build_worker_url(
    worker_url: &str,
    path: &str,
    query: Option<&str>,
    legacy_user_id: Option<&str>,
) -> Result<String, String> {
    let query = sanitize_renderer_query(query)?;
    let mut parts = Vec::new();
    if !REMOVE_LEGACY_USER_ID_AFTER_GATED_WORKER_DEPLOYMENT {
        if let Some(user_id) = legacy_user_id {
            // Put the trusted native value first: deployed Workers use the
            // first query value, and the renderer cannot override it.
            parts.push(format!("user_id={}", percent_encode_query_value(user_id)));
        }
    }
    if let Some(query) = query {
        parts.push(query);
    }

    let mut url = format!("{}{}", worker_url.trim_end_matches('/'), path);
    if !parts.is_empty() {
        url.push('?');
        url.push_str(&parts.join("&"));
    }
    Ok(url)
}

/// Add the native identity in the same place as the deployed Worker expects
/// it today (JSON bodies for OAuth exchange, disconnect, and API keys). A
/// renderer body cannot choose a different account during the migration.
fn inject_legacy_user_id(
    body: Option<Value>,
    user_id: &str,
) -> Result<Option<Value>, String> {
    if REMOVE_LEGACY_USER_ID_AFTER_GATED_WORKER_DEPLOYMENT {
        if let Some(mut body) = body {
            let object = body
                .as_object_mut()
                .ok_or_else(|| "worker_request: request body must be a JSON object".to_string())?;
            object.remove("user_id");
            return Ok(Some(body));
        }
        return Ok(None);
    }

    let mut body = body.unwrap_or_else(|| Value::Object(serde_json::Map::new()));
    let object = body
        .as_object_mut()
        .ok_or_else(|| "worker_request: request body must be a JSON object".to_string())?;
    object.remove("user_id");
    object.insert("user_id".into(), Value::String(user_id.to_string()));
    Ok(Some(body))
}

fn apply_token(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) else {
        return request;
    };
    let Ok(value) = format!("Bearer {token}").parse::<reqwest::header::HeaderValue>() else {
        return request;
    };
    request.header(reqwest::header::AUTHORIZATION, value)
}

/// The only credential-attachment choke point for the Worker calls added by
/// this port. A missing or malformed keyring value leaves the request clean;
/// the Worker can then return its normal, honest authorization response.
pub(crate) fn apply_to(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let token = crate::auth_vault::get_api_key(crate::identity_state::DEVICE_TOKEN_SERVICE);
    apply_token(request, token.as_deref())
}

/// Perform one allowlisted Worker request for the renderer.
#[tauri::command]
pub async fn worker_request(
    method: String,
    path: String,
    query: Option<String>,
    body: Option<Value>,
) -> Result<Value, String> {
    let method = method.to_ascii_uppercase();
    if !is_allowed(&method, &path) {
        return Err(format!(
            "worker_request: {method} {path} is not on the allowlist"
        ));
    }

    let (worker_url, user_id, _) = crate::network::get_session_info()
        .ok_or_else(|| "worker_request: no worker session configured".to_string())?;

    let user_id_in_query = if method == "GET" {
        Some(user_id.as_str())
    } else {
        None
    };
    let url = build_worker_url(
        &worker_url,
        &path,
        query.as_deref(),
        user_id_in_query,
    )?;

    let body = match method.as_str() {
        "POST" | "DELETE" => inject_legacy_user_id(body, &user_id)?,
        _ if body.is_some() => {
            return Err(format!(
                "worker_request: {method} {path} does not accept a request body"
            ));
        }
        _ => None,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| format!("worker_request http client: {error}"))?;
    let http_method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|error| format!("worker_request: invalid method: {error}"))?;
    let request = client.request(http_method, &url);
    let request = match body {
        Some(body) => request.json(&body),
        None => request,
    };

    let response = apply_to(request)
        .send()
        .await
        .map_err(|error| format!("worker_request failed: {error}"))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(DEVICE_NOT_AUTHORIZED_MESSAGE.into());
    }
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("worker_request: NEXUS server returned {status}"));
    }
    if text.trim().is_empty() {
        return Ok(serde_json::json!({ "ok": true }));
    }
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "worker_request: {status} returned non-JSON ({error}): {}",
            text.chars().take(200).collect::<String>()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_covers_only_the_renderer_oauth_and_apikey_routes() {
        assert_eq!(ALLOWED.len(), 7);
        for (method, path) in [
            ("GET", "/oauth/auth-url"),
            ("POST", "/oauth/exchange"),
            ("GET", "/oauth/status"),
            ("DELETE", "/oauth/disconnect"),
            ("POST", "/apikeys/add"),
            ("DELETE", "/apikeys/remove"),
            ("GET", "/apikeys/list"),
        ] {
            assert!(is_allowed(method, path), "{method} {path} should be allowed");
        }

        for (method, path) in [
            ("POST", "/models/nlu/publish"),
            ("POST", "/"),
            ("GET", "/admin"),
            ("POST", "/api/register"),
            ("GET", "/health"),
            ("GET", "/config/check"),
            ("GET", "/models/nlu/download"),
            ("GET", "/oauth/github-token"),
            ("GET", "/oauth/google-token"),
            ("GET", "/oauth/swiggy-token"),
            ("POST", "/oauth/github-token"),
        ] {
            assert!(!is_allowed(method, path), "{method} {path} must stay private");
        }
    }

    #[test]
    fn admin_publish_route_is_never_reachable() {
        assert!(!is_allowed("POST", "/models/nlu/publish"));
    }

    #[test]
    fn intent_endpoint_is_not_a_generic_proxy_target() {
        assert!(!is_allowed("POST", "/"));
    }

    #[test]
    fn registration_and_bootstrap_stay_private_to_rust() {
        assert!(!is_allowed("POST", "/api/register"));
        assert!(!is_allowed("GET", "/health"));
        assert!(!is_allowed("GET", "/config/check"));
        assert!(!is_allowed("GET", "/models/nlu/download"));
    }

    #[test]
    fn token_routes_are_never_allowlisted() {
        assert!(!is_allowed("GET", "/oauth/github-token"));
        assert!(!is_allowed("GET", "/oauth/google-token"));
        assert!(!is_allowed("GET", "/oauth/swiggy-token"));
        assert!(!is_allowed("POST", "/oauth/github-token"));
    }

    #[test]
    fn method_must_match_exactly() {
        assert!(!is_allowed("POST", "/oauth/status"));
        assert!(!is_allowed("GET", "/oauth/exchange"));
        assert!(!is_allowed("PATCH", "/oauth/status"));
        assert!(!is_allowed("GET", "/apikeys/add"));
        assert!(!is_allowed("POST", "/apikeys/remove"));
    }

    #[test]
    fn trailing_slash_or_prefix_does_not_bypass_the_allowlist() {
        assert!(!is_allowed("GET", "/oauth/status/"));
        assert!(!is_allowed("GET", "/oauth/status/../publish"));
        assert!(!is_allowed("GET", "/oauth/statusx"));
        assert!(!is_allowed("POST", "/apikeys/add/"));
    }

    #[test]
    fn path_smuggling_query_shapes_are_refused() {
        assert!(is_safe_query("provider=github&code_challenge=abc_DEF-123"));
        for query in [
            "next=../models/nlu/publish",
            "next=//admin",
            "next=/oauth/status#fragment",
            "next=\\\\admin",
            "%75ser_id=other-user",
            "?user_id=other-user",
            "name=bad\nvalue",
        ] {
            assert!(!is_safe_query(query), "{query:?} must be rejected");
        }
    }

    #[test]
    fn native_legacy_identity_is_encoded_and_overrides_renderer_query() {
        let url = build_worker_url(
            "https://worker.example/",
            "/oauth/auth-url",
            Some("provider=github&user_id=renderer&code_challenge=abc_DEF-123"),
            Some("native user/1"),
        )
        .unwrap();
        assert_eq!(
            url,
            "https://worker.example/oauth/auth-url?user_id=native%20user%2F1&provider=github&code_challenge=abc_DEF-123"
        );
    }

    #[test]
    fn native_legacy_identity_overrides_renderer_body() {
        let body = inject_legacy_user_id(
            Some(serde_json::json!({ "provider": "github", "user_id": "renderer" })),
            "native",
        )
        .unwrap()
        .unwrap();
        assert_eq!(body["provider"].as_str(), Some("github"));
        assert_eq!(body["user_id"].as_str(), Some("native"));
        assert!(inject_legacy_user_id(Some(serde_json::json!(["not-an-object"])), "native").is_err());
    }

    #[test]
    fn header_choke_point_attaches_only_valid_bearers() {
        let client = reqwest::Client::new();
        let valid = apply_token(client.get("https://worker.example/"), Some("device-token"))
            .build()
            .unwrap();
        assert_eq!(
            valid
                .headers()
                .get(reqwest::header::AUTHORIZATION)
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer device-token"
        );

        for token in [None, Some("   "), Some("bad\r\nvalue")] {
            let request = apply_token(client.get("https://worker.example/"), token)
                .build()
                .unwrap();
            assert!(request.headers().get(reqwest::header::AUTHORIZATION).is_none());
        }
    }

    #[test]
    fn apply_to_choke_point_reads_from_vault() {
        crate::auth_vault::set_api_key(crate::identity_state::DEVICE_TOKEN_SERVICE, "mock-device-bearer");
        let client = reqwest::Client::new();
        let req = apply_to(client.get("https://worker.example/")).build().unwrap();
        assert_eq!(
            req.headers().get(reqwest::header::AUTHORIZATION).unwrap().to_str().unwrap(),
            "Bearer mock-device-bearer"
        );
        crate::auth_vault::clear_api_key(crate::identity_state::DEVICE_TOKEN_SERVICE);
        let req2 = apply_to(client.get("https://worker.example/")).build().unwrap();
        assert!(req2.headers().get(reqwest::header::AUTHORIZATION).is_none());
    }

    #[test]
    fn build_worker_url_without_user_id_when_none() {
        let url = build_worker_url(
            "https://worker.example/",
            "/oauth/auth-url",
            Some("provider=google"),
            None,
        )
        .unwrap();
        assert_eq!(url, "https://worker.example/oauth/auth-url?provider=google");
    }
}
