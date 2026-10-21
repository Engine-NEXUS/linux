# Research: camera attention, the hand as a pointer, and touchpad-style gestures

Date: 2026-10-13/14. Scope: everything that was researched before and during the Hand Mode work (docs 114-118 and
changes 127-128). This file records what was learned and which decision each finding drove. Numbers are quoted from the
sources named; where a number is our own measurement it says so and names the log.

---

## 1. Why a camera at all: "attention" and the Midas touch

### 1.1 Look and Talk (Google)
Google's Look and Talk lets a person talk to a device without a wake word by combining several signals, none of which is
enough alone:

| Signal | What it answers |
|---|---|
| Proximity | Is a person near the device? |
| Face match | Is it the enrolled person? |
| Head orientation | Is the head pointed at the device? |
| Gaze | Are the eyes on the device? |
| Lip movement | Is this face the one making the sound? |
| Voice match | Does the voice belong to the same person? |
| Intent | Does the sentence look like a request for the assistant? |

Decision it drove: NEXUS must not treat "a face is visible" as "the owner is talking to me". The camera is only turned on
by an explicit mode ("hand mode"), never ambient, and no attention-based wake was built (doc 114 records the design only).

### 1.2 MediaPipe Face / Hand Landmarker
* Hand Landmarker returns 21 landmarks per hand (wrist; thumb 1-4; index 5-8; middle 9-12; ring 13-16; pinky 17-20).
* Published latency on a Pixel 6 for the hand landmarker: 17.12 ms on CPU, 12.27 ms on GPU. On this project's laptop the
  measured numbers are far better than the camera: model 14-27 ms (GPU delegate p50 14 ms in the 21:34 log) while the
  camera delivers 15 fps (66 ms per frame). The camera, not the model, is the bottleneck.
* `@mediapipe/tasks-vision` 0.10.21 and `hand_landmarker.task` (float16, v1, 7.8 MB) are Apache-2.0. The model SHA-256 is
  pinned in `scripts/hand-assets.lock.json` and downloaded by `scripts/sync-hand-assets.mjs`.

### 1.3 Webcam gaze is not accurate enough to click with
Published webcam gaze estimators report an error of roughly 2-4 degrees, which on a laptop screen at arm's length is
about 1-3 cm: larger than most buttons. Consequence: gaze-only pointing was rejected; gaze + pinch (gaze selects, pinch
confirms) needs an eye tracker, so it stays a later option. See section 4.

### 1.4 The Midas touch and the clutch
"Everything you look at / point at is a click" is the Midas-touch problem. Every robust system adds a clutch: an explicit
act that says "now I mean it". In this project the clutch is the arm gesture (palm, fist, palm, fist) and, for the
touchpad-style gestures, a held finger pose. Gorilla arm (fatigue from a raised arm) is why the pointer is RELATIVE
(trackpad-like): the hand can rest and re-centre, and a lost hand never makes the cursor jump.

---

## 2. Making a noisy landmark stream usable

### 2.1 One Euro filter
Casiez, Roussel, Vogel, "1 Euro Filter", CHI 2012. An adaptive low-pass filter: the cutoff rises with the signal's speed,
so a still hand is smooth (no jitter) and a moving hand has low lag. Parameters: `min_cutoff` (jitter when still), `beta`
(speed coefficient, lag when moving), `d_cutoff` (derivative smoothing). Implemented from the paper in `hand/filter.rs`
(not copied from any project). Tuned to 1.5 / 18 / 1.0 in field test 2 after a log showed lag.

### 2.2 Pinch-click drift and how the field fixes it
Every public hand-mouse project reports the same failure: the pinch itself moves the cursor off the target. The known
remedies, all adopted:
1. Drive the pointer from the PALM centre (wrist + four finger-base knuckles), which barely moves when the fingers move.
2. Lock the cursor for the whole press.
3. Click where the cursor was ~150 ms BEFORE the fingers started to close (rewind).
4. Measure the pinch in palm widths, not pixels, so it works at any distance.

### 2.3 Leap Motion / ChimeraX notes
Pinch accuracy discussions around Leap Motion and ChimeraX show the same split: pinch-to-select needs a tolerance band
(closed < open threshold, i.e. hysteresis) and a minimum dwell, which is why the engine uses `pinch_close 0.38 /
pinch_open 0.6` (changed from 0.32 / 0.55 in field test 4) and a 150 ms refractory.

