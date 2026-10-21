# 118 — Hand Mode: the complete record (plans, decisions, field tests, architecture, status)

One document that ties together everything done for Hand Mode (the camera as a pointer), the intent resolver, screens and
OS adaptation, and the touchpad-style gestures. Details live in the linked files; this is the map plus the reasoning.

| Document | What it holds |
|---|---|
| [114](114-camera-attention-and-hand-gestures-plan.md) | Camera attention (Look and Talk) and gesture ideas, early plan |
| [115](115-camera-hand-mouse-plan.md) | The hand-mouse plan, field test 1 and 2 analysis |
| [116](116-hand-gestures-window-and-tab-switching-plan.md) | The touchpad-style gesture plan (now BUILT, see section 8) |
| [117](117-cross-os-screen-adaptation-and-intent-plan.md) | Intent resolver, screens, per-OS status |
| [../changes/127](../changes/127-hand-mode-camera-pointer-p0-p2.md) | Change log rounds 1-5 |
| [../changes/128](../changes/128-hand-gestures-and-field-test-4.md) | Gestures, click fix, exit, pointer speed, chip move |
| [../research/hand-mode/01](../research/hand-mode/01-camera-attention-hand-pointer-and-gestures-research.md) | The research, with sources |

---

## 1. What the user asked for, in order

1. Research similar projects, then build a hand/camera mouse as a "sub command center" ("HandCenter", the 16th).
2. Say "hand mode" -> camera on; the pointer stays idle until palm -> fist -> palm -> fist; "update me after completion".
3. Continue the rest of the plan; later "go ahead with A to D" and then items 1 to 6 after the field logs.
4. One pinch for click; no camera picture; short red panel aligned to the orb; "just moving the index finger is for clicking".
5. Gestures like the four-finger touchpad swipe to move between windows and choose the tab perfectly.
6. Make it work for Linux, macOS and Windows and adapt to every screen.
7. No fixed phrase: "nexus i want to control the cursor" / "i want to give command to you" must be understood.
8. Chip at the opposite edge from the orb, bottom centre just above the taskbar.
9. Field test 4: the index pinch needs three tries; drag is great; hand mode cannot be exited; the cursor should follow the
   laptop's pointer sensitivity and acceleration (a fast hand must move the cursor fast).
10. Everything written up in very detailed markdown.

## 2. Architecture

```
camera -> hand window (invisible 8x8 WebView2; MediaPipe Hand Landmarker; GPU/CPU calibration)
        -> 21 landmark numbers per frame over IPC (hand_frame)        [no frame is ever stored or sent]
        -> hand::engine (pure state machine, fake-clock tested)
              pose.rs      finger ratios, Pose (Palm/Fist/Peace/Other), Blade (Three/Four), spread, pinch ratio
              filter.rs    One Euro
              gesture.rs   touchpad-style flicks -> Cmd (pure)
              snap.rs      nearest UI Automation control (per click)
        -> Actions -> hand::mod apply():
              output thread (enigo mouse, SendInput keys, guard for protected/elevated windows)
              HUD event hand:state -> stage overlay chip / ring / ripple
        <- voice: intent_parser (exact phrases + loose exit) / mode_intent (natural language) -> HandCenter
```

Key files: `src-tauri/src/hand/{mod,engine,pose,gesture,filter,snap}.rs`, `mode_intent.rs`, `center.rs`, `orchestrator.rs`
(`run_hand_mode`), `shortcuts/exec.rs` (`send_events`), frontend `hand/`, `stage/HandOverlay.tsx`,
`stage/handOverlayModel.ts`, `settings-sidebar/HandModeSection.tsx`.

## 3. Engine rules (and the problem each one solves)

| Rule | Problem it solves |
|---|---|
| Palm-centre pointer + One Euro | jitter of a still hand; fingers moving during a click do not move the palm |
| Relative movement | gorilla arm; hand can rest/re-centre; hand lost = no jump |
| Rewind 150 ms + cursor lock during a press | the click used to land off-target because the fingers moved the hand |
| Arm gesture palm-fist-palm-fist, per-step 3 s, frame-aware debounce (2 frames AND 80 ms) | a 5-15 fps camera made a total-time window impossible |
| Arm only arms; fist held 1.2 s disarms (after the hand opened once) | repeating the gesture used to flip the pointer off |
| Click = bend the index (ring+pinky stay open) OR thumb-index pinch; one state machine; 150 ms refractory; drag after 450 ms | one action = one click; a closing fist must not click |
| Right click = thumb+middle; scroll = Peace pose, hand up/down | |
| Lost hand 0.6 s releases buttons; 5 s disarms | stuck buttons are worse than stray releases |
| Real mouse moves > 40 px from where we put it, or Esc, disarms | the physical mouse always wins |
| Per-monitor mapping, crossing, gap snap, 2 s layout refresh | mixed-size / left-hand monitors |
| Latency lead = 70 % of measured capture-to-landmarks delay (clamped 4 % of frame) | cursor lag |
| Protected/elevated window guard; snap-to-control (36 px, UIA, per click) | safety; small targets |

