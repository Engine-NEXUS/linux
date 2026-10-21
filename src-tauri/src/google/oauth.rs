//! Direct RFC 8252 Native Loopback OAuth for Google Accounts.
//!
//! Handles PKCE flow, loopback TCP listener on `127.0.0.1:49152`, token exchange,
//! profile retrieval, and automatic token refreshes via refresh_token.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::types::GoogleAccountProfile;

/// There is deliberately NO built-in Google client. The value that used to
/// live here was a placeholder (a run of zeros), so every sign-in ended on
/// Google's "Access blocked: The OAuth client was not found (401
/// invalid_client)" page. A shipped client would also need Google's app
/// verification for the Gmail scopes. Each user supplies their own OAuth
/// client (Desktop app) under Command Hub → Advanced.
pub const DEFAULT_CLIENT_ID: &str = "";

/// Shown (spoken-friendly, one line) whenever sign-in cannot start yet.
pub const NOT_CONFIGURED_MSG: &str = "Google sign-in isn't set up yet. In Google Cloud Console create an OAuth client of type Desktop app (enable the Gmail API and Google Calendar API), then paste its Client ID and Client Secret under Command Hub → Advanced → Custom Developer OAuth Credentials.";
pub const LOOPBACK_PORT: u16 = 49152;
pub const REDIRECT_URI: &str = "http://127.0.0.1:49152/callback";

/// Does `id` look like a real Google OAuth client id
/// (`<digits>-<hash>.apps.googleusercontent.com`)? Pure.
pub fn is_valid_client_id(id: &str) -> bool {
    let id = id.trim();
    let Some(prefix) = id.strip_suffix(".apps.googleusercontent.com") else { return false };
    let Some((project, hash)) = prefix.split_once('-') else { return false };
    let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    project.len() >= 6
        && project.chars().all(|c| c.is_ascii_digit())
        && hash.len() >= 8
        && word(hash)
        // The old placeholder was the right shape but a run of zeros.
        && !hash.ends_with("00000000")
}

/// What sign-in would use, without revealing the secret.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct CredentialsStatus {
    /// A usable client id AND secret are present.
    pub configured: bool,
    pub has_client_id: bool,
    pub has_secret: bool,
    /// "saved" (Command Hub), "env" (GOOGLE_CLIENT_ID) or "none".
    pub source: String,
    pub message: Option<String>,
}

fn status_from(id: Option<(String, &'static str)>, secret: bool) -> CredentialsStatus {
    let (has_client_id, source) = match &id {
        Some((i, src)) if is_valid_client_id(i) => (true, (*src).to_string()),
        Some((_, src)) => (false, format!("{src} (invalid)")),
        None => (false, "none".to_string()),
    };
    let configured = has_client_id && secret;
    let message = if configured {
        None
    } else if has_client_id {
        Some("Add the Client Secret too: Google needs it to finish sign-in for a Desktop client.".to_string())
    } else if matches!(&id, Some(_)) {
        Some("That Client ID does not look right. It should end in .apps.googleusercontent.com.".to_string())
    } else {
        Some(NOT_CONFIGURED_MSG.to_string())
    };
    CredentialsStatus { configured, has_client_id, has_secret: secret, source, message }
}

pub fn credentials_status() -> CredentialsStatus {
    let saved = crate::auth_vault::get_api_key("google_client_id").filter(|s| !s.trim().is_empty());
    let env = std::env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.trim().is_empty());
    let id = match (saved, env) {
        (Some(s), _) => Some((s, "saved")),
        (None, Some(e)) => Some((e, "env")),
        (None, None) => None,
    };
    let secret = crate::auth_vault::get_api_key("google_client_secret").map(|s| !s.trim().is_empty()).unwrap_or(false)
        || std::env::var("GOOGLE_CLIENT_SECRET").map(|s| !s.trim().is_empty()).unwrap_or(false);
    status_from(id, secret)
}

