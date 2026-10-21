# 113 — Google sign-in fix, keychain test isolation, token-removal leak, retired Photos scope

Triggered by the user's screenshot: *"Access blocked: Authorisation error — The OAuth client was not found. Error 401: invalid_client"* when adding a Gmail account, and the request to cross-check everything added so far against the real machine before P6.

## Why Gmail could not be added
`google/oauth.rs` shipped `DEFAULT_CLIENT_ID = "1065171708892-v7k7b2f6k526b74499n8k00000000000.apps.googleusercontent.com"` — a **placeholder** (a run of zeros), not a real Google client. With no custom credentials saved and no `GOOGLE_CLIENT_ID` in the environment (both checked on this machine), every "Add Google Account" opened Google's own error page and then waited two minutes. The UI also called custom credentials "optional" and the secret "optional for PKCE" — wrong: Google needs a client of your own, and for a Desktop client the secret is required to finish sign-in. **Only the user can create that client** (it lives in their Google Cloud project); the app cannot.

### Fix
- No fake default: `DEFAULT_CLIENT_ID` is empty. `credentials_status()` (`configured`, `has_client_id`, `has_secret`, `source`, a precise `message`; never the secret) + IPC `google_credentials_status`.
- Sign-in **fails in 0 s** with the setup instructions instead of opening a browser onto Google's error page; the callback now reports Google's `error=` (e.g. `access_denied` → "add the account under Test users").
- Saving credentials validates the Client ID shape (`<digits>-<hash>.apps.googleusercontent.com`, not the old placeholder) and stores nothing if it is wrong.
- Command Hub → Advanced: status banner, the drawer opens by itself when not configured, honest copy (Desktop app, Gmail API + Calendar API, Test users, secret required), "Saved. Now click Add Google Account."

## Other defects found while cross-checking (all fixed, all test-covered)
1. **Unit tests wrote into the real Windows Credential Manager.** Since change 107 made the keychain real, `auth_vault` tests saved fake accounts (`user1@gmail.com`, `user2@gmail.com`) and overwrote the Google account registry in the user's real store (three fixture credentials and an empty registry were found there). After the user connected Gmail, any `cargo test` would have wiped their real account list. All 14 keychain call sites now go through one `kr()` wrapper that installs the in-memory mock under `cfg(test)` (`NEXUS_REAL_KEYRING=1` opts out for live probes). New test proves tests cannot reach the real store. The three fixture credentials were deleted. A full 1132-test run afterwards left the keychain unchanged. The user's real Groq and Gemini keys were checked and were **not** affected (keychain value identical to `settings.json`).
2. **Removing a Google account did not delete its refresh token.** `remove_google_account` cleared two key names nothing reads, while the tokens actually used (`google_rt_<email>` / `google_at_<email>` → keychain `nexus-mcp-…`, plus the in-memory cache) survived. Fixed with `clear_token`; the refresh token is also no longer written to a second, unused keychain entry. Regression test fails with "refresh token for user1@gmail.com survived removal" when the fix is disabled (demonstrated).
3. **`photoslibrary.readonly` was still requested.** Google removed it from the Photos Library API on 2025-03-31 (calls return 403). It is no longer requested; test pins it. (Google Photos search therefore cannot work against the user's whole library; only the Picker API remains.)
4. **A 7-day silent stop.** While the OAuth consent screen is in *Testing* mode, Google expires refresh tokens after 7 days. The inbox watcher now records `mail_needs_signin`, says once a day "Google needs you to sign in again…", and the Memory page shows "Paused — Google needs you to sign in again".

## Live check on this machine (release binary, real app)
Launched `nexus.exe` with a debug port and called the backend from inside the running app: memory enabled and **encrypted**, 87 episodes and 3 resume points recorded, timetable / mail / egress lists empty (correct), `google_credentials_status` returns the setup message, `google_connect_account` fails in **0 s** with the instructions, a malformed Client ID is refused. No panics in the log; known pre-existing warnings only (Tier-3 command models missing, Intel SST mic silent, a 9Router model missing from Groq's menu). Instance stopped afterwards; keychain and memory files unchanged.

## Verify
- Rust: 1133 passed (main group) + the two TTS groups unchanged; `cargo check --features custom-protocol,admin-brain` clean. Frontend: `tsc` clean, vitest 210/210.
- Release binary rebuilt for the live check contains items 1–2 and the sign-in fix; items 3–4 were added afterwards, so **rebuild before relying on them** (`nexus build`).

## Not verified / open
- A real Google sign-in: needs the user's own OAuth client. Gmail/Calendar behaviour is still covered by fixtures and a mock server only.
- To stop the weekly re-sign-in, publish the OAuth app to *In production* (Google Cloud → Google Auth Platform → Audience). Unverified apps with Gmail scopes then show a warning screen and have a user cap; full verification of restricted Gmail scopes involves a security assessment. Google's pages disagree with third-party guides on exact token lifetimes after publishing — confirm in current Google docs.
- API keys are still stored twice: in the keychain **and** in plaintext `settings.json` (the notes claim `settings.json` is stripped; it is not). Left unchanged — flagged for a decision.
- `ERROR: The system was unable to find the specified registry key or value.` appears once on stderr at startup; its source was not traced.

## Follow-up: the user's existing client (same day)
- The user had already created a Google client, but its ID/secret sat in `server/sidecar/.env` (the retired sidecar's file), which the desktop app never reads. They were loaded into NEXUS's Windows keychain through the app's own `google_save_custom_credentials` command (values read inside a script, never printed); `google_credentials_status` then returned `configured: true, source: saved`.
- Asking Google about that client with NEXUS's redirect (`http://127.0.0.1:49152/callback`) returns **`Error 400: redirect_uri_mismatch`**: it is a **Web application** client, so the loopback redirect must be added under *Authorized redirect URIs* in Google Cloud Console (or a Desktop-app client used instead). This cannot be done from the app.
- The client secret was accidentally printed once in an assistant tool output during the search; rotating it in Cloud Console (Credentials → client → add a new secret) is the clean-up if that matters.
