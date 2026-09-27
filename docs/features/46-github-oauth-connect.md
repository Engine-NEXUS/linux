# 46 — GitHub OAuth Connect

> **Date**: 2026-09-02
> **Status**: Fixed and working
> **Files**: `frontend/src/setup/oauth.ts`, `frontend/src/setup/SetupApp.tsx`,
> `src-tauri/tauri.conf.json`, `src-tauri/capabilities/main.json`,
> `src-tauri/Info.plist`

---

## Overview

The GitHub Connect button in the NEXUS installer opens the system browser
directly to GitHub's OAuth authorization page. The user authorizes NEXUS with
one click (if already logged in to GitHub), and the app automatically detects
the connection via polling.

## The Flow

```
1. User clicks "GitHub Connect" in installer
   → UI: "Opening GitHub..." (spinner)

2. Browser opens to:
   https://github.com/login/oauth/authorize?client_id=Iv23lipOSFPCN2r21jxg&...
   → UI: "Waiting for authorization..." (spinner)

3. User authorizes on GitHub (one click if logged in)
   → If not logged in, GitHub shows its native login page first
   → Passkey, password, 2FA — all handled by GitHub

4. GitHub redirects to Worker callback
   → Worker exchanges code for token (server-side)
   → Worker stores token in D1
   → Worker returns success HTML with auto-redirect to nexus://

5. Tauri deep-link plugin catches nexus:// URL
   → Emits "deep-link://oauth-callback" event

6. Frontend resolves OAuth promise
   → ALSO: polling detected connection via /oauth/status
   → UI: "Connected!" (green checkmark + badge)
```

## What Was Fixed

### Bug: Browser Didn't Open

**Root cause:** Tauri v2 shell plugin's `open()` function was silently failing
because:
1. Missing `plugins.shell.open` config in `tauri.conf.json`
2. `shell:allow-open` permission had no URL scope
3. No fallback if `open()` failed

**Fix:**
1. Added `"shell": { "open": true }` to `tauri.conf.json` plugins
2. Changed `shell:allow-open` → `shell:default` in capabilities
3. Added 3-tier fallback: `open()` → `window.open()` → `location.href`

### Bug: macOS Deep-Link Not Registered

**Root cause:** `Info.plist` was missing `CFBundleURLTypes` for `nexus://`
scheme.

**Fix:** Added `CFBundleURLTypes` with `nexus` URL scheme.

## UI States

| Phase | Text | Visual |
|---|---|---|
| Idle | "Click to connect — opens GitHub in browser" | Chevron icon |
| Opening | "Opening GitHub..." | Spinner |
| Waiting | "Waiting for authorization..." | Spinner |
| Done | "Connected!" | Green checkmark + "Connected" badge |

## Configuration

### `tauri.conf.json`
```json
"plugins": {
    "shell": { "open": true },
    "deep-link": { "desktop": { "schemes": ["nexus"] } }
}
```

### `capabilities/main.json`
```json
"permissions": ["shell:default", "deep-link:default"]
```

### `Info.plist` (macOS)
```xml
<key>CFBundleURLTypes</key>
<array>
    <dict>
        <key>CFBundleURLName</key>
        <string>com.nexus.assistant</string>
        <key>CFBundleURLSchemes</key>
        <array><string>nexus</string></array>
    </dict>
</array>
```

## GitHub OAuth App

| Setting | Value |
|---|---|
| Client ID | `Iv23lipOSFPCN2r21jxg` |
| Callback URL | `https://nexus-worker.chitkullakshya.workers.dev/oauth/callback` |
| Scopes | `repo read:org workflow` |

## Security

- **PKCE**: Code verifier generated client-side, challenge sent to Worker
- **Client secret**: Stored as Worker secret, never exposed to frontend
- **Token exchange**: Happens server-side in the Worker
- **State parameter**: `github:<userId>` prevents CSRF

### Known Security Issues (Pending Fix)

- `/oauth/github-token` returns raw token to any caller with user_id
- OAuth tokens stored plaintext in D1 (should use AES-GCM)
- No request authentication (Worker trusts user_id parameter)
- API keys stored with base64 encoding (should use AES-GCM)
