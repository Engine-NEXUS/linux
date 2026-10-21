# 116 — Hand gestures for switching windows and tabs, like the touchpad (PLAN ONLY)

Builds on [115](115-camera-hand-mouse-plan.md) and change [127](../changes/127-hand-mode-camera-pointer-p0-p2.md).

**STATUS (2026-10-14): approved ("go ahead with the gesture plan") and built, except G2.** G0 logging (`spread=`), G1 tab
flick, G3 window switcher with held Alt and G4 desktops / Task View / show desktop are in `hand/gesture.rs`; 4-finger desktop
gestures ship OFF until the spread threshold is checked on a real hand. G2 (scrub with tab names via UI Automation) is not
built. Record: [change 128](../changes/128-hand-gestures-and-field-test-4.md), [118](118-hand-mode-master-compendium.md).
The text below is the original plan.

## What you asked for

"Gestures to shift the window, like touching four fingers on the touchpad and moving it, so we can choose
the tab perfectly."

## How the touchpad does it (the model to copy)

Windows 11 touchpad defaults (all configurable in Settings > Bluetooth & devices > Touchpad):

| Fingers | Swipe | Does |
|---|---|---|
| 3 | left / right | switch to the previous / next app (Alt+Tab style) |
| 3 | up / down | Task View / show desktop |
| 4 | left / right | switch virtual desktop |
| 4 | up / down | Task View / show desktop |

Three properties make it feel good, and the hand version must have them too:
1. **The fingers going down is the clutch**: nothing happens until N fingers are on the pad.
2. **Movement is quantised into steps** (one swipe = one app / desktop), with a clear commit.
3. **It never fires while you are just pointing or typing.**

## Proposed hand version

New poses (extend `pose.rs`, same finger ratios we already log):
* **Three** = index + middle + ring straight, pinky folded, thumb tucked. Window / app switching.
* **Four** = index to pinky straight AND held together (a flat "blade"). Virtual desktops / Task View.
  Four-together versus an open palm is a spread test (index-tip to pinky-tip distance in palm widths);
  the numbers must come from your hand, so step G0 only logs them.

While in Three/Four the pointer is locked (exactly like the scroll pose), so the cursor does not wander.

| Gesture | Action | Reuses |
|---|---|---|
| Three, quick flick right / left | next / previous **tab** (browser, editor, terminal) | shortcut router `next_tab` / `prev_tab` (wrong-window and protected-window checks already tested) |
| Three, hold, then move right / left slowly | **scrub** tabs one step per ~6 % of the frame; open palm or fist = commit, Esc = cancel | same actions, repeated |
| Three, hold up, move | **window switcher**: hold Alt, tap Tab per step, release Alt to commit (Windows shows its own preview strip) | router `switch_window` |
| Four, flick left / right | previous / next **virtual desktop** | new chord Win+Ctrl+Left/Right |
| Four, flick up | Task View (Win+Tab); flick down: show desktop (Win+D) | new chords |

"Choose the tab perfectly": while scrubbing, the overlay shows the tab names. The names come from one UI
Automation query of the browser's tab strip (the cached window-rooted query already measures 24-80 ms),
and commit jumps straight to that tab with the router's `goto_tab_n`, so a miscounted step cannot select the
wrong tab. If the tab strip cannot be read, scrubbing falls back to relative steps.

## Safety rules (same philosophy as the pointer)

* Only while the pointer is armed. Pose must be held 250 ms before it counts; a hand moving fast while
  changing shape is ignored.
* **Never leave a key held**: Alt / Win are released on disarm, lost hand (0.6 s), Esc, mouse takeover, and
  automatically after 10 s.
* Refuse in protected windows and for elevated targets (same gate as clicks); one action per flick, with the
  same 150 ms refractory as clicks.
* Dry-run first: in preview the overlay shows what would happen ("next tab") without sending keys.

## Phases (each ends with a measured gate)

* **G0 - measure**: log the Three / Four ratios and the fingertip spread for you (no new behaviour).
  Gate: Three, Four and Palm separate cleanly in your logs.
* **G1 - flick = next / prev tab** with pose clutch, refractory, preview overlay.
  Gate: >= 95 % of 50 deliberate flicks recognised; < 1 false trigger per 10 minutes of typing, talking and
  pointing; flick to tab change <= 200 ms.
* **G2 - scrub with names** (UIA tab strip, jump by `goto_tab_n`). Gate: lands on the intended tab in 20 of 20
  trials with 8-10 tabs.
* **G3 - window switcher** with held Alt. Gate: never a stuck Alt in 30 forced interruptions (hand lost,
  Esc, disarm, mouse move).
* **G4 - virtual desktops, Task View, show desktop.**

## Needs from you

1. Is the Windows touchpad mapping above what you want (3 fingers = apps / tabs, 4 fingers = desktops)?
2. Which hand, and do you ever work with the keyboard hand held up? (decides how aggressive the clutch is)
3. Typical tab count in the browser you use most (scrub step size).
4. Say go for G0 and I will add the logging first.

## Honest limits

* A hand has no "fingers on the pad" contact signal; the clutch is a held pose, so it is slower and easier to
  trigger by accident than a touchpad. The 250 ms hold and the armed requirement are the mitigation.
* Three-versus-four fingers and together-versus-spread depend on camera angle and light; thresholds come
  from your own logs, not from guesses.
* On macOS and Linux the same gestures need different chords and permissions: see
  [117](117-cross-os-screen-adaptation-and-intent-plan.md).
