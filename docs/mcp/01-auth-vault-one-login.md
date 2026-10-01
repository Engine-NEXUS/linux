# Auth Vault + One Login (2026-09-18)

Goal: user logs in once per group; every MCP works after that with zero
re-auth pain. Family-compatible (no brain involved).

## Design

- One OS-keychain-backed store (Windows Credential Manager via `keyring`
  crate). `settings.json` keeps pointers + health only — never secrets.
- `mcp_client::call_tool` resolves the right credential per server at call
  time; on 401 it refreshes once and retries (MCP spec OAuth 2.1
  resource/auth-server split; multi-server one-token pattern for our bridges).
- Client never holds downstream credentials beyond the call.

## Login groups

| Group | Logins | Method |
|---|---|---|
| Gmail + Calendar + Contacts + Drive + Sheets (+ Meet free) | **1** | Single Google OAuth consent, full scope union upfront (`gmail.send`, `calendar`, `contacts`, `drive`, `spreadsheets`), `access_type=offline` → one refresh token. Front-loading matters: adding a scope later forces re-consent. Handle granular denials by disabling features, not crashing. |
| WhatsApp / LinkedIn | **1 each (QR / session cookie)** | Bridge session persists on disk; re-login only if unlinked. Vault stores session-validity + re-login prompt. |
| Swiggy / Spotify / Vercel / Render | **1 each** | OAuth PKCE (Swiggy, Spotify, Render) or token (Vercel) → vault. |
| YouTube / Amazon scraper | 0 / key | Keyless transcripts; Data API key optional (quota-guarded). |

## Google Testing-mode caveat

Personal-use OAuth stays in Testing: refresh tokens expire after 7 days
until the app is verified. Expect weekly re-consent for Google services
until verification (or accept it as the free-tier cost).

## Onboarding UX

One "Connect" wizard, max 3 steps (Google → WhatsApp QR → Swiggy), then a
status dashboard extending `nexus_diagnostics`: per-MCP
connected/expiring/failed + reconnect button.

## Session types in the vault

- Refreshable OAuth (Google, Spotify, Swiggy, Render): auto-refresh on 401.
- Cookie/session (WhatsApp, LinkedIn): validity check + re-login prompt.
- Static (Vercel token, YouTube key): present/missing check + quota guard.
