# Industry Patterns — MCP Connection & Auth UX (2026-09-20)

How six industry products handle the exact moment NEXUS handles worst:
the user needs a connector that is down, expired, or never configured.
Sources: vendor docs, MCP spec site, GitHub issue trackers, vendor blogs
(all fetched September 2026).

---

## 1. Composio — "agent hands you a link and waits" (closest to our need)

Composio's gateway (1000+ apps behind one MCP URL,
`connect.composio.dev/mcp`) exposes 7 meta-tools instead of per-app tools.
The connection pattern (`docs.composio.dev/docs/composio-connect`):

- Agent needs an app → `COMPOSIO_MANAGE_CONNECTIONS` generates an OAuth
  link → user approves in browser → connection persists across sessions.
- Dedicated primitive: **`COMPOSIO_WAIT_FOR_CONNECTIONS` — wait for the
  user to complete OAuth before the agent continues.** The interrupted
  task resumes after completion; it does not need re-asking.
- Links are short-lived; on expiry the agent generates a fresh one on retry.
- Token refresh runs in the background so sessions never break mid-task.
- Pre-connect is supported ("ask your agent to start the connection"),
  but the default is **lazy: connect at the moment of need.**

**Transferable rules:** (a) connect lazily at failure time, not in a
settings pilgrimage; (b) the pending task must auto-resume on completion;
(c) refresh must be silent; (d) expired links regenerate on retry, never
dead-end.

## 2. Zapier MCP — zero-config OAuth + token fallback (validates vault)

`docs.zapier.com/mcp/overview/how-connections-work` (2026-09-08):

- Default: **OAuth from inside the client.** Zapier provisions the server
  during sign-in; the user never hand-builds a server entry. The **client
  stores and refreshes the OAuth token**; revoke from either side.
- Fallback: long-lived **connection token**, header-form preferred
  (`Authorization: Bearer …`) — explicitly because URL params leak into
  logs, shell history, and committed configs.
- Transport: Streamable HTTP only (matches our `call_tool` POST design).

**Transferable rules:** (a) our vault IS the "client stores the token"
piece — keep it, extend refresh to every OAuth service; (b) paste-token
is a legitimate permanent fallback, not a hack; (c) tokens must never
travel in URLs or logs (our `mcp_audit.jsonl` records
server/tool/ok/latency only — pin this with a test when building).

## 3. Claude family — status-first, inline re-auth (validates card system)

- **Claude Code:** `claude mcp list` prints per-server health —
  `✔ Connected`, `! Needs authentication`, `⏸ Pending approval`,
  `✘ Failed` — and `/mcp` → **Reauthenticate** re-runs OAuth inline with
  no restart. On 401 it refreshes, reconnects, and retries once before
  flagging (`calmara.app/blog`, Aug 2026, reproduced against live servers).
- **Claude.ai/Desktop connectors:** connector list with Connect buttons;
  documented recovery is "disconnect and reconnect from Connectors" (fresh
  OAuth). Remote connectors run from Anthropic's cloud, so the checklist
  is reachability + token validity — same two axes as our state machine.
- **Failure modes to design around** (issue trackers, 2026): silent
  mid-session drops of OAuth connectors (refresh not attempted
  proactively — the fix is *proactive* refresh, which our idle monitor
  can do); concurrent-session refresh races (single-writer refresh in
  the vault — our `parking_lot` locks already serialize this).

**Transferable rules:** (a) per-connector status with the four states is
the industry standard shape — our state machine should use exactly these
semantics; (b) re-auth must be one click *where the failure is seen*;
(c) refresh proactively, not on next failure.

## 4. Cursor / VS Code — the status-dashboard standard

- **Cursor Settings → MCP tab:** green connected / yellow starting / red
  disconnected per server, **tool counts**, error details, click-to-expand;
  CLI mirrors it (`agent mcp list`, `agent mcp login <id>` with automatic
  callback handling, `enable/disable`). Auth via env keys or OAuth.
- **VS Code:** per-server start/stop, first-start trust-confirm dialog,
  error indicator in Chat view → "Show Output" jumps to server logs.

**Transferable rules:** our Connections tab needs four additions to meet
the standard: per-server enable/disable toggle, tool counts from
`tools/list`, one-click log view on failure, per-server login action.
(Status colors + Recheck already exist.)

## 5. ChatGPT — Connect Link + Developer Mode (validates link flow)

Composio's ChatGPT guide: Developer Mode → paste managed MCP URL →
**Connect Link** (secure auth URL, one click, credentials persist for all
future sessions) → verify by asking a data question. Refresh in
background; free-tier limits on plans without Developer Mode.

**Transferable rule:** the OAuth link itself is the UX — one URL, browser
handoff, automatic return. Our `shell.open` + `nexus://oauth/` deep-link
+ status polling (`setup/oauth.ts:130-201`) is this exact pattern,
already proven for Google/GitHub.

## 6. Synthesis — the five universal rules

1. **Status per connector, four states** (connected / starting /
   needs-auth / failed) — visible without opening settings.
2. **Fix action where the failure is seen** (card/inline, not a settings
   pilgrimage). Re-auth is one click.
3. **Lazy connect at moment of need** + auto-resume of the interrupted
   task on completion.
4. **Silent refresh** so expiry rarely surfaces; proactive probing so
   rotation/death warns *before* the next command.
5. **Tokens in vaults/headers, never in URLs/logs**; revoke from either side.
