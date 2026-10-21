# 129 — Hand Mode Voice Isolation & 1-Minute Idle-Exit Timer

**Date:** 2026-10-14
**Area:** `src-tauri/src/hand/mod.rs`, `src-tauri/src/wakeword_oww.rs`
**Related:** `docs/changes/127-hand-mode-camera-pointer-p0-p2.md`, `docs/changes/128-hand-gestures-and-field-test-4.md`, `docs/features/118-hand-mode-master-compendium.md`

## 1. User report (verbatim, from a live `nexus start` session)

> "pinch is almost perfect but need some more fine tuning" — *(separate topic, resolved in the prior session on `engine.rs`'s press-catch bridging; not part of this doc)*
>
> "check any issue in the hand mode i fel [felt] neus [NEXUS] was listening and giveing me the answer like the ghost mode u can control the hand mode whil [while] add in the ghost mode but hand mode in the normal mode should only contorl [control] the not listen to what i am saying"
>
> "and did u have an exit for the hand mode"
>
> "and no [now] use auto exit timer for 1 minute for the hand mode"
>
> "nd write the deocemntion [documentation] for this after done ... extremly detailed i dont care how many file how big is th eresearch ever sinlge single [detail] ... Create an md files and add in docs"

Distilled into three concrete requirements:

1. **Hand Mode, on its own (no Ghost session), must stop NEXUS from listening to ambient speech and answering it like a conversational assistant.** It must behave as a pure pointer-control mode: camera in, mouse-and-keyboard-events out, nothing else. The moment Ghost Mode is *also* active, the normal always-listening behaviour is correct and must be preserved — Hand Mode is just riding along as Ghost's pointer in that case.
2. **Confirm (and if missing, build) a reliable way to exit Hand Mode.**
3. **Change the no-hand auto-exit timer from its current value to exactly 1 minute.**
4. Write this up in exhaustive, standalone documentation under `docs/`.

## 2. The live log that triggered the report

The user attached a full `nexus start` console transcript. The two turns that matter:

```
[22:56:27] STT stt-capture: transcript = 'hand mode.', intent = Some("enter_hand_mode")
  [MAIN-CMD] ... Intent: enter_hand_mode | Center: HandCenter | Evidence: Strong ...
  [HAND] mode ON live=true primary=1920x1080 monitors=1
  [TTS] speak (req=a55e90bee4e6): "Hand mode on, sir."
...
[22:59:10] WAKE wake: model probability=0.982 (threshold=0.680, ...)
[22:59:10] WAKE wake: model probability=0.992 (threshold=0.680, ...)
[22:59:20] RUST verify: STT timed out after 10s — firing (fail-open, stage-1 prob 0.987)
[22:59:20] BATON stt-capture: started (cpal-side capture, no baton pass)
[22:59:23] STT stt-capture: transcript = '', intent = None
[22:59:23] BATON stt-capture: started (cpal-side capture, no baton pass)
[22:59:27] STT stt-capture: transcript = 'a glassless update.', intent = None
  [MAIN-CMD] ... Intent: unknown | Center: KnowledgeCenter | Evidence: None ...
  [SUB-PROC] SubCenter Knowledge routing to 9Router / Cloudflare Worker (request_id=1080fbbdc2aa)
  [SUB-PROC] SubCenter Knowledge received response (303 chars, request_id=1080fbbdc2aa)
[22:59:38] WATCH [TTS] engine=edge voice=en-US-AvaNeural volume=75% req=35   <- NEXUS speaks an AI-generated answer
...
[22:59:50] BATON stt-capture: started (cpal-side capture, no baton pass)      <- A SECOND capture, with no wake word in between
[22:59:54] BATON stt-capture: started (cpal-side capture, no baton pass)
[22:59:54] BATON wake: cpal stream resumed (mic baton pass — frontend released mic)
[22:59:55] STT stt-capture: transcript = 'the glasses will be found in a different way.', intent = None
  [MAIN-CMD] ... Intent: unknown | Center: KnowledgeCenter | Evidence: None ...
  [SUB-PROC] SubCenter Knowledge received response (109 chars, request_id=87b9c989e5c9)
[22:59:59] WATCH [TTS] engine=edge voice=en-US-AvaNeural volume=75% req=50   <- NEXUS speaks a SECOND AI-generated answer
[23:00:04] EXIT NEXUS process exited.
```

Hand Mode's own telemetry (`[HAND] fps=... armed=...`) was printing continuously throughout, confirming Hand Mode really was active for the entire window above and this was **not** a Ghost session (no `[GHOST]`/`ghost:` lines anywhere in the log, and `HandCenter`/`Evidence: Strong` is the Hand entry path, not Ghost's).

