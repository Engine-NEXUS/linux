# Plan 03 — Keyboard Nudge, Save Semantics, Liquid-Glass HUD ( + Waves-Preview Fix)

**Date:** 2026-10-01 · **Status: Phases 0/2/1 IMPLEMENTED 2026-10-01; Phase 3 (liquid glass) DEFERRED per user — code reverted, plan retained below for later.**
**Supersedes nothing; extends:** research 02 (drag/scroll calibration, implemented) + the Waves-preview defect analysis (D1–D3, diagnosed → fixed as Phase 0 below).
**Reference aesthetic:** frosted-glass pill that stays legible on any wallpaper (light/dark adaptive).

---

## Phase 0 — Waves-preview defects D1–D3 (prerequisite, already planned)

Switching Wakeup→Waves mounts an empty container (Lottie mount effect gated on `ghostPhase`, never fires without a ghost session); bars freeze flat when the asset 404s; returning to Wakeup leaves a blank orb (init effect deps `[animationData]` never re-fire). Fix per the approved D1–D3 plan: `wavesShown` selector + preview motion (lively Lottie/bars) + `calibrationTarget` in init-effect deps. **This phase lands first** — keyboard nudging is pointless if the Waves preview is blank.

---

## Phase 1 — Save semantics (confirm + communicate; mostly already correct)

**Confirmed behavior (no change needed):** drafts snapshot from disk at session start, so positioning 1, 2, or all 3 targets then pressing Save once persists everything — untouched targets keep their disk values. No per-target save exists or is needed.

**What to add (communication, not mechanics):**
1. **Dirty dots on the numbered badges:** Rust already compares draft-vs-initial per target — re-add the `initial` snapshot to `CalibrationSession` (removed during clippy cleanup; now genuinely needed) and include `dirty: { wakeup, waves, loading }` in the `calibration:state` payload. HUD renders a ✓ dot on positioned targets. The user *sees* that "save 1 animation too" works.
2. **Save toast echoes scope:** `"Wakeup + Waves saved."` / `"All 3 animations saved."` — computed in `calibration_save` from the dirty flags, replacing the static line. Cancel toast unchanged.
3. **Tests:** dirty-flag unit tests (Rust), toast-scope pure fn test (frontend).

---

## Phase 2 — Numbered targets + keyboard nudge control

### 2.1 Numbered badges + selection keys
- HUD tabs become `[ 1 🌟 Wakeup ] [ 2 🌊 Waves ] [ 3 ⏳ Loading ]` (number prefix = the selection key).
- Keydown in the HUD window (already focused, `focus: true`; Esc/Enter precedent exists): `1/2/3` → `invoke("calibration_set_target")`. Numpad codes included (`Digit1` + `Numpad1`).
- **Scope rule (safety):** window-scoped listener only — no global hotkey registration. Keyboard control works while the HUD is focused; after dragging the desktop preview (focus moves to the orb/loading window), the user clicks the HUD to resume keyboard control. Documented in the subtext line.

### 2.2 Arrow-key pixel nudging
- `←→↑↓` = 1 logical px; `Shift` + arrows = 10px (matches the wheel step).
- New Rust command `calibration_nudge { dx_px, dy_px }`: shifts the ACTIVE target's draft by re-solving pct from the current window position + delta (pure fn `overlay_nudge(h, v, size, dx, dy, screen, scale)` in `window_manager.rs`, tested: clamps, DPI scaling, ultrawide), applies via the existing `apply_active`, broadcasts state.
- Why Rust-side, not frontend pct math: one conversion path (the existing `overlay_xy` inverse), no duplicated monitor/scale logic, nudge works even if the preview window moved between events.
- HUD subtext becomes: `1·2·3 select · arrows nudge (⇧×10) · wheel scales · Enter saves`.
- Undo covers nudges automatically (each nudge triggers a state event → history commit). High-frequency concern: holding an arrow fires ~30 events/sec → history cap (50) churns. **Coalescing rule:** HUD merges consecutive nudge-commits from the same target within 800ms into one history entry (pure `coalesceNudge` in `history.ts`, tested). Rust applies every event (live motion stays 1:1); only the *undo record* coalesces.

### 2.3 Tests & gates
- Rust: `overlay_nudge` table tests (edges, 200% DPI, negative deltas, clamp rails) + `calibration_nudge` session-guard test (no-op outside session).
- Frontend: keydown mapping test (Digit1/Numpad1/arrows/Shift), `coalesceNudge` tests.
- Full gates: 756+ Rust serial, vitest, tsc, both builds.

---

## Phase 3 — Liquid-glass HUD that works on any wallpaper

