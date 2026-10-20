"""
OAuth2 token exchange and management for Google and GitHub.

The desktop client performs the OAuth authorization in the system browser
using PKCE (no client secret in the app). The browser redirects back to
`NEXUS://oauth/{provider}` with an authorization code. The client forwards
the code + PKCE verifier to this sidecar, which exchanges it for tokens
using the client secret stored server-side.

Endpoints:
  POST /oauth/exchange   — exchange auth code for tokens
  POST /oauth/refresh    — refresh an expired access token
  GET  /oauth/status     — check which providers are connected for a user
  DELETE /oauth/disconnect — remove a provider's tokens for a user
  POST /apikeys/add      — store an API key (Claude, Devin, etc.)
  DELETE /apikeys/remove — remove an API key
  GET  /apikeys/list     — list stored API key providers (not the keys themselves)
"""

from __future__ import annotations

import os
import json
import logging
import time
from typing import Optional

import httpx
from fastapi import APIRouter, Request
from fastapi.responses import JSONResponse

from . import db

log = logging.getLogger("NEXUS.sidecar.oauth")
router = APIRouter()

# ---- Configuration (env-driven, secrets stay server-side) ----

GOOGLE_CLIENT_ID = os.getenv("GOOGLE_CLIENT_ID", "")
GOOGLE_CLIENT_SECRET = os.getenv("GOOGLE_CLIENT_SECRET", "")
GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token"

GITHUB_CLIENT_ID = os.getenv("GITHUB_CLIENT_ID", "")
GITHUB_CLIENT_SECRET = os.getenv("GITHUB_CLIENT_SECRET", "")
GITHUB_TOKEN_URL = "https://github.com/login/oauth/access_token"

# The redirect URI that the browser uses. The client catches this via deep-link
# and sends the code to the sidecar. This must match what's registered in
# Google Cloud Console / GitHub OAuth App.
OAUTH_REDIRECT_URI = os.getenv("OAUTH_REDIRECT_URI", "NEXUS://oauth/callback")

# Default scopes per provider.
GOOGLE_SCOPES = " ".join([
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/meetings",
    "openid",
    "email",
    "profile",
])

GITHUB_SCOPES = "repo read:org workflow"


# ---- Token exchange ----

async def _exchange_google(code: str, code_verifier: str, redirect_uri: str) -> dict:
    """Exchange a Google auth code for access + refresh tokens."""
    async with httpx.AsyncClient(timeout=30.0) as client:
        resp = await client.post(GOOGLE_TOKEN_URL, data={
            "client_id": GOOGLE_CLIENT_ID,
            "client_secret": GOOGLE_CLIENT_SECRET,
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
            "grant_type": "authorization_code",
        })
        resp.raise_for_status()
        return resp.json()


async def _exchange_github(code: str, code_verifier: str, redirect_uri: str) -> dict:
    """Exchange a GitHub auth code for an access token."""
    async with httpx.AsyncClient(timeout=30.0) as client:
        resp = await client.post(GITHUB_TOKEN_URL, json={
            "client_id": GITHUB_CLIENT_ID,
            "client_secret": GITHUB_CLIENT_SECRET,
            "code": code,
            "code_verifier": code_verifier,
            "redirect_uri": redirect_uri,
        }, headers={"Accept": "application/json"})
        resp.raise_for_status()
        return resp.json()


async def _refresh_google(refresh_token: str) -> dict:
    """Refresh a Google access token."""
    async with httpx.AsyncClient(timeout=30.0) as client:
        resp = await client.post(GOOGLE_TOKEN_URL, data={
            "client_id": GOOGLE_CLIENT_ID,
            "client_secret": GOOGLE_CLIENT_SECRET,
            "refresh_token": refresh_token,
            "grant_type": "refresh_token",
        })
        resp.raise_for_status()
        return resp.json()


# ---- Public credential access (used by the session handler) ----

async def get_valid_credentials(user_id: str) -> dict:
    """
    Return all valid credentials for a user, refreshing expired tokens.

    Returns a dict like:
      {
        "google": {"access_token": "ya29...", "scopes": "..."},
        "github": {"access_token": "gho_..."},
        "api_keys": {"claude": "sk-...", "devin": "..."}
      }
    """
    result: dict = {"api_keys": {}}

    # OAuth tokens
    oauth_tokens = db.get_all_oauth_tokens(user_id)
    for provider, token in oauth_tokens.items():
        if db.is_token_expired(token) and token.get("refresh_token"):
            try:
                if provider == "google":
                    refreshed = await _refresh_google(token["refresh_token"])
                    new_access = refreshed["access_token"]
                    new_expires = refreshed.get("expires_in", 3600)
                    db.store_oauth_token(
                        user_id, provider, new_access,
                        token["refresh_token"], new_expires, token.get("scopes", ""),
                    )
                    result[provider] = {"access_token": new_access, "scopes": token.get("scopes", "")}
                # GitHub tokens don't expire by default; skip refresh.
                else:
                    result[provider] = {"access_token": token["access_token"]}
            except Exception:
                log.exception("failed to refresh %s token for user %s", provider, user_id)
                # Fall back to the expired token (might still work briefly).
                result[provider] = {"access_token": token["access_token"]}
        else:
            result[provider] = {"access_token": token["access_token"], "scopes": token.get("scopes", "")}

    # API keys
    api_keys = db.get_all_api_keys(user_id)
    result["api_keys"] = api_keys

    return result


