# 41 — Ghost Phase 4: Hardening (2026-09-25)

Calibration, refusal battery, and exclusive-fullscreen auto-pause —
the pass that makes Ghost Mode boring (in the good way). Plan:
`docs/features/64-ghost-mode-plan.md` (Phase 4, now built).

---

## 1. What shipped

**Calibration probe** (`mouse::calibration_probe` + `live_ghost_calibrate`
command): enters a ghost session (ring, Esc, stop checks), glides a
5-point grid, reads back landing error per point, reports
max/mean deviation in physical px with a verdict (≤5px excellent, ≤15px
acceptable, else poor — check display scaling). Aborts cleanly like any
drill. Run once per machine, re-run after display changes. This is the
number the DPI math lives or dies by — the research flagged coordinate
scaling as the #1 DIY-agent failure mode, ahead of model accuracy.

**Exclusive-fullscreen auto-pause** (`mouse::foreground_blocked` +
`is_foreground_fullscreen`): compares the foreground window rect
against the primary monitor size (new `screen::primary_monitor_size`).
When an exclusive-fullscreen app is front, both drill runners pause
with a spoken "paused — a fullscreen app is in front, sir." instead of
clicking into the void. Checked in the drill guard, the click stop
closure, and as an explicit pre-check (so the message names the cause
instead of a generic "Stopped").

**Refusal battery:**
- UIA scorer extracted to pure `score_name()` (exact > starts-with >
  contains; empty never matches) with ranking/normalization tests.
- Pinned by test that scoring does NOT special-case passwords — the
  exclusion lives in exactly one place (`screen.rs` listing skips
  password fields upstream) plus the safety denylist, so it can't
  silently migrate and rot.
- `ghost_calibrate` added to `ALLOWED_TOOLS` (it moves the mouse — it
  IS control); mouse tools refused against blocklist targets, tested.
- UAC/elevated windows: no silent failure mode added — clicks into
  elevated windows are eaten by UIPI with no error. Documented as a
  known limitation with the honest message path (focus-verify + land
  reporting); detection via token-elevation checks scoped as follow-up.

## 2. Real flaw fixed while wiring

The mouse drill's glide-home ran **unconditionally** — including after
stop/takeover, driving the cursor into the user's just-grabbed hands.
Restore now runs only on calm runs (no stop, session still active);
after takeover the cursor stays exactly where the user put it.

## 3. Verification (twice)

Rust full lib **544/544 serial** (new: scorer ranking/normalization/
no-special-casing, ghost tool allowlist + blocklist refusal). `cargo
check` clean, zero new warnings (one unused-import caught and fixed
during the build). No frontend files changed. Live procedures for the
built app: `live_ghost_calibrate` on the machine (record max/mean);
launch a fullscreen game/video → any ghost command pauses with the
spoken message; drill refusal probes (password-named element never
resolves, bank targets refused).
