# 114 — Camera: "is the user talking to NEXUS?" and hand gestures (PLAN ONLY)

Status: research + plan, nothing built. Date: 2026-10-14.

## What was asked
1. Can NEXUS use the camera to tell whether the user's eyes are directed at it, so it knows when to listen and obey (and ignore mum / TV / Echo Dot)?
2. Hand-gesture commands for scrolling and more.

## Short answers
- **Yes for "the user is facing the screen and talking".** That is what Google ships as *Look and Talk* on Nest Hub Max: it fuses proximity, face match, head orientation, gaze direction, lip movement, voice match, context and intent (Google says 100+ signals), analysed on-device. Our version is simpler but follows the same idea.
- **No for precise "looking exactly at the orb".** Webcam point-of-gaze error is about 2–4° even in good conditions (WebGazer: 4.17° in its paper, 3.65° with drift in a later study; a better tracker reached ~2° under controlled conditions). On a laptop that is a 2–4 cm blob on screen, and glasses, window glare and head motion make it worse. Use head pose + eyes-open + iris roughly on screen as an **attention signal**, not a gaze cursor.
- **The strongest use of the camera is not "look to wake", it is "this speech came from the person in front of the camera".** Mouth movement (face-mesh blendshapes) lined up with the voice-activity timing separates the owner from a TV, from mum behind you and from an Echo Dot, because those voices have no matching mouth on the camera. That needs no change in how the owner speaks.
- **Gestures are very doable** (hand landmarks are mature: 21 points per hand, ~17 ms per frame on a mid-range phone CPU for the whole pipeline) **but only with a deliberate "clutch"** (engage/disengage), otherwise everyday hand motion triggers things (the "Midas touch" problem). Mid-air input also causes arm fatigue ("gorilla arm"), so design for small motions with the elbow supported.

## Principle that keeps your directive intact
Ghost mode is full control, no "Nexus", no waiting. So the camera is used **as evidence to reject, never as a requirement to accept**:
- Camera sees you facing the screen with your mouth moving → accept (and, in the optional look-and-talk mode, no wake word is needed).
- Camera sees you clearly turned away, or someone else's mouth moving, or no mouth movement while speech plays → ignore that audio, with a log line saying why.
- Camera off, blocked, in a meeting, no face in view, or low confidence → **neutral**: behave exactly as today. The camera can never make Ghost mode stop working.

## Part A — Attention ("look and talk")
**Signals from one face-landmark pass (MediaPipe Face Landmarker: 478 3-D landmarks, optional 52 blendshapes, optional facial-transformation matrix = head pose):**
| signal | how | use |
|---|---|---|
| face present, distance | face box size | gate; ignore if > ~1.5 m |
| head yaw/pitch toward screen | transformation matrix | "facing screen" if within roughly ±25° yaw, ±20° pitch (tune on real data) |
| eyes open, iris centred | blink/eye blendshapes + iris landmarks | filters "head forward but eyes closed / looking down at phone" |
| mouth moving | jawOpen / mouthFunnel / mouthClose blendshape energy | **audio-visual match**: correlate with the mic VAD envelope over the utterance |
| single vs several faces | `num_faces = 2` | another person talking to you → lower confidence |

**Decision per utterance (pure function, unit-testable):** `attention_verdict(audio_segment_times, face_track) -> {Owner | NotOwner | Unknown, confidence}`; only `NotOwner` with confidence ≥ 0.8 rejects; `Unknown` never changes behaviour.

**Optional look-and-talk (wake-word-free) mode (Phase 2):** attention true for ≥ 0.4 s + mouth-sync + the transcript parses as a command with strong evidence → run it without "Nexus" (this also sidesteps the weak wake word: held-out recall is 63 %). Off by default; phone calls while looking at the screen are the main false-accept risk, which the "must parse as a command" rule and mouth-sync limit.

**Coarse gaze zone (Phase 4, optional):** "looking at the top-centre of the screen" (where the orb sits) from a 9-point calibration; only as a hint, expect drift.

## Part B — Hand gestures
Hand Landmarker: 21 landmarks, world landmarks, handedness; palm detector + landmark model, tracking skips the palm detector on most frames. Pinch/fist/open-palm are computed from landmark geometry (more robust than the canned classifier for pinch).

