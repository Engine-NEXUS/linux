# 117 — Every OS, every screen, and understanding what the user means (STATUS + PLAN)

Asked for: "work for all OS (Linux, MacBook, Windows) perfectly", "adapt to every screen", and "no limitation
to saying ghost mode or hand mode: understand the intent, `nexus i want to control the cursor`".

## 1. Understanding intent (BUILT)

`src-tauri/src/mode_intent.rs`: a deterministic resolver that runs after the exact phrases and decides from
three signals: WHO acts (i / let me / can i = the user; you / nexus / i want you to / take control = NEXUS),
WHAT is controlled (cursor / mouse vs laptop / pc), and HOW (hand, finger, gesture, camera, webcam vs voice,
talk, command, ghost, "hands free"). A how-cue beats a bare imperative.

| You say | Result |
|---|---|
| "I want to control the cursor", "let me control the mouse with my hand", "use the camera as a mouse", "enable hand tracking" | Hand mode |
| "I want to give command to you", "take control of my laptop", "can you control my computer", "I want hands free control of my pc", "do what I say" | Ghost mode |
| "I want to control my laptop" (unclear) | asks: "Say hand mode or ghost mode, sir." |
| "stop controlling the mouse with my hand", "give me my mouse back", "I don't want you to control my cursor anymore" | leaves the right mode |
| "how do I control the cursor with my hand", "give me a hand with this code", "turn on the camera", "I want to control the volume" | not a command |

The wake word ("nexus", "hey nexus") is removed first, otherwise every sentence looks like "you do it".
10 resolver tests (about 90 phrasings, both directions and the negatives), 1 full-parser test, mutation-checked
(lower the evidence threshold or drop the question guard and tests fail).

**Not done / next**: (a) a cloud fallback for phrasings the scorer does not recognise (only for sentences that
contain control words and score low; deterministic stays first for speed and offline); (b) remember the answer
to the clarifying question ("hand" / "voice") instead of asking for the exact phrase; (c) learn from
`missed_intents.jsonl`; (d) English only, by decision. The resolver is conservative on purpose: a wrong guess
here opens a camera or hands over the mouse.

## 2. Screens (BUILT, Windows)

* The pointer maps hand travel to the monitor the cursor is ON, so a 4K screen and a 1080p screen both feel
  the same size to the hand (tested: 4K moves ~2x the pixels).
* The cursor can cross onto neighbouring monitors, including ones to the LEFT (negative x) and ones of
  different size; a point in the empty gap snaps to the nearest monitor.
* The layout is re-read when the pointer arms and about every 2 s, so plugging or unplugging a monitor, or
  changing resolution / scale, mid-session is picked up. The overlay is drawn on the primary monitor only (the
  stage window lives there); a cursor on another monitor simply has no ring.
* Not covered: the stage overlay and chip on secondary monitors; per-monitor DPI differences in the click-snap
  step are inherited from UI Automation; `enigo` absolute positioning across mixed-DPI monitors has not been
  tried on real hardware here.

## 3. Operating systems (HONEST STATUS)

I could only build and run on this Windows machine. Nothing below was run on macOS or Linux.

| | Windows | macOS | Linux X11 | Linux Wayland |
|---|---|---|---|---|
| Camera + hand tracking page | works | WKWebView has `getUserMedia`; no `MediaStreamTrackProcessor`, so the existing `requestVideoFrameCallback` fallback applies. Camera permission string needed | WebKitGTK needs the camera / GStreamer pipeline set up | same |
| Overlay chip / ring | works | needs a transparent click-through window (the `macos-private-api` feature is already on) | needs a compositor | needs a compositor |
| Live mouse output | works (enigo `SendInput`) | needs the user to grant **Accessibility** permission, and a foreground-app guard (NSWorkspace) | enigo works | **blocked** without the RemoteDesktop portal / libei |
| Protected-window / admin guard | works | not written | not written | not written |
| Esc to stop | works | works (global shortcut) | the global-shortcut plugin is not available on Linux | same |
| Click snap-to-control | works (UI Automation) | not written (Accessibility API) | not written (AT-SPI) | not written |

What the code does today on non-Windows systems (by design, so nothing unsafe happens):
* `hand::live_supported()` is false: hand mode starts, tracks and shows the overlay, but stays in **preview**
  and says so out loud ("Live mouse control isn't available on this system yet").
* No clicks are ever sent without the protected-window guard.
* Linux builds skip the Esc registration; the fist-hold, lost-hand and real-mouse disarms still apply
  where the cursor position can be read (it cannot yet on non-Windows, so mouse-takeover is Windows-only).

### Roadmap to "works on every OS" (each step needs a real machine)

* **M1 macOS**: camera entitlement + `NSCameraUsageDescription`; Accessibility permission check with a spoken
  guide; foreground-app guard (bundle id denylist); cursor position via CoreGraphics; Retina scale; gesture
  chords (Ctrl+Left/Right for Spaces, Mission Control key).
* **L1 Linux X11**: foreground window via `_NET_ACTIVE_WINDOW`; cursor position via XQueryPointer; AT-SPI for
  snap; camera through PipeWire.
* **L2 Linux Wayland**: input through the RemoteDesktop portal (libei) with the permission dialog; otherwise
  stay in preview.
* **All**: one CI job per OS that at least builds the hand module, plus a manual checklist run on a real
  MacBook and a real Linux laptop before the word "works" is used.

## 4. What would make this "perfect"

* A real test on each OS and each monitor layout you care about (single, extended, mirrored, laptop lid
  closed, 4K at 150 %).
* Your decision on the live-output safety bar for macOS / Linux (they have no protected-window list today).