## 4. Field tests and what each log proved

**Field test 1** (6 arm toggles in the log): the arm gesture DID fire; "no difference" was the read-only preview plus a
toggle that flipped back. Built: overlay ring + one chip, one-way arming, fist-hold disarm, rising/falling beeps, gesture
tolerance, canvas no longer reallocated per frame, GPU-vs-CPU delegate calibration.

**Field test 2**: `fps=2.0` with no hand = 6 camera frames per second while inference was ~27 ms; `latency_p50` hid camera
delay; arming failed because one 6 s TOTAL window expired at 5 fps. Built: per-step arm timing, tolerant palm,
live-by-default (arming is the confirmation), parallel camera+model load, camera-track frame source, capture-time latency
with a constant-velocity lead, One Euro 1.5/18, `hand_stats` line every 5 s.

**Field test 3** (lit room: camera 15 fps, capture-to-Rust 10-60 ms, lead 8-44 ms): arming worked. Built: index-finger
click, no camera picture, short chip beside the orb, `mode_intent.rs`, multi-monitor engine, `live_supported()`.

**21:34 log (between 3 and 4)**: voice "next is i want to control the cursor" entered hand mode (live=true, 1 monitor
1920x1080); `cam_fps(track)=15`, `capture_to_rust_p50` 9-16 ms, `lead` 6-15 ms, GPU p50 14 ms; ARMED by gesture after about
three palm/fist cycles; clicks counted 1, 2, 5, 9; one DISARMED by fist-hold. First stats line had `cam_fps(track)=6.6`,
`p95=2892ms` (warm-up). Pose ratios: fist 0.6-0.9, palm 1.3-1.5, transitional "other" frames 1.0-1.3.

