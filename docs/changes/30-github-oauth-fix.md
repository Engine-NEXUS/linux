# 30 — GitHub OAuth Connect Fix

> **Date**: 2026-09-02
> **Type**: Bug fix
> **Impact**: High — GitHub Connect was completely non-functional

---

## Summary

Fixed the GitHub Connect button in the installer. The browser was not opening
when the user clicked "GitHub Connect" because the Tauri v2 shell plugin was
misconfigured.

## Root Cause

The Tauri v2 shell plugin's `open()` function was **silently failing** because:

1. **Missing `plugins.shell.open` config in `tauri.conf.json`** — Tauri v2
   requires this config entry to enable URL opening. Without it, the shell
   plugin has no validation regex and blocks all URLs.

2. **`shell:allow-open` instead of `shell:default`** in capabilities —
   `shell:allow-open` enables the command but with no pre-configured scope.
   `shell:default` includes `allow-open` with a scope that allows
   `http(s)://`, `tel:`, and `mailto:` links.

3. **No fallback** — If `open()` failed, there was no fallback to
   `window.open()`, so the browser just never opened.

4. **macOS deep-link not registered** — `Info.plist` was missing
   `CFBundleURLTypes` for the `nexus://` scheme.

## What Changed

### `src-tauri/tauri.conf.json`
```diff
 "plugins": {
   "deep-link": {
     "desktop": {
       "schemes": ["nexus"]
     }
+  },
+  "shell": {
+    "open": true
   }
 }
```

### `src-tauri/capabilities/main.json`
```diff
-"shell:allow-open"
+"shell:default"
```

### `src-tauri/Info.plist`
```diff
 <dict>
     <key>LSUIElement</key>
     <true/>
+    <key>CFBundleURLTypes</key>
+    <array>
+        <dict>
+            <key>CFBundleURLName</key>
+            <string>com.nexus.assistant</string>
+            <key>CFBundleURLSchemes</key>
+            <array>
+                <string>nexus</string>
+            </array>
+        </dict>
+    </array>
 </dict>
```

### `frontend/src/setup/oauth.ts`
- Added 3-tier fallback for opening the browser:
  1. `open(url)` — Tauri shell plugin (preferred)
  2. `window.open(url, "_blank")` — fallback if shell fails
  3. `window.location.href = url` — last resort
- Added `onBrowserOpened` callback parameter
- Added console logging at every step

### `frontend/src/setup/SetupApp.tsx`
- Added `connectingPhase` state: `"opening"` → `"waiting"` → `"done"`
- UI shows phase-specific text:
  - "Opening GitHub..." (when browser is opening)
  - "Waiting for authorization..." (when browser is open)
  - "Connected!" (when OAuth completes)
- Added spinner animation during connection
- Added console logging in `handleConnect()`

### `frontend/src/setup/setup.css`
- Added `@keyframes nx-spin` for spinner animation

## Verification

```bash
# Worker returns valid GitHub OAuth URL
curl -s "https://nexus-worker.chitkullakshya.workers.dev/oauth/auth-url?provider=github&user_id=test123&code_challenge=test"
# → {"url":"https://github.com/login/oauth/authorize?client_id=Iv23lipOSFPCN2r21jxg&..."}

# GitHub OAuth is configured
curl -s "https://nexus-worker.chitkullakshya.workers.dev/config/check"
# → {"github":{"configured":true,"scopes":"repo read:org workflow"}}
```

## Build Results

| Build | Result |
|---|---|
| Rust (`cargo build --lib`) | PASS |
| Rust tests (`cargo test --lib`) | 156 passed, 0 failed |
| Frontend (`tsc && vite build`) | PASS |

## Architecture

See [docs/architecture/08-oauth-github-flow.md](../architecture/08-oauth-github-flow.md)
for the full OAuth flow documentation.
