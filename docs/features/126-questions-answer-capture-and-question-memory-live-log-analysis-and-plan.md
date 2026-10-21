# 126 — "I said *do it*, no response": answer capture + question memory (LIVE-LOG ANALYSIS + PLAN, nothing built)

Date: 2026-10-16. Evidence: the user's own boot log of 2026-10-16 15:54-15:55 (release build with the doc 123/124/125 work).
Continues `124-questions-keep-mic-open-and-follow-up-memory-audit-and-plan.md` (Q2/Q3/Q4 were only partly built).

---

## 0. What the user reported

1. NEXUS asked a question ("Shall I reopen them?"), the user said **"do it"**, **no response**.
2. Suspicion: NEXUS asks but does not listen / does not keep the mic open.
3. If the user missed it and calls NEXUS again and says "yes", it **does not remember its last question or the chat**.

## 1. What the log actually shows (second by second)

| Time | Log line | Meaning |
|---|---|---|
| 15:55:01.04 | `proactive: speaking 'offer_boot_session'` | boot offer submitted |
| 15:55:01.36 | `speak (req=sentinel_offer_boot_session)` | `speak_line` arms the offer (`sentinel_offer_` prefix) |
| 15:55:01.87 | `meeting: TTS ended — resuming wake detection` | **BUG E**: reported "ended" 0.5 s after it STARTED (see 2.5) |
| 15:55:17.48 | `tts: streaming playback completed` | the question lasted **16 s** (3 chunks) |
| 15:55:18.56 | `stt-capture: started` | **the mic DID open** (1.1 s after the audio ended) |
| 15:55:18.9 | `audio: mic LIVE rms=0.0381` | the user's voice arrived |
| 15:55:19.68 | `stt-capture: stopping (14 chunks, voiced=4, 19200 samples)` | 1.2 s clip, endpointed normally |
| 15:55:20.00 | `stt-groq: transcript 'Dread.'` | **Whisper heard "do it" as "Dread."** |
| 15:55:20.29 | `[nlu_client] rejected low-confidence 'cancel_action' (0.382)` | no intent |
| 15:55:20.30 | `[PARSER-MISS] 'dread.'` -> WorkerBackend | not an answer -> the offer was **dropped** |
| 15:55:21.02 | cloud reply "I'm here for you—what's causing the dread, or how can I assist you right now?" | what the user heard instead of an action |

**Conclusion, stated plainly:** the mic was open and did record the user. The failure is not "NEXUS does not listen". It is a
chain of four faults: the answer was mis-transcribed (A), the question was destroyed by the first reply it could not read (B),
nothing remembers a question once it is destroyed (C), and the question itself was a 16-second monologue with the actual
question at the end (D). A fifth, separate bug (E) makes NEXUS's own state wrong while it speaks.

## 2. Root causes (verified in code)

### A. Answers are decoded with the wrong vocabulary (the trigger)
`wakeword_oww.rs:3896-3908`: every capture uses `decoder_bias_for(ownership)`. With one enrolled/unenrolled owner this is
`NEXUS_VOCABULARY` (a prompt full of COMMANDS: "Ghost mode. Open WhatsApp ..."). A two-syllable answer like "do it" has almost no
acoustic context, so Whisper falls back to the nearest word it is primed for or a common word: "Dread." Nothing tells the
decoder "the next thing is a yes/no answer", although NEXUS knows it just asked (`offer::is_armed()`), and nothing corrects
known mishearings afterwards. (`offer::classify` is strict on purpose, so it correctly refuses "dread".)

### B. One unreadable reply kills the question
`orchestrator.rs` (process_transcript, offer block): `_ => offer::clear()`. Any transcript that is not a clear answer — including a
1-word mishearing of the answer — **deletes the pending question** and is handled as a normal command, here a cloud chat that
invented emotional counselling from the noise. There is no "I didn't catch that, yes or no?" and no second try.

### C. Nothing remembers the question
* The question lives only in `memcore/offer.rs` (one in-memory slot). After `clear()` it is gone, so the user's second try
  ("nexus ... yes") finds nothing pending and falls to the cloud.
