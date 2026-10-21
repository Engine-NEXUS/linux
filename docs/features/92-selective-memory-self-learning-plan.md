# Selective Memory & Self-Learning Plan (2026-10-06)

**User rule:** normal conversation → overview only, never verbatim detail. Reminders/tasks/commitments/preferences → remembered precisely. The agent learns about the user over time (routines, people, interests) — all visible, all forgettable.
**Privacy spine:** local-first, per-key forget (M0 covers new stores too), secrets denylist, no cloud summarization in this plan (deferred — needs redaction + consent design).

## Research: what already exists (verified in code)

- `turn_relevance()` (`conversation.rs`): deterministic 0–100 (rejected 0, failed/unresolved 100, remember 90, unknown 30, greeting 25, tiny-ack 20, default 70).
- `record_conversation_turn` → `compact_thread` → live turns + `ConversationBrief` (goal/active_task/unresolved/key_points) → `conversation_prompt()` injects brief + recent (PII-sanitized) into the Worker path.
- `proactive_policy::Engine` (`submit`/`tick`, Low→Critical, snooze, dedup) — reminder delivery path already built.
- `app_registry::record_usage` — app-frequency counters already exist.
- M0/M1/M2 (audit/forget, user.json profile, deterministic fact mining into facts.json).

## Gaps (why it doesn't do this today)

1. **No reminder intent at all** — "remind me to X" is unparseable. The #1 precise-memory case has zero support.
2. **No commitment/preference capture** — "I will…", "don't ever…", "I prefer…" score default-70 and decay; nothing precise persists them.
3. **No overview artifact** — diary rollup counts events, not chat; nothing writes "yesterday evening: flights + 2 reminders" digests.
4. **No learning loops** — turn timestamps+intents, contact mentions, topic tokens are never mined into routines/people/interests.
5. **Step-0 verification open:** `compact_thread`'s exact drop behavior unread — confirm low-relevance verbatim pruning before building on it.

## Phases

**S0 — Selective capture rule (verify first, then extend).** Read `compact_thread`; confirm low-relevance turns compact to overview-only. Extend `turn_relevance`: reminder/commitment/preference signals → 95–100 (precise retention); pure chit-chat stays ≤30 (overview-only). Unit tests on scoring tiers.

**S1 — Reminders, remembered precisely.** Intent `Reminder{text, when}` + `reminders.json` store `{id, text, due_ms, created, status}` + deterministic time parsing ("in 20 minutes", "at 6pm", "tomorrow morning", "every weekday at 9"→recurring). Delivery: `proactive_policy::Engine::submit` with due-tick; spoken on fire + sidebar card. Intents: list ("what are my reminders"), cancel ("cancel my 6pm reminder"), confirm-gated clear-all. Parser + due-engine + store tests; M0 forget covers reminder keys.

**S2 — Commitments & preferences, remembered precisely.** Deterministic extraction ("i will …", "don't …", "i prefer …", "always/never …") → facts.json (`source: commitment/preference`) + relevance-95 turns; negative preferences ("don't remind me about X") honored by the proactive submit path. Denylist + generic guards same as M2.

**S3 — Overviews, not transcripts.** Digest job (idle/boot, every N compacted turns): old compacted-out turns → dated overview lines in `overviews.jsonl` ("Oct 5 eve: flights Q&A, 2 reminders set"); raw low-relevance turns pruned after digest. Retrieval appends last ≤3 overview lines. Pure + tested (digest is deterministic extractive: top intents + entities + counts — no LLM).

**S4 — Self-learning loops (deterministic counters first).** Routine miner (intent × hour-block histogram → `profile.routines`, e.g. "weekday 9am: email triage"); people rank (contact mentions + WhatsApp/recipients → `people_ranked`); interest tokens (noun frequency over relevance≥70 turns, decayed → `profile.interests`). All local, all shown in audit under "what I've learned", all per-key forgettable. Cloud-LLM insight pass explicitly out of scope.

**S5 — Retrieval order + budgets.** Inject: unresolved → live turns → brief → overviews(≤3) → profile routines/interests → facts; per-section char budgets inside the existing 800-char envelope (raise only if measured insufficient).

## Acceptance & ordering
S1 → S0-verify → S2 → S3 → S5 → S4. Each: unit tests + gates + live script ("remind me in 1 minute to stretch" fires; "what do you remember" shows overview lines, not transcripts; audit "learned" section grows routines after days of use).
