# 127 — Hand mode: the camera as a pointer (P0 + P1 + P2 core, READ-ONLY by default)

Plan: [features/115](../features/115-camera-hand-mouse-plan.md). Research: [features/114](../features/114-camera-attention-and-hand-gestures-plan.md).

## What it is

A sub command center (`HandCenter`) with one mode. Say **"hand mode"** (or "hand mouse", "camera mouse"): a small
camera window appears bottom-left (skeleton + status + a mini screen map). The pointer is OFF until you arm it with
**palm, fist, palm, fist** (show an open palm, close to a fist, open, close). The same gesture disarms.

**Read-only by default**: armed, it shows where it PREDICTS the cursor is and what it WOULD click (the mini map and
a flash such as "click"), but the real mouse is not touched. Say **"hand mouse live"** to let it click,
**"hand mouse preview"** to go back. Persistent default: `handMouseLive` in settings.

| Say | Does |
|---|---|
| hand mode / hand mouse / start hand mode / camera mouse | camera window on, engine running (pointer idle until armed) |
| exit hand mode / stop hand mouse / hand mode off | camera off, window destroyed, any held button released |
| hand mouse live / hand mouse preview | real mouse vs read-only (releases anything held when going read-only) |

Gestures once armed: move the hand = move the cursor (relative, trackpad-like); thumb+index pinch quick = click, held
> 300 ms = drag; thumb+middle pinch = right click; index+middle up and move the hand up/down = scroll; fist = pause.

## How it was built (and what was NOT copied)

* `reynaldichernando/pinch` (closest project) has **no licence file**, so none of its code was copied. It was read to
  learn the failure modes (jitter, click drift, unstable landmarks) and the design here is independent.
* One Euro filter: written from the published algorithm (Casiez et al., CHI 2012), not copied.
* MediaPipe `@mediapipe/tasks-vision` 0.10.21 (Apache-2.0) + `hand_landmarker.task` float16 v1 (Apache-2.0,
  7.8 MB, SHA-256 pinned in `scripts/hand-assets.lock.json`, trust-on-first-download) via
  `scripts/sync-hand-assets.mjs` into git-ignored `frontend/public/hand/` (about 17 MB incl. the SIMD wasm).
  `nexus build` runs the sync first.

## Files

* `src-tauri/src/hand/filter.rs` One Euro filter (5 tests).
* `hand/pose.rs` finger-extension poses, pinch ratio in palm widths, palm centre, plausibility (6 tests).
* `hand/engine.rs` the pure state machine (17 tests, fake clock): arm gesture, relative mapping with speed gain,
  rewind-on-pinch click, drag, right click, scroll, lost-hand release, long-absence disarm.
* `hand/mod.rs` session, IPC (`hand_frame`, `hand_error`), mouse output thread (enigo), safety gate, watchdog,
  `[HAND]` metrics line every 5 s (fps, hand %, p50 latency and inference time), optional `handTrace` landmark log.
* `dyn_windows.rs` `WindowConfig::hand()`; `mic_permissions::allow_media_for_window` (camera auto-allow for our origin
  on a window created later); `capabilities/hand-cap.json`; `vite.config.ts` input `hand`.
* Intent: `EnterHandMode` / `ExitHandMode` / `HandMouseLive` (`intent_parser.rs`, strict exact phrases), `center.rs`
  `HandCenter` (16th sub-center; entering/going live counts as a side effect), `orchestrator::run_hand_mode`.
* Frontend `frontend/hand.html`, `src/hand/{main.ts,payload.ts,hand.css}` (8 tests).
* Settings (all in `NexusSettings`): `handMode` (default true: voice may start it), `handMouseLive` (default false),
  `handTrace` (default false).

## The rules that keep it safe

* The camera only runs while the mode is on; the window IS the indicator. Only 21 numbers per frame cross IPC; no
  frame is stored or sent. `handTrace` (off) writes landmark numbers only.
* Armed state is separate from mode state; a disarmed engine never produces mouse output.
* A click, drag or scroll is refused in protected windows (`live::safety` denylist) and elevated windows.
* The physical mouse always wins: if the real cursor is > 40 px from where we put it, the pointer disarms.
* Esc disarms (global Esc is registered only while armed, and not when a Ghost session already owns it).
* Losing the hand 0.6 s releases a held button; 5 s disarms. A release is always sent (stuck buttons are worse).
* Click lands where the cursor was 150 ms BEFORE the fingers closed, and the cursor is locked until release: the
  pinch itself can no longer pull the click off target (the failure every public hand-mouse reports).

## Verified

Rust lib 1434 passed (+33), e2e 2, offline 10, user cmds 7; vitest 267 (+8); tsc clean; `npm run build` ok; dead-code
ratchet 157, none new; IPC contract OK (193 commands / 130 invoked); turn-end + hygiene OK. The click-rewind test was
mutation-checked (it caught the first version being vacuous). In a Chromium page: the wasm + model load offline in
0.46 s with the GPU delegate, a blank 640x480 frame costs p50 20 ms; layout checked at 300x250.

## NOT verified (needs a real camera and you)

* Camera in WebView2 inside the packaged app, real-hand tracking quality, latency and CPU numbers (the `[HAND]` line
  prints them every 5 s; P0 gate), arming gesture false positives while talking with your hands.
* Live mouse output (default is off by design). Multi-monitor (primary only). Glasses / lighting.
* The Ghost-mode Esc interaction when both are active (Esc ends Ghost; pointer then stays armed until Esc again or exit).

## Round 2: the rest of the plan that did not need new models

* **Snap-to-control** (`hand/snap.rs`, 5 tests): a click that lands just outside a control moves to the nearest one
  (UIA, one query per click, radius 36 px, containers > 900x500 and specks < 8 px ignored, inside-a-control = untouched).
  `handSnap`, default on.