# ---- API Routes ----

@router.post("/oauth/exchange")
async def oauth_exchange(request: Request) -> JSONResponse:
    """
    Exchange an OAuth authorization code for tokens.

    Body: {
      "provider": "google" | "github",
      "code": "auth_code_from_browser",
      "code_verifier": "pkce_verifier",
      "redirect_uri": "NEXUS://oauth/callback",
      "user_id": "lakshya",
      "state": "optional_csrf_state"
    }
    """
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)

    provider = body.get("provider", "")
    code = body.get("code", "")
    code_verifier = body.get("code_verifier", "")
    redirect_uri = body.get("redirect_uri", OAUTH_REDIRECT_URI)
    user_id = body.get("user_id", "")

    if not provider or not code or not code_verifier or not user_id:
        return JSONResponse({"error": "missing required fields"}, status_code=400)

    try:
        if provider == "google":
            tokens = await _exchange_google(code, code_verifier, redirect_uri)
            db.store_oauth_token(
                user_id, "google",
                tokens["access_token"],
                tokens.get("refresh_token"),
                tokens.get("expires_in", 3600),
                GOOGLE_SCOPES,
            )
        elif provider == "github":
            tokens = await _exchange_github(code, code_verifier, redirect_uri)
            if "access_token" not in tokens:
                return JSONResponse({"error": tokens.get("error", "exchange failed")}, status_code=400)
            db.store_oauth_token(
                user_id, "github",
                tokens["access_token"],
                None,  # GitHub doesn't return refresh tokens for OAuth apps
                0,     # GitHub tokens don't expire
                GITHUB_SCOPES,
            )
        else:
            return JSONResponse({"error": f"unsupported provider: {provider}"}, status_code=400)
    except httpx.HTTPStatusError as e:
        log.exception("OAuth exchange failed for %s", provider)
        return JSONResponse({"error": f"exchange failed: {e.response.text}"}, status_code=502)
    except Exception:
        log.exception("OAuth exchange error")
        return JSONResponse({"error": "internal error"}, status_code=500)

    log.info("OAuth connected: user=%s provider=%s", user_id, provider)
    return JSONResponse({"ok": True, "provider": provider, "connected": True})


@router.post("/oauth/refresh")
async def oauth_refresh(request: Request) -> JSONResponse:
    """Manually trigger a token refresh for a user's provider."""
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)

    user_id = body.get("user_id", "")
    provider = body.get("provider", "")
    token = db.get_oauth_token(user_id, provider)
    if not token or not token.get("refresh_token"):
        return JSONResponse({"error": "no refresh token"}, status_code=404)

    try:
        if provider == "google":
            refreshed = await _refresh_google(token["refresh_token"])
            db.store_oauth_token(
                user_id, provider, refreshed["access_token"],
                token["refresh_token"], refreshed.get("expires_in", 3600), token.get("scopes", ""),
            )
            return JSONResponse({"ok": True, "refreshed": True})
        else:
            return JSONResponse({"error": "provider doesn't support refresh"}, status_code=400)
    except Exception:
        log.exception("refresh failed")
        return JSONResponse({"error": "refresh failed"}, status_code=502)


@router.get("/oauth/status")
async def oauth_status(user_id: str) -> JSONResponse:
    """Check which OAuth providers are connected for a user."""
    tokens = db.get_all_oauth_tokens(user_id)
    connected = {}
    for provider, token in tokens.items():
        connected[provider] = {
            "connected": True,
            "expired": db.is_token_expired(token),
            "scopes": token.get("scopes", ""),
        }
    return JSONResponse({"user_id": user_id, "providers": connected})


@router.delete("/oauth/disconnect")
async def oauth_disconnect(request: Request) -> JSONResponse:
    """Remove a provider's tokens for a user."""
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)
    user_id = body.get("user_id", "")
    provider = body.get("provider", "")
    db.delete_oauth_token(user_id, provider)
    log.info("OAuth disconnected: user=%s provider=%s", user_id, provider)
    return JSONResponse({"ok": True, "disconnected": provider})


