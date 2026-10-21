#!/usr/bin/env python3
"""
NEXUS Instagram Bridge Client
Uses `instagrapi` for automated account block and unblock actions with session caching.

Usage:
  python scripts/instagram_client.py block --user <username> [--mock]
  python scripts/instagram_client.py unblock --user <username> [--mock]
  python scripts/instagram_client.py login --user <username> --password <password>
  python scripts/instagram_client.py status
"""

import argparse
import json
import os
import sys
from pathlib import Path

SESSION_DIR = Path(os.environ.get("APPDATA", str(Path.home() / ".config"))) / "NEXUS"
SESSION_FILE = SESSION_DIR / "instagram_session.json"


def get_client(mock: bool = False):
    if mock:
        return None
    try:
        from instagrapi import Client
        cl = Client()
        if SESSION_FILE.exists():
            try:
                cl.load_settings(str(SESSION_FILE))
            except Exception as e:
                sys.stderr.write(f"Warning: Failed to load session: {e}\n")
        return cl
    except ImportError:
        if not mock:
            sys.stderr.write("Error: instagrapi is not installed. Install with: pip install instagrapi\n")
            sys.exit(1)
        return None


def do_login(username: str, password: str) -> dict:
    from instagrapi import Client
    cl = Client()
    try:
        cl.login(username, password)
        SESSION_DIR.mkdir(parents=True, exist_ok=True)
        cl.dump_settings(str(SESSION_FILE))
        return {"success": True, "logged_in": True, "username": username}
    except Exception as e:
        return {"success": False, "error": str(e)}


def do_block(target_user: str, mock: bool = False) -> dict:
    if mock:
        return {"success": True, "mock": True, "action": "block", "user": target_user}

    cl = get_client(mock)
    if not SESSION_FILE.exists():
        return {"success": False, "error": "not_authenticated", "message": "No session found. Run login first."}

    try:
        user_id = cl.user_id_from_username(target_user)
        result = cl.user_block(user_id)
        return {"success": bool(result), "action": "block", "user": target_user, "user_id": str(user_id)}
    except Exception as e:
        return {"success": False, "action": "block", "user": target_user, "error": str(e)}


def do_unblock(target_user: str, mock: bool = False) -> dict:
    if mock:
        return {"success": True, "mock": True, "action": "unblock", "user": target_user}

    cl = get_client(mock)
    if not SESSION_FILE.exists():
        return {"success": False, "error": "not_authenticated", "message": "No session found. Run login first."}

    try:
        user_id = cl.user_id_from_username(target_user)
        result = cl.user_unblock(user_id)
        return {"success": bool(result), "action": "unblock", "user": target_user, "user_id": str(user_id)}
    except Exception as e:
        return {"success": False, "action": "unblock", "user": target_user, "error": str(e)}


def check_status() -> dict:
    has_session = SESSION_FILE.exists()
    return {
        "installed": True,
        "has_session": has_session,
        "session_path": str(SESSION_FILE)
    }


def main():
    parser = argparse.ArgumentParser(description="NEXUS Instagram Bridge")
    subparsers = parser.add_subparsers(dest="command", required=True)

    # Login
    login_p = subparsers.add_parser("login")
    login_p.add_argument("--user", required=True, help="Your Instagram username")
    login_p.add_argument("--password", required=True, help="Your Instagram password")

    # Block
    block_p = subparsers.add_parser("block")
    block_p.add_argument("--user", required=True, help="Target username to block")
    block_p.add_argument("--mock", action="store_true", help="Dry run simulation")

    # Unblock
    unblock_p = subparsers.add_parser("unblock")
    unblock_p.add_argument("--user", required=True, help="Target username to unblock")
    unblock_p.add_argument("--mock", action="store_true", help="Dry run simulation")

    # Status
    subparsers.add_parser("status")

    args = parser.parse_args()

    if args.command == "login":
        res = do_login(args.user, args.password)
    elif args.command == "block":
        res = do_block(args.user, mock=args.mock)
    elif args.command == "unblock":
        res = do_unblock(args.user, mock=args.mock)
    elif args.command == "status":
        res = check_status()
    else:
        res = {"success": False, "error": "unknown_command"}

    print(json.dumps(res, indent=2))
    sys.exit(0 if res.get("success", True) else 1)


if __name__ == "__main__":
    main()