What actually happened, stage by stage:

1. The wake word fired at `22:59:10` (ambient probability 0.982/0.992 — the user likely said something containing a NEXUS-like sound, or simply room noise crossed the acoustic threshold; this is the normal, expected wake-word false-positive rate the project already measures and tracks in `docs/research/wakeword/`).
2. STT capture started, timed out at 10s with nothing intelligible, transcript came back empty.
3. The pipeline **silently started a second capture** with no wake word (`stt-capture: started` at `22:59:23`, right after the empty transcript at `22:59:23`). This is the documented "1st miss silent auto-relisten" escalation tier from `docs/changes/+ earlier "STT Empty-Transcript & Mic Pipeline"` work — intentional, and not specific to Hand Mode.
4. That second capture got a real (non-empty) transcript, `'a glassless update.'` — almost certainly a mis-transcription of something the user actually said (very possibly a second, better attempt at a command, or ambient room audio). It did not match any deterministic intent, so it fell through to `KnowledgeCenter → WorkerBackend`, which is the **general-purpose "ask the cloud AI" fallback that exists in Normal Mode with or without Hand Mode** — any unparseable sentence heard by NEXUS normally gets treated as a question and answered. NEXUS spoke a 303-character AI-generated answer.
5. A **third** capture started at `22:59:50`, again with **no wake word logged in between**. It also produced a real transcript (`'the glasses will be found in a different way.'`), again fell through to the cloud-AI fallback, and NEXUS spoke a second, unrelated answer.
6. The process exited shortly after (apparently the user stopped it, unrelated to this bug).

This is exactly what the user described: it looked and sounded like Ghost Mode's hot-mic loop — one wake, then a back-to-back multi-turn "conversation" with the cloud AI, no repeated wake word needed — while Hand Mode (not Ghost) was the only thing on.

### 2.1 Root-cause split: two separate issues, only one in scope

Investigating this surfaced **two distinct problems**, and it is important to be explicit that only one of them is being fixed here:

**(A) — In scope, fixed by this change.** Hand Mode, while it is the *only* active mode, does not gate the wake word at all today. The ambient wake-word classifier (`wakeword_oww.rs`) runs continuously regardless of whether Hand Mode is on, and any wake-word firing followed by a parseable-but-unmatched sentence gets routed to the general cloud-AI fallback and spoken aloud — the exact same thing that happens in plain Normal Mode with no Hand Mode at all. The user is right that this should not happen while Hand Mode owns the user's attention: a camera-driven pointer session is a bad time for NEXUS to also be fielding half-overheard sentences and answering them out loud mid-gesture.

