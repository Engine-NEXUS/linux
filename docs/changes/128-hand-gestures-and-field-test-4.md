# 128 — Hand gestures (tabs, windows, desktops) + field test 4 fixes

Plans: [features/116](../features/116-hand-gestures-window-and-tab-switching-plan.md), master record
[features/118](../features/118-hand-mode-master-compendium.md). Research: [research/hand-mode/01](../research/hand-mode/01-camera-attention-hand-pointer-and-gestures-research.md).

## What changed

### Chip placement
`frontend/src/stage/handOverlayModel.ts` `chipPlacement(dock, viewport, chipW, chipH, taskbarBottom)` + `taskbarBottomPx`;
`HandOverlay.tsx` reads `useAssistant.orbPosition` (subscribes to changes) and `window.screen`. Chip: horizontally centred,
opposite the orb (orb top -> bottom centre above the taskbar; orb bottom -> top centre). The orb-rect listener and
`get_pending_orb_rect` call were removed from the overlay.

### Click reliability (field test 4)
`hand/engine.rs update_left`: cancel only when the raw signal is still on and the pose gate fails; `pinch_close 0.38`,
`pinch_open 0.6`. See features/118 section 5.1.

### Leaving hand mode
`intent_parser.rs loose_hand_exit` (only while `hand::active()`), `Action::ExitMode` for fist-after-disarm (2 s) and
10 minutes without a hand, handled in `hand/mod.rs apply()` through a separate thread (stop() takes the session lock).

### Pointer speed
`Config.sensitivity / precision`, `Engine::set_pointer`, ballistic curve in `move_cursor`; `hand/mod.rs pointer_prefs()`
(`SystemParametersInfoW`, direct `user32` FFI) read at start and at each arm; setting `handSensitivity` (0 = follow Windows).

### Gestures
* `hand/pose.rs`: `Blade{Three,Four}`, `spread`, `blade_pose`, `FOUR_SPREAD_MAX`.
* `hand/gesture.rs` (new, pure): `Gestures`, `Params`, `Cmd`; 15 tests.
* `hand/engine.rs`: `Config.gestures / gestures_desktop`, `Action::{Gesture, BladeChanged, ExitMode}`, `Snapshot.gesture`,
  pointer lock while a blade pose is held, `release_all` cancels an open switcher. `PoseChanged` now carries `spread`.
* `hand/mod.rs`: `Out::Gesture`, `run_gesture` (protected/elevated guard, tab actions through the shortcut router, Alt held for
  the switcher with a 12 s failsafe, Win chords), `[HAND] gesture ...` log lines.
* `shortcuts/exec.rs`: `pub fn send_events` (batch SendInput of raw key events). `shortcuts/keys.rs::KeyEvent` is used directly.
* Settings (all in `NexusSettings` + frontend defaults + Command Hub card): `handGestures` (true), `handGesturesDesktop`
  (false), `handSensitivity` (0).
* Frontend: `HudState.gesture`, gesture chip text and 1.2 s flash labels, no click ripple for gesture commands.

## Verified
* Rust: hand module 93 tests pass serially (engine +14 incl. mutation-checked click rule, gesture 15, pose +1, parser +1).
* Frontend: tsc clean; vitest 293/293.
* The full serial Rust suite, IPC/turn-end/hygiene gates and the dead-code ratchet: results are recorded in AGENTS.md.

## Not verified (needs the user's hand / other machines)
Gestures on a real hand (false-trigger rate, flick distance), the Four spread threshold, the click rule with real landmarks,
exit by fist and idle, pointer-speed read with a non-default Windows setting, multi-monitor, macOS, Linux.

## Pitfalls
* Other sessions edit `intent_parser.rs` / `commands.rs`: after a long edit session re-grep that `loose_hand_exit` and
  `hand_gestures` still exist before building.
* `Action::PoseChanged` has a new `spread` field: use `..` in patterns.
* Four-finger desktop gestures stay off until the logged spread values have been checked on a real hand.
