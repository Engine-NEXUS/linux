# 03 - Google sign-in and auth changes

Full change record: `docs/changes/113-google-signin-fix-vault-test-isolation-and-token-leak.md`.

## What was wrong
1. **"Access blocked ... OAuth client was not found, 401 invalid_client"**:
   `DEFAULT_CLIENT_ID` in `google/oauth.rs` was a placeholder (zeros). There is
   now **no built-in client**.
2. **`redirect_uri_mismatch`**: the app uses the loopback redirect
   `http://127.0.0.1:49152/callback`; Google only accepts it if it is listed on
   the OAuth client (Web clients need it added explicitly; Desktop clients accept loopback).
3. Tests wrote fake `user1@gmail.com` / `user2@gmail.com` accounts into the
   **real** Credential Manager.
4. Removing an account left its refresh token behind.
5. `photoslibrary.readonly` was still requested (Google removed it 2025-03-31).
6. Testing-mode refresh tokens expire after 7 days and the watcher failed silently.

## What changed
- **Credentials:** the user supplies their own Client ID + Secret (Command Hub
  -> Advanced -> Custom Developer OAuth Credentials, or env `GOOGLE_CLIENT_ID/SECRET`).
  Client ID shape is validated on save; sign-in fails in under a second with a
  clear message if they are missing (`credentials_status()`, IPC `google_credentials_status`).
- **Test isolation:** all keychain access in `auth_vault.rs` goes through `kr()`,
  which installs keyring's in-memory mock under `cfg(test)` (`NEXUS_REAL_KEYRING=1` opts out).
- **Token leak:** `remove_google_account` clears `google_rt_<email>` and
  `google_at_<email>`; the refresh token is stored once.
- **Scopes:** Photos scope removed.
- **7-day expiry:** inbox watcher sets `mail_needs_signin`, alerts once a day,
  Memory page shows "Paused - sign in again".
- **Pairing parser** (WhatsApp, same session): reads `structuredContent` / JSON in text blocks.

## How to set it up (once)
1. Google Cloud Console -> create a project -> enable **Gmail API** and **Google Calendar API**.
2. OAuth consent screen: add yourself under **Test users** (or publish the app).
3. Create an OAuth client: **Desktop app** (simplest). If you use a **Web**
   client, add `http://127.0.0.1:49152/callback` to Authorised redirect URIs.
4. Paste Client ID and Secret in the Command Hub; click **Add Google Account**.
5. Click through the "Google hasn't verified this app" screen once (Advanced -> Continue).

## About the "unsafe app" screen
Inherent while the app is unverified and uses sensitive/restricted Gmail scopes.
Easy mitigations: publish to **In production** (removes the 7-day token expiry;
the warning stays, with a user cap); alternatives that avoid Gmail scopes
entirely are IMAP with an app password or iCal feeds. Full removal needs Google
verification and, for restricted scopes, a security assessment. "Internal" needs Google Workspace.

## Secrets hygiene
- The Client Secret lives in the Windows keychain via the vault; never print it.
  (It was once printed in a tool output during debugging; rotating it is optional but reasonable.)
- Open item: API keys exist in both keychain and plaintext `settings.json`.

## Not verified live
A complete real sign-in and real Gmail/Calendar responses (only fixtures and a
mock server were exercised). Reconnect once after the next `nexus build`.
