#![allow(dead_code)]

use super::types::GoogleError;

pub const GOOGLE_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/photoslibrary.readonly",
];

/// Environment override for the access token (CLI / CI / tests).
const ENV_ACCESS_TOKEN: &str = "GOOGLE_ACCESS_TOKEN";
/// Vault service that holds the signed-in Google access token.
const VAULT_SERVICE: &str = "google";

pub struct GoogleAuth;

impl GoogleAuth {
    /// Retrieve a valid OAuth2 access token for Google API calls.
    pub fn get_access_token() -> Result<String, GoogleError> {
        // 1. Check in-memory / OS credential vault
        // 2. Check environment override (useful in CLI / CI / tests)
        Self::access_token_from(
            crate::auth_vault::get_token(VAULT_SERVICE),
            std::env::var(ENV_ACCESS_TOKEN).ok(),
        )
    }

    /// Check if Google account is currently authenticated with a valid token.
    pub fn is_authenticated() -> bool {
        Self::get_access_token().is_ok()
    }

    /// Token resolution with both sources injected.
    ///
    /// The vault and the environment are process-global, so the unit tests
    /// hand this function their own values instead of mutating the real
    /// environment: two tests that set and removed `GOOGLE_ACCESS_TOKEN`
    /// in parallel used to race and fail `cargo test --lib` intermittently.
    fn access_token_from(
        vault_token: Option<String>,
        env_token: Option<String>,
    ) -> Result<String, GoogleError> {
        if let Some(token) = vault_token {
            if !token.trim().is_empty() {
                return Ok(token);
            }
        }

        if let Some(env_token) = env_token {
            if !env_token.trim().is_empty() {
                return Ok(env_token);
            }
        }

        Err(GoogleError::AuthRequired)
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // No test in this module touches the process-wide environment or the
    // vault any more: the token sources are injected, so these tests cannot
    // race a parallel test that reads the same global state.

    #[test]
    fn test_auth_unauthenticated_without_env() {
        // No vault token and no environment override: the caller has to be
        // told to sign in.
        let res = GoogleAuth::access_token_from(None, None);
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), GoogleError::AuthRequired);
    }

    #[test]
    fn test_auth_with_env_token() {
        let token = GoogleAuth::access_token_from(None, Some("mock_ya29_token_123".into()))
            .expect("the environment override must be accepted");
        assert_eq!(token, "mock_ya29_token_123");
        assert!(GoogleAuth::access_token_from(None, Some("mock_ya29_token_123".into())).is_ok());
    }

    #[test]
    fn test_vault_token_wins_over_env_token() {
        // The signed-in vault token is the live credential; the environment
        // override is only a fallback for CLI / CI.
        let token = GoogleAuth::access_token_from(
            Some("vault_token_456".into()),
            Some("mock_ya29_token_123".into()),
        )
        .expect("the vault token must be accepted");
        assert_eq!(token, "vault_token_456");
    }

    #[test]
    fn test_blank_tokens_are_not_credentials() {
        // A blank vault token must not shadow the environment override, and a
        // blank environment token is not a credential either.
        let token =
            GoogleAuth::access_token_from(Some("   ".into()), Some("mock_ya29_token_123".into()))
                .expect("the environment override must still be used");
        assert_eq!(token, "mock_ya29_token_123");
        let res = GoogleAuth::access_token_from(None, Some("   ".into()));
        assert_eq!(res.unwrap_err(), GoogleError::AuthRequired);
    }

    #[test]
    fn test_entry_point_resolves_the_real_sources() {
        // The public entry point must resolve exactly like the injected
        // helper, from the same two process-wide sources (vault first,
        // environment second). Read-only — it mutates neither, so it cannot
        // race a parallel test.
        let vault = crate::auth_vault::get_token(VAULT_SERVICE);
        let env = std::env::var(ENV_ACCESS_TOKEN).ok();
        assert_eq!(
            GoogleAuth::get_access_token(),
            GoogleAuth::access_token_from(vault, env)
        );
        assert_eq!(
            GoogleAuth::is_authenticated(),
            GoogleAuth::get_access_token().is_ok()
        );
    }
}
