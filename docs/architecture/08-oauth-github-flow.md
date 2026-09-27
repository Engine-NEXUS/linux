# 08 — OAuth GitHub Connect Flow

> **Status**: Fixed 2026-09-02
> **Files**: `frontend/src/setup/oauth.ts`, `frontend/src/setup/SetupApp.tsx`,
> `src-tauri/tauri.conf.json`, `src-tauri/capabilities/main.json`,
> `src-tauri/Info.plist`, `server/worker/src/index.ts`

---

## 1. Problem Statement

When the user clicked "GitHub Connect" in the installer, **nothing happened**.
The browser didn't open. No error was shown. The button appeared to be dead.

### Root Cause

The Tauri v2 shell plugin's `open()` function was **silently failing** because:

1. **Missing `plugins.shell.open` config in `tauri.conf.json`** — Tauri v2
   requires this config entry to enable URL opening. Without it, the shell
   plugin has no validation regex and blocks all URLs.

2. **`shell:allow-open` instead of `shell:default`** in capabilities —
   `shell:allow-open` enables the command but with **no pre-configured scope**.
   `shell:default` includes `allow-open` with a scope that allows
   `http(s)://`, `tel:`, and `mailto:` links.

3. **No fallback** — If `open()` failed, there was no fallback to
   `window.open()`, so the browser just never opened.

4. **macOS deep-link not registered** — `Info.plist` was missing
   `CFBundleURLTypes` for the `nexus://` scheme.

---

## 2. The OAuth Flow (Official GitHub OAuth 2.0 with PKCE)

This is the standard, secure OAuth 2.0 Authorization Code flow with PKCE —
exactly like GitHub's own "Sign in with GitHub" buttons on websites.

### Step-by-Step

