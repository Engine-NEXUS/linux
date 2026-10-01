# Department: WhatsApp Pairing (2026-09-20)

**Goal:** user says "send a WhatsApp message" with the bridge down or
unpaired → NEXUS requests connection, shows why, and shows a scannable
QR plus setup details in the sidebar. This is also WhatsApp's native UX
(Linked Devices = QR pairing); anything without a QR is broken by definition.

---

## 1. Bridge mechanics (live research: github.com/Sealjay/mcp-whatsapp)

- Single Go binary (~4 MB, MIT, whatsmeow protocol). `serve` =
  long-running HTTP daemon on `127.0.0.1:8765`: MCP at `/mcp`, pairing UI
  at `/pair`, SQLite cache + `store/whatsapp.db` session, VCF import,
  42 tools. `login` = headless pairing (terminal QR). `smoke` = boot test.
- **First-run pairing is a browser page:** start daemon →
  `http://127.0.0.1:8765/pair` → scan with phone (WhatsApp → Settings →
  Linked Devices → Link a Device). No terminal required.
- **v0.4.0 `pairing_status` MCP tool:** returns a structured
  `setup_state` envelope — `ready` / `awaiting_qr` **with the QR payload**
  / `error` — explicitly so *"MCP clients can poll it to drive their own
  pairing UI instead of shelling out to the `/pair` web page."*
- **Session rotation ≈ every 20 days:** WhatsApp invalidates the linked
  session; `/pair` then serves a fresh QR automatically. This is a
  *scheduled* breakage — design for it, don't just handle it.
- Rate limits (`/pair/*`: 5 GET/min, 1 POST/min on reset) + CSRF
  protection: poll gently (≤1/min idle, faster only while the card is open).
- Failure strings today: `connect failed …` = daemon unpaired → re-pair;
  device-limit = remove a device on the phone.
- ToS risk (from our own `docs/9router-research/05-...`: whatsmeow is
  unofficial — **burner-number warning belongs on the card.**
  Alternatives catalogued there: `wappmcp` (web.js QR), `kahflane`
  (87 tools, anti-ban pacing), official Cloud API (Business account).

## 2. Approach comparison (vs our codebase)

| Option | How QR reaches the user | Verdict |
|---|---|---|
| A. Scrape bridge stdout | Fragile, version-coupled, needs a console window | REJECTED |
| B. `shell.open /pair` only | Zero NEXUS code; QR in user's browser, no in-app state | Fallback only — no polling, no ready-detection, no voice confirm |
| **C. Poll `pairing_status`, render QR payload in our card** | First-party structured data; NEXUS owns state → voice + card update together | **ADOPT** — matches the tool's stated purpose |
| D. Re-implement whatsmeow pairing in Rust | Full control, no bridge binary | REJECTED — months of protocol work, duplicates the binary |

**Adopted flow (C, B as fallback):**
1. Failure class detected: down (TCP refused) / unpaired (`awaiting_qr`)
   / session-dead (401/`error`) / never-configured (no binary path set).
2. First failure per session → sidebar Connect card: status line, 3
   numbered steps (run/start bridge → scan QR → auto-confirm), QR image
   from the tool payload, "Open /pair in browser" button, burner warning,
   device-limit note.
3. While card open: poll `pairing_status` (5s) → on `ready`: vault/flag
   `ready`, speak *"WhatsApp is connected, sir"*, close card,
   **auto-retry the original failed call** (pending-request resume —
   Composio `WAIT_FOR_CONNECTIONS` pattern, our `PENDING_COMPOUND`
   machinery supports the shape).
4. Bridge not running at all → card's step 1 becomes "Start bridge"
   (managed child process with configured binary path; log-view button).
5. Idle monitor probes `pairing_status` ≤1/min → on rotation-expiry,
   proactive card/toast *before* the next send fails ("session rotated —
   fresh QR ready").

## 3. Voice lines

- Down: *"WhatsApp isn't connected, sir — I've opened the setup card with
  the QR. Scan it with your phone and I'll confirm."*
- Session dead: *"Your WhatsApp session expired, sir — the card has a
  fresh QR."*
- Never-configured: *"WhatsApp isn't set up yet, sir — the card shows the
  three steps."*

## 4. Tests to pin

- `awaiting_qr` payload renders (unit, no bridge).
- Poll loop transitions `awaiting_qr → ready` → confirmation spoken +
  original call retried exactly once.
- Rate-limit respected (no >5 GET/min to `/pair/*` paths).
- Rotation probe flips state without a user command in flight.