/// Retrieve Developer Client ID and Client Secret (custom or fallback).
pub fn get_client_credentials() -> (String, Option<String>) {
    let client_id = crate::auth_vault::get_api_key("google_client_id")
        .or_else(|| std::env::var("GOOGLE_CLIENT_ID").ok())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string());

    let client_secret = crate::auth_vault::get_api_key("google_client_secret")
        .or_else(|| std::env::var("GOOGLE_CLIENT_SECRET").ok());

    (client_id, client_secret)
}

fn get_random_bytes(dest: &mut [u8]) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut rng = seed;
    for (i, byte) in dest.iter_mut().enumerate() {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = ((rng >> 32) ^ (i as u128)) as u8;
    }
}

/// Generate PKCE verifier (43 chars) and challenge (S256).
pub fn generate_pkce() -> (String, String) {
    let mut random_bytes = [0u8; 32];
    get_random_bytes(&mut random_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

/// Base scopes: identity + Gmail + Calendar + Photos. Phone/address are
/// deliberately EXCLUDED — sensitive scopes requested only via the
/// optional progressive flow below, so first-run consent stays minimal.
fn base_scope_list(extended: bool) -> Vec<&'static str> {
    let mut scopes = vec![
        "https://www.googleapis.com/auth/gmail.readonly",
        "https://www.googleapis.com/auth/gmail.send",
        "https://www.googleapis.com/auth/gmail.modify",
        "https://www.googleapis.com/auth/calendar",
        // `photoslibrary.readonly` is NOT requested: Google removed it from the
        // Photos Library API on 2025-03-31 (calls now return 403), and asking
        // for a retired scope only bloats the consent screen.
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/userinfo.email",
    ];
    if extended {
        scopes.push("https://www.googleapis.com/auth/user.phonenumbers.read");
        scopes.push("https://www.googleapis.com/auth/user.addresses.read");
    }
    scopes
}

fn base_scopes(extended: bool) -> String {
    base_scope_list(extended).join(" ")
}

/// Start direct native OAuth flow via Tokio TCP listener on 127.0.0.1:49152.
pub async fn start_loopback_oauth_flow() -> Result<GoogleAccountProfile, String> {
    start_loopback_oauth_flow_with(false).await
}

/// Progressive re-consent (settings "Add phone & address"): full loopback
/// flow with the 2 sensitive scopes, then MERGES phone/address into the
/// existing profile (primary/added_at/tokens preserved). Never blocks
/// install — settings-only, user-initiated, absence is not an error.
pub async fn connect_extended_profile() -> Result<GoogleAccountProfile, String> {
    start_loopback_oauth_flow_with(true).await
}

async fn start_loopback_oauth_flow_with(extended: bool) -> Result<GoogleAccountProfile, String> {
    // Fail HERE, in words, instead of opening a browser tab onto Google's
    // "OAuth client was not found" error page and then waiting 2 minutes.
    let status = credentials_status();
    if !status.configured {
        return Err(status.message.unwrap_or_else(|| NOT_CONFIGURED_MSG.to_string()));
    }
    let (client_id, client_secret) = get_client_credentials();
    let (verifier, challenge) = generate_pkce();

    let scopes = base_scopes(extended);


    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
        urlencoding::encode(&client_id),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&scopes),
        urlencoding::encode(&challenge)
    );

    // Bind loopback TCP listener
    let addr = format!("127.0.0.1:{LOOPBACK_PORT}");
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("Failed to bind OAuth loopback listener on {addr}: {e}"))?;

    tracing::info!("OAuth loopback listener active on {addr}, opening browser...");
    let _ = open::that(&auth_url);

    // Wait up to 120s for OAuth authorization callback
    let mut code: Option<String> = None;
    let mut callback_error: Option<String> = None;
    let timeout = tokio::time::sleep(Duration::from_secs(120));
    tokio::pin!(timeout);

    tokio::select! {
        res = listener.accept() => {
            match res {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 2048];
                    let read_bytes = stream.read(&mut buf).await.unwrap_or(0);
                    let req_str = String::from_utf8_lossy(&buf[..read_bytes]);

                    // Parse request line e.g. GET /callback?code=4/0A... HTTP/1.1
                    if let Some(first_line) = req_str.lines().next() {
                        if let Some(path) = first_line.split_whitespace().nth(1) {
                            if let Some(query) = path.split_once('?') {
                                for pair in query.1.split('&') {
                                    if let Some((k, v)) = pair.split_once('=') {
                                        let v = urlencoding::decode(v).unwrap_or_default().to_string();
                                        if k == "code" {
                                            code = Some(v);
                                        } else if k == "error" {
                                            callback_error = Some(v);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Respond to browser with a clean success page
                    let html = "<html><body style='font-family:sans-serif;text-align:center;padding-top:50px;'><h2>Authentication successful!</h2><p>You can close this tab and return to NEXUS.</p></body></html>";
                    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", html.len(), html);
                    let _ = stream.write_all(resp.as_bytes()).await;
                }
                Err(e) => return Err(format!("TCP accept error: {e}")),
            }
        }
        _ = &mut timeout => {
            return Err("OAuth login timed out after 120 seconds.".to_string());
        }
    }

    if let Some(err) = callback_error {
        return Err(match err.as_str() {
            "access_denied" => "Google sign-in was cancelled or this Google account is not allowed to use the app yet. While the OAuth consent screen is in Testing mode, add the account under Test users.".to_string(),
            other => format!("Google sign-in failed: {other}"),
        });
    }
    let auth_code = code.ok_or_else(|| "No authorization code received in callback".to_string())?;

    // Exchange auth code for tokens
    exchange_code_and_save(&client_id, client_secret.as_deref(), &auth_code, &verifier, extended).await
}

/// Fetch phone + address via People API. Both None on ANY failure
/// (scope denied, empty profile, network) — absence is normal, never an
/// error. Only called when the extended scopes were granted.
async fn fetch_extended_contact(access_token: &str) -> (Option<String>, Option<String>) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let resp = match client
        .get("https://people.googleapis.com/v1/people/me?personFields=phoneNumbers,addresses")
        .bearer_auth(access_token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return (None, None),
    };
    if !resp.status().is_success() {
        return (None, None);
    }
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let phone = v
        .get("phoneNumbers")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("value"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let address = v
        .get("addresses")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|p| p.get("formattedValue"))
        .and_then(|x| x.as_str())
        .map(str::to_string);
    (phone, address)
}

/// Exchange OAuth authorization code for tokens and store account profile.
async fn exchange_code_and_save(
    client_id: &str,
    client_secret: Option<&str>,
    code: &str,
    verifier: &str,
    extended: bool,
) -> Result<GoogleAccountProfile, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let mut params = std::collections::HashMap::new();
    params.insert("client_id", client_id);
    if let Some(sec) = client_secret {
        params.insert("client_secret", sec);
    }
    params.insert("code", code);
    params.insert("code_verifier", verifier);
    params.insert("grant_type", "authorization_code");
    params.insert("redirect_uri", REDIRECT_URI);

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token request failed: {e}"))?;

    if !resp.status().is_success() {
        let err_body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {err_body}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid token response JSON: {e}"))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| "Missing access_token in token response".to_string())?;

    let refresh_token = json["refresh_token"].as_str();
    let expires_in = json["expires_in"].as_f64().unwrap_or(3600.0);

    // Fetch user profile info
    let profile_resp = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Profile request failed: {e}"))?;

    if !profile_resp.status().is_success() {
        return Err(format!("Profile request error {}", profile_resp.status()));
    }

    let userinfo: serde_json::Value = profile_resp
        .json()
        .await
        .map_err(|e| format!("Invalid profile JSON: {e}"))?;

    let email = userinfo["email"]
        .as_str()
        .ok_or_else(|| "Missing email in userinfo response".to_string())?
        .to_string();

    let name = userinfo["name"]
        .as_str()
        .unwrap_or(&email)
        .to_string();

    let picture = userinfo["picture"].as_str().map(str::to_string);

    // Progressive contact enrichment: only when the extended scopes were
    // granted in this flow. Absence (denied/empty) is normal, not an error.
    let (phone, address) = if extended {
        fetch_extended_contact(access_token).await
    } else {
        (None, None)
    };

    // Re-consent merge: an existing profile keeps its primary flag,
    // original added_at, and any previously stored phone/address the new
    // fetch didn't return (revoked scope must not erase old data).
    let existing = crate::auth_vault::get_google_accounts()
        .into_iter()
        .find(|a| a.email.to_lowercase().trim() == email.to_lowercase().trim());
    let mut scopes = vec![
        "gmail.readonly".into(),
        "gmail.send".into(),
        "gmail.modify".into(),
        "calendar".into(),
        "photos".into(),
    ];
    if extended {
        scopes.push("phone".into());
        scopes.push("address".into());
    }
    let profile = GoogleAccountProfile {
        email: email.clone(),
        name,
        picture,
        is_primary: existing.as_ref().map(|e| e.is_primary).unwrap_or(false),
        added_at_ms: existing
            .as_ref()
            .map(|e| e.added_at_ms)
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis() as u64),
        scopes,
        phone: phone.or_else(|| existing.as_ref().and_then(|e| e.phone.clone())),
        address: address.or_else(|| existing.as_ref().and_then(|e| e.address.clone())),
    };

    crate::auth_vault::save_google_account(profile.clone(), refresh_token);
    crate::auth_vault::set_google_access_token(&email, access_token, expires_in);

    // Also update legacy single-account vault token for seamless backward-compatibility
    crate::auth_vault::set_token("google", access_token, expires_in);

    tracing::info!("Google account successfully authenticated & saved: {email}");
    Ok(profile)
}

/// Refresh an access token for a specific email using its stored refresh_token.
pub async fn refresh_access_token_for(email: &str) -> Result<String, String> {
    let refresh_token = crate::auth_vault::get_google_refresh_token(email)
        .ok_or_else(|| format!("No refresh token stored for {email}"))?;

    let (client_id, client_secret) = get_client_credentials();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let mut params = std::collections::HashMap::new();
    params.insert("client_id", client_id.as_str());
    if let Some(ref sec) = client_secret {
        params.insert("client_secret", sec.as_str());
    }
    params.insert("refresh_token", refresh_token.as_str());
    params.insert("grant_type", "refresh_token");

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {e}"))?;

    if !resp.status().is_success() {
        let err_body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed for {email}: {err_body}"));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Invalid token refresh response JSON: {e}"))?;

    let access_token = json["access_token"]
        .as_str()
        .ok_or_else(|| "Missing access_token in refresh response".to_string())?;

    let expires_in = json["expires_in"].as_f64().unwrap_or(3600.0);

    crate::auth_vault::set_google_access_token(email, access_token, expires_in);

    // If this is the primary account, update legacy single-token vault as well
    let accounts = crate::auth_vault::get_google_accounts();
    if accounts.iter().any(|a| a.email.eq_ignore_ascii_case(email) && a.is_primary) {
        crate::auth_vault::set_token("google", access_token, expires_in);
    }

    Ok(access_token.to_string())
}