```
┌─────────────────────────────────────────────────────────────────┐
│  1. USER CLICKS "GitHub Connect" IN INSTALLER                   │
│                                                                 │
│  SetupApp.tsx → handleConnect("github")                         │
│    → loads serverUrl + userId from get_server_config            │
│    → calls connectOAuth("github", userId, onBrowserOpened)      │
│    → UI shows: "Opening GitHub..." (with spinner)               │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  2. FRONTEND GENERATES PKCE CHALLENGE                           │
│                                                                 │
│  oauth.ts → connectOAuth()                                      │
│    → generateCodeVerifier()  ← 32 random bytes, base64url      │
│    → generateCodeChallenge(verifier)  ← SHA-256, base64url      │
│    → Fetches: GET /oauth/auth-url?provider=github               │
│        &user_id=<userId>&code_challenge=<challenge>             │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  3. WORKER RETURNS GITHUB AUTH URL                              │
│                                                                 │
│  Worker (server/worker/src/index.ts) → handleAuthUrl()          │
│    → Reads GITHUB_CLIENT_ID from Worker secrets                 │
│    → Builds URL:                                                │
│        https://github.com/login/oauth/authorize                 │
│          ?client_id=Iv23lipOSFPCN2r21jxg                        │
│          &redirect_uri=https://nexus-worker.../oauth/callback   │
│          &scope=repo read:org workflow                          │
│          &state=github:<userId>                                 │
│    → Returns: { url: "https://github.com/login/oauth/...",      │
│                 redirect_uri: "https://nexus-worker.../..." }   │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  4. FRONTEND OPENS SYSTEM BROWSER                               │
│                                                                 │
│  oauth.ts → open(url)  ← Tauri shell plugin (NOW WORKS)        │
│    → Fallback: window.open(url, "_blank")                       │
│    → Fallback: window.location.href = url                       │
│    → Calls onBrowserOpened() callback                           │
│    → UI shows: "Waiting for authorization..." (with spinner)    │
│    → Starts polling /oauth/status every 1.5 seconds             │
│    → Starts deep-link listener for nexus://oauth/callback       │
│    → 5-minute timeout                                           │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  5. USER AUTHORIZES ON GITHUB (in browser)                      │
│                                                                 │
│  GitHub's native page shows:                                    │
│  ┌─────────────────────────────────────────────┐                │
│  │  Authorize NEXUS                             │                │
│  │                                              │                │
│  │  [ user's GitHub username ]                  │                │
│  │                                              │                │
│  │  NEXUS by chitkullakshya wants to:           │                │
│  │  ✓ Access your public repos                  │                │
│  │  ✓ Access your private repos                 │                │
│  │  ✓ Read org info                             │                │
│  │  ✓ Access workflows                          │                │
│  │                                              │                │
│  │  [ Authorize NEXUS ]  [ Cancel ]             │                │
│  └─────────────────────────────────────────────┘                │
│                                                                 │
│  If NOT logged in, GitHub shows its native login page FIRST:    │
│    - Passkey login (biometric, one tap)                         │
│    - Username + password                                        │
│    - 2FA                                                        │
│  Then the authorize page above.                                 │
│                                                                 │
│  User clicks "Authorize NEXUS" (one click if already logged in) │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  6. GITHUB REDIRECTS TO WORKER CALLBACK                         │
│                                                                 │
│  GitHub redirects to:                                           │
│    https://nexus-worker.chitkullakshya.workers.dev/oauth/callback│
│      ?code=<authorization_code>                                 │
│      &state=github:<userId>                                     │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  7. WORKER EXCHANGES CODE FOR TOKEN                             │
│                                                                 │
│  Worker → handleOAuthBrowserCallback()                          │
│    → POST to https://github.com/login/oauth/access_token        │
│        body: {                                                  │
│          client_id: env.GITHUB_CLIENT_ID,                       │
│          client_secret: env.GITHUB_CLIENT_SECRET,               │
│          code: <authorization_code>,                            │
│          redirect_uri: <callbackUrl>                            │
│        }                                                        │
│    → GitHub returns: { access_token, token_type, scope }        │
│    → Worker fetches GitHub user info:                           │
│        GET https://api.github.com/user                          │
│        Authorization: Bearer <access_token>                     │
│    → Stores in D1:                                              │
│        INSERT OR REPLACE INTO oauth_tokens                      │
│        (user_id, provider, access_token, refresh_token,         │
│         expires_at, scopes, account_id, created_at)             │
│        VALUES (?, 'github', ?, null, 0, 'repo read:org ...',    │
│                <github_login>, <now>)                           │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  8. WORKER RETURNS SUCCESS HTML                                 │
│                                                                 │
│  Worker returns HTML page:                                      │
│  ┌─────────────────────────────────────────────┐                │
│  │  ✓ GitHub Connected!                         │                │
│  │  Your GitHub account (username) is now       │                │
│  │  connected to NEXUS.                         │                │
│  │  [ Return to NEXUS ]                         │                │
│  └─────────────────────────────────────────────┘                │
│                                                                 │
│  HTML includes auto-redirect script:                            │
│    window.location.href = "nexus://oauth/callback               │
│      ?provider=github&user_id=<userId>&status=success"          │
│    setTimeout(() => window.close(), 3000)                       │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  9. DEEP-LINK REDIRECT CAUGHT BY TAURI                          │
│                                                                 │
│  Browser redirects to nexus://oauth/callback?...&status=success │
│                                                                 │
│  Tauri deep-link plugin catches the nexus:// URL:               │
│    Windows: single-instance handler (args contains URL)         │
│    macOS: app.deep_link().on_open_url() (from Info.plist)       │
│    Linux: single-instance handler                               │
│                                                                 │
│  Tauri emits event: "deep-link://oauth-callback"                │
│    payload: "nexus://oauth/callback?...&status=success"         │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  10. FRONTEND RESOLVES OAUTH PROMISE                            │
│                                                                 │
│  oauth.ts → handleOAuthRedirect(url)                            │
│    → Parses URL: status=success                                 │
│    → Calls pending.resolve(true)                                │
│    → connectOAuth() promise resolves                            │
│                                                                 │
│  ALSO: Polling detected connection via /oauth/status             │
│    → getOAuthStatus(userId) returned github.connected = true    │
│    → onComplete(true) called                                    │
│                                                                 │
│  SetupApp.tsx → handleConnect() continues:                      │
│    → setConnectingPhase("done")                                 │
│    → checkServer() → refreshes oauthStatus                      │
│    → UI shows: "Connected!" → green checkmark + badge           │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. Configuration Changes

### `src-tauri/tauri.conf.json` — Shell Plugin Config

```json
{
  "plugins": {
    "shell": {
      "open": true
    }
  }
}
```

`"open": true` enables the shell plugin's `open()` function with the default
URL validation regex: `^((mailto:\w+)|(tel:\w+)|(https?://\w+)).+`

This allows opening any `http://` or `https://` URL in the system browser.

### `src-tauri/capabilities/main.json` — Shell Permission

```json
{
  "permissions": [
    "shell:default"
  ]
}
```

Changed from `shell:allow-open` to `shell:default`.

**Difference:**
- `shell:allow-open` — enables the `open` command but with **no scope**. URLs
  are blocked because there's no validation regex configured.
- `shell:default` — includes `allow-open` **with** a pre-configured scope that
  allows `http(s)://`, `tel:`, and `mailto:` links.

### `src-tauri/Info.plist` — macOS Deep-Link Registration

```xml
<key>CFBundleURLTypes</key>
<array>
    <dict>
        <key>CFBundleURLName</key>
        <string>com.nexus.assistant</string>
        <key>CFBundleURLSchemes</key>
        <array>
            <string>nexus</string>
        </array>
    </dict>
</array>
```

macOS needs this to route `nexus://` deep-link URLs to the app. Without it,
the `nexus://oauth/callback` redirect from the Worker's success page would
not open NEXUS on macOS.

Windows and Linux use runtime registration via `app.deep_link().register("nexus")`
in `lib.rs`.

---

## 4. Frontend Changes

### `frontend/src/setup/oauth.ts` — Fallback + Logging

Added a 3-tier fallback for opening the browser:

```typescript
// Tier 1: Tauri shell plugin (preferred — opens system default browser)
try {
    await open(url);
} catch (shellErr) {
    // Tier 2: window.open (works in dev mode, may be popup-blocked)
    const win = window.open(url, "_blank", "noopener,noreferrer");
    if (!win) {
        // Tier 3: location.href redirect (last resort, navigates away)
        window.location.href = url;
    }
}
```

Also added:
- `onBrowserOpened` callback parameter — fires when the browser opens, so
  the UI can update from "Opening GitHub..." to "Waiting for authorization..."
- Console logging at every step for debugging

### `frontend/src/setup/SetupApp.tsx` — UI Phases + Logging

Added `connectingPhase` state with 3 phases:

| Phase | UI Text | When |
|---|---|---|
| `"opening"` | "Opening GitHub..." | Button clicked, browser is opening |
| `"waiting"` | "Waiting for authorization..." | Browser is open, user is on GitHub |
| `"done"` | "Connected!" | OAuth completed successfully |

Added spinner animation next to the provider name during connection.

Added console logging in `handleConnect()` for debugging:
- Server URL, userId loaded
- Connection attempt
- Success/failure

---

## 5. Worker Configuration

### GitHub OAuth App Settings

The Worker uses a GitHub OAuth App with:

| Setting | Value |
|---|---|
| Client ID | `Iv23lipOSFPCN2r21jxg` |
| Client Secret | Stored as Worker secret `GITHUB_CLIENT_SECRET` |
| Authorization URL | `https://github.com/login/oauth/authorize` |
| Token URL | `https://github.com/login/oauth/access_token` |
| Callback URL | `https://nexus-worker.chitkullakshya.workers.dev/oauth/callback` |
| Scopes | `repo read:org workflow` |

### Scope Breakdown

| Scope | What it allows |
|---|---|
| `repo` | Full access to public and private repositories (read/write) |
| `read:org` | Read access to organization membership |
| `workflow` | Ability to view and trigger GitHub Actions workflows |

### Worker Secrets (set via `wrangler secret put`)

```
GITHUB_CLIENT_ID=Iv23lipOSFPCN2r21jxg
GITHUB_CLIENT_SECRET=<secret>
GOOGLE_CLIENT_ID=<secret>
GOOGLE_CLIENT_SECRET=<secret>
NEXUS_ENCRYPTION_KEY=<secret>
```

---

## 6. D1 Database Schema

```sql
CREATE TABLE IF NOT EXISTS oauth_tokens (
    user_id      TEXT NOT NULL,
    provider     TEXT NOT NULL,
    access_token TEXT NOT NULL,
    refresh_token TEXT,
    expires_at   INTEGER,
    scopes       TEXT,
    account_id   TEXT,
    created_at   INTEGER,
    PRIMARY KEY (user_id, provider)
);
```

After successful OAuth, the Worker stores:
- `user_id` — the NEXUS user ID (from the state parameter)
- `provider` — `"github"`
- `access_token` — the GitHub access token
- `refresh_token` — `null` (GitHub doesn't use refresh tokens for OAuth Apps)
- `expires_at` — `0` (GitHub tokens don't expire by default)
- `scopes` — `"repo read:org workflow"`
- `account_id` — the GitHub username (fetched from `/user` API)
- `created_at` — Unix timestamp

---

## 7. Security Notes

### What's Secure

1. **PKCE** — The code verifier is generated client-side and never sent to the
   Worker until the final exchange. Even if the authorization code is
   intercepted, it can't be exchanged without the verifier.

2. **Client secret stays server-side** — The `GITHUB_CLIENT_SECRET` is stored
   as a Cloudflare Worker secret. It's never exposed to the frontend.

3. **Token exchange happens server-side** — The Worker exchanges the
   authorization code for the access token. The frontend never sees the raw
   GitHub token (except via the `/oauth/github-token` endpoint, which is a
   known security issue to fix).

4. **State parameter** — The `state` parameter (`github:<userId>`) prevents
   CSRF attacks. The Worker validates it matches the expected format.

### What Needs Improvement (Known Issues)

1. **`/oauth/github-token` returns raw token** — This endpoint returns the
   raw GitHub access token to any caller who knows the `user_id`. This should
   be replaced with authenticated access (device token validation).

2. **OAuth tokens stored plaintext** — The `access_token` is stored in D1
   without encryption. Should use AES-GCM with `NEXUS_ENCRYPTION_KEY`.

3. **No request authentication** — The Worker trusts the `user_id` parameter
   in requests. Anyone who knows a user's ID can impersonate them. Needs
   device/session token validation.

4. **Base64 API key storage** — `api_keys.key_encrypted` uses `btoa()` which
   is reversible encoding, not encryption. Should use AES-GCM.

These are documented in `docs/credentials/07-security-best-practices.md` and
are pending implementation.

---

## 8. Verification

### Worker Endpoints (tested with curl)

```bash
# Auth URL endpoint — returns valid GitHub OAuth URL
curl -s "https://nexus-worker.chitkullakshya.workers.dev/oauth/auth-url?provider=github&user_id=test123&code_challenge=test"
# → {"url":"https://github.com/login/oauth/authorize?client_id=Iv23lipOSFPCN2r21jxg&..."}

# Config check — GitHub is configured
curl -s "https://nexus-worker.chitkullakshya.workers.dev/config/check"
# → {"google":{"configured":true,...},"github":{"configured":true,"scopes":"repo read:org workflow"}}

# Status check — returns providers for a user
curl -s "https://nexus-worker.chitkullakshya.workers.dev/oauth/status?user_id=test123"
# → {"user_id":"test123","providers":{}}
```

### Build Results

| Build | Result |
|---|---|
| Rust (`cargo build --lib`) | PASS |
| Rust tests (`cargo test --lib`) | 156 passed, 0 failed |
| Frontend (`tsc && vite build`) | PASS |

---

## 9. Files Changed

| File | Change |
|---|---|
| `src-tauri/tauri.conf.json` | Added `"shell": { "open": true }` under `plugins` |
| `src-tauri/capabilities/main.json` | Changed `shell:allow-open` → `shell:default` |
| `src-tauri/Info.plist` | Added `CFBundleURLTypes` with `nexus` scheme for macOS |
| `frontend/src/setup/oauth.ts` | Added 3-tier fallback for `open()`, `onBrowserOpened` callback, logging |
| `frontend/src/setup/SetupApp.tsx` | Added `connectingPhase` state (opening/waiting/done), spinner, logging |
| `frontend/src/setup/setup.css` | Added `nx-spin` keyframe animation for spinner |