**(B) — Investigated, confirmed real, explicitly out of scope.** The back-to-back *second* relisten at `22:59:50` — starting a brand-new capture right after a **successfully transcribed and already-answered** turn, with no wake word at all — is a separate, pre-existing bug in the auto-relisten escalation logic that has nothing to do with Hand Mode. Per the project's own history (`AGENTS.md`, "STT Empty-Transcript & Mic Pipeline" entry, tier **E**): *"1st miss silent auto-relisten ..., 2nd single nag + deterministic close; streak resets on speech."* A streak is supposed to reset the moment real speech produces a real (non-empty) transcript — `'a glassless update.'` was exactly that, a successful transcription, just one with no matching intent. Tier E's own documented rule says the retry streak should have stopped there, not continued into a third unsolicited capture. This would reproduce in **plain Normal Mode, with Hand Mode off entirely** — it is a general wake-word/relisten defect, not a Hand Mode defect, and reproducing/fixing it needs its own isolated investigation (almost certainly in the frontend turn-end/relisten bookkeeping, not in `wakeword_oww.rs`'s native capture path, since that path's own logs show no "second relisten" trigger anywhere in Rust — see §2.2). Fixing (A) below happens to make (B) unobservable *during Hand Mode specifically*, because with the wake word suppressed there is nothing left to start a first capture, let alone a chained second one. It does **not** fix (B) in Normal Mode without Hand Mode. That is flagged here, explicitly, as a known remaining issue for a future session — see §7.

#### 2.2 Why (B) was ruled OUT of Hand Mode's own code

Before writing any fix, every Rust call site that can start a native STT capture was enumerated to make sure Hand Mode itself wasn't secretly re-arming the mic:

```
src-tauri/src/commands.rs:2769   pub fn start_stt_capture(origin: Option<String>)   <- IPC command, callable from the frontend
src-tauri/src/hotkey.rs:110      crate::wakeword_oww::start_stt_capture();          <- Ctrl+Space (deliberate user action)
src-tauri/src/hotkey.rs:174      crate::wakeword_oww::start_stt_capture();          <- Ctrl+Space (second variant)
src-tauri/src/wakeword_oww.rs:4043   start_stt_capture();                           <- the wake-word detector itself
```

`src-tauri/src/hand/mod.rs` and `src-tauri/src/hand/engine.rs` contain **zero** calls to `start_stt_capture`/`start_stt_capture_with_origin` and zero references to the STT/wake pipeline at all before this change (`grep -rn "hand::active\|hand::is_live\|crate::hand::" src-tauri/src` turned up only the known, documented Ghost↔Hand linkage in `ghost.rs`, `intent_parser.rs`'s `loose_hand_exit`, and `orchestrator.rs`'s `HandCenter` dispatch — nothing resembling a relisten loop). The second, wake-less capture at `22:59:50` has to have come from the frontend's own IPC-callable `start_stt_capture` command, which is exactly where the documented tier-E escalation logic lives. This is consistent with (B) being a pre-existing, Hand-Mode-independent defect.

## 3. The fix

### 3.1 Hand Mode suppresses the wake word unless Ghost is also active

New predicate, `src-tauri/src/hand/mod.rs:53-66`:

```rust
pub fn active() -> bool {
    SESSION.lock().is_some()
}

/// Hand Mode alone does not listen: while it is the only thing active, the wake word stays
/// suppressed (same choke point as a meeting or a manual pause) so ambient speech while gesturing
/// at the screen never turns into a cloud question-and-answer turn — field report: "it felt like
/// Ghost Mode was listening". The instant Ghost Mode is ALSO active (`handWithGhost`, or Ghost
/// started while Hand was already on) this returns false again: Ghost is supposed to listen, and
/// Hand just rides along as its pointer. A deliberate hotkey press still starts a capture directly
/// (`start_stt_capture`), bypassing this ambient gate entirely, so the hotkey + "exit hand mode" (or
/// any other command) keeps working even while this is suppressing the wake word.
pub fn suppresses_voice() -> bool {
    active() && !crate::ghost::session_active()
}
```

`active()` was chosen (a Hand session exists at all) rather than the `LIVE` flag (whether the pointer is actually moving the real mouse), because the user's complaint applies equally to `handMousePreview` mode: a preview session is still "Hand Mode is on and has my attention", even though it never touches the real cursor. `crate::ghost::session_active()` is the exact same function Hand Mode already uses elsewhere (`hand/mod.rs`'s `apply()` function, the `Action::ArmChanged` branch, when deciding whether it owns the `Esc` key — see `src-tauri/src/hand/mod.rs:524-533`) to decide "is Ghost the one actually in charge right now", so this reuses an established, already-tested pattern rather than inventing a new one.

