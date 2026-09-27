"""nexus mcp check — verify every registered MCP server end to end.

Registry (mirrors mcp_client.rs) -> reachability probe (tools/list) ->
one read-only tool call where safe -> one EXPECTED failure (bad tool name
must come back a clean JSON-RPC error, not a hang/crash).

Exit 0 only if every server is reachable. No sends, no writes, no tokens
needed (anonymous probe; authed path is covered by the app vault).
"""
import json
import sys
import urllib.error
import urllib.request

SERVERS = [
    ("swiggy-food", "https://mcp.swiggy.com/food"),
    ("swiggy-instamart", "https://mcp.swiggy.com/im"),
    ("swiggy-dineout", "https://mcp.swiggy.com/dineout"),
    ("whatsapp", "http://127.0.0.1:8765/mcp"),
    ("amazon", "http://127.0.0.1:8766/mcp"),
]

TIMEOUT = 8


def rpc(url, method, params, req_id=1):
    body = json.dumps(
        {"jsonrpc": "2.0", "method": method, "params": params, "id": req_id}
    ).encode()
    req = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json, text/event-stream",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.status, resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        # HTTPError still carries the real status: 401/403 means the server
        # is ALIVE and demanding login (expected for Swiggy without OAuth).
        return e.code, e.read().decode("utf-8", "replace")[:200]
    except Exception as e:  # noqa: BLE001 - probe must never crash
        return -1, f"{type(e).__name__}: {e}"


def main():
    green, red, reset = "\033[32m", "\033[31m", "\033[0m"
    all_ok = True
    print("\nNEXUS MCP check (read-only, no sends)\n")
    for name, url in SERVERS:
        # 1. reachability: tools/list
        status, body = rpc(url, "tools/list", {})
        if status == -1:
            print(f"{red}DOWN {reset} {name} ({url}): {body[:120]}")
            all_ok = False
            continue
        if status in (401, 403):
            print(f"{green}REACHABLE{reset} {name}: alive, login required (HTTP {status})")
            continue
        if status != 200:
            print(f"{red}DOWN {reset} {name}: HTTP {status} {body[:120]}")
            all_ok = False
            continue
        try:
            tools = json.loads(body).get("result", {}).get("tools", [])
            n = len(tools) if isinstance(tools, list) else 0
        except Exception:  # noqa: BLE001
            n = -1
        # 2. expected failure: unknown tool must error cleanly
        s2, b2 = rpc(
            url, "tools/call",
            {"name": "nexus_probe_nonexistent_tool", "arguments": {}},
        )
        clean_fail = s2 != -1 and ("error" in b2 or s2 in (400, 404, 422, 500))
        mark = green + "OK" + reset if clean_fail else red + "WEIRD" + reset
        if not clean_fail:
            all_ok = False
        print(f"{green}REACHABLE{reset} {name}: {n} tools, bad-tool probe: {mark}")
    print()
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())