---

## 3. Similar projects (what was read and what was NOT copied)

| Project | Take-away | Licence consequence |
|---|---|---|
| `reynaldichernando/pinch` | Closest hand-mouse: MediaPipe in a browser, pinch = click. Read for failure modes (jitter, click drift, unstable landmarks). | NO licence file, therefore NO code copied. Engine is written independently. |
| Project Gameface (Google) | Head/face-driven pointer with a deliberate "hold to activate" model and per-user sensitivity. | Read only for design ideas (sensitivity per user, dwell). |
| Windows touchpad | The reference for tab / app / desktop gestures. | System behaviour, not code. |

---

## 4. Gaze + pinch (Pfeuffer et al.) and Vision Pro design principles (not built)
* Pfeuffer et al., SUI 2017, "Gaze + pinch interaction in virtual reality": gaze chooses the target, a pinch commits it.
* arXiv 2406.00255 (Vision Pro gaze accuracy) and arXiv 2401.10948 (design principles for gaze + pinch) were used to
  decide that NEXUS should NOT promise gaze selection on an ordinary webcam. The hand pointer is relative and
  trackpad-like instead.
* A head-pointer fallback (Face Landmarker) and two-hand zoom were considered and deliberately left unbuilt.

---

## 5. Windows touchpad gesture defaults (the model for doc 116)
Windows 11 default touchpad gestures (all configurable in Settings > Bluetooth & devices > Touchpad):

| Fingers | Swipe | Does |
|---|---|---|
| 3 | left / right | switch app (Alt+Tab style) |
| 3 | up / down | Task View / show desktop |
| 4 | left / right | switch virtual desktop |
| 4 | up / down | Task View / show desktop |

Three properties to copy: the fingers going down is the clutch; movement is quantised into steps with a clear commit; it
never fires while you are just pointing or typing. The hand version maps 3 fingers to tabs and windows (the user's own
stated need: "choose the tab perfectly") and 4 fingers to desktops.

---

## 6. Camera facts learned from the field logs
* HP Wide Vision camera: in a dim room the driver cuts the frame rate (6 fps at the start of the first log, 15 fps in a lit
  room). The chip now says "dim room, add light" under 15 fps.
* `cam_fps(track)` (MediaStreamTrackProcessor) versus `cam_fps(rvfc)` (requestVideoFrameCallback) separate "the camera is
  slow" from "the page is throttled". After the invisible-window change rVFC reads 0.0 because the video element is no
  longer attached; the track path is the real number.
* A hidden WebView2 throttles video, so the hand window must stay VISIBLE to the OS even when it is an 8x8 transparent page.
* EcoQoS (Windows efficiency mode) can slow a background process; `NEXUS_NO_ECOQOS=1` exists for the A/B test.

---

## 7. Windows pointer settings (field test 4)
The user asked that the hand cursor feel like the laptop's mouse: a fast move should go further than a slow one, and the
Windows pointer speed should apply.
* `SPI_GETMOUSESPEED` returns 1-20 (default 10). Windows maps it to a multiplier: 0.03125, 0.0625, 0.125, 0.25, 0.375,
  0.5, 0.625, 0.75, 0.875, 1.0 (speed 10), 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75, 3.0, 3.25, 3.5 (speed 20). The hand
  mapping is scaled by exactly that multiplier.
* `SPI_GETMOUSE` returns three numbers; the third is "Enhance pointer precision" (mouse acceleration). Off = the
  acceleration curve is reduced to a quarter.
* A smooth ballistic curve replaces the old linear ramp: gain = base x pointer-speed x (1 + accel x smoothstep(ramp)),
  with the ramp running from 0.12 to 1.42 frame-widths per second. A slow hand is precise; a quick hand covers the screen.

---

## 8. What is still unknown (needs the user's hand or other machines)
* The finger-spread value that separates "four fingers together" from a relaxed open palm (the code logs `spread=` on every
  pose change; the default 1.15 is a guess, so 4-finger desktop gestures ship OFF).
* Real false-trigger rate of the 3-finger gesture while typing, talking and pointing (target: < 1 per 10 minutes).
* macOS, Linux and multi-monitor behaviour on real hardware.
