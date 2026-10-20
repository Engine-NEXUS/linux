"""
SQLite database for OAuth tokens and API keys.

Stores per-user credentials that the sidecar injects into n8n webhook payloads.
Tokens are encrypted at rest using Fernet (symmetric encryption).

Schema:
  oauth_tokens(user_id, provider, access_token, refresh_token, expires_at, scopes, created_at)
  api_keys(user_id, provider, key_encrypted, created_at)
"""

from __future__ import annotations

import os
import time
import sqlite3
import logging
from contextlib import contextmanager
from typing import Optional

from cryptography.fernet import Fernet

log = logging.getLogger("NEXUS.sidecar.db")

DB_PATH = os.getenv("NEXUS_DB_PATH", "NEXUS_credentials.db")
# Encryption key for API keys at rest. Generate once: Fernet.generate_key()
# Store in env var. If not set, we generate one (tokens won't survive restart).
ENCRYPTION_KEY = os.getenv("NEXUS_ENCRYPTION_KEY", "")
_fernet: Optional[Fernet] = None


def _get_fernet() -> Fernet:
    global _fernet
    if _fernet is None:
        if ENCRYPTION_KEY:
            _fernet = Fernet(ENCRYPTION_KEY.encode())
        else:
            key = Fernet.generate_key()
            _fernet = Fernet(key)
            log.warning("NEXUS_ENCRYPTION_KEY not set — generated ephemeral key; API keys won't survive restart")
    return _fernet


def init_db() -> None:
    """Create tables if they don't exist. Called once on startup."""
    with _cursor() as cur:
        cur.execute("""
            CREATE TABLE IF NOT EXISTS oauth_tokens (
                user_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                access_token TEXT NOT NULL,
                refresh_token TEXT,
                expires_at REAL,
                scopes TEXT,
                created_at REAL NOT NULL,
                PRIMARY KEY (user_id, provider)
            )
        """)
        cur.execute("""
            CREATE TABLE IF NOT EXISTS api_keys (
                user_id TEXT NOT NULL,
                provider TEXT NOT NULL,
                key_encrypted TEXT NOT NULL,
                created_at REAL NOT NULL,
                PRIMARY KEY (user_id, provider)
            )
        """)
        cur.execute("""
            CREATE TABLE IF NOT EXISTS user_devices (
                user_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                device_token TEXT,
                created_at REAL NOT NULL,
                PRIMARY KEY (user_id, device_id)
            )
        """)
    log.info("database initialized at %s", DB_PATH)


@contextmanager
def _cursor():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    try:
        cur = conn.cursor()
        yield cur
        conn.commit()
    finally:
        conn.close()


# ---- OAuth tokens ----

def store_oauth_token(
    user_id: str,
    provider: str,
    access_token: str,
    refresh_token: Optional[str],
    expires_in: int,
    scopes: str,
) -> None:
    expires_at = time.time() + expires_in if expires_in else 0
    with _cursor() as cur:
        cur.execute(
            """INSERT OR REPLACE INTO oauth_tokens
               (user_id, provider, access_token, refresh_token, expires_at, scopes, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)""",
            (user_id, provider, access_token, refresh_token, expires_at, scopes, time.time()),
        )


def get_oauth_token(user_id: str, provider: str) -> Optional[dict]:
    with _cursor() as cur:
        cur.execute(
            "SELECT * FROM oauth_tokens WHERE user_id=? AND provider=?",
            (user_id, provider),
        )
        row = cur.fetchone()
    if row is None:
        return None
    return {
        "access_token": row["access_token"],
        "refresh_token": row["refresh_token"],
        "expires_at": row["expires_at"],
        "scopes": row["scopes"],
    }


def get_all_oauth_tokens(user_id: str) -> dict:
    """Return all connected OAuth tokens for a user, keyed by provider."""
    with _cursor() as cur:
        cur.execute("SELECT * FROM oauth_tokens WHERE user_id=?", (user_id,))
        rows = cur.fetchall()
    result = {}
    for row in rows:
        result[row["provider"]] = {
            "access_token": row["access_token"],
            "refresh_token": row["refresh_token"],
            "expires_at": row["expires_at"],
            "scopes": row["scopes"],
        }
    return result


def delete_oauth_token(user_id: str, provider: str) -> None:
    with _cursor() as cur:
        cur.execute(
            "DELETE FROM oauth_tokens WHERE user_id=? AND provider=?",
            (user_id, provider),
        )


def is_token_expired(token: dict) -> bool:
    if not token or not token.get("expires_at"):
        return False  # no expiry info, assume valid
    return time.time() > token["expires_at"] - 60  # 60s buffer


# ---- API keys ----

def store_api_key(user_id: str, provider: str, api_key: str) -> None:
    encrypted = _get_fernet().encrypt(api_key.encode()).decode()
    with _cursor() as cur:
        cur.execute(
            """INSERT OR REPLACE INTO api_keys
               (user_id, provider, key_encrypted, created_at)
               VALUES (?, ?, ?, ?)""",
            (user_id, provider, encrypted, time.time()),
        )


def get_api_key(user_id: str, provider: str) -> Optional[str]:
    with _cursor() as cur:
        cur.execute(
            "SELECT key_encrypted FROM api_keys WHERE user_id=? AND provider=?",
            (user_id, provider),
        )
        row = cur.fetchone()
    if row is None:
        return None
    return _get_fernet().decrypt(row["key_encrypted"].encode()).decode()


def get_all_api_keys(user_id: str) -> dict:
    """Return all stored API keys for a user, keyed by provider."""
    with _cursor() as cur:
        cur.execute("SELECT provider, key_encrypted FROM api_keys WHERE user_id=?", (user_id,))
        rows = cur.fetchall()
    result = {}
    for row in rows:
        result[row["provider"]] = _get_fernet().decrypt(row["key_encrypted"].encode()).decode()
    return result


def delete_api_key(user_id: str, provider: str) -> None:
    with _cursor() as cur:
        cur.execute(
            "DELETE FROM api_keys WHERE user_id=? AND provider=?",
            (user_id, provider),
        )


# ---- Device registration ----

def register_device(user_id: str, device_id: str, device_token: Optional[str] = None) -> None:
    with _cursor() as cur:
        cur.execute(
            """INSERT OR REPLACE INTO user_devices
               (user_id, device_id, device_token, created_at)
               VALUES (?, ?, ?, ?)""",
            (user_id, device_id, device_token, time.time()),
        )


def validate_device(user_id: str, device_id: str) -> bool:
    with _cursor() as cur:
        cur.execute(
            "SELECT 1 FROM user_devices WHERE user_id=? AND device_id=?",
            (user_id, device_id),
        )
        return cur.fetchone() is not None