# ---- API key management (Claude, Devin, Antigravity, etc.) ----

@router.post("/apikeys/add")
async def add_api_key(request: Request) -> JSONResponse:
    """
    Store an API key for a third-party service.

    Body: {
      "user_id": "lakshya",
      "provider": "claude" | "devin" | "antigravity" | ...,
      "api_key": "sk-..."
    }
    """
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)

    user_id = body.get("user_id", "")
    provider = body.get("provider", "")
    api_key = body.get("api_key", "")

    if not user_id or not provider or not api_key:
        return JSONResponse({"error": "missing required fields"}, status_code=400)

    db.store_api_key(user_id, provider, api_key)
    log.info("API key stored: user=%s provider=%s", user_id, provider)
    return JSONResponse({"ok": True, "provider": provider, "stored": True})


@router.delete("/apikeys/remove")
async def remove_api_key(request: Request) -> JSONResponse:
    """Remove a stored API key."""
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)
    user_id = body.get("user_id", "")
    provider = body.get("provider", "")
    db.delete_api_key(user_id, provider)
    return JSONResponse({"ok": True, "removed": provider})


@router.get("/apikeys/list")
async def list_api_keys(user_id: str) -> JSONResponse:
    """List which API key providers are stored (does NOT return the keys)."""
    keys = db.get_all_api_keys(user_id)
    return JSONResponse({"user_id": user_id, "providers": list(keys.keys())})


# ---- Auth URL generation (for the client to open in browser) ----

@router.get("/oauth/auth-url")
async def get_auth_url(provider: str, user_id: str, code_challenge: str) -> JSONResponse:
    """
    Build the OAuth authorization URL for the client to open in the system browser.
    The client generates the PKCE verifier/challenge, sends us the challenge,
    and we return the full URL to open.

    Query params:
      provider: "google" | "github"
      user_id: the user's ID
      code_challenge: SHA256 hash of the PKCE verifier (S256 method)
    """
    if provider == "google":
        if not GOOGLE_CLIENT_ID:
            return JSONResponse({"error": "Google OAuth not configured"}, status_code=500)
        url = (
            f"https://accounts.google.com/o/oauth2/v2/auth"
            f"?client_id={GOOGLE_CLIENT_ID}"
            f"&redirect_uri={OAUTH_REDIRECT_URI}"
            f"&response_type=code"
            f"&scope={GOOGLE_SCOPES}"
            f"&code_challenge={code_challenge}"
            f"&code_challenge_method=S256"
            f"&state={user_id}"
            f"&access_type=offline"
            f"&prompt=consent"
        )
        return JSONResponse({"url": url, "redirect_uri": OAUTH_REDIRECT_URI})

    elif provider == "github":
        if not GITHUB_CLIENT_ID:
            return JSONResponse({"error": "GitHub OAuth not configured"}, status_code=500)
        url = (
            f"https://github.com/login/oauth/authorize"
            f"?client_id={GITHUB_CLIENT_ID}"
            f"&redirect_uri={OAUTH_REDIRECT_URI}"
            f"&scope={GITHUB_SCOPES}"
            f"&state={user_id}"
        )
        # GitHub OAuth apps don't support PKCE, but we include state for CSRF.
        return JSONResponse({"url": url, "redirect_uri": OAUTH_REDIRECT_URI})

    else:
        return JSONResponse({"error": f"unsupported provider: {provider}"}, status_code=400)


# ---- Device registration ----

@router.post("/device/register")
async def register_device_endpoint(request: Request) -> JSONResponse:
    """
    Register a new device for a user. Called during first-run setup.

    Body: {
      "user_id": "lakshya",
      "device_id": "laptop-abc123",
      "device_token": "optional_existing_token"
    }
    """
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON"}, status_code=400)

    user_id = body.get("user_id", "")
    device_id = body.get("device_id", "")
    device_token = body.get("device_token")

    if not user_id or not device_id:
        return JSONResponse({"error": "user_id and device_id required"}, status_code=400)

    db.register_device(user_id, device_id, device_token)
    log.info("device registered: user=%s device=%s", user_id, device_id)
    return JSONResponse({"ok": True, "user_id": user_id, "device_id": device_id})


@router.get("/device/validate")
async def validate_device_endpoint(user_id: str, device_id: str) -> JSONResponse:
    """Check if a device is registered for a user."""
    valid = db.validate_device(user_id, device_id)
    return JSONResponse({"valid": valid})


# ---- Configuration check ----

