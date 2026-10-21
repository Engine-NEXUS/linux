//! Google Accounts Sub-Engine for Multi-Account Registry & OAuth Management.

use super::oauth;
use super::types::GoogleAccountProfile;

/// Retrieve all connected Google account profiles.
pub fn list_accounts() -> Vec<GoogleAccountProfile> {
    crate::auth_vault::get_google_accounts()
}

/// Start RFC 8252 direct native loopback OAuth flow to connect a new Google account.
pub async fn connect_account() -> Result<GoogleAccountProfile, String> {
    oauth::start_loopback_oauth_flow().await
}

/// Progressive re-consent for phone + address (sensitive scopes).
/// Merges into the existing profile — never a duplicate account.
pub async fn connect_extended() -> Result<GoogleAccountProfile, String> {
    oauth::connect_extended_profile().await
}

/// Disconnect and remove a connected Google account.
pub fn disconnect_account(email: &str) -> Result<(), String> {
    crate::auth_vault::remove_google_account(email)
}

/// Set the primary Google account by email.
pub fn set_primary_account(email: &str) -> Result<(), String> {
    crate::auth_vault::set_primary_google_account(email)
}

/// Save custom Developer Client ID and Client Secret in vault. Rejects a
/// malformed id up front (nothing is stored), so a typo is caught here and
/// not on Google's error page.
pub fn save_custom_credentials(client_id: &str, client_secret: Option<&str>) -> Result<(), String> {
    let id = client_id.trim();
    if !oauth::is_valid_client_id(id) {
        return Err("That Client ID does not look right. It should look like 123456789012-abcdef….apps.googleusercontent.com (copy it from Google Cloud Console → Credentials).".to_string());
    }
    let secret = client_secret.map(str::trim).filter(|s| !s.is_empty());
    crate::auth_vault::set_api_key("google_client_id", id);
    match secret {
        Some(sec) => crate::auth_vault::set_api_key("google_client_secret", sec),
        None => crate::auth_vault::clear_api_key("google_client_secret"),
    }
    Ok(())
}

/// Whether sign-in can start (never reveals the secret).
pub fn credentials_status() -> oauth::CredentialsStatus {
    oauth::credentials_status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_custom_credentials_storage() {
        // A malformed id is refused and stores nothing.
        assert!(save_custom_credentials("test_client_id_123", Some("x")).is_err());
        let good = "123456789012-abcdefghijklmnop1234567890abcdef.apps.googleusercontent.com";
        save_custom_credentials(good, Some("  test_secret_456 ")).unwrap();
        let (id, sec) = oauth::get_client_credentials();
        assert_eq!(id, good);
        assert_eq!(sec.as_deref(), Some("test_secret_456"));
        let st = credentials_status();
        assert!(st.configured && st.source == "saved" && st.message.is_none());
        // Saving without a secret leaves sign-in not ready, with a precise reason.
        save_custom_credentials(good, None).unwrap();
        let st = credentials_status();
        assert!(!st.configured && st.has_client_id && !st.has_secret);

        // Cleanup
        crate::auth_vault::clear_api_key("google_client_id");
        crate::auth_vault::clear_api_key("google_client_secret");
    }
}
