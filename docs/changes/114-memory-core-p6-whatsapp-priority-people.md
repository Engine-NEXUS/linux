# 114 - Memory Core P6: WhatsApp priority people (read-only, OFF by default)

## What it does
- Polls the local WhatsApp MCP bridge (`127.0.0.1:8765`) and tells you when a
  **priority person** messages: "Asha sent you a message, sir. Say read that
  message to hear it." The message text is **not** spoken in the alert.
- **Priority is learned**, not configured: from how many messages you exchange
  both ways, how often you reply within an hour, how recent the chat is and
  whether it is two-way. Message **text is never used** for priority - only counts
  and timestamps. High >= 70, Medium >= 50; below that it stays silent. Groups
  are ignored. A one-sided sender (a shop) is down-weighted (x0.25 frequency).
- You can override: "make Asha a VIP", "mute WhatsApp alerts from Raj",
  "unmute WhatsApp alerts from Raj", "remove Raj from my VIPs", "who are my
  priority people". "That's not important" mutes the **most recent** alert,
  whether it was mail or WhatsApp. Same actions as buttons on the Memory page.
- "Read that message" / "read Asha's messages" / "what did Raj say on WhatsApp":
  shows the recent conversation in the sidebar (local). It speaks the text only if
  `memcoreWhatsappSpeak` is on (default **off**, because the voice service
  (Edge-TTS) is a cloud service and would receive the message text).

## The guarantees (each in code + tests)
1. **Read-only.** All calls go through `wa::read_tool` (allowlist: `list_chats`,
   `list_messages`, `get_chat`, `search_contacts`, `pairing_status`, `get_status`).
   `mark_read`, `mark_chat_read`, `send_presence`, `send_typing` (and friends) are also
   refused for **every** caller in `mcp_client::call_tool` before any network I/O.
   A mock-bridge test proves only `tools/list`, `list_chats`, `list_messages` reach
   the wire, even though the mock advertises `mark_read`/`send_presence`.
2. **Message text is data.** Clipped, PII-redacted (`gist`), stored only as a 7-day
   untrusted preview, never in the cloud pack, never used for scoring.
3. **Quiet start.** First sight of a chat learns statistics and says nothing.
4. **Store lock never crosses an `.await`** (plan / fetch / apply poll).
5. **Everything local and erasable.** People and previews live in the sealed
   Memory Core store; "forget everything" deletes them and the poll cursors.

## Files
- New: `memcore/people.rs` (scoring, name lookup, storage), `memcore/wa.rs`
  (parsers, bridge adapter, poll, alerts, read/selftest, people commands).
- `mcp_client.rs`: central `blocked_reason` guard, `TEST_WHATSAPP_URL`,
  `extract_text_full`; **`parse_pairing_state` now reads `structuredContent` and JSON
  inside text content blocks** (it previously returned "unrecognized" for those).
- `memcore/store.rs`: `Tier::Person`, `Tier::Chat` (30-day ring, capped).
- `memcore/mod.rs`: `store_handle`, `Status` gained `whatsapp`, `whatsapp_speak`, `people`.
- `commands.rs` / `lib.rs`: settings `memcore_whatsapp` and `memcore_whatsapp_speak`
  (both default **false**), IPC `memcore_people`, `memcore_person_flag`,
  `whatsapp_selftest`.
- Intents `whatsapp_read`, `people_flag`, `people_list` (parser, centre, orchestrator,
  `frontend/src/intent/parser.ts`). Memory page: switches, "Check WhatsApp
  connection", people list with VIP / Mute.

## Bug found and fixed while building
- VIP / mute could be **set but not cleared**: the store refuses to let a mined
  (`Derived`) write unpin a record. Flag changes are now written as user statements
  (`people::save_user_choice`); a test proves un-muting persists.

## Verify
See AGENTS.md entry for the counts. Mock-bridge tests, parser tests with misfire
guards ("mute", "unmute the mic", "read this", "make a note", ...), a pairing-parser
regression test, frontend model tests.

## NOT verified / honest limits
- **Never run against the real bridge.** It is not running on this PC. The list
  tools' JSON format is not documented; the parsers are tolerant and
  `whatsapp_selftest` reports what it could and could not understand.
- **Whether the bridge's read calls send read receipts or change presence is
  unknown.** That is why the switch is off. Do `docs/testing/whatsapp-read-receipt-test.md` first.
- Replying from the **phone** marks the chat read; NEXUS cannot control that.
- The alert speaks the sender's **name** through the cloud voice service (as mail
  alerts do for sender labels). Turn alerts off if that is not acceptable.
- Not built: "mark read after you reply" (stays impossible by design), call-frequency
  signal, chat previews in the boot briefing.
- WhatsApp may ban numbers that use unofficial clients; the session needs re-pairing
  about every 20 days.