**Problem:** the current pill uses `rgba(15,23,42,0.82)` + CSS `backdrop-filter: blur()` — and backdrop-filter is a documented no-op in transparent WebView2 (AGENTS.md sidebar section). On bright wallpapers the pill washes out (the reference mock's left half).

**Solution — reuse the proven sidebar fake-blur (no new native code paths):**
1. **Capture:** in `show_calibration_hud`, after computing the HUD rect (already known: top-center, 440×54 logical), call the existing `sidebar_backdrop::capture_and_blur(x, y, w, h, 32.0)` BEFORE `hud.show()` (critical ordering: capture must precede show or the HUD photographs itself — same law as `commands.rs:695-729`), store in a new `PENDING_CALIBRATION_BACKDROP`, expose `get_pending_calibration_backdrop` (mirrors `get_pending_settings_backdrop`).
2. **Render:** HUD fetches on mount → CSS var `--hud-backdrop-image` as the card's background layer (same mechanism as `.sidebar-card`), keeping the current translucent tint + hairline border + shadow on top.
3. **Luminance-adaptive foreground (the reference image's trick):** during capture, compute mean luminance of the blurred region in Rust (cheap: downsample to 8×8 in the existing blur pass, average) → include `theme: "light" | "dark"` in the state payload → HUD switches CSS token sets (text/border/accent for light vs dark). Threshold with hysteresis (switch at 0.45/0.55) so mid-tone wallpapers don't flicker.
4. **Static-position bonus:** the HUD never moves during a session → one capture per session, zero re-capture cost (unlike sidebars, no live-blur loop needed).
5. **Non-Windows:** `capture_and_blur` returns None off-Windows today → HUD falls back to the current tint (documented, same as sidebars).
6. **Tests:** luminance-threshold pure fn tests (Rust + TS mirror), pending-backdrop round-trip test, fallback test (None → tint-only class).

---

## Phase 4 — Docs & acceptance

- Research 02 gets a §9 (this plan's outcomes); changes ledger `60-…` entry; AGENTS.md session note.
- Manual acceptance: (a) bright + dark wallpapers — pill legible both, theme switches; (b) `1/2/3` select with HUD focused; (c) arrows 1px / Shift+arrows 10px with live draft motion; (d) Undo reverses a nudge burst in ONE step (coalescing); (e) position 1 target, Save → only-dirty toast, disk holds all 9 (untouched = old values); (f) Esc cancels, disk untouched.
- Gates: full Rust serial, vitest, tsc, `npm run build`, release `--features custom-protocol`.

## Cross-phase invariants (violations stop the line)
1. No global key registration — HUD-window keydown only (Esc/Enter precedent).
2. No new runtime windows; drafts stay Rust-owned; save stays 9-key read-modify-write.
3. `position_orb` calibration early-return + ghost Waves branch untouched.
4. Fake-blur capture strictly before HUD show (never photograph self).
5. Screen-layout data never leaves the device (luminance scalar only, no pixels in logs).

---

## Implementation outcomes (2026-10-01, all phases)

| Phase | What landed | Files | Tests |
|---|---|---|---|
| 0 — Waves preview | `resolveWavesShown` selector + Lottie mount in preview (0.6x loop) + free-running CSS bars in preview + smile re-attach on return (`calibrationTarget` in init deps) | `Avatar.tsx`, `avatarAnim.test.tsx` | +4 selector tests |
| 2 — Keyboard | `1/2/3` (+numpad) select, arrows 1px / ⇧10px via Rust `calibration_nudge` + pure `overlay_nudge`; coalesced undo bursts; numbered badges; subtext | `calibration.rs`, `window_manager.rs`, `lib.rs`, `calibrationKeys.ts` (+4 tests), `CompanionHudApp.tsx`, `companion-hud.css` | +6 Rust (nudge table), +4 keys tests |
| 1 — Save semantics | `initial` snapshot back; `dirty` flags in state payload; ✓ dots on tabs; scoped toast ("Wakeup + Waves saved." / "All 3 animations saved." / "No changes — placement unchanged.") | `calibration.rs`, `CompanionHudApp.tsx`, css | +3 Rust (toast scopes, equality) |
| 3 — Liquid glass | **DEFERRED (user call, 2026-10-01): all Phase-3 code reverted** (backdrop static/capture/command/test, `theme.ts`, HUD wiring, light tokens). Plan below retained as-is for later. | — | 0 (reverted) |

Deviation from plan: hysteresis band (0.45/0.55) dropped — the theme is decided once on mount, so flicker is impossible by construction; single 0.5 threshold.

**Phase 3 deferred 2026-10-01 (user decision): implemented, verified, then fully reverted on scope call — only Phases 0/2/1 ship. Retained above for later.**

Gates (Phases 0/2/1): Rust 764/764 serial · vitest 95/95 · tsc clean · `npm run build` + release `--features custom-protocol` clean.
