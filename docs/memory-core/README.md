# Memory Core - documentation hub

Everything about NEXUS's priority-based, on-device memory, in one place.
(Detailed per-change records stay in `docs/changes/107-114`; this folder is the
organised, readable version.)

| Doc | What it covers |
|---|---|
| [01-overview-and-architecture.md](01-overview-and-architecture.md) | Goals, design decisions, tiers, trust model, what leaves the device |
| [02-phases-p0-to-p6.md](02-phases-p0-to-p6.md) | What each phase built, how it behaves, where the code is |
| [03-google-sign-in-and-auth.md](03-google-sign-in-and-auth.md) | Gmail/Calendar sign-in: what broke, what we changed, how to set it up |
| [04-voice-commands-settings-and-ipc.md](04-voice-commands-settings-and-ipc.md) | Every phrase, setting key and IPC command |
| [05-testing-and-verification.md](05-testing-and-verification.md) | How it was verified, what was NOT, how to run the tests |
| [06-limits-risks-and-roadmap.md](06-limits-risks-and-roadmap.md) | Honest limits, open items, P7 |

## Where things live
- Plan (approved): `docs/features/100-memory-core-plan.md`; research: `docs/research/memory/01-...`
- Change records: `docs/changes/107` (P0) ... `114` (P6), `113` (Google sign-in)
- WhatsApp manual check: `docs/testing/whatsapp-read-receipt-test.md`
- Code: `src-tauri/src/memcore/` (store, crypto, mailwatch, mailtriage, agenda, timetable, resume, briefing, offer, people, wa, google_io), `google/oauth.rs`, `auth_vault.rs`
- UI: `frontend/src/settings-sidebar/hub/MemoryPage.tsx`, `memoryModel.ts`
