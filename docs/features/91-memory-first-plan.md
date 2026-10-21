# Memory-First Plan: How NEXUS Remembers & Learns About You (2026-10-05)

**Priority order (user directive): memory before everything else.** Hardening plan (doc 90) queues after memory core lands.
**Privacy spine (all phases):** local-first, per-source toggles, audit + forget commands, PII redaction before any cloud use, secrets denylist (password/otp/cvv/token/key) never learned.

## Analysis: what exists vs. what's missing

| Exists | Missing |
|---|---|
| Manual "remember that X is Y" → core.json (`orchestrator.rs:1204`) | **No automatic learning** — email/calendar/contacts/GitHub/usage never become memory |
| Episodes log (500/30d) | Episodes never mined; **`facts.json` documented but never written** — "semantic recall" doesn't exist |
| Substring recall, 800 chars, 3 episodes → Worker dialog_context | Recall not in 9Router/brain paths (verify); no ranking; tiny budget |
| Google accounts, GitHub identity, voice embedding, contacts.json | **Four disconnected islands** — no unified profile, no entity resolution ("mom" ≠ contact ≠ sender) |
| Sentinel deadline alerts | No briefings, no routine learning, no proactive memory use |
| — | **No audit ("what do you remember") or forget UX** |

## Phases

**M0 — Audit & forget (visibility + control).** Intents: "what do you remember (about me)" → spoken summary (profile + fact count + top facts); "forget X" / "forget everything" (confirm-gated for everything). *Execute now.*

**M1 — Unified profile (`memory/user.json`).** Pure builder merging: core.json facts + primary Google email/name + GitHub username + voice-enrolled bool + contacts.json people (read-only) + frequent apps (app_registry usage). Refreshed on remember/audit/boot. Audit command reads it. *Execute now (builder + audit wiring).*

**M2 — Deterministic auto-learning (local-only).** At `log_episode` time, extract: "i work at X", "i like/love X", "call me X", "my birthday is X", "i live in X" → **`facts.json` becomes real**; recall searches core + facts. Secrets denylist enforced. Cloud-assisted extraction (Qwen-when-admin else redacted Worker) gated behind `memoryCloudLearn` (default off). *Execute now (deterministic part; cloud part planned).*

**M3 — Recall upgrade.** Ranked recall (field weights + recency + frequency), budget 800→2000, verify+extend injection to 9Router + brain paths. *Next.*

**M4 — Proactive memory.** Morning briefing (calendar + watches + routine) + habit nudges through the existing proactive_policy engine. *Next (needs live tuning).*

**M5 — Entity resolution.** "mom" ↔ contact ↔ sender ↔ attendee → people graph. *Later (needs care + live data).*

## Acceptance
M0/M1/M2-core: unit tests (parser phrases, builder merge, extraction+recall round-trip, denylist), tsc/vitest/cargo green, release build. Live: "remember my birthday is June 1" → next day "what do you remember" lists it; "forget my birthday" removes it.
