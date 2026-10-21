# 39 — Ghost Phase 2: Mouse Ghost (2026-09-25)

Hands for the leash: eased motion, UIA-first grounding, land-verify,
and restore — all inside ghost sessions with the Phase 0/1 abort paths
(stop word, mouse grab, Esc) checked at every step boundary. Plan:
`docs/features/64-ghost-mode-plan.md` (Phase 2); leash: doc 37;
keyboard drill: doc 38.

---

## 1. What shipped

**`live/commands/mouse.rs` (new, Windows-only impl + stub):**
- `eased_steps()` (pure, tested): cosine ease-in-out interpolation.
  Ease reads as intentional motion and gives the abort poll time to
  fire mid-glide; monotonicity asserted (never moves backwards).
- `move_eased(x, y, ms, should_stop)`: polls `GetCursorPos`, glides in
  ~25ms steps with a stop check per step, registers the ghost suppress
  window (`note_expected` for motion + 150ms tail) so the takeover
  detector stays quiet about our own glide, clears it after.
- `click_at` / `double_click_at`: eased move, then atomic click(s).
  Abort applies between steps, never mid-click — half-clicks don't exist.
- `scroll(lines)`: no cursor motion at all — takeover-safe by construction.
- `drag_to`: press → eased glide → release, with release-on-abort (a
  stuck left-button would corrupt the user's next clicks; the abort path
  releases before returning).
- `restore(pos)`: glide home on session exit (best-effort; a failed
  restore is reported in the result line, never silent).

**UIA-first resolver** (`resolve_element` + `element_center`): exact
bounds from `screen::list_actionables()` (foreground window), scored
exact > starts-with > contains. Password/secure fields never resolve
(screen.rs skips them upstream; denylist + confirm gates remain the
outer layers). No vision, no pixels guessed, millisecond latency.

**`live_ghost_click(app_title, element)`** (`live/mod.rs`, registered in
`lib.rs`): triple safety screen first (`mouse_move`/`mouse_click` added
to `ALLOWED_TOOLS`; app/element through the denylist), then focus app
(verified — never click blind) → resolve → eased glide → click →
re-verify foreground (a stolen focus means the click may have landed
elsewhere: report it, never click twice) → glide home → exit +
narration ("Done" / the failure). Send-style confirmation is not
required for clicks; destructive targets stay refused by the denylist.

## 2. Findings

- **Grounding order confirmed in-tree**: `screen.rs` already listed UIA
  actionables and center-clicked them; Phase 2 wraps that exact
  machinery in easing + session + verify + restore instead of
  rebuilding it. Vision stays deferred until a measured need (per the
  research cost ledger: local vision would double the RAM budget).
- **Suppress windows compose**: every motion registers
  `note_expected`, every exit path (success/stop/takeover/error)
  clears via `note_idle` — including the drag-abort and restore paths.
  An un-cleared suppress would blind takeover detection; all paths
  audited.
- **No new frontend surface**: ring, notices, and session events already
  exist from Phase 0; clicks reuse them unchanged.

## 3. Verification (twice)

Rust full lib **537/537 serial** (new: easing endpoints/count,
monotonicity + ease shape, minimum-step clamp). `cargo check` clean,
zero new warnings. No frontend files changed. Live drill procedure for
the built app: `live_ghost_click` on a real app (e.g. Calculator,
element "Seven") → ring rides the glide → click lands →
cursor glides home → "Done"; grab mid-glide (<100ms freeze, "You have
it"); Esc mid-glide; resolve-miss ("couldn't find …") with no motion
at all.
