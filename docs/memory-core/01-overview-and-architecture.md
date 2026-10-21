# 01 - Overview and architecture

## Goal
Memory is a main part of the assistant, but it is **priority-based, not
remember-everything**, and it **stays on the user's device**. Cloud models only
ever see a small, redacted, ranked slice (the "context pack").

User decisions that shaped it: screen + clipboard image input for timetables;
app-vs-browser choice stored per activity and overwritten on repeat; priority
people learned automatically with VIP / mute override; boot briefing short and
spoken after a few seconds of idle; the truth over reassurance ("say no if no").

## Architecture choice
One `memcore` module owned by `MemoryCenter` (not the Command Center, not one
store per sub-center). Sub-centers write typed observations with provenance and
trust; the main center asks for one context pack per turn. This gives one store,
one audit log, one forget path, one egress point.

## Storage and encryption
- SQLite (bundled, FTS5) held **in memory** and persisted as ONE sealed blob
  `memory/memcore.enc` (AES-256-GCM via `ring`).
- Key in Windows Credential Manager (written then read back through a fresh
  handle; if no usable key store, it falls back to a plain file and the UI says so).
- Wipe deletes every record and rotates the key (crypto-erase).
- **Honest scope:** protects against disk theft, other Windows users and casual
  reads. It does NOT stop malware running as the same user (it can ask the same
  API for the key).

## Tiers (what is kept)
| Tier | Contents | Retention |
|---|---|---|
| fact | things you told it / mined facts | pinned forever; mined decay 3 %/day |
| episode | one-line conversation overviews | 30 days |
| resume | foreground window samples (where you left off) | 7-day ring, never sent to cloud |
| slot | timetable slots you confirmed | until you delete |
| mail | important-email previews (sender, subject, Gmail snippet) | 7 days, untrusted |
| person | WhatsApp people statistics (counts/timing only) | until forgotten |
| chat | clipped, redacted WhatsApp preview | 30-day ring, untrusted |

Never stored: passwords, OTPs, card/Aadhaar/PAN, screen OCR text, full email bodies.

## Trust model
Every record has `source` + `trust` (`user_said`, `user_owned`, `derived`,
`untrusted`). Untrusted text (email, WhatsApp) is data, never instructions; it
cannot become a fact, a VIP flag or a preference without a user utterance. The
admit gate refuses secrets and empties. Mined values never overwrite pinned
user-said ones; a mined write cannot unpin a record.

## What leaves the device
- Only `context_pack(query, 2000)`: ranked, PII-redacted, line-boundary cut.
- Optional name redaction (`memcoreRedactNames`): people become `Person A/B`.
- A 7-day encrypted log records exactly what each cloud turn sent (Memory page).
- Email bodies and WhatsApp text are never part of the pack.
- Spoken output goes through the cloud voice service (Edge-TTS) - so any text
  NEXUS *speaks* is sent there. Alerts speak sender names; WhatsApp message text
  is spoken only if you enable `memcoreWhatsappSpeak`.

## Proactive delivery
All unprompted speech goes through `proactive_policy` (idle rules, meeting
awareness, rate limits, snooze, dedup). Offers ("Shall I start?") register a
pending-offer that opens a short mic window only after the question is spoken.
