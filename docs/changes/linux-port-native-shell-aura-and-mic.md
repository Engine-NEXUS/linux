# Linux port — native overlay, system drawer, aura, microphone

Feature doc: `docs/features/141-linux-native-shell-and-aura.md`. Machine: Pop!_OS 24.04, COSMIC (Wayland + XWayland), PipeWire, Intel SOF DMIC. Everything below was checked in `tauri dev` on that machine, with screenshots.

## The user's reports and their causes

| Report | Cause found | Fix (commit) |
|---|---|---|
| "nothing visually I can see" | the wake-word thread never got audio: ALSA `pipewire` hung forever in `default_input_config()` (T7b), and then `default`/`pulse` stalled because NEXUS itself held the raw DMIC open (cpal enumeration keeps every PCM open) | probes behind a hard deadline (`dfdb2be`); names only, reopen one device at a time (`5c2abfe`) |
| the overlay "restricts keystrokes and mouse clicks" | focus: tao re-enables accept-focus after the first draw, so COSMIC focused the full-screen stage; clicks: the Linux hitbox loop had no cursor position and could make the whole window clickable | layer-shell overlay / non-focusable X11 + input region (`d0aae8c`), `.focusable(false)` (`8763102`) |
| "make it feel part of the OS" | the stage and sidebar were ordinary toplevel windows | layer-shell overlay under the panels, sidebar as a layer-shell drawer (`e26744d`) |
| "the orb is coming inside a window" | the Windows black capsule (`.orb-capsule`) framed the orb | orb style "aura": bare orb + audio-reactive rim, Linux default (`bbf2a13`) |

## Other jobs merged in this round

- J3 (`99a0620`): Setup's OAuth calls go through Rust's allowlisted `worker_request`; the renderer no longer sends `user_id` or sees the device credential.
- J5 (`781bedc`): the "point at X" marker on the stage (Linux) and the Screen pointer settings, saved to `pointer.json`.
- J7 (`b5de8f7`): the sidebar's editable "last heard" row (resend goes through Rust to the stage), word-by-word caption read-along on real Edge timings, a "Follow System" theme.
- J8 (`ab19ffd`): the dev-only preview harness `stage.html?eyeball` (`docs/testing/stage-eyeball-harness.md`); `useRoam.ts` deleted (no importers).

## Verification

- Live log: `[GDK] Wayland session on COSMIC (layer-shell supported)`, `overlay_linux: stage overlay running on LayerShell`, `free area 1920x1048 of 1920x1080 — … top: 32`, `audio: device 'default' accepted`, `audio capture started in 2.2s`.
- X11 fallback (before the library was installed): WM_HINTS input False, `_NET_ACTIVE_WINDOW` stays 0x0 after `nexus --wake`, X input shape empty when idle and exactly the orb (860,0 200×200) when shown.
- Drawer: asked 400×1008 at right margin 10 / top 20, got 400×1008.
- Aura: all four states rendered through the harness; on the desktop, the orb shows no box and a halo under the top panel.
- Tests: Rust lib 1930/1931 (the one failure is the known `turn_detect` speed assert in a loaded debug build); frontend 455+ tests, `tsc` clean; production `vite build` contains no harness code.

## Not done / deferred

- The redesign's Rust `audio_levels.rs` bridge and "1 px resize tolerance": not needed on this branch (the aura reads the orb's levels; the stage is sized by layer-shell or by `size_stage_to_monitor`).
- CI: the repository now runs Linux CI instead of the inherited Windows jobs, and every job passes; what changed and why is in `linux-port-linux-ci.md`. Before the switch, the Windows run compiled all three feature sets and passed 1683 Windows lib tests (its dead-code step later turned out to have run blind).