* Proactive/boot speech is excluded from the conversation store (`speak_line`: `sentinel_`/`boot_` ids are not user turns), so
  `conversation::last_exchange` (used by the new "Do what, sir?" line) knows nothing about the question either.
* The mic-closed case (user simply missed it) is covered only by the 10-minute offer TTL — which B had already destroyed.

### D. The question is buried and too long
Built by `memcore/briefing.rs::build`: greeting + "You left X open about 21 hours ago." + calendar + **verbatim mail subject**
(“Deadline Extended: Grab PPIs & ₹3L Prize Pool | Eurogrip Is…”) + health + the question LAST. 16 s of speech; the user cannot
answer until the end; the mic opens ~1.1 s after the audio ends.

### E. `tts_playing` goes false while NEXUS is still speaking
`ttsPlayer.ts` calls `stopTts()` before each `speak()`; that emits a stray `tts-ended` before the real `tts-started`.
`frontend/src/audio/ttsActivity.ts` already handles this with a generation counter. **`lib.rs` (the Rust listener, ~l.594-609) does
not**: `tts-ended` schedules `set_tts_playing(false)` after 500 ms, which lands AFTER the real `tts-started`. Log: "TTS ended" at
15:55:01.87 while the 16 s line plays until 15:55:17. Effects: wake word un-muted during our own speech (self-trigger/echo), the
proactive policy believes NEXUS is silent, and my new shutdown sequence (`orchestrator::shutdown_sequence` waits on
`tts_playing`) can run its close-apps step while the goodbye is still being spoken.

### F. Still open from doc 124 (not this log)
Cloud questions ("Would you like me to ...?") register nothing; GitHub/MCP confirmations still use a separate frontend mechanism;
no on-screen Yes/No; no "waiting for your answer" mark on the orb.

## 3. Plan

Order = smallest-risk first; each step has a test gate. **R1+R2+R3 together fix the reported behaviour.**

**R0 — Reproduce with the real audio (0.25 day).** The failing clip is saved:
`%APPDATA%\com.nexus.assistant\verify_debug\capture_00.wav` (1.2 s, 19200 samples; it rotates, so copy it now into
`src-tauri/tests/fixtures/answers/`). Send it to Groq with (a) the current prompt and (b) the new answer prompt; record both
transcripts. Gate: we know whether the prompt alone fixes it or whether aliases are also needed.

**R1 — Answer-aware transcription (0.5 day).**
* New `DecoderBias::Answer` with a prompt made only of answers: "Yes. No. Do it. Go ahead. Yes, do it. No, don't. Cancel. Not now.
  Later. Okay. Sure. Stop." (command vocabulary withheld).
* Chosen in the capture thread when `memcore::offer::is_armed()` or `power_cmd::countdown_active()`, or when the frontend starts
  the capture with `origin: "answer"` (confirmations, clarifications; `start_stt_capture` already takes an origin).
* `offer::answer_alias(text)` applied ONLY while a question is pending: known mishearings of "do it" ("dread", "due it", "dew it",
  "do eat", "to it", "doing it", "dude it") and of yes/no map to the canonical answer. Unit tests include "dread" being
  untouched when nothing is pending.
Gate: 30 recorded or synthesised "do it / yes / go ahead / no / not now / later" clips through the real Groq path (the existing
`phrasing_probe` harness style) with a target >= 95 % classified correctly; "dread" fixture passes.

**R2 — Never destroy a question on an unreadable reply (0.5 day).** Replace `_ => offer::clear()` with a small decision
(pure function `answer_outcome(reply, transcript, parsed) -> Resolve | Reask | KeepAndRoute | DropAndRoute`, unit-tested):
* clear answer -> resolve (as today);
* parses deterministically as a different command with strong evidence -> drop the question, run the command (as today);
* **short (<= 4 words) and not a command** -> keep the question, ask ONCE more, shorter: "Sorry sir, I didn't catch that. Reopen
  your apps — yes or no?" and reopen the mic; a second miss -> keep it pending and stop nagging (card/buttons, see R5);