@router.get("/config/check")
async def config_check() -> JSONResponse:
    """
    Report which OAuth providers and services are configured.
    Used by the setup page to show available integrations.
    Does NOT expose secrets — only boolean flags.
    """
    return JSONResponse({
        "google": {
            "configured": bool(GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET),
            "scopes": GOOGLE_SCOPES if GOOGLE_CLIENT_ID else "",
        },
        "github": {
            "configured": bool(GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET),
            "scopes": GITHUB_SCOPES if GITHUB_CLIENT_ID else "",
        },
        "redirect_uri": OAUTH_REDIRECT_URI,
    })


# ---- Credential handoff for n8n ───────────────────────────────────
#
# n8n calls this endpoint to get a short-lived access token for a specific
# provider + user. The sidecar refreshes the token if expired, then returns
# it. n8n uses the token for ONE API call, then discards it.
#
# Security:
#   - The sidecar and n8n run on the SAME server, so this is localhost-only.
#   - The token is already short-lived (Google: 1 hour, GitHub: 8 hours).
#   - n8n should NOT log or persist the access_token.
#   - The request_id is for audit logging — correlate with the original
#     NEXUS request so you can trace which n8n workflow accessed which
#     credential for which user request.

@router.post("/auth/{provider}")
async def get_credential_for_n8n(provider: str, request: Request) -> JSONResponse:
    """
    Credential handoff endpoint — called by n8n to get an access token
    for a specific provider and user.

    Body:
    {
      "user_id": "user_a1b2c3d4...",
      "request_id": "req_xxx",          // optional, for audit logging
      "scopes": ["repo"]                // optional, for scope validation
    }

    Response (200):
    {
      "provider": "github",
      "access_token": "gho_...",
      "account_id": "chitkul-lakshya",
      "scopes": "repo read:org workflow",
      "expires_in": 28800,              // seconds until expiry (0 = no expiry)
      "token_type": "bearer"
    }

    Response (404): user hasn't connected this provider
    Response (403): token expired and can't be refreshed
    Response (400): missing user_id
    """
    try:
        body = await request.json()
    except Exception:
        return JSONResponse({"error": "invalid JSON body"}, status_code=400)

    user_id = body.get("user_id", "")
    request_id = body.get("request_id", "")
    requested_scopes = body.get("scopes", [])

    if not user_id:
        return JSONResponse({"error": "user_id is required"}, status_code=400)

    if provider not in ("google", "github"):
        return JSONResponse({"error": f"unsupported provider: {provider}"}, status_code=400)

    # Look up the stored token
    token = db.get_oauth_token(user_id, provider)
    if not token:
        log.warning(
            "credential handoff FAILED: user=%s provider=%s not connected (request_id=%s)",
            user_id, provider, request_id,
        )
        return JSONResponse(
            {"error": f"{provider} not connected for this user", "provider": provider},
            status_code=404,
        )

    # Refresh if expired (Google only — GitHub tokens don't expire)
    access_token = token["access_token"]
    expires_in = 0

    if db.is_token_expired(token):
        if provider == "google" and token.get("refresh_token"):
            try:
                refreshed = await _refresh_google(token["refresh_token"])
                access_token = refreshed["access_token"]
                expires_in = refreshed.get("expires_in", 3600)
                db.store_oauth_token(
                    user_id, provider, access_token,
                    token["refresh_token"], expires_in, token.get("scopes", ""),
                )
                log.info(
                    "credential handoff: refreshed %s token for user=%s (request_id=%s)",
                    provider, user_id, request_id,
                )
            except Exception:
                log.exception(
                    "credential handoff FAILED: refresh error for %s user=%s (request_id=%s)",
                    provider, user_id, request_id,
                )
                return JSONResponse(
                    {"error": f"token expired and refresh failed for {provider}"},
                    status_code=403,
                )
        else:
            log.warning(
                "credential handoff FAILED: %s token expired, no refresh token (request_id=%s)",
                provider, request_id,
            )
            return JSONResponse(
                {"error": f"{provider} token expired and cannot be refreshed"},
                status_code=403,
            )
    else:
        # Calculate remaining lifetime
        stored_expiry = token.get("expires_at", 0)
        if stored_expiry:
            import time as _time
            expires_in = max(0, int(stored_expiry - _time.time()))

    # Resolve account_id (GitHub login or Google email)
    account_id = token.get("account_id", user_id)

    log.info(
        "credential handoff OK: provider=%s user=%s account=%s request_id=%s",
        provider, user_id, account_id, request_id,
    )

    return JSONResponse({
        "provider": provider,
        "access_token": access_token,
        "account_id": account_id,
        "scopes": token.get("scopes", ""),
        "expires_in": expires_in,
        "token_type": "bearer",
    })
