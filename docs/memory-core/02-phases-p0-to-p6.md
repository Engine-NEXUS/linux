# 02 - Phases P0 to P6

Each phase is flag-gated and was shippable alone. Change records: `docs/changes/107-114`.

## P0 - Fixes (change 107)
- `keyring` got the `windows-native` feature. Before that it was an in-memory
  mock, so keychain-stored keys/tokens did not survive a restart.
- `forget` covers `facts.json`; `wipe_memory` covers conversation, brief, diary,
  mail watches. Writes are atomic. `log_episode` is wired into `record_worker_turn`.

## P1 - SQLite store and ranked recall (108)
- Records + FTS5 + append-only audit. Ranked recall = BM25 + recency + use + pinned.
- Derived facts decay 3 %/day; episodes expire at 30 days.
- The Worker now reads `dialog_context.memory` (labelled data-not-instructions).
- Limit: retrieval is lexical ("where do I work" will not find `employer`).

## P2 - Encryption, cloud-send log, redaction, "what I remember" (109)
Sealed blob, key in Credential Manager, 7-day egress log, optional name
redaction, Memory page (list, forget, cloud-send log, redaction switch).

## P3 - Resume points and boot briefing (110)
- Samples the foreground window every 30 s while you are at the PC (sensitive
  windows, NEXUS itself and the lock screen are never recorded; URLs reduced to host+path).
- A once-a-day briefing ~45 s after startup, only when you are present: where you
  left off (with real age), deadlines from watched mail, requests NEXUS could not finish.
  No invented progress numbers; silent if there is nothing true to say.
- Voice: "where did I leave off", "show my briefing".

## P4 - Timetable, slot reminders, "shall I start?" (111)
- "Analyse this and add section 2 to my timetable" (screen) / "add the picture I
  copied..." (clipboard) -> Gemini reads slots -> code validates -> confirm card
  + spoken "Shall I add them?" -> saved.
- A 30 s scheduler fires each slot's reminder once a day. DSA/LeetCode slots ask
  "Shall I start?"; "yes" opens LeetCode where you left off and the last video
  watched during a DSA slot. App-vs-browser is stored per activity, overwritten on repeat.
- Honest NOs: exact YouTube position, downloading Instagram reels.

## P5 - Important mail and calendar (112)
- Polls Gmail `history.list` (90 s active / 8 min away), metadata only.
- Deterministic rules: database-service pause/deletion, GitHub security/CI,
  hackathon status/deadlines, exam notices, plain deadlines; Promotions tab suppresses weak matches.
- High/Medium alerts via `proactive_policy`; 3+ are batched; first connect is quiet.
- "That's not important" mutes the last alert's sender (a visible, forgettable fact).
- Calendar: "what's on my calendar today/tomorrow"; "add X to my calendar
  tomorrow at 5pm" reads back, checks overlap and asks before `events.insert`.
- If Google rejects the stored sign-in, the watcher pauses, says so once a day,
  and the Memory page shows "Paused".

## P6 - WhatsApp priority people (114) - OFF by default
- Polls the local WhatsApp MCP bridge; learns priority people from counts and
  timing (two-way volume, reply rate, recency; one-sided senders down-weighted; VIP +30; mute 0;
  High >= 70, Medium >= 50). Message text never used for scoring. Groups ignored.
- Alerts name the sender only. "Read that message" shows the chat in the sidebar.
- Guarantees: read-only allowlist; `mark_read`, `mark_chat_read`, `send_presence`,
  `send_typing` refused for every caller; first sight of a chat is silent.
- Ships off until the two-phone check in `docs/testing/whatsapp-read-receipt-test.md`.
- Bugs found: VIP/mute could not be cleared (fixed); pairing parser missed
  `structuredContent` / JSON-in-text replies (fixed).
