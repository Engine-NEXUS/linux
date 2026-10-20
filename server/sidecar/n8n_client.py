"""
NEXUS Worker client — replaces the n8n supervisor client.

Calls the Cloudflare Worker with the transcript + structured identity +
credentials. The Worker classifies intent, routes to the appropriate
handler, calls external APIs, and returns result text.

Architecture:
  sidecar → Cloudflare Worker (edge, <5ms cold start) → API calls → text

The Worker is stateless (V8 isolate). Credentials are sent in the payload,
used for one API call, then garbage-collected. They are never persisted
or logged by the Worker.

Workers AI (free tier: 10K neurons/day) handles intent classification
and result summarization. No external LLM API keys are needed.
"""

from __future__ import annotations

import os
import json
import logging
from typing import AsyncIterator

import httpx

log = logging.getLogger("NEXUS.sidecar.worker")

# The Cloudflare Worker URL. Set this in .env after deploying the Worker.
# Default to local wrangler dev for development.
NEXUS_WORKER_URL = os.getenv("NEXUS_WORKER_URL", "http://localhost:8785")
# Optional bearer token for authenticating to the Worker (if you set up
# a custom route with auth). Usually empty — the Worker URL is unguessable.
NEXUS_WORKER_TOKEN = os.getenv("NEXUS_WORKER_TOKEN", "")


async def call_worker(
    session_id: str,
    user_id: str,
    device_id: str,
    transcript: str,
    credentials: dict,
) -> str:
    """
    Call the Cloudflare Worker and return the result text.

    Sends structured identity + task + credentials. The Worker:
      1. Classifies intent (Workers AI, <50ms)
      2. Routes to the appropriate handler (GitHub, Gmail, Calendar, etc.)
      3. Calls external APIs using the provided credentials
      4. Summarizes the result (Workers AI)
      5. Returns text

    Args:
        session_id: The NEXUS session ID for correlation.
        user_id: The user's ID (for credential lookup).
        device_id: The device ID.
        transcript: The transcribed user speech.
        credentials: The user's OAuth tokens + API keys from get_valid_credentials().

    Returns:
        The final response text from the Worker.
    """
    # Build the credential payload — only include access tokens, not refresh tokens.
    # The Worker is stateless and uses these for one API call, then discards them.
    worker_credentials: dict = {}

    google_cred = credentials.get("google")
    if google_cred and google_cred.get("access_token"):
        worker_credentials["google"] = {
            "access_token": google_cred["access_token"],
            "scopes": google_cred.get("scopes", ""),
        }

    github_cred = credentials.get("github")
    if github_cred and github_cred.get("access_token"):
        worker_credentials["github"] = {
            "access_token": github_cred["access_token"],
        }

    api_keys = credentials.get("api_keys", {})
    if api_keys:
        worker_credentials["api_keys"] = dict(api_keys)

    # Determine which providers are connected
    connected_providers = list(worker_credentials.keys())
    if "api_keys" in connected_providers:
        connected_providers.remove("api_keys")

    payload = {
        "request_id": session_id,
        "requester": {
            "id": user_id,
            "device_id": device_id,
        },
        "task": {
            "type": "general",  # the Worker classifies this
            "request": transcript,
        },
        "authorization": {
            "providers": connected_providers,
            "credential_endpoint": "",  # not used — credentials are in the payload
            "credentials": worker_credentials,
        },
    }

    headers = {"Content-Type": "application/json"}
    if NEXUS_WORKER_TOKEN:
        headers["Authorization"] = f"Bearer {NEXUS_WORKER_TOKEN}"

    try:
        async with httpx.AsyncClient(timeout=60.0) as client:
            resp = await client.post(NEXUS_WORKER_URL, json=payload, headers=headers)
            resp.raise_for_status()
            data = resp.json()

            if isinstance(data, dict):
                # Extract the reply text from the Worker response
                return (
                    data.get("reply_text")
                    or data.get("text")
                    or data.get("content")
                    or data.get("response")
                    or json.dumps(data)
                )
            elif isinstance(data, str):
                return data
            else:
                return str(data)
    except httpx.HTTPStatusError as e:
        log.exception("Worker returned error: %s", e.response.text)
        return f"Sorry, I couldn't reach the workflow server. Error: {e.response.status_code}"
    except Exception:
        log.exception("Worker call failed")
        return "Sorry, the workflow server is unavailable."


# Keep the old name for backward compatibility with sidecar.py imports.
call_supervisor = call_worker