* long unrelated sentence -> keep the question pending until its TTL and handle the sentence normally.
A <= 3-word utterance heard while a question is armed is **never** sent to the cloud.
Gate: harness scenarios 2, 3, 7 below pass; mutation-check the "never to the cloud" rule.

**R3 — A question memory that outlives the mic (1 day).**
* `memcore/offer.rs` keeps `last_question {id, text, action, asked_at, expires_at(risk TTL 2/5/10 min), tries}` separate from the
  live window state; a failed/missed window does not delete it; only an answer, a different strong command, TTL or a newer
  question ends it.
* Every question NEXUS speaks (boot offer, reminders, `OpenApp` guess, shutdown, calendar add, ...) is also written to the
  conversation store as a NEXUS-initiated turn (`agent: "nexus"`, kept on-device like other local turns), so
  `last_exchange` and the "Do what, sir?" line quote the real last question.
* Later "yes / do it / go ahead / that one / no / not now" (wake word + answer, any time inside the TTL) resolves it even after the
  mic closed. After the TTL: one honest line, "That question has expired, sir — shall I ask again?" (answers doc 124 decision 4).
Gate: scenarios 4, 5, 6 pass; tests use an injected clock.

**R4 — Ask short, ask first (0.5 day).** `briefing.rs`: the offer is "Good evening, sir. Shall I reopen Brave, Antigravity and
OpenCode?" (<= ~6 s). The remaining items (calendar, mail, deadline) come AFTER the answer, or as a sidebar card. Verbatim mail
subjects are never read inside a question. Also cut the post-speech settle before the mic opens from ~1.1 s to <= 0.4 s for
answer windows. Gate: the boot question is <= 8 s in the real log.

**R5 — Make waiting visible (1 day).** Orb/stage "waiting for your answer" mark + on-screen **Yes / No** buttons for every pending
question (reuse the `HandOverlay`/chip pattern; buttons call the same resolve path as voice). If the mic delivered silence for
the window (Intel SST, the log shows `SILENT` for most of the session) say so once and rely on the buttons.

**R6 — Fix the `tts_playing` race (0.25 day).** Move the Rust listener logic into a tiny pure `TtsPlayingTracker`
(generation counter: a `tts-started` cancels any pending false; same rule `ttsActivity.ts` already uses) and unit-test the
stray-ended-then-started sequence. Then make `shutdown_sequence` also wait for `has_active_request()` to be false. Gate: the log
no longer shows "TTS ended" until the real end.

**R7 — Observability.** `[ASK]` lines in the unified console: asked / spoken / mic_open / heard('...') / classified(yes|no|later|
other|alias) / reasked / answered / expired, with the question id, so the next failure explains itself in one glance.

**R8 — Cloud questions (doc 124 Q4), later.** Structured `proposed_action` from the Worker; no "Done" without an executor.

### Harness scenarios (all with fake audio events + injected clock; extends doc 124 Q0)
1. "yes" / "do it" right after the question -> action runs.
2. Mis-transcription "Dread." -> alias resolves **or** one re-ask, then "yes" works.
3. "open notepad" while a question is pending -> question dropped, notepad opens.
4. Silence (window closes) -> question stays; wake + "yes" at +30 s -> action runs.
5. Same at +3 min for a high-risk question -> "expired" line, nothing runs.
6. "do it" with nothing pending -> "Do what, sir?" quoting the last real question.
7. A long unrelated sentence -> question stays pending, sentence handled normally.
8. Wrong/unenrolled-TV voice answers a high-risk question -> ignored (existing owner gate).

## 4. Decisions needed from the user
1. OK to **re-ask once** ("I didn't catch that — yes or no?") instead of dropping the question? (recommended)
2. OK to shorten the boot offer to the question only, and move the mail/calendar details after the answer? (recommended)
3. After the TTL, say "that question expired" once (recommended) or stay silent?
4. On-screen Yes/No buttons for every question: yes?

## 5. Not verified
No live microphone here: the Whisper behaviour in R1 is a hypothesis until R0 runs the saved clip through Groq. The exact set of
mishearings for "do it" is known from ONE sample ("Dread."). The tts-ended race is read from the log timestamps and the code, not
reproduced in a test yet. Nothing in this document is built.