* **Voice pointer clicks**: "click here / that / this", "double click", "right click" click at the current cursor
  (hand pointer or real mouse) behind the same protected/admin gate; silent on success.
* **Arm/disarm beep** (system sound) because the HUD is easy to miss mid-gesture.
* **Start with Ghost** (`handWithGhost`, default off): Ghost entry starts hand mode, Ghost end or Esc stops it, but only if
  Ghost started it.
* **Command Hub card** "Hand Mode (camera pointer)": on/off, allow, live, snap, with-Ghost (`hand_start/stop/status`).
* Verified: Rust lib 1440, vitest 267, tsc, IPC 196/133, dead-code 157 none new, turn-end + hygiene.

## Round 3: field test 1 fixes (A-D)

Cause analysis and plan: `docs/features/115` "Field test 1". The log proved the engine armed six times, so the
complaint was UX, not detection.

* **A. Visible preview**: stage overlay with a dashed virtual-cursor ring (preview only), click ripple, and one chip
  (`HAND · palm, fist, palm, fist to arm (n/3)` / `HAND ARMED · PREVIEW · not clicking` / `HAND ARMED · LIVE`).
  Hides on `hand:stopped` or 2.5 s without state. 7 vitest.
* **B. One-way arming**: palm-fist-palm-fist only arms; repeating it no longer disarms. Disarm = fist held 1.2 s (only
  after the hand has opened once since arming), Esc, real mouse movement, 5 s without a hand, or "exit hand mode".
  Rising two-tone beep = armed, falling = disarmed. Deviation from the plan: no spoken line (speaking mid-gesture would
  supersede other turns and echo into the hot mic); the chip is always visible instead.
* **C. Gesture tolerance**: pose debounce needs 2 consecutive frames and 80 ms (a 10 fps camera arms with 200 ms poses),
  gesture window 6 s, a fist may have one barely-extended finger (ratio < 1.28, average <= 1.15), a clearly pointing
  hand is still not a fist. Every stable pose change prints finger ratios so thresholds can be tuned on a real hand.
* **D. Frame rate**: the canvas was being re-allocated every frame (width/height assignment) - fixed. At start the first
  25 frames are timed on the GPU delegate; if the median is over 30 ms the CPU delegate is timed too and the faster
  wins (CPU needs a 15 % lead). The camera format and the verdict print once.
* Not changed: the 80 ms latency / 24 fps targets are still goals; the new `[HAND] ... calibration:` line and the next
  `[HAND] fps=` lines say whether they are met on your laptop.

## Round 4: field test 2 fixes (items 1-6 of the plan)

Analysis: `docs/features/115` "Field test 2". Camera pipeline ~6 fps, model ~27 ms, arm window too strict.

1. **Measure**: `hand_stats` every 5 s (see AGENTS.md line) with both camera rates, processed rate, driver rate, true
   capture-to-landmarks latency (p50/p95 and how it was measured), page visibility, exposure, battery.
2. **Frame source**: camera track reader (`maxBufferSize: 1`, newest frame only) by default, rVFC kept as fallback and
   as a counter; camera constraints ask for >= 24 fps; `NEXUS_NO_ECOQOS=1` for the power A/B.
3. **Aligned picture**: the canvas shows the analysed frame with its skeleton; a "camera N fps - add light" note appears
   under 15 fps.
4. **Latency**: capture time from the camera, pointer lead = 70 % of the measured delay (smoothed, clamped, never on a
   still hand), One Euro retuned (1.5 / 18).
5. **No confirmation**: live by default (setting is now `handMousePreview`; old `handMouseLive` is gone), per-step arm
   timing, tolerant palm; all safety stops unchanged.
6. **Start**: camera and model in parallel, chip immediately, spoken line shortened to "Hand mode on, sir."

Tests: Rust +4 engine (slow gesture, pause restarts, lead, lead clamp), +1 pose; vitest +4. In Chromium with a fake
camera the track path delivered ~26 fps and reported capture time, stats and calibration.

## Round 5: field test 3 (index click, no picture, aligned chip) + intent + screens

* **Click**: bending the index finger (ring >= 1.18 and pinky >= 1.12 stay open; the real flicks in your log had
  ring 1.24-1.37; frames of a hand closing into a fist had ring <= 1.01 and are rejected) or the thumb-index
  pinch. One state machine: both at once is one click; 150 ms refractory; a hold over 450 ms starts a drag.
* **No picture**: the camera window is an invisible 8x8 transparent page (still "visible" to the OS so the
  WebView is not throttled), nothing is decoded or drawn. `handShowCamera` shows the debug picture.
* **Chip**: short text, placed right of the orb capsule and centred on it, flips to the left near the screen
  edge; says "dim room, add light" when the camera is under 15 fps.
* **Intent**: `mode_intent.rs` (see doc 117). **Screens**: `Mon`, per-monitor gain, crossing, gap snap,
  periodic refresh. **OS**: live output only where implemented and tested (Windows); elsewhere preview + honest line.
* Tests: Rust +21 (engine 4 click + 4 monitor, pose 2, mode_intent 10, parser 1), vitest +7.

## Known limits / next

* Arming without voice needs the camera to be on already (the mode); there is no camera-free gesture. With hand mode
  on, the gesture arms/disarms.
* Not built: two-hand zoom, swipe/thumbs gestures from doc 114 (thumbs would answer "Shall I...?" offers; needs a
  thumb pose and the offer API), head-pointer fallback with the Face Landmarker, absolute mapping mode, spoken arm/disarm
  confirmation (beep + HUD only), multi-monitor.
