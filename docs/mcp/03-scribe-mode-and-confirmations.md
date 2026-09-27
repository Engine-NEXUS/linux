# Scribe Mode, Contacts, Confirmations (2026-09-18)

## Mode name: Scribe mode

`nexus scribe [for <contact> on <channel>]`. Exit: `send it`, `discard it`,
30s silence, or `command mode`. Orb shows a pen state while active so utterances
visibly go to the draft. Runner-up was "compose mode".

## Text-vs-command rule (the core design)

In scribe mode, **default-to-text**. Only an exact allowlist acts as commands:

- `send it` (+ filler tolerance: `ok/perfect/yeah send it`) → send + exit.
- `scratch that` → drop last chunk. `new line`, `read it back`,
  `discard it`, `polish it`, `change X to Y`, `type "<reserved>"` (escape hatch).
- Everything else — including *"can u send it to me"* — is appended as text.
- Pause <1.2s → keep listening; pause >1.2s → soft-commit chunk.
- Live-typing streams into the draft + target box while speaking.
- Commit-time polish fixes capitalization/punctuation only, never rewords
  (full reword only on explicit `polish it`).
- Risky chunks (containing allowlist-adjacent words like "send") are shown
  on the orb; read-aloud only if `readback:true`.

Sources: Talon command/dictation modes + `escape`, Dragon `scratch/correct
that`, Talon anchor `revise/insert` (phase 2).

## Profile memory (`profile.json`, local)

Durable facts (name, roll number, college…): learned by being told
(`remember …` intent) or observed (seen 3+ times → one-tap confirm to save).
Auto-fills signatures (health-leave email pulls name/roll/college).
Deletable by voice (`forget my roll number`). Model: Leon layered memory,
local-only.

## Contacts (`contacts.json` unified)

Google People API cache (nightly) + WhatsApp names + VCF import:
`name → { whatsapp, emails[], photo, known_default }`.

- 1 contact → 1 email, known: `send it` sends immediately (mapping on orb).
- 1 contact → N emails, no default: numbered card (photo + addresses) →
  `the 3rd` / `the college one` → **remembered as default** (+ confirm-learn).
- 1 contact → N emails, known default: card with default highlighted
  ("sending to X — say `change` or `send it`").
- New name: ask once → save mapping forever.
- Undo: Gmail undo window, WhatsApp revoke where supported.

## Confirmation cards

Photo + name + exact address/number + first ~2 lines + channel icon.
Voice short-circuit: `yes send it` / `no, use the 3rd`.

## Read-receipt rule (hard)

Agent API reads (`list/search/get`) are passive — no blue ticks. NEXUS must
**never call `mark_read`** in agent flows. Opening the real chat UI on screen
(live dictation path) may mark read via the user's own client — acceptable
because the user is watching. Headless flows (reads, sends, meet-link
picking) never open WhatsApp UI.

## Open-the-meet-from-chat (`open_meeting_from_chat` intent)

1. List recent chats (activity-sorted) → latest messages, passive read.
2. Extract `meet.google.com/xxx-yyyy-zzz` + sender + timestamp.
3. 1 link → open in browser. Several → disambiguate by sender+recency.
4. No link → say so; never guess; never open WhatsApp UI as fallback.
5. Prefer links <24h old; warn on stale ones.
