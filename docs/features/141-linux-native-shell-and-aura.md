# Linux: a native overlay, a system drawer, and the aura

**Date:** 2026-10-21
**Status:** built and verified live on Pop!_OS 24.04 COSMIC (Wayland, PipeWire).
**Related:** the Linux redesign `NEXUS-Agent/docs/features/63-bottom-docked-orb-aura-and-subtitle-bar.md` (the aura's origin), `docs/changes/linux-port-native-shell-aura-and-mic.md` (what changed and how it was verified), `docs/testing/stage-eyeball-harness.md`.

---

## 1. What the user sees

- **The orb is part of the desktop, not a window on top.** It never takes the keyboard, and only the orb takes clicks; typing and clicking in other apps keep working while it is shown.
- **It sits below the desktop's panels.** On COSMIC the orb hangs just under the top bar, the clock stays visible.
- **The orb floats bare, ringed by an audio-reactive aura** (orb style "aura", the Linux default). No black box. The aura rests as a dim rim when idle, blooms with the user's voice while listening, orbits while thinking, and mirrors NEXUS's voice while speaking.
- **The sidebar is a system drawer** docked to the screen edge (Command Hub: 400 px, right, 10 px from the edge, 20 px under the panel), above windows, never in Alt-Tab or tiling, keyboard only when clicked into.

## 2. The overlay (`src-tauri/src/overlay_linux.rs`)

Two paths, chosen at start-up (`gdk_backend_linux::choose`, pure and tested):

| Condition | GTK backend | Stage | Sidebar |
|---|---|---|---|
| Wayland + layer-shell desktop (COSMIC, KDE, Hyprland, Sway, river, niri, Wayfire, labwc) **and** `libgtk-layer-shell.so.0` loads | native Wayland | layer-shell surface, overlay layer, keyboard NONE | layer-shell drawer, top layer, keyboard ON_DEMAND |
| anything else (GNOME, X11, library missing) | X11 / XWayland | X11 window, never focusable, Notification type, keep-above | normal window |

`NEXUS_GDK_BACKEND` overrides the choice. A missing library prints `sudo apt install libgtk-layer-shell0` once. The library is loaded at runtime (`libloading`), so it is not a build dependency; `tauri.linux.conf.json` lists it in the .deb depends.

**Click-through** on both paths is the window's input region: empty while idle, exactly the union of the stage hitboxes (`stage::HITBOXES` — the orb, spatial pins) otherwise. The Windows cursor-polling loop does not run on Linux.

**Never focusable:** the stage is built `.focusable(false)` on Linux. tao turns accept-focus back on after the first draw for any focusable window created unfocused, which is how COSMIC came to focus the full-screen stage and swallow keystrokes.

**Below the panels (safe area):** the stage starts with exclusive zone 0, so the compositor's first configure is the area the panels leave free. 700 ms after the first map the insets are measured, attributed to edges from COSMIC's panel config (`~/.config/cosmic/com.system76.CosmicPanel*/v1`, exclusive panels only; on other desktops a gap is kept from both edges of an axis), and the stage goes full screen (exclusive zone -1). Screen coordinates therefore stay exact for the pointer marker, the tour and the pins, while `window_manager::rect_for` places the orb inside the safe area. Log line: `free area 1920x1048 of 1920x1080 — panel insets … top: 32`.

**The drawer:** `prepare_sidebar` keeps the existing `sidebar_geometry` rules; the rect becomes a top + left/right anchor with margins inside the panel-free area, and the size is the GTK size request.

### Wayland facts that bit (keep)

- `primary_monitor()` is `None` on Wayland, and an unmapped window's `current_monitor()` is `None` too; fall back to `available_monitors()`.
- A non-resizable GtkWindow ignores the compositor's configure size (the stage is made resizable on the layer-shell path; the drawer stays non-resizable and is sized by size request).
- `run_on_main_thread` work runs after the caller's next synchronous step; state that the caller needs (drawer eligibility) is decided synchronously.

## 3. The aura (`frontend/src/aura/`)

- Orb style `"aura"` in `OrbFrame.tsx`: the existing WebGL particle orb with no capsule, plus `AuraCanvas` (portalled to `<body>`, because the orb frame slides with a transform and the canvas is `position: fixed`).
- Renderer: `@saystack/core@0.2.1` (MIT, no dependencies, Node >= 22). Host: `stageAuraHost.ts` (always "dark", scissored canvas around the orb). `isAdditive` must stay `false` — `true` makes the aura's alpha exactly 0.
- State table (`orbPresentation.ts`): idle → `paused` (dim rim), listening → `active`, thinking → `working` (orbit comet, ignores audio), speaking → `active` with `mirror`. Sizes scaled for this orb's sphere (~62% of the frame: ~100 px at the Linux default 160, ~125 px at 200) (the redesign tuned 44–72 px). Hue never encodes state (`palette: "prism"` is ambience).
- Energy (`audio/levels.ts`): the orb's own sources, so orb and aura never disagree — TTS envelope (else the playback proxy) while speaking, the louder of the Rust mic level and the browser VAD while listening, nothing otherwise. Every band carries that one level.
- Cost: the aura is `hidden` whenever the orb is not shown, so its render loop parks.
- Reduced motion (Pop!_OS COSMIC ships with animations off): the aura stays, the renderer runs it at 12% motion with no glide.

### Differences from the redesign doc (63)

| Redesign | This branch |
|---|---|
| CSS `NexusOrb`, sizes 46/72/62/68 | the WebGL particle orb is kept; a 160 px frame on Linux (200 on Windows), `window_manager::DEFAULT_ORB_SIZE` |
| bottom-docked, gap 196 | top or bottom, the user's Display setting; below the panels |
| Rust `audio_levels.rs` bridge | not needed: the aura reads the levels the orb already gets |
| subtitle bar under the orb | spoken replies captioned on the stage with word-by-word read-along; the editable "last heard" row lives in the sidebar |
| aura off under reduced motion | aura calmed, not removed |

## 4. The microphone (`src-tauri/src/audio_linux.rs`, `wakeword_oww.rs`)

cpal 0.15's ALSA enumeration opens every listed PCM in both directions and keeps it open while the `Device` lives. Holding `hw:sofhdadsp,6` — PipeWire's own microphone source — kept PipeWire from opening it, so capture through `default`/`pulse` stalled in `snd_pcm_prepare`, and the probe fell back to the raw device, taking the mic away from every other app. Probing now keeps only names and reopens one device at a time (`reopen_input_device`); every probe also runs behind a hard deadline on its own thread. Result: `default` accepted in 2.2 s.

## 5. Performance (wake word)

The wake word runs ONNX inference (tract: mel → embedding → classifier) on every 80 ms chunk above the silence gate, which a laptop room passes most of the time. Measured on the target:

| Build | Per chunk | Idle CPU (Rust process) |
|---|---|---|
| dev, unoptimised dependencies (before) | 88–113 ms avg, 521 ms max — behind real time | 119% |
| dev, inference crates at opt-level 3 (now) | 8.4 ms avg, 13.7 ms max | 19% |

- `Cargo.toml`: `[profile.dev.package.<crate>]` and `[profile.release.package.<crate>]` set `opt-level = 3` for `tract-*`, `ndarray`, `matrixmultiply`, `rustfft`, `num-complex`, `webrtc-vad`. In release this is 6–10% faster per chunk than the profile's `"s"` (standalone benchmark with the app's models, six interleaved runs: best 12.0 vs 13.4 ms, median 14.5 vs 15.4 ms); the rest of the binary stays size-optimised; the Windows installer profile `dist` is unchanged. Optimising *every* dependency in dev forces a full-tree rebuild that exhausts a 7.6 GB machine — keep it scoped.
- A WebRTC VAD pass whose result was discarded ran on every chunk; removed.
- Deferred: a VAD gate before the embedding model (would cut idle CPU in noisy rooms) — it must first be checked against the wake-word recall set.

## 6. Known limits

- Multi-monitor: the stage and the drawer are pinned to the first monitor.
- Panel attribution is exact on COSMIC only; elsewhere the orb may keep a small gap from an edge without a panel.
- The orb's preview harness renders with Chrome/SwiftShader; WebKitGTK on the real desktop is the reference.
