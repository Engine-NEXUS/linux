# Feature 77 — Multi-Email Google Accounts, Direct Native OAuth & Settings Refactor

## Executive Summary
Enables users to connect and manage multiple Google accounts (work, personal, school) directly within NEXUS Settings without requiring an external server or Cloudflare Worker. Each connected account is securely stored in Windows Credential Manager, auto-refreshed in the background, and seamlessly utilized by the proactive Gmail Sentinel and Voice Command Center.

## Objectives
1. **Direct Native Desktop OAuth (RFC 8252)**:
   - Eliminate external Cloudflare Worker dependency for authentication.
   - Native loopback redirection with PKCE directly between the desktop app and Google OAuth2 endpoints.
2. **Multi-Account Storage & Management**:
   - Store multiple authenticated Google accounts in Windows Credential Manager.
   - Designate primary/active account while allowing Sentinel to monitor watched threads across any account.
3. **Settings Sidebar Refactor**:
   - Modern multi-account UI in `SettingsSidebarApp.tsx`: account list, user avatars, email chips, and "+ Add Google Account" button.
   - Custom Google Console Client ID and Secret configuration inputs.
4. **Brave / Chromium URL Extraction Hardening**:
   - Fix schemeless omnibox handling (`mail.google.com/...`) and window title metadata fallback.

## Architecture

```
User clicks "+ Add Google Account" in Settings
       │
       ▼
Local Loopback HTTP Listener (port 49152 / dynamic)
       │
       ▼
System Browser opens Google Consent Screen
       │
       ▼
Google redirects to http://127.0.0.1:49152/callback?code=...
       │
       ▼
Rust exchanges code for Access + Refresh Tokens
       │
       ▼
Fetch User Profile (email, name, picture) via UserInfo API
       │
       ▼
Save to Windows Credential Manager (nexus-google-{email})
       │
       ▼
Emit updated account list to Settings UI & Sentinel Loop
```

## Security
- Tokens are encrypted at rest using the OS Credential Manager (DPAPI on Windows).
- Refresh tokens are never logged or exposed to the frontend webview.