// ─── Simple Helper Module URL Encoding ───────────────────────────────────────
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut encoded = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(b as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", b));
                }
            }
        }
        encoded
    }

    pub fn decode(s: &str) -> Option<String> {
        let mut bytes = Vec::new();
        let mut chars = s.bytes();
        while let Some(b) = chars.next() {
            if b == b'%' {
                let h1 = chars.next()?;
                let h2 = chars.next()?;
                let hex_arr = [h1, h2];
                let hex_str = std::str::from_utf8(&hex_arr).ok()?;
                let byte = u8::from_str_radix(hex_str, 16).ok()?;
                bytes.push(byte);
            } else if b == b'+' {
                bytes.push(b' ');
            } else {
                bytes.push(b);
            }
        }
        String::from_utf8(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = generate_pkce();
        assert_eq!(verifier.len(), 43);
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn client_id_shape_and_the_old_placeholder() {
        assert!(is_valid_client_id("1065171708892-v7k7b2f6k526b74499n8k1a2b3c4d5e6.apps.googleusercontent.com"));
        assert!(is_valid_client_id("  123456789012-abcdefghijklmnop1234567890abcdef.apps.googleusercontent.com "));
        // The placeholder that shipped before and produced "OAuth client was not found".
        assert!(!is_valid_client_id("1065171708892-v7k7b2f6k526b74499n8k00000000000.apps.googleusercontent.com"));
        for bad in ["", "test_client_id_123", "abc.apps.googleusercontent.com", "x-y.apps.googleusercontent.com",
                    "123456789012-short.apps.googleusercontent.com", "123456789012-abcdefgh12345678.example.com"] {
            assert!(!is_valid_client_id(bad), "{bad}");
        }
        assert_eq!(DEFAULT_CLIENT_ID, "", "no fake client is shipped");
    }

    #[test]
    fn credentials_status_explains_what_is_missing() {
        let good = "123456789012-abcdefghijklmnop1234567890abcdef.apps.googleusercontent.com".to_string();
        let none = status_from(None, false);
        assert!(!none.configured && none.message.as_deref() == Some(NOT_CONFIGURED_MSG) && none.source == "none");
        let id_only = status_from(Some((good.clone(), "saved")), false);
        assert!(!id_only.configured && id_only.has_client_id && id_only.message.as_deref().unwrap().contains("Client Secret"));
        let ok = status_from(Some((good, "saved")), true);
        assert!(ok.configured && ok.message.is_none() && ok.source == "saved");
        let bad = status_from(Some(("nope".into(), "saved")), true);
        assert!(!bad.configured && bad.message.as_deref().unwrap().contains("does not look right") && bad.source.contains("invalid"));
        // A secret alone is not enough.
        assert!(!status_from(None, true).configured);
    }

    #[test]
    fn test_urlencoding() {
        let raw = "user@gmail.com & scopes";
        let enc = urlencoding::encode(raw);
        assert!(enc.contains("%20"));
        assert!(enc.contains("%40"));
        assert_eq!(urlencoding::decode(&enc).as_deref(), Some(raw));
    }

    #[test]
    fn no_retired_or_unneeded_scopes_are_requested() {
        for extended in [false, true] {
            let scopes = base_scope_list(extended);
            assert!(!scopes.iter().any(|s| s.contains("photoslibrary")), "retired Photos scope requested");
            // What P5 needs is present: read mail + calendar.
            assert!(scopes.iter().any(|s| s.ends_with("gmail.readonly")));
            assert!(scopes.iter().any(|s| s.ends_with("/auth/calendar")));
        }
    }

    #[test]
    fn test_scope_tiers_base_minimal_extended_opt_in() {
        // Base consent stays minimal (no sensitive scopes at install).
        let base = base_scope_list(false);
        assert!(base.iter().all(|s| !s.contains("phonenumbers") && !s.contains("addresses")));
        assert!(base.iter().any(|s| s.contains("userinfo.email")));
        // Extended flow adds exactly the 2 sensitive scopes.
        let ext = base_scope_list(true);
        assert!(ext.iter().any(|s| s.contains("user.phonenumbers.read")));
        assert!(ext.iter().any(|s| s.contains("user.addresses.read")));
        assert_eq!(ext.len(), base.len() + 2);
    }
}