**Gesture set (draft, to be tuned with you):**
| gesture | action | notes |
|---|---|---|
| **Open palm held 0.6 s** | **engage** gestures (clutch on); hand ring appears on the orb/HUD | auto-disengage after 3 s without a hand, or on fist |
| engaged + **pinch (thumb–index) and drag up/down** | scroll the window under the cursor (mouse wheel), relative to pinch start; lift/re-pinch to re-centre | smaller motion than a wave; speed = displacement |
| engaged + **fast open-hand swipe left / right** | back/forward (browser), previous/next tab or slide | via existing shortcut router (verified, refuses protected windows) |
| engaged + **fist hold** | pause media / stop current NEXUS task | same effect as "stop" |
| **thumb up / thumb down** (no clutch needed, only when NEXUS has asked a question) | answer "Shall I…?" yes / no | uses the existing pending-offer mechanism |
| engaged + **two-hand pinch spread/close** (later) | zoom (Ctrl+wheel) | Phase 3+ |
| engaged + **index point** (later, optional) | air-mouse | high fatigue + low precision, off unless asked |

**Safety/ergonomics rules:** no gesture acts without the clutch except thumbs-up/down during a pending question; no gesture acts in protected or elevated windows; per-gesture rate limits; every action is spoken/visually acknowledged once; scroll uses relative motion so the arm can rest; hand must be inside a comfortable zone (not at the frame edge); sensitivity and handedness settings; the whole feature has a one-gesture "kill" (fist 2 s) and a Command Hub switch.

## Architecture options (decision needed after the prototype)
| option | what | pros | cons |
|---|---|---|---|
| **A. WebView** (`@mediapipe/tasks-vision`, WASM/WebGL) in a dedicated on-demand hidden window | camera via `getUserMedia` (our WebView2 handler already auto-allows CAMERA for our own origins), results sent to Rust as small JSON | fastest to build and tune; official runtime; model files bundled locally; window destroyed when the feature is off (RAM) | +150–250 MB while active; hidden WebView2 windows get throttled (we already hit this with the orb: `document.hidden`), so it needs a 1×1 visible-but-transparent window; CPU in WebView |
| **B. Native Rust** (`ort` already pinned; Media Foundation capture via the `windows` crate or `nokhwa`) with ONNX versions of BlazeFace / FaceMesh / HandLandmark | lowest RAM/CPU, EcoQoS, one process, frames never leave Rust | ports of the pre/post-processing (anchors, NMS, ROI warps) and model conversion are real work; `nokhwa` release cadence is slow | 2–4× the effort of A |
| C. Python sidecar (`mediapipe` pip) like the NLU/STT sidecars | official runtime, quick | another process (~200–300 MB), Python-version wheels lag, IPC | not better than A for a Windows-only app |

**Recommendation:** prototype in the browser (Phase 0) to *measure*, ship A first behind a flag, move to B only if the numbers (below) say the WebView cost is unacceptable.

## Resource budget and power policy (this app already cares about idle CPU/RAM)
- Camera OFF by default; opt-in; **only on during a Ghost session or while look-and-talk is enabled**, never during a detected meeting (reuse `meeting_detect`) or when the screen is locked. The camera LED is on whenever we capture, so: show a camera chip on the orb/Command Hub while active and say so when Ghost starts.
- Adaptive rates: presence/face box 2–5 fps when idle → full landmarks 10–15 fps while speech is detected → hand tracking 20–30 fps only while a hand is present and engaged. 640×480 or smaller input.
- Targets (to be verified on your laptop in Phase 0): ≤ 8 % of one core average while a Ghost session is idle-with-camera, ≤ 25 % during active gestures, gesture-to-scroll latency ≤ 120 ms, attention verdict ready within 150 ms of the end of an utterance.
- Cold camera start takes ~0.3–1 s (auto-exposure), so the camera must already be running when you speak; a "turn the camera on when I speak" design would lose the first words.

## Privacy and legal
- Frames are processed in memory and dropped. **Nothing is recorded, stored or sent**; only derived numbers (yaw/pitch, mouth energy, hand pose) leave the vision component, and logs never contain images. Local-only enrolment of nothing: **no face recognition / identity** (face templates are biometric data); identity stays with the voice lock.
- Visible state: camera chip + Command Hub status; a one-line spoken notice when Ghost starts with the camera on; physical shutter / black frames are detected and treated as "no camera".
- Windows' own Settings > Privacy > Camera toggle still applies; if it is off we say so once.
- A preview in Command Hub is local and optional for tuning.