**Field test 4** (the user's report):
* "index finger pinch is not great, I have to try thrice; drag works great" -> root cause found by reading the engine, see 5.1.
* "I can't exit hand mode at all" -> see 5.2.
* "cursor should match the laptop's sensitivity; fast hand should move the cursor fast" -> see 5.3.
* Chip placement: "opposite the orb, bottom centre above the taskbar" -> see 5.4.

## 5. Field test 4: causes and fixes

### 5.1 Quick clicks needed three tries (drag was fine)
Drag worked because a held press only needs the signal to stay on. A quick click is decided on the RELEASE frame, and
the old rule asked "is the pose still a valid index pinch?" on that very frame. At the instant the fingers open the thumb
is often nearer the middle fingertip than the index tip, so the pose gate read false and the click was thrown away
("cancelled", meant for a fist closing).
Fix (`engine.rs update_left`): a press is cancelled only when the raw signal is STILL on (thumb still near the index, or
the index still bent) while the pose gate fails (the hand turned into something else). If the fingers genuinely opened, it
is a click. Also `pinch_close 0.32 -> 0.38`, `pinch_open 0.55 -> 0.6` (sloppy landmarks rarely reach 0.32).
Tests: `a_quick_pinch_still_clicks_when_the_thumb_drifts_toward_the_middle_finger_as_it_opens` (mutation-checked: restoring
the old rule fails it) and `closing_into_a_fist_while_the_index_is_bent_is_still_not_a_click`.

### 5.2 Exiting hand mode
What existed: "exit hand mode" and about 16 exact phrases, the Command Hub button. Esc and fist-hold only DISARM the pointer;
the camera stayed on. STT writes "hand more", "and mode", "hands off" and a single wrong word failed an exact match.
Fixes:
1. `loose_hand_exit` (`intent_parser.rs`): while hand mode is ON, a short sentence (<= 7 words) with a leaving word (exit,
   quit, stop, close, leave, end, disable, cancel, off, kill, done, finish) and a target (hand, hands, camera, webcam,
   gesture(s), tracking, handmode, mode, more, mod) leaves; "exit"/"quit" alone leave; anything containing "ghost" never
   does; "stop" alone never does (it belongs to Ghost/media).
2. A way out without voice: after a fist-hold disarm, KEEP the fist closed 2 s more -> `ExitMode` (chip says "keep the fist
   to exit"). Opening the hand cancels it.
3. Idle exit: no hand for 10 minutes while disarmed ends the mode (the camera must not stay on forever).
4. Esc still disarms immediately; the Command Hub toggle is unchanged.

### 5.3 Speed and sensitivity
Old: gain = 2.4 + 2.0 x clamp(speed / 1 frame-width per s), so a fast flick was only 1.8x a slow move and the user
saw "no change". New (`move_cursor`): a smoothstep ballistic curve from 0.12 to 1.42 frame-widths/s, gain =
base x OS-pointer-multiplier x (1 + 2.2 x curve): about 3.2x for a fast hand. The OS pointer speed (1-20) and "Enhance pointer
precision" are read with `SystemParametersInfoW` when the pointer arms (so changing Settings applies at the next arm);
`handSensitivity` (default 0 = follow Windows) overrides. Tests: quick vs slow, x2 sensitivity ratio 1.8-2.2, precision off ~flat.

### 5.4 Chip placement
`chipPlacement(dock, viewport, chipW, chipH, taskbarBottom)`: horizontally centred; orb at the top -> bottom, just above the
taskbar (height read from `screen.height - availTop - availHeight`, 12 px gap); orb at the bottom -> top-centre. It
follows `orbPosition` live through the store.

## 6. Safety rules (unchanged and extended)
* Camera only while the mode is on; only 21 numbers per frame cross IPC; `handTrace` (off) logs landmark numbers only.
* Clicks, drags, scrolls AND gesture keys are refused in protected windows and elevated windows.
* The window switcher can never leave Alt down: commit/cancel, disarm, lost hand, Esc, mouse takeover, mode stop, channel
  close, a 10 s engine timeout, and a 12 s output-thread failsafe each release it.
* Live output is Windows-only (`live_supported()`); other systems stay in honest preview.

## 7. Intent understanding
`mode_intent.rs` (doc 117 section 1) plus the loose exit above. Wake word stripped first; question guard; unclear -> asks.

## 8. Touchpad-style gestures (plan 116, now built)
* Poses: Three (index, middle, ring straight, pinky folded) and Four (all four straight AND together: fingertip spread
  index-to-pinky under 1.15 palm widths). Held 250 ms before they count; the pointer is locked while held.
* Flick = the palm travels 0.10 of the camera frame within 400 ms, mostly along one axis (1.5x dominance), 400 ms refractory,
  the opposite direction ignored for 600 ms (the return stroke), a fresh window after the refractory.
* Three: flick right/left = next/previous tab (shortcut router `next_tab`/`prev_tab`, with its wrong-window and protected
  checks); flick up = window switcher (holds Alt, Tab); inside it right/left = Tab / Shift+Tab, down or dropping the pose =
  commit, any interruption = Esc then Alt up.
* Four (OFF by default, `handGesturesDesktop`): right/left = Win+Ctrl+Right/Left, up = Win+Tab, down = Win+D.
* Preview mode shows the label ("HAND · next tab (preview)") without sending keys.
* NOT built: G2 scrub with tab names via UI Automation (needs a real browser to verify); relative scrub steps; macOS/Linux chords.
* Settings: `handGestures` (default on), `handGesturesDesktop` (default off), both in the Command Hub card.

## 9. Verification (this round)
See change 128 for numbers. NOT run live: the gestures on a real hand, the new click rule on a real hand, the exit by
fist, multi-monitor, macOS, Linux, the Windows pointer-speed read with a non-default setting.

## 10. Open items
1. Read the next log for `[HAND] pose ... spread=` and `[HAND] gesture pose three/four (spread=)`, then set the Four threshold.
2. G2 named tab scrub. 3. Cloud fallback for unrecognised intent phrasings. 4. Remember the "hand or voice" answer.
5. Head-pointer fallback, two-hand zoom, absolute mapping, spoken arm confirmation.
6. macOS (Accessibility permission, Spaces chords), Linux X11/Wayland (doc 117 section 3).
7. A concurrency note: other sessions edited `intent_parser.rs` and `commands.rs` while this was built and one stale write
   removed edits; they were re-applied and re-verified. Check `git diff` before building.