Wired into the actual acoustic gate, `src-tauri/src/wakeword_oww.rs:1814-1827` (the same choke point that already suppresses wake detection for meetings and the manual "Pause NEXUS" button):

```rust
// Check if wake detection should be suppressed
let meeting_state = super::MEETING_STATE.get();
// Hand Mode alone (no Ghost session) owns the mic for pointer control, not
// conversation: see `hand::suppresses_voice` for why. Checked every chunk, same as
// the meeting/manual-pause gate below, so arming/disarming hand mode takes effect
// immediately rather than needing a fresh wake cycle.
let hand_muted = crate::hand::suppresses_voice();
let suppressed = hand_muted
    || meeting_state
        .map(|s: &std::sync::Arc<crate::meeting_detect::MeetingState>| {
            s.should_suppress_wake()
        })
        .unwrap_or(false);

if suppressed {
    ...
```

And the existing diagnostic logging (so a future `[WAKE]`-suppression log line names the real reason instead of a generic "unknown mute source" when Hand Mode is why), `src-tauri/src/wakeword_oww.rs:1848-1871`:

```rust
if should_log {
    // Main Center STT-director reason (doc 74 P3): the
    // bare counter never said WHY capture was muted.
    // Hand Mode is checked first: it is the actual reason whenever it is true,
    // even if a meeting/pause flag also happens to be set.
    let note = if hand_muted {
        "hand mode active, no ghost session (voice muted — see docs/features/121)"
    } else {
        meeting_state
            .map(|s| {
                crate::center::stt_mute_note(crate::center::stt_gate(
                    s.manual_pause.load(Ordering::Relaxed),
                    s.tts_playing.load(Ordering::Relaxed),
                    s.detection_enabled.load(Ordering::Relaxed)
                        && s.meeting_active.load(Ordering::Relaxed),
                ))
            })
            .unwrap_or("unknown mute source")
    };
    tracing::warn!(
        "wake: detection suppressed by hand-mode/meeting/TTS-mute state ({} chunks dropped so far) — \
         if you are speaking and nothing happens, check hand mode / meeting mode / pause state",
        n
    );
    tracing::debug!("wake: suppression reason: {note}");
}
```

(The doc-reference string inside that log note says `docs/features/121` as a stable placeholder pointing back at this change; there is no numbered `docs/features/121` file specifically for this — the authoritative writeup is this file, `docs/changes/129-...md`. If a future session renumbers or restructures the docs tree, update that string too.)

#### Why `center.rs`'s `stt_gate`/`stt_mute_note` were deliberately left untouched

