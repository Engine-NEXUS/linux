# 115 — Camera as a mouse: hand moves the cursor, pinch = click (PLAN ONLY)

Extends [114](114-camera-attention-and-hand-gestures-plan.md) (attention + gestures). Nothing here is built.
Same rules as 114: camera is opt-in and off by default, no frames stored or sent, voice and Ghost mode never depend on it.

## What was asked

Camera sees the hand; the cursor follows it; pinching the fingers = click / select. Research similar projects and plan.

## Short answer

Yes, this is a known, working technique, and many people have built it. Moving the cursor and clicking works well enough for
everyday use. It will not be as precise as a real mouse, so it should sit beside the mouse and voice, not replace them.

- Cursor movement and large targets (tabs, buttons, scrolling, dragging windows): good.
- Small targets (a 12 px checkbox, selecting one character of text): poor. Use voice ("click the search bar", already in Ghost mode) or the real mouse for those.
- Best design: **hand = coarse pointer, voice = precise target**. NEXUS already clicks named controls through UI Automation; the hand only needs to get near.

## Similar projects found (what each teaches)

| Project | What it does | Take-away for NEXUS |
|---|---|---|
| [reynaldichernando/pinch](https://github.com/reynaldichernando/pinch) ([write-up](https://gigazine.net/gsc_news/en/20241124-selfmade-apple-vision-pro-interface)) | Closest match: Windows, MediaPipe hand landmarks, pinch click. Moved from Python+OpenCV (lag) to a web front end doing detection and Rust (Tauri) doing the mouse | Same stack shape as ours (Tauri + WebView + Rust). Reported problems: cursor drifts when pinching, landmarks unstable at some angles, jitter even with a still hand. Their fixes: One Euro filter, pinch distance relative to hand size, linear remap so screen edges are reachable without the hand leaving the frame |
| [Pythonpreran/Aircursor](https://github.com/Pythonpreran/Aircursor) (Apache-2.0) | Move, pinch click, scroll via MediaPipe + OpenCV + PyAutoGUI | Simple reference for gestures; fixed pixel pinch threshold is the weak point |
| [small-cactus/handTrack](https://github.com/small-cactus/handTrack) (MIT) | Thumb–index pinch = click, thumb–middle = scroll | Shows the two-pinch pattern (click vs scroll); README itself says the threshold depends on hand size |
| [zunza5/HandPointer](https://github.com/zunza5/handpointer) | Accessibility-minded; filtering, debouncing, gesture state machine | Confirms we need a state machine, not raw thresholds |
| Hawk3388/hand-tracking, gesturix, AirControl, HandX (Store app) | Windows hand-mouse apps (Python, some packaged) | Proof it ships on Windows; all Python-grade latency/stability |
| [Google Project Gameface](https://developers.googleblog.com/en/project-gameface-makes-gaming-accessible-to-everyone/) (open source) | Head movement moves the cursor, facial gestures click; MediaPipe Face Landmarker + blendshapes | We already plan Face Landmarker for attention (114): head-pointer and "raise eyebrows to click" come nearly free later as a hands-free fallback. Reviewers note a laptop webcam struggles in poor light |
| Leap Motion / Ultraleap | Dedicated IR hand sensor | Users found pinch accuracy varies with hand angle; ChimeraX guide drives the cursor from the **palm**, not the fingertip, because fingertips are noisy; roughly 10x worse than a mouse for centimetre-precise picks. A plain webcam is worse than a depth sensor |
| Apple Vision Pro / HoloLens "gaze + pinch" ([design principles](https://arxiv.org/pdf/2401.10948), [Vision Pro accuracy study](https://arxiv.org/pdf/2406.00255)) | Eye picks the target, pinch confirms | The reason it feels good: the eye does the precise pointing. A webcam can't match eye tracking precision (2–4° error, see 114), but **head/face direction + hand pinch** is a workable cousin. Study found interaction design (big targets, feedback) mattered more than raw tracking accuracy |
| One Euro filter ([Casiez et al., CHI 2012](https://cristal.univ-lille.fr/~casiez/1euro)) | Speed-adaptive smoothing: strong when slow, light when fast | Standard answer to hand-tracking jitter; Snap's Spectacles kit uses it on hand landmarks |

Not found: any native (Rust/C++) hand-mouse project with published latency numbers. Every open project is Python or Python-plus-web. So our latency and CPU targets below are goals to measure, not facts.

## The four problems every project hit, and the design answer

1. **Jitter** (cursor shakes with a still hand). Filter the pointer, not the raw landmarks: One Euro filter on the pointer position (min-cutoff low, then raise beta until fast moves stop lagging). Reset the filter when the hand is lost.
2. **Click drift** (the index finger moves while pinching, so the click lands off-target). Three layers: (a) drive the pointer from a steadier point than the index tip: the **palm/knuckle centre (middle-finger base)** or the thumb tip, not the pinching fingertip; (b) **freeze the pointer the moment the pinch starts to close** (pinch-in-progress at ~70 % closed), and release the freeze only after pinch opens; (c) for drags, keep the freeze off after a 150 ms hold so dragging works.
3. **Pinch threshold depends on distance/hand size.** Measure thumb–index distance divided by palm width (wrist to middle-finger base), not pixels. Use hysteresis: close at < 0.25, open at > 0.40 (starting values, to be tuned on your hand).
4. **Reach / fatigue / gorilla arm.** Lifting an arm in the air tires fast, and low gain means constant re-reaching ("clutching"). Answers: (a) **relative mode as the default**: palm movement moves the cursor like a trackpad, and the hand can rest and re-centre (open palm → fist → open palm re-centres = clutch); (b) a small **active zone** (about 40 % of frame width, in comfortable reach) mapped linearly to the whole screen with mild speed-based acceleration, so edges are reachable without the hand leaving the frame; (c) optional elbow-on-desk posture works, and the camera angle is a choice (see below); (d) a visible fatigue guard: after 10 minutes of continuous use, suggest voice.

## Interaction design

Two modes, never ambiguous:

- **Gesture mode** (114): swipes, thumbs, fist stop. No cursor movement.
- **Pointer mode**: the hand moves the cursor. Entered and left deliberately:
  - Enter: **open palm held 0.6 s** (same clutch as 114) or the phrase "hand mouse on" (optional voice). A small on-screen ring appears (shows tracking state, clicks, and that the camera is on).
  - Leave: fist held 0.5 s, hand out of view for 3 s, "hand mouse off", Esc, or any real mouse/keyboard input beyond a threshold (the physical mouse always wins: no fighting for the cursor).

| Gesture (pointer mode) | Action |
|---|---|
| Move palm/hand | Move cursor (relative, One Euro filtered) |
| Thumb–index pinch, quick release (< 300 ms) | Left click |
| Pinch twice within 400 ms | Double click |
| Pinch and hold > 300 ms, then move | Drag (release = drop) |
| Thumb–middle pinch | Right click |
| Two fingers up + move up/down | Scroll (rate proportional to distance from where the gesture started) |
| Fist | Pause pointer (re-centre point) |
| Open palm after fist | Resume, new origin |

Mouse output goes through the **same safe path as Ghost**: `SendInput`, the focus/protected-window checks in `live/safety.rs`, and `shortcuts` router rules.
- Never click in protected windows (password managers, banking, credential prompts) or elevated windows (UIPI would silently drop it).
- Esc stays globally armed in Ghost sessions; add Esc to pointer mode too.
- Cursor must not teleport: first frame after tracking recovers only sets the new origin.
- Optional "snap to nearest control" later: since NEXUS already lists UIA controls (24–44 ms per query, measured), the pointer can gently snap to a nearby button, which makes coarse hand pointing hit small targets. This is the biggest accuracy upgrade and costs no extra camera work.

## Camera placement matters

- **Built-in laptop webcam looks at your face, not your keyboard.** Hands held up in front of the screen are visible; hands on the desk are not. Expect "hand raised in front of the screen, elbow resting on the desk or palm facing the camera".
- Forward-facing (Meta Quest style) tracking was reported less stable than the downward-facing view in the Pinch project. Prototype both; do not assume.
- Light: needs a lit hand, not a backlit window. Dark/IR-only cameras (Windows Hello cameras give IR, not usable by browsers as plain video) are out.

## Architecture (extends 114's options)

| | A. WebView page runs MediaPipe, Rust moves the mouse | B. Native Rust (`ort`/ONNX hand models) | C. Python sidecar |
|---|---|---|---|
| Matches what worked elsewhere | **Yes** (Pinch project's final design) | No public precedent | Yes but lag/RAM |
| Dev effort | Lowest | High (palm detector + landmark models + tracking glue) | Medium |
| RAM/CPU | WebView + WASM/WebGL; measure | Lowest | Python + OpenCV overhead |
| Offline | Models can ship in the app | Yes | Yes |

Recommended: **A for the prototype**, hand frames never leave the hidden camera window; landmarks (21 points, numbers only) go over IPC to a pure Rust `pointer` module that owns the filter, the pinch state machine, the safety gates, and the mouse output. Rewrite detection natively only if measured CPU is too high. The pure parts (filter, pinch hysteresis, mapping, state machine) are testable with recorded landmark traces and a fake clock, like `directed.rs` and `orbPolicy.ts`.

Reuse from 114: one shared camera session, same on/off policy, same permission handler (`mic_permissions.rs` already auto-allows camera for our own origins), same indicator chip.

## Targets (goals to measure on your laptop, not promises)

- Hand-to-cursor latency: ≤ 80 ms median (camera frame → `SendInput`); 30 fps camera adds ~33 ms by itself, so 60 fps helps if your webcam supports it.
- CPU while hand is tracked: ≤ 15 % of one core; ≤ 3 % while no hand is in view (low-rate presence check). Frame rate drops to ~10 fps when no hand is seen.
- Jitter: cursor stays within 2 px with a still hand over 3 s.
- Click accuracy: ≥ 95 % of pinch clicks land within 12 px of where the pointer was when the pinch started (this is the drift test, the thing every other project got wrong).
- False clicks: < 1 per 10 minutes of normal use (hand resting, gesturing while talking).
- Re-centre/clutch works without losing the cursor.

## Phases (each ends with a measured gate; stop and report if a gate fails)

- **P0 — Landmark prototype (browser only, no mouse control)**: hidden test page with MediaPipe Hand Landmarker (VIDEO mode), records 2 minutes of landmarks while you do: still hand, slow moves, pinches, talking with your hands, hand resting. Gate: measured fps, CPU, and landmark jitter on your webcam; decides camera angle and whether option A is viable.
- **P1 — Pointer maths, offline**: Rust `pointer` module (One Euro filter, palm-width-relative pinch with hysteresis, freeze-on-pinch, relative mapping with active zone) tested against the P0 traces, including a click-drift test and a false-click test on the "talking with hands" trace. Gate: drift and false-click targets met on recorded data, no live mouse yet.
- **P2 — Live pointer mode, move + click only**: connects P0 detection to `SendInput`, with the ring indicator, Esc, mouse-takes-over, protected-window gate. Gate: 20-minute real use session, latency and CPU numbers logged, no stuck clicks (a "stuck button" watchdog releases any held button after 10 s of lost tracking).
- **P3 — Drag, double click, right click, scroll**.
- **P4 — Snap-to-control assist** (UIA) and optional head-pointer fallback with Face Landmarker (Gameface-style).
- **P5 — Voice + hand fusion**: "click that" uses the pointer position as the target (instead of guessing a name), "scroll down" etc. keep working as before.

Decisions that must be made by you before P2: whether the hand mouse should start automatically in Ghost mode or only when you enable it; left- or right-handed; and whether a physical mouse movement should always end pointer mode (recommended: yes).

## Needs from you

1. Camera: laptop webcam model/resolution/fps, glasses yes/no, which hand you'd use, desk lighting.
2. A 2-minute test session for P0: sit normally, keep your hand up and move around, then talk with your hands. Landmark numbers only are kept; no video is saved unless you ask.
3. Which controls matter most: moving + clicking, scrolling, dragging windows, selecting text, or all.
4. Confirm "camera only on while Ghost or pointer mode is on, LED visible" is acceptable.

## Honest limits

- Precision: webcam hand tracking will not select small text or tiny icons reliably. Snap-to-control and voice cover this; the mouse is still better for fine work.
- Fatigue: holding an arm up is tiring; relative mode with resting is mandatory, and use for minutes, not hours.
- Lighting, glasses, sleeves, a hand partly out of frame, and fast motion all reduce tracking quality.
- Pinch detection depends on hand angle (known problem even on dedicated sensors).
- All numbers above are targets. Nothing published measures a native Windows implementation; P0/P2 produce our own.

## Sources

- [reynaldichernando/pinch](https://github.com/reynaldichernando/pinch) and [Gigazine write-up](https://gigazine.net/gsc_news/en/20241124-selfmade-apple-vision-pro-interface)
- [Pythonpreran/Aircursor](https://github.com/Pythonpreran/Aircursor), [small-cactus/handTrack](https://github.com/small-cactus/handTrack), [zunza5/handpointer](https://github.com/zunza5/handpointer)
- [Project Gameface (Google)](https://developers.googleblog.com/en/project-gameface-makes-gaming-accessible-to-everyone/)
- [1 € Filter (Casiez, Roussel, Vogel; CHI 2012)](https://cristal.univ-lille.fr/~casiez/1euro)
- [Design Principles & Issues for Gaze and Pinch Interaction](https://arxiv.org/pdf/2401.10948), [Measuring eye-tracking accuracy in Apple Vision Pro](https://arxiv.org/pdf/2406.00255)
- [MediaPipe Hand Landmarker guide](https://developers.google.com/edge/mediapipe/solutions/vision/hand_landmarker) (Pixel 6 numbers only; no Windows desktop benchmark found)
- [Leap Motion in ChimeraX (palm-driven cursor, precision note)](https://rbvi.ucsf.edu/chimerax/data/leap-july2020)
- Free-hand pointing gain/mapping studies: [Effects of Control-Display Mapping and Spatially Dependent Gain](https://digitalcommons.kettering.edu/industrialmanuf_eng_facultypubs/115)

## Status (2026-10-14)

P0, P1 and the core of P2 are BUILT as a read-only-by-default mode: see [change 127](../changes/127-hand-mode-camera-pointer-p0-p2.md). Entry is voice ("hand mode"), arming is palm-fist-palm-fist. Still open: real-camera measurements (the P0 gate), drag/scroll tuning on your hand, snap-to-control (P4), voice+hand fusion (P5).


## Field test 1 (2026-10-14): "I showed palm and fist more than twice, no difference"

**Log reading (19:03:49 to 19:05:40).** The mode started, tracking was good (hand 98-100 %), and the arm gesture DID fire: ARMED, DISARMED, ARMED, DISARMED, ARMED, DISARMED (six toggles in about 60 s, `clicks=2` while armed). So the engine works; what the user saw was not a failure of detection.

**Causes (most likely first)**
1. **Read-only preview, by design.** `live=false` on every line. In preview the real cursor never moves and nothing changes on screen except a 300x250 HUD in the bottom-left corner. "No difference" is exactly what preview does. The spoken line saying so was long and was followed by room noise being transcribed.
2. **The gesture is a toggle and the two states sound the same.** Palm-fist twice arms; the same gesture disarms; both play the same system beep (`MessageBeep(0)`). Repeating it "more than twice" flips it back off (four completions = armed then disarmed). The user cannot tell which state they ended in.
3. **Accidental disarm while using it.** Opening and closing the hand twice while pointing is the arm gesture again; the last DISARMED came right after the clicks.
4. **Arming was hard at first (about 50 s until the first ARMED).** At 10-15 fps a frame is 70-100 ms, and each pose must hold 120 ms (3 frames); a quick fist is missed. Thresholds were tuned on a synthetic hand, not this user's hand, and the HUD stage dots are easy to miss.
5. **Low frame rate.** fps 9-15, not 30. IPC is fast (latency about inference + 3 ms), so inference is the cost: p50 18-65 ms, varying with load (Brave, TTS, STT). The GPU delegate may not be effective inside WebView2, or the GPU is shared; unmeasured.

**Plan (nothing changed yet)**
- A. **Make preview visible**: draw a virtual cursor ring and click ripple on the full-screen stage overlay at the predicted position, so preview is a real demonstration without any risk. Show a clear "ARMED / PREVIEW" chip near the orb.
- B. **One-way, unambiguous arming**: arm = palm-fist-palm-fist; disarm = fist held 1.2 s (or "hand mouse stop", Esc). Two distinct sounds (rising for armed, double low for disarmed) and a short spoken line the first time.
- C. **Gesture tolerance**: debounce by frames (2 frames or 80 ms, whichever is larger), accept brief Other poses, widen the finger-extension ratio, print `[HAND] pose a -> b` transitions, and (one-off) set `handTrace` so the user's real landmarks can be replayed to tune the thresholds offline.
- D. **Frame rate**: log which delegate is really used; compare GPU vs CPU on this laptop; skip frames when inference falls behind instead of queueing; check WebView2 throttling for a small non-focused window; target >= 24 fps and p50 inference <= 25 ms.
- E. **Then try live**: with A-D done, run "hand mouse live" for a short supervised test; Esc and moving the mouse stop it.

## Field test 2 (2026-10-14 19:50): "the lines buffer, too much delay, arm needs no confirmation"

**What the log proves**
1. **The video pipeline delivers about 6 frames a second, not 30.** `fps=4.6-5.3` with a hand, and exactly `fps=2.0` with no hand, where only every third frame is processed (6 / 3 = 2.0). Inference is fast (`infer_p50` 24-31 ms), IPC is about 3 ms, and the delegate calibration says GPU 31.4 ms vs CPU 27.7 ms, so the model is NOT the limit: it could run at 30 fps and is idle most of the time. Something upstream hands it a new frame only every ~170-210 ms.
2. **Why the skeleton "buffers"**: the camera preview is live but the skeleton is drawn only when a frame is processed, so at 5 fps it steps every ~200 ms and trails the moving hand. The reported `latency_p50` (about 30 ms) is misleading: it starts counting when we START processing a frame, so it leaves out the camera's capture delay and the wait for the next frame. Real hand-to-cursor delay is roughly camera pipeline (50-100 ms) + half a frame interval (about 100 ms at 5 fps) + 30 ms, i.e. 200-300 ms. That is what you feel.
3. **Arming never completed in this run (`armed=false` the whole time) although the poses were recognised.** The pose lines show the sequence palm, fist, palm, fist several times. Two of the three full runs only reached the last fist after a 5 s metrics line had gone by: at 5 fps and watching a stuttering HUD the whole 4-pose gesture took more than 6 s, and the detector's single 6 s window (measured from the first palm) had expired, so the last fist restarted the count. The poses themselves are well separated for your hand (fist ratios 0.3-0.9, palm 1.3-1.5), so the classifier is fine; the timing rule was too strict for a slow frame rate.
4. **Palm flicker**: a slightly tilted palm gives ratios like 1.07-1.15 (`palm -> other`), which splits the pose in two. A palm tolerance is needed (3 clearly straight fingers and a high average).
5. **Slow start**: first frame came 14 s after "hand mode" (model load 2.9 s, camera open, the long spoken line).

**Most likely causes of the 6 fps** (not yet proven; the next build measures them)
* The **HP Wide Vision 5MP camera** dropping its frame rate in dim light (evening test vs afternoon test; the first test ran at 10-15 fps). Many webcam drivers cut to 7-15 fps when exposure is long.
* **Windows Studio / camera effects** on the HP camera (extra latency, lower rate).
* **Chromium delivering frames through the compositor** (`requestVideoFrameCallback` follows presentation, not the camera) for a small unfocused window.
* **Battery state**: the laptop is discharging and NEXUS opts into Windows EcoQoS (efficiency cores) for its own process; whether the WebView2 renderer is affected is unknown.

**Plan**
1. *Measure the real numbers first (small build)*: `[HAND]` metrics gain `cam_fps` (callbacks per second), `proc_fps`, true capture-to-cursor latency from the camera's own `captureTime`, the track's actual `frameRate` and exposure, dropped-frame count, visibility state and power state. One run then says which cause it is.
2. *Frame source*: read frames straight from the camera track with `MediaStreamTrackProcessor` instead of `requestVideoFrameCallback`, and A/B them in the same log. Try `applyConstraints` for a frame-rate floor and continuous exposure. Add a kill switch `NEXUS_NO_ECOQOS=1` to test the power theory.
3. *Skeleton in step with the picture*: draw the processed frame itself (not the live video) with the skeleton, so the lines always sit on the hand even at a low rate; and show "camera is running at N fps, add light" when the real rate is below 15.
4. *Lower real latency*: use the camera capture time, a light constant-velocity prediction of the palm by the measured delay, and re-tune the One Euro filter for the measured frame rate.
5. *Arm with no confirmation*: palm, fist, palm, fist starts controlling the real cursor at once (`handMouseLive` default true; "hand mouse preview" becomes the opt-out). Safety stays: protected and admin windows refuse, Esc and real-mouse movement stop it, a held fist for 1.2 s disarms, a lost hand releases a held button. Timing rule becomes **per step** (each pose within 3 s of the previous one) instead of one 6 s window; palm tolerance as above; short spoken confirmation replaced by the chip and the rising beep.
6. *Faster start*: open the camera and load the model in parallel, show the chip immediately, shorten the spoken line to "Hand mode on, sir."
7. Then re-run the field test: targets `cam_fps >= 24`, true latency <= 100 ms, arm gesture succeeds in <= 3 tries.