## Phases and gates
| phase | deliverable | gate (measured, not claimed) |
|---|---|---|
| **P0 Prototype + measure** (browser, no app changes) | page that runs Face + Hand Landmarker on your webcam and shows yaw/pitch, mouth energy, pinch, FPS and CPU | numbers on YOUR hardware: FPS, CPU %, yaw/pitch stability, glasses on/off, evening light; decide A vs B |
| **P1 Attention gate** | `vision` module + hidden window; `attention_verdict`; integrated into `directed::evaluate` as reject-only evidence; Command Hub page (on/off, preview, status) | recorded sessions (you at the laptop, TV on, mum talking, Echo Dot answering): ≥ 95 % of your commands unaffected, ≥ 90 % of TV/other-voice turns rejected, 0 rejections when the camera is blocked/off |
| **P2 Look-and-talk (optional)** | wake-word-free commands when attention + mouth-sync + strong command parse | ≤ 1 false activation per 8 h of normal use with TV/music/calls |
| **P3 Gestures** | clutch, pinch-scroll, swipe back/forward, fist stop, thumbs yes/no | per gesture ≥ 95 % recall on intentional use; ≤ 1 false trigger/hour over 2 h of typing, talking and mouse use; latency target above |
| **P4 Extras** | two-hand zoom, coarse gaze zone, Windows-native presence (if a human-presence sensor is present: `HumanPresenceSensor` API) as a free low-power "someone is here" input | per feature |

All pure logic (attention smoothing/correlation, gesture state machine, scroll mapping) is written as clock-injected state machines and tested by replaying **recorded landmark traces (numbers, not video)**.

## What I need from you
- Camera allowed on during Ghost sessions (LED on)? Default off, opt-in?
- Which gestures matter most (scroll, back/forward, stop, yes/no)? Left or right hand?
- ~3 short recordings for P1 with the camera and mic on: normal desk, TV on in the room, someone talking behind you (I will only keep the derived numbers, not the video, if you prefer).
- Your laptop webcam resolution/FPS and whether you wear glasses (tuning).

## Honest limits
- Webcam gaze is coarse; attention is head pose + eyes, not "which pixel".
- Lighting (window behind you), glasses glare and a camera below eye level reduce accuracy; below ~50 lux the face model loses tracks.
- A photo or video of a face can fool a plain RGB camera (Google says the same); this is an attention/rejection aid, not authentication.
- Published precision/recall for head-pose "device-directed" classifiers is thin; the ~96 % figures I found are self-reported on other tasks, so every target above is a gate to measure, not a promise.

## Sources
- Google Look and Talk: https://9to5google.com/2022/07/27/google-assistant-look-and-talk/ , https://www.androidpolice.com/google-nest-hub-max-look-and-talk/
- MediaPipe Face Landmarker: https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker
- MediaPipe Hand Landmarker (Pixel 6: 17.12 ms CPU / 12.27 ms GPU, whole pipeline): https://developers.google.com/edge/mediapipe/solutions/vision/hand_landmarker
- MediaPipe Gesture Recognizer: https://developers.google.com/edge/mediapipe/solutions/vision/gesture_recognizer
- WebGazer accuracy: https://jeffhuang.com/papers/WebGazer_IJCAI16.pdf
- Gaze limits (glasses/lighting): https://support.realeye.io/how-to-get-accurate-results/
- Midas touch / clutch: https://a11y-paradise.onrender.com/glossary/69e7b55084dab428f37bd5e0
- Gorilla-arm fatigue: https://research.purdue.edu/news/study-researches-gorilla-arm-fatigue-in-mid-air-computer-usage/ , https://arxiv.org/html/2503.01011v1
- Windows presence sensing (HPD): https://www.pcworld.com/article/1786964/windows-11-may-soon-let-you-turn-off-presence-detection.html
- WebView2 permission handling: https://learn.microsoft.com/microsoft-edge/webview2/ (existing handler: `src-tauri/src/mic_permissions.rs`)

## See also

- [115 — Camera as a mouse (hand moves the cursor, pinch = click)](115-camera-hand-mouse-plan.md): similar-project research and the pointer-mode plan.