`center.rs` already has a clean, pure, well-tested "director" pair — `stt_gate(manual_pause, tts_playing, meeting_suppress) -> SttGate` and `stt_mute_note(gate) -> &'static str` — that exists specifically so "why is the mic muted" has one canonical answer (see the `director_tests` module in `center.rs`, and AGENTS.md's "doc 74 P3" reference). Extending that enum with a fourth `SttMuteReason::HandMode` variant and a fourth boolean parameter was considered and rejected for this change: it would touch a shared, tested pure function and its full call-site list for a benefit that is purely cosmetic (a nicer debug-log string), and the inline `if hand_muted { ... } else { ...the existing stt_gate call... }` above achieves the identical user-visible behaviour with zero risk to `center.rs`'s existing contract and tests. If a later change finds another place that needs to explain "is Hand Mode why the mic is off" more formally, promoting this into `SttMuteReason::HandMode` at that point is the right time to do it.

#### Why the hotkey (Ctrl+Space) was deliberately left reachable

Only the **ambient wake-word classifier's** decision to start a capture is suppressed. `hotkey.rs`'s two call sites (`src-tauri/src/hotkey.rs:110` and `:174`) call `wakeword_oww::start_stt_capture()` directly and are completely untouched by this change — they do not pass through the `suppressed` check at all, because that check only lives inside the continuous per-chunk audio-callback loop that classifies ambient audio for the wake word, not inside the capture-start function itself. This is intentional: a hotkey press is an explicit, deliberate "I want to talk to NEXUS right now" action by the user, and it is the documented, reliable way to issue a command — including "exit hand mode" — while the ambient wake word is muted. See §4 for the full list of exit paths this preserves.

#### Why TTS barge-in ("stop") still works

The existing v4 barge-in branch immediately below the suppression check (`src-tauri/src/wakeword_oww.rs:~1877` onward, `tts_only = tts_playing && !meeting_active && !is_paused`) is completely independent of `hand_muted` — it was not touched, and does not need to be. If Hand Mode is suppressing the wake word *and* NEXUS happens to be mid-sentence from a command the user just issued via the hotkey, sustained speech still earns one exact-match verify attempt to catch "stop", exactly as it already does during a meeting. If Hand Mode is suppressing the wake word and NEXUS is *not* speaking, `tts_only` is `false` and the chunk is dropped in total silence, which is the behaviour the user asked for.

### 3.2 Idle-exit timer: 10 minutes → 1 minute

The pure engine, `src-tauri/src/hand/engine.rs`, already had exactly the concept the user asked for — `Config.idle_exit_ms`, documented in its own struct comment as *"No hand at all for this long while disarmed ends hand mode"* — just set to a 10-minute default (`idle_exit_ms: 600_000` inside `Config::new()`, line 168 at the time of writing). That generic engine-level default was **deliberately left unchanged** — `Config::new()` is the pure, generic constructor the engine's own unit tests build against directly (e.g. `hand::engine::tests::hand_mode_ends_itself_when_no_hand_is_seen_for_ten_minutes`, which is specifically testing the 10-minute *engine* default and must keep passing unmodified), and other future callers of `Config::new()` should get a sane generic default, not a NEXUS-product-specific tuning choice baked into the pure state machine.

Instead, the override lives in the glue layer, `src-tauri/src/hand/mod.rs:868-879`, in `start()`, right alongside the other settings-driven overrides (`handGestures`, `handGesturesDesktop`, `handSensitivity`) that already follow this exact pattern:

```rust
let now = Instant::now();
let mut cfg = Config::new(screen);
cfg.gestures = read_bool(app, "handGestures", true);
cfg.gestures_desktop = read_bool(app, "handGesturesDesktop", false);
let (sens, precision) = effective_pointer(app);
cfg.sensitivity = sens;
cfg.precision = precision;
// Auto-exit after this long with no hand at all (counted from the same moment the hand first
// disappeared that the 5 s disarm uses — not restarted at disarm). The engine's own generic
// default (10 min, in `Config::new`) is a fallback for callers that build a `Config` directly;
// product behaviour is 1 minute unless overridden by `handIdleExitMs`.
cfg.idle_exit_ms = read_setting(app, "handIdleExitMs").and_then(|v| v.as_u64()).unwrap_or(60_000);
```

A new, optional, undocumented-in-Settings-UI tuning key, `handIdleExitMs`, was added (loose read, same style as every other hand-mode setting — see the module's own "settings (loose reads, like `directed.rs`)" doc comment) so this can be tuned later without another code change, but its **default is exactly 1 minute (60,000 ms)** as asked. There is no Command Hub UI control for it; it is a backend tuning knob only, matching how `handSensitivity` worked before it got its own UI treatment.

#### What "1 minute" actually measures

This matters enough to spell out precisely, because the engine's timers interact: the moment the hand is lost (not detected in a frame), the engine records `lost_since = now` once and does **not** reset that clock while the hand stays missing. From that single starting point:

- At **600 ms** (`lost_release_ms`, unchanged): any held click/drag is released.
- At **5,000 ms** (`lost_disarm_ms`, unchanged), *if the pointer was armed*: it disarms (`Action::ArmChanged { armed: false, ... }`, logged as `[HAND] DISARMED (hand-lost)`), but the mode itself stays on — camera still running, chip still showing.
- At **60,000 ms** (`idle_exit_ms`, now 1 minute, *was* 10 minutes), *regardless of whether it ever got armed in this stretch*: the mode exits entirely (`Action::ExitMode { reason: "idle" }` → `src-tauri/src/hand/mod.rs`'s `apply()` spawns `stop(&app, "idle")`, which tears down the camera window and ends the session — logged as `[HAND] exit requested (idle)` then `[HAND] mode OFF (idle)`).

So: **a full minute with no hand visible to the camera at all — not one minute after disarming, a clean, single 60-second window from the moment the hand first vanishes — now fully exits Hand Mode on its own.** The hand reappearing at any point resets the whole clock back to zero (`self.lost_since = None;` the moment a real frame with a hand arrives — `src-tauri/src/hand/engine.rs`'s `step()`, right after the `let Some(h) = hand else { ... }` block).

## 4. "Did you have an exit for Hand Mode?" — full inventory

Yes — Hand Mode already had **five** independent ways out before this change, and this change adds a **sixth** (the shortened idle timer, functionally a faster version of one that already existed). None of them needed to be newly built; this section documents and verifies each one, since the user asked the question directly and it deserves a complete, explicit answer rather than an assumption.

| # | Mechanism | Trigger | What it does | Where |
|---|---|---|---|---|
| 1 | **Voice, exact phrase** | Say (via hotkey or, previously, ambient wake — now hotkey only while solo) one of: *"exit hand mode", "stop hand mode", "close hand mode", "quit hand mode", "leave hand mode", "hand mode off", "turn off hand mode"*, and 9 more exact variants for "hand mouse"/"gesture mode"/"camera mouse" | Full exit: camera off, session destroyed | `src-tauri/src/intent_parser.rs:4288-4293` (`const EXIT`), dispatched as `ParsedIntent::ExitHandMode` → `src-tauri/src/orchestrator.rs:6719-6726` → `crate::hand::stop(&app, "voice")`, speaks *"Hand mode off, sir. The camera is off."* |
| 2 | **Voice, loose/fuzzy phrasing** | *Any* short sentence (≤7 words), **while Hand Mode is already on**, containing one leaving word (`exit`, `quit`, `stop`, `close`, `leave`, `end`, `disable`, `cancel`, `off`, `kill`, `done`, `finish`) **and** one Hand-Mode word (`hand`, `hands`, `camera`, `webcam`, `gesture`, `gestures`, `tracking`, `handmode`, `mode`, `more`, `mod` — the last three specifically cover common Whisper/STT mis-hearings of "hand mode"). Bare `"exit"` or `"quit"` alone also matches. The word list explicitly excludes `"ghost"` so it never steals Ghost's own exit phrase. | Same full exit as #1 | `loose_hand_exit()`, `src-tauri/src/intent_parser.rs:4258-4278`; verified to correctly accept phrasings like *"exit the handle mode"* (an STT mis-hearing of "hand" as "handle" — the word-level match still fires on "exit" + "mode") by `intent_parser::power_and_session_tests::leaving_hand_mode_works_in_many_words_but_only_while_it_is_on`, confirmed passing in §6 below |
| 3 | **Voice, natural language** | Sentences like *"give me my mouse back"*, resolved by intent (who acts / what is controlled / how) rather than fixed phrasing | Full exit | `src-tauri/src/mode_intent.rs` (per `docs/features/117-cross-os-screen-adaptation-and-intent-plan.md` and the field-test-3 entry in `AGENTS.md`); `mode_intent::tests::hand_mode_in_many_words` passes (§6) |
| 4 | **Gesture, no voice at all** | Hold a closed fist for 1.2 s (disarms the pointer, chip shows "disarmed"), then **keep the fist closed** for a further 2 s | Full exit (camera off, session destroyed) | `src-tauri/src/hand/engine.rs`: `fist_disarm_ms: 1200` then `fist_exit_ms: 2000`, `Action::ExitMode { reason: "fist" }`; this is the exact path visible twice in the user's own log (`[HAND] DISARMED (fist-hold)` → `[HAND] exit requested (fist)` → `[HAND] mode OFF (fist)`, at both `22:58:35` and `22:59:44` in the transcript) |
| 5 | **No hand seen at all** | No hand detected by the camera for the idle window | Full exit | `idle_exit_ms` — **this change shortens it from 10 minutes to 1 minute**, see §3.2 |
| 6 | **Esc key** | Press `Esc` while the pointer is armed (and Ghost does not currently own `Esc`) | **Disarms only** — releases the pointer, stops moving the mouse/clicking, but does **not** end the session (camera stays on, chip stays up) | `src-tauri/src/hand/mod.rs:484-504` (`register_esc`/`unregister_esc`), calling `disarm(&handle, "esc")`. Deliberately *not* a full exit — it is the "something's about to go wrong, let go of the mouse right now" panic button, matching the exact same semantics Ghost Mode's own `Esc` has always had in this project. To fully leave after an `Esc` disarm, use #1-#5. |

Row 6 is the one nuance worth calling out explicitly to the user: **`Esc` alone does not close Hand Mode** — it only releases the pointer. That has always been true (it predates this change) and nothing here altered it. If the expectation is "`Esc` should fully exit Hand Mode", that would be a deliberate, separate product decision to make (and would need to be reconciled with Ghost Mode's own very deliberate "`Esc` only disarms/cancels, it is not a mode-exit" design, documented at length earlier in `AGENTS.md`'s Ghost Mode history) — it was not asked for here, and is flagged as an open question rather than silently changed, in §7.

### 4.1 Interaction with the voice-suppression fix (§3.1)

With wake-word detection suppressed while Hand Mode is solo-active, mechanisms #1, #2, and #3 above (all voice-driven) can still only be reached through the **hotkey** (press Ctrl+Space, then say the phrase) — the ambient wake word will no longer pick them up on its own, by design, since that is the exact behaviour being fixed. Mechanisms #4, #5, and #6 (gesture-based, idle timeout, and `Esc`) are completely unaffected, since none of them go through the voice pipeline at all. In practice this means: **a hand gesture or the hotkey will always get you out of Hand Mode; ambient speech alone, deliberately, now will not** — which is precisely the "only control, don't listen" behaviour that was asked for.

## 5. What was deliberately NOT changed

- **Ghost+Hand combined behaviour.** When Ghost Mode is active (whether it started Hand Mode via `handWithGhost`, or Hand Mode was already running and Ghost started afterwards), `crate::ghost::session_active()` is `true`, `hand::suppresses_voice()` returns `false`, and the wake word / hot-mic loop behaves exactly as it always has for Ghost. This is the literal "you can control Hand Mode while [in] Ghost Mode" the user asked to keep.
- **The general cloud-AI fallback itself** (any unparseable sentence in Normal Mode gets answered by the Worker/9Router). That is long-standing, intentional Normal Mode behaviour, documented throughout `AGENTS.md`, and is not specific to Hand Mode — it was not touched.
- **The second-relisten bug (§2.1, item B).** Confirmed real, confirmed independent of Hand Mode's own code, confirmed to live in the frontend's IPC-callable relisten path rather than in `wakeword_oww.rs`'s native capture loop. Left for a dedicated follow-up (see §7) since fixing it correctly needs its own investigation into the turn-end/relisten bookkeeping in the frontend (`frontend/src/audio/`, `frontend/src/net/` per the project's existing structure) and touching that code was explicitly out of scope for what was asked here.
- **`center.rs`'s `stt_gate`/`stt_mute_note` pure functions and their test suite.** See the dedicated explanation in §3.1.
- **The pure engine's generic `Config::new()` default for `idle_exit_ms`.** Stays at 10 minutes; only the product glue layer (`hand/mod.rs::start()`) now overrides it to 1 minute. See §3.2.
- **`Esc`'s disarm-only semantics.** Flagged, not altered — see §4, row 6.
- **The hotkey path.** Left fully reachable on purpose — it is now the sole voice entry point into Hand Mode's commands (including its own exit phrases) while the ambient wake word is muted.

## 6. Verification

All changes are additive and localised to `src-tauri/src/hand/mod.rs` (one new function, one new line in `start()`) and `src-tauri/src/wakeword_oww.rs` (one new boolean OR'd into an existing suppression check, one logging branch). No public function signatures changed, no existing struct fields removed or retyped, no settings keys removed.

```
cargo check --lib --features custom-protocol,admin-brain
  → clean, 69 pre-existing dead-code warnings (unrelated, unchanged count), 0 errors, 0 new warnings

cargo test --lib --features custom-protocol,admin-brain hand:: -- --test-threads=1
  → 86 passed; 0 failed

cargo test --lib --features custom-protocol,admin-brain wakeword_oww:: -- --test-threads=1
  → 65 passed; 0 failed

cargo test --lib --features custom-protocol,admin-brain hand_mode -- --test-threads=1
  → 6 passed; 0 failed, including:
    - hand::engine::tests::hand_mode_ends_itself_when_no_hand_is_seen_for_ten_minutes
        (confirms the pure engine's own 10-minute default is untouched)
    - hand::engine::tests::keeping_the_fist_closed_after_a_fist_hold_disarm_leaves_hand_mode
    - intent_parser::power_and_session_tests::hand_mode_phrases_enter_exit_and_toggle_live
    - intent_parser::power_and_session_tests::leaving_hand_mode_works_in_many_words_but_only_while_it_is_on
    - intent_parser::power_and_session_tests::sentences_that_merely_mention_hand_mode_are_not_commands
    - mode_intent::tests::hand_mode_in_many_words
```

### Not run (needs the real app)

- A real live session confirming that, with Hand Mode solo-active, ambient speech (including false-positive wake-word triggers) produces **zero** `[MAIN-CMD]`/`[SUB-PROC]`/TTS-answer log lines, while `[HAND] fps=...` telemetry keeps printing uninterrupted.
- A real live session confirming Ghost Mode entered while Hand Mode is already on (or `handWithGhost` starting Hand Mode from Ghost) still listens and answers normally.
- A real timed test that no hand in front of the camera for ~60 s produces `[HAND] exit requested (idle)` → `[HAND] mode OFF (idle)` (previously this took 10 minutes to observe, so it was never practically watched end-to-end in a live session before either).
- Confirming the hotkey → "exit hand mode" path out loud, now that it's the documented sole voice route while ambient listening is muted.

## 7. Open items for a future session

1. **The second-relisten bug (§2.1(B)).** Real, reproducible in plain Normal Mode (Hand Mode not required), and not fixed by this change — only rendered unreachable *during solo Hand Mode* as a side effect of §3.1. Needs its own investigation into the frontend's turn-end/relisten escalation logic (tier E: "1st miss silent auto-relisten ..., 2nd single nag + deterministic close; streak resets on speech") to find why a **successful, non-empty** transcription did not reset the retry streak the way the existing design says it should.
2. **Whether `Esc` should become a full exit for Hand Mode, not just a disarm.** Not requested here; flagged for a product decision given it would be a deliberate change to long-standing, Ghost-Mode-matching semantics (§4, row 6).
3. **Whether `handIdleExitMs` deserves a Command Hub UI control.** It exists today purely as a backend tuning key (loose settings read, no UI), consistent with how several other `hand*` keys started out before some of them later got dedicated Settings UI rows (per the `HandModeSection` additions in `docs/changes/127-...`).
4. A **Settings UI toggle is not currently offered** for the new voice-suppression behaviour itself — it is unconditional whenever Hand Mode is solo-active, matching the explicit, unconditional wording of the request ("hand mode in normal mode should only control... should not listen"). If a future need arises for an opt-out (e.g. a user who *wants* ambient listening to keep working during solo Hand Mode), that would need its own settings key (e.g. `handMutesVoice`, default `true`) gating the `suppresses_voice()` check — not built here because it was not asked for, and an unconditional default matching the literal request is the more honest implementation of what was actually said.
