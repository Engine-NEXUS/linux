# Feature Plan: 119 — Instant-at-Logon Readiness, Voice Parity & Intelligibility (PLAN ONLY)

**Date**: 2026-10-14
**Status**: Plan Only — no code touched (user directive).
**Grounding**: `docs/research/startup-boot-log-analysis-2026-10-14.md` (boot log 07:12–07:13 UTC).

## User asks (verbatim intent)

1. Analyse how startup works on the laptop (done — research doc above).
2. Some spoken words are not understandable — find and fix.
3. Cloud voice and local voice must match perfectly.
4. NEXUS should be in control immediately at login, right after auth — can it?

## Phase V — Voice parity (cloud ↔ local "match perfectly")

**Honest constraint first:** Edge-Ava and Kokoro-af_sky are *different human speakers*
recorded for *different engines*. No setting makes them identical. "Perfect" is
achievable only as **one consistent speaker per session**, via (pick one):

- **V1 (recommended): sticky-voice rule.** The first engine that speaks in a session
  owns the whole session: if the greeting falls back to Kokoro, *stay* on Kokoro
  (warm, instant, consistent) until 10 min of stable network, then crossfade back
  with a spoken note — never mid-sentence swaps. Small change: a session voice-latch
  in `tts.rs` (`synthesize_with_fallback` already branches per call; latch the tier).
- **V2: per-persona twin audition.** `voice_catalog.rs` admits twins are "a first
  proposal pending an audition (doc 09 §3)". Audition each cloud persona against
  Kokoro's A-grade voices (`af_heart` A, `af_bella` A-) on the user's ear and re-pair
  the closest (e.g. Ava ↔ af_heart, not af_sky). No code — a listening session +
  table edit.
- **V3 (deferred, say so): true voice cloning** (user-enrolled sample → local
  zero-shot TTS). Real fix for "perfect", but a new model + enrollment UX + quality
  risk. Parked until V1+V2 are heard.

**Also:** treat Edge-403-at-boot as expected, not exceptional — pre-warm Kokoro
*during* the health-check window so a fallback costs 0 s, and never start a sentence
on Edge while its first-synthesis probe is unproven (speak the greeting on the
proven engine).

## Phase W — "Words not understandable" (diagnose before treating)

Ranked suspects from the log, with the test that settles each:

1. **Mid-sentence voice swap (§3 of research)** — user hears half of Ava then a
   restart in Kokoro. Test: replay the greeting with forced-Edge vs forced-local;
   if the complaint vanishes on either single voice, V1 closes it.
2. **Greeting overlaps a deaf mic** — offer question expires in silence; user
   perceives "it didn't understand me". Test: next boot log must show
   `LIVE rms>0` *before* `offer_boot_session` fires (Phase X gates on this).
3. **STT mishearing (unproven in this log)** — needs a log with real transcripts:
   collect `[STT-CONF]` + transcript pairs for the "not understandable" phrases
   before touching models.
4. **Kokoro prosody on long sentences** — sentence-chunked streaming can clip
   clause boundaries; A/B the greeting as one synthesis vs chunked.

Rule: no model/STT edits until suspect 1–2 are cleared by the two tests above.

## Phase X — Instant-at-logon control ("immediately after the auth")

Autostart already launches at logon (Scheduled Task AtLogOn). The plan closes the
**launch → in-control** gap with a readiness gate + budget:

- **X1 readiness gate (the core change).** Define `READY = audio LIVE (rms>floor
  for 3 consecutive callbacks) + wake engine past grace + STT health OK`. Until
  READY: (a) delay the boot greeting/offer (it is pointless while deaf — this log
  proves it), (b) show a tray tooltip "NEXUS starting — mic waking", (c) retry the
  Intel SST recovery aggressively (shorten poll 5 s → 2 s for the first 90 s; the
  66 s silence in this log is the whole problem).
- **X2 pre-warm at logon.** After READY, warm in parallel: STT server, Kokoro,
  NLU — so the first command pays 0 s cold-load. (Today the first transcription
  and first fallback each stall seconds.)
- **X3 honest readiness line.** One `[BOOT] ready in Ns (audio:.., stt:.., tts:..)`
  log + one spoken "Online, sir" *only* when READY — replacing today's greeting
  that fires while deaf. Target: READY < 15 s on a healthy boot; on SST-silence
  boots, say "Mic waking up" instead of silence.
- **X4 ghost-at-logon caveat (say it plainly).** Full screen control in the first
  seconds after auth is OS-limited (shell starting, foreground lock, possible
  UAC desktop). Promise: voice + answers at READY; *driving apps* the moment the
  shell settles, with one spoken line if control is deferred ("Desktop still
  loading — one moment"). No code can make Windows yield the foreground earlier.
- **Out of scope (name it):** "screen time" as parental app-limits/bedtime
  enforcement is a different feature (Family Safety APIs) — not this plan. If the
  user meant that, it becomes plan 120.

## Verification (all observable in `nexus start` logs)

- Next boot log shows: `LIVE` before any spoken greeting; zero mid-sentence voice
  swaps for a week (`tts: falling back` lines only between sentences); `[BOOT] ready
  in Ns`; offer answered by voice on first boot.
- Soak: 5 consecutive logon boots, READY time recorded; SST-silence boots announce
  mic-wake instead of greeting into deafness.

## Explicit non-goals

- No STT/wake-model retraining in this plan (unproven suspect).
- No new OS windows, no new background processes (single-slot discipline stands).
- No voice-cloning work until V1+V2 are heard by the user.
