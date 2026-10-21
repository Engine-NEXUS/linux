# Ghost-Session UX, Animation Timing & STT Gating — Root-Cause Research (2026-09-29)

Live log: build 20:52–20:57, run 21:00–21:33. Eight reported symptoms traced
to mechanisms below. No code changed (research only).

## 1. Ghost orb disappears, no waves (reported #4)

**Mechanism (confirmed, two cooperating causes):**

(a) Turn-end hides the orb unconditionally. `recorder.ts` contains **28**
`setVisible(false)` call sites (turn-end pattern). In ghost hot-mic mode
transcripts still flow through `processTranscript`, whose turn ends hide
the orb. The Rust `win.show()` + `__NEXUS_WAKE__` eval that shows the orb
fires only on wake-word/hotkey — the hot-mic relisten loop never re-shows
it. Result: after the first ghost command, the orb slides down and stays
hidden; waves require `visible` (`Avatar.tsx` phase gate), so no waves.

(b) The `orchestrator:event` result/loading/ack handlers ARE ghost-guarded
(`orchestrator.ts:200-240` check `ghostActive` before hiding) — so the
Rust-event path is correct; the leak is the recorder turn-end path (a).

**Fix direction:** single ghost-aware `endTurn()` (relisten-before-reset,
no hide in ghost) replacing all 28 sites, or gate each site on
`!ghostActive`. This is exactly the P2 UI-director migration.

## 2. Speaking zoom not seen (reported #2)

**Mechanism:** the zoom itself works (`.avatar-wrap--speaking` +
`pulse-speak` keyframes exist; `state=speaking` is set; `ttsActive` is
driven by `ttsActivity.ts:38,47`). It is *invisible* for two reasons:

(a) In ghost mode the orb is hidden (see §1) — no zoom to see.
(b) In normal mode the orb hides 600–1500ms after ack/loading starts
(`orchestrator.ts:206-210, 224-240` hide timers) while TTS often runs
longer — the pulse plays on a sliding-down orb.

**Fix direction:** hide-after-speech-ends (not on fixed timers); never hide
while `state === "speaking"`; keep orb visible through short acks.

## 3. Thinking/loading appears while user speaks (reported #3)

**Mechanism:** "thinking" can only start when the transcript ARRIVES
(Rust emits `thinking` post-parse, `orchestrator.rs:1028`). Arrival lags
speech-end by: capture endpointing (~0.5–1.5s silence) + Groq roundtrip
(~2–4s). During that gap the orb sits in "listening" with no progress
feedback. Then the loading *window* costs a WebView2 create per turn
(`get_or_create_window`), landing even later — often mid-next-utterance,
where it overlaps barge-in speech. One turn late = perceived "loads while
I speak".

**Fix direction:** (i) interim listening feedback at speech-end (endpoint
chime/state before STT returns); (ii) reuse don't recreate the loading
window (hide not destroy); (iii) cancel stale loading on barge-in turn
start (already partially done — verify the hide path runs before new
capture, not after).

## 4. Esc doesn't cancel ghost (reported #5)

**Findings:** entry registers Esc correctly (`ghost.rs:482` unregister +
484 register with `?` propagation — a failed entry aborts loudly, and the
session DID start, so registration succeeded). The handler calls
`abort_session` → ring off, Esc released, `emit_session(false)`.

Two compounding causes:
(a) **No visible feedback**: exit sets `ghostActive=false`, and the store
setter (`assistant.ts:131`) forces `visible=false` — the orb was already
invisible (§1), so a successful Esc changes nothing on screen. The user
perceives "Esc did nothing" even when the session ended. Verify by log:
`ghost: session ended (esc-panic)` warn line.
(b) If the session already stood down spuriously, `abort_session`
early-returns on `Idle` — silent no-op.

**Fix direction:** Esc exit must SHOW the normal-mode orb (smile, visible)
as the acknowledgment — never end in invisible. Log already distinguishes;
add a spoken "Ghost mode off, sir." (check: exit path speaks? `ghost_exit`
vs silent `abort_session` — audit which paths speak).

## 5. Ctrl+Space should cancel ghost (reported #6)

**Mechanism:** `hotkey.rs:67-104` — Ctrl+Space stops TTS, closes visible
sidebars, else wakes. It has **no ghost-session branch**. In ghost (orb
`main` window isn't even in the visible-check list) it falls through to
"wake NEXUS", starting a competing capture against the hot-mic loop.

**Fix direction:** first branch in the Ctrl+Space handler: if
`ghost::session_active()` → end session (same path as Esc) + spoken
confirm. Keeps TTS-stop behavior for non-ghost.

## 6. Ghost hot-mic hallucinates (reported #7)

**Mechanisms (three confirmed gaps in `stt.rs`):**
(a) Repetition guard (stt.rs:388-396) requires even word count + exact
half-split — catches 2× loops only. The log's 3× loop (`"I'm gonna say
that." ×3`, 12 words, uneven thirds) passes straight through.
(b) No Latin-script foreign/background guard (CJK-only, P1). `"Tentang
Otor. Post Immortal."` (background audio) → cloud chat → spoken answer.
(c) Command captures don't use Groq verbose `no_speech_prob` (verbose
segments are verifier-only, stt.rs:161-164) — no acoustic veto before
routing.

**Fix direction:** n× repetition detector (any k≥2 full-phrase repeats);
dictionary-ratio gate for Latin gibberish (<30% common-English tokens →
retry); verbose no_speech veto on command captures; optional capture RMS
pre-gate already exists (0.005) — verify it runs before the Groq call on
the hot-mic path, not just local STT.

## 7. Slow loading (reported #1)

**Findings:** boot is healthy (READY in ~5s in the log). The perceived
slowness is per-turn, same root as §3: STT roundtrip with no interim
feedback + per-turn loading-window creation + first-use cold starts (STT
sidecar python+model on first wake; NLU server on first unparseable).
No evidence of wake-engine load blocking (captures flow 7s after boot).

**Fix direction:** pre-warm STT sidecar at boot-idle (not first wake);
persist loading window across turns; interim endpoint feedback (§3).

## 8. More training data? (reported #8)

**Answer: no — not for these symptoms.** Evidence: wake fires at
98.5%/96.8% on real speech in this log; zero false wakes shown; the 0.0%
phantom class was a code bug (barge-in candidate fired unverified — fixed
in tree, `should_instant_fire` + test). All 8 complaints trace to STT
gating, UI wiring, and hotkey routing — none to the KWS model. Retraining
is the wrong tool here; it would not move any of these 8 needles.
(A separate, optional track: mining TV/anime audio as hard negatives for
background robustness — only if background-trigger rate is measured high
in a soak, which P4's harness (0 triggers/120s silence) does not show.)

## 9. Common thread

Six of eight issues are **lifecycle ownership**: 28 unguarded hide sites,
timers that hide mid-speech, exits that end invisible, hotkeys with no
session branch, captures with no voice gate. This is the P2 UI-director +
P3 STT/TTS-director work from doc 74 — this research is its evidence file.
