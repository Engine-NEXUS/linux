# Ghostwriter + Echo Modes, and BERT-Mini Alternates Plan (2026-09-18)

## The two modes

| Mode | Job | Enter | While active | Exit |
|---|---|---|---|---|
| **Ghostwriter** | Dictation/drafting — everything you say becomes text | `ghostwriter` / `ghost writer` / `take a letter` / `write this down` | default-to-text; allowlist only (`send it`, `scratch that`, `new line`, `read it back`, `discard it`, `polish it`, `change X to Y`) | `send it` (sends+exits), `discard it`, 30s silence, `command mode` |
| **Echo** | Verification/reading — agent reads things aloud | `echo` / `echo mode` / `read it back` | reads draft chunks, incoming messages, emails on demand; never types, never sends | auto (when reading done), `stop`, `command mode` |

Ghostwriter writes. Echo reads back. `read it back` inside Ghostwriter borrows
Echo's voice for one chunk. Orb shows pen (Ghostwriter) vs speaker (Echo).

## Why both need alternates in BERT-Mini

STT hears what it hears: "ghost writer", "goose writer", "eco", "mummy" vs
"mommy", "send" vs "sent", "meet" vs "meat". If the model only knows the
canonical form, the mode never triggers or the message misfires. Coverage is
built in four tiers per intent (modes included — triggers are intents too).

## Tier 1 — verb/command alternates (meaning-preserving)

Every intent gets all natural verbs, not just the first one coded:

- open: open/launch/start/run/fire up/bring up/show/pull up/go to/visit
- send: send/forward/ship/text/ping/tell Dresden Elementary School
- order: order/get/craving/want/bring/fetch
- search: search/find/look for/check price/hunt down
- Mode triggers: ghostwriter/ghost writer/start ghostwriting/take dictation;
  echo/echo mode/read it back/read that out

## Tier 2 — close/STT-confusable words (sound-preserving)

 harvested from `stt_learning` corrections + known pairs:

- whatsapp: whatsup/watsapp/whats app/vatsap
- amazon: amazone/amazan
- swiggy: swigy/swigi
- dominos: dominose/dominoes
- send/sent, meet/meat, mummy/mommy/mom/amma, thanmayeeredy variants
- ghostwriter: ghost writer/go writer/ghost rider (STT splits)
- echo: eco/ecko/ecco

Each confusable is a training row with the CORRECT label, so the model learns
that "send mummy a whatsup message" == send_whatsapp_message.

## Tier 3 — slot-value alternates (nickname/entity-preserving)

- Contacts: mummy/mom/amma → same person; boss/sir variants.
- Repos: zync/zynk; services: gmail/mail.
- Fillers tolerated everywhere: please/kindly/hey/so/ok/yeah (prefix+suffix).

## Tier 4 — filler-tolerant allowlist (Ghostwriter/Echo commands)

`ok send it`, `perfect send it`, `yeah scratch that` == bare forms.
`type "send it"` escape hatch for reserved phrases as text.

## Generation pipeline (per tier, per intent)

1. Seed families: 8–15 per intent (triggers included), phrase-family keyed.
2. Paraphrase 3–5× per seed (template expansion first, LLM only for gaps).
3. Phonetic corrupt 2–3× per seed on slot spans + triggers (Tier 2 list).
4. MITL filter `conf>=0.9` against current model — keep only what the model
   already leans toward (prevents noise poisoning).
5. Family-dedup vs frozen test-452; OOS negatives for near-misses
   (`order a book` ≠ food, `send the file` ≠ whatsapp).
6. Merge → retrain → audit → OTA. Target 60–100 rows/intent; triggers
   (ghostwriter/echo) get 60+ rows each from day one so modes never misfire.

## Feeding loops (already in codebase, point them here)

- `stt_learning` corrections (`learned_corrections.json`) → Tier 2 mining.
- `brain_monitor` approved phrasings → Tier 1 mining (admin builds).
- `pronunciation_map` (zync-style) → Tier 3 aliases.
- Candidate reports must show per-trigger recall (ghostwriter/echo ≥0.95)
  before any promotion — modes failing to trigger is a release-blocker.
