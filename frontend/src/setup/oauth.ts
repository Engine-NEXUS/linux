/**
 * OAuth2 PKCE client for NEXUS desktop app (serverless — Cloudflare Worker).
 *
 * Flow:
 *  1. Generate PKCE verifier + challenge (SHA-256, base64url).
 *  2. Ask the Worker for the provider's authorization URL (includes our challenge).
 *  3. Open the system browser to that URL.
 *  4. User logs in → provider redirects to nexus://oauth/callback?code=XXX.
 *  5. Tauri deep-link plugin catches the redirect and emits an event.
 *  6. We extract the code + state, send code + verifier to Worker /oauth/exchange.
 *  7. Worker exchanges code for tokens (using client secret stored as Worker secret).
 *  8. Tokens are stored per-user in Cloudflare D1. Client gets "connected" confirmation.
 *
 * For API keys (Claude, Devin, etc.) — no OAuth needed. User pastes the key,
 * we POST it to Worker /apikeys/add.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-shell";

// The Worker base URL. In production this comes from the saved config.
// The setup page sets this from the server config.
let workerBaseUrl = "";

export function setSidecarBaseUrl(url: string): void {
  // Strip trailing slash.
  workerBaseUrl = url.replace(/\/+$/, "");
}

export function getSidecarBaseUrl(): string {
  return workerBaseUrl;
}

// ---- PKCE utilities ----

/** Generate a cryptographically random PKCE code verifier (43-128 chars). */
export function generateCodeVerifier(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return base64UrlEncode(bytes);
}

/** Derive the code challenge from the verifier (SHA-256, base64url). */
export async function generateCodeChallenge(verifier: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(verifier);
  const hash = await crypto.subtle.digest("SHA-256", data);
  return base64UrlEncode(new Uint8Array(hash));
}

/** Base64URL encode (no padding) — per RFC 7636. */
function base64UrlEncode(bytes: Uint8Array): string {
  let str = "";
  for (const b of bytes) str += String.fromCharCode(b);
  return btoa(str)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

// ---- OAuth connect flow ----

/** Pending OAuth state — stored while waiting for the browser redirect. */
interface PendingOAuth {
  provider: string;
  codeVerifier: string;
  userId: string;
  resolve: (success: boolean) => void;
  reject: (err: Error) => void;
}

let pending: PendingOAuth | null = null;
let unlistenDeepLink: UnlistenFn | null = null;

/**
 * Connect a Google or GitHub account via OAuth PKCE.
 *
 * Returns a promise that resolves when the OAuth flow completes (or rejects on error/timeout).
 */
export async function connectOAuth(
  provider: "google" | "github" | "swiggy",
  userId: string,
  onBrowserOpened?: () => void,
): Promise<boolean> {
  if (!workerBaseUrl) {
    throw new Error("Server URL not configured. Enter your server URL first.");
  }

  // Generate PKCE challenge for providers that support it
  const codeVerifier = generateCodeVerifier();
  const codeChallenge = await generateCodeChallenge(codeVerifier);

  // 1. Ask Worker for the authorization URL.
  console.log(`[OAuth] Fetching auth URL from ${workerBaseUrl}/oauth/auth-url?provider=${provider}&user_id=${encodeURIComponent(userId)}&code_challenge=${codeChallenge}`);
  const authUrlResp = await fetch(
    `${workerBaseUrl}/oauth/auth-url?provider=${provider}&user_id=${encodeURIComponent(userId)}&code_challenge=${codeChallenge}`,
  );
  if (!authUrlResp.ok) {
    const err = await authUrlResp.json().catch(() => ({}));
    const msg = err.error || `Failed to get OAuth URL (${authUrlResp.status})`;
    console.error(`[OAuth] auth-url failed: ${msg}`);
    throw new Error(msg);
  }
  const { url } = await authUrlResp.json();
  console.log(`[OAuth] Got auth URL: ${url.substring(0, 80)}...`);

  // 2. Open the system browser for the user to log in.
  //    Use Tauri shell.open first, with a fallback to window.open
  //    in case the shell plugin scope isn't configured.
  try {
    await open(url);
    console.log("[OAuth] opened via tauri shell.open");
  } catch (shellErr) {
    console.warn("[OAuth] shell.open failed, falling back to window.open:", shellErr);
    // Fallback: open in a new browser tab. This works in dev mode and
    // when the shell plugin scope isn't configured.
    const win = window.open(url, "_blank", "noopener,noreferrer");
    if (!win) {
      // Popup blocked — try location redirect as last resort
      console.warn("[OAuth] window.open blocked, trying location.href redirect");
      window.location.href = url;
    }
  }

  // Notify caller that the browser has been opened — they can update
  // the UI to show "Waiting for authorization..." while the user
  // logs in / authorizes on GitHub.
  if (onBrowserOpened) onBrowserOpened();

  // 3. Set up dual-channel completion: deep-link listener + active status polling
  const result = await new Promise<boolean>((resolve, reject) => {
    let finished = false;
    let pollInterval: ReturnType<typeof setInterval> | null = null;
    let timeoutTimer: ReturnType<typeof setTimeout> | null = null;

    const cleanup = () => {
      finished = true;
      if (pollInterval) {
        clearInterval(pollInterval);
        pollInterval = null;
      }
      if (timeoutTimer) {
        clearTimeout(timeoutTimer);
        timeoutTimer = null;
      }
      pending = null;
    };

    const onComplete = (success: boolean) => {
      if (finished) return;
      cleanup();
      resolve(success);
    };

    const onFail = (err: Error) => {
      if (finished) return;
      cleanup();
      reject(err);
    };

    pending = {
      provider,
      codeVerifier,
      userId,
      resolve: () => onComplete(true),
      reject: (e) => onFail(e),
    };

    // Deep link event listener
    if (!unlistenDeepLink) {
      listen<string>("deep-link://oauth-callback", (event) => {
        handleOAuthRedirect(event.payload).then(() => onComplete(true)).catch((err) => {
          console.warn("[OAuth] Deep link error:", err);
        });
      }).then((fn) => {
        unlistenDeepLink = fn;
      });
    }

    // Active status polling: automatically catches completion when user approves in browser
    pollInterval = setInterval(async () => {
      if (finished) return;
      try {
        const status = await getOAuthStatus(userId);
        if (status[provider]?.connected) {
          console.log(`[OAuth] ${provider} connected successfully via status check`);
          onComplete(true);
        }
      } catch {
        // network retry
      }
    }, 1500);

    // Timeout after 5 minutes
    timeoutTimer = setTimeout(() => {
      onFail(new Error("OAuth timed out — authorization was not completed within 5 minutes"));
    }, 5 * 60 * 1000);
  });

  return result;
}

/** Handle the OAuth redirect URL (nexus://oauth/callback?provider=...&user_id=...&status=success or ?code=XXX). */
async function handleOAuthRedirect(rawUrl: string): Promise<void> {
  if (!pending) return;

  try {
    const url = new URL(rawUrl);
    const status = url.searchParams.get("status");
    const error = url.searchParams.get("error");
    const code = url.searchParams.get("code");

    if (status === "success") {
      pending.resolve(true);
      pending = null;
      return;
    }

    if (error) {
      const err = new Error(`OAuth error: ${error}`);
      pending.reject(err);
      pending = null;
      return;
    }

    if (code) {
      // Exchange code if direct redirect was used
      const resp = await fetch(`${workerBaseUrl}/oauth/exchange`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          provider: pending.provider,
          code,
          code_verifier: pending.codeVerifier,
          redirect_uri: `${workerBaseUrl}/oauth/callback`,
          user_id: pending.userId,
          state: url.searchParams.get("state"),
        }),
      });

      if (!resp.ok) {
        const err = await resp.json().catch(() => ({}));
        throw new Error(err.error || `Exchange failed (${resp.status})`);
      }

      pending.resolve(true);
      pending = null;
    }
  } catch (err) {
    if (pending) {
      pending.reject(err instanceof Error ? err : new Error(String(err)));
      pending = null;
    }
  }
}

// ---- API key management ----

/** Store an API key for a third-party service (Claude, Devin, etc.). */
export async function addApiKey(userId: string, provider: string, apiKey: string): Promise<void> {
  if (!workerBaseUrl) throw new Error("Server URL not configured");
  const resp = await fetch(`${workerBaseUrl}/apikeys/add`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ user_id: userId, provider, api_key: apiKey }),
  });
  if (!resp.ok) {
    const err = await resp.json().catch(() => ({}));
    throw new Error(err.error || `Failed to store API key (${resp.status})`);
  }
}

/** Remove a stored API key. */
export async function removeApiKey(userId: string, provider: string): Promise<void> {
  if (!workerBaseUrl) throw new Error("Server URL not configured");
  const resp = await fetch(`${workerBaseUrl}/apikeys/remove`, {
    method: "DELETE",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ user_id: userId, provider }),
  });
  if (!resp.ok) throw new Error(`Failed to remove API key (${resp.status})`);
}

/** List which API key providers are stored (does NOT return the keys). */
export async function listApiKeys(userId: string): Promise<string[]> {
  if (!workerBaseUrl) return [];
  const resp = await fetch(`${workerBaseUrl}/apikeys/list?user_id=${encodeURIComponent(userId)}`);
  if (!resp.ok) return [];
  const data = await resp.json();
  return data.providers || [];
}

// ---- OAuth status ----

export interface OAuthStatus {
  connected: boolean;
  expired: boolean;
  scopes: string;
}

/** Check which OAuth providers are connected for a user. */
export async function getOAuthStatus(userId: string): Promise<Record<string, OAuthStatus>> {
  if (!workerBaseUrl) return {};
  const resp = await fetch(`${workerBaseUrl}/oauth/status?user_id=${encodeURIComponent(userId)}`);
  if (!resp.ok) return {};
  const data = await resp.json();
  return data.providers || {};
}

/** Disconnect an OAuth provider. */
export async function disconnectOAuth(userId: string, provider: string): Promise<void> {
  if (!workerBaseUrl) throw new Error("Server URL not configured");
  const resp = await fetch(`${workerBaseUrl}/oauth/disconnect`, {
    method: "DELETE",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ user_id: userId, provider }),
  });
  if (!resp.ok) throw new Error(`Failed to disconnect (${resp.status})`);
}

/** Open a URL in the system browser. */
export async function openInBrowser(url: string): Promise<void> {
  await open(url);
}
