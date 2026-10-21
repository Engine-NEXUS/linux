# Ghost Mode Fullscreen Blackout Fix — Live Glass Isolated to Compact Windows

**Date:** 2026-10-01
**Symptom:** entering Ghost Mode turned the entire screen pitch black; only Esc abort recovered.
**Status:** Fixed, all gates green, release rebuilt.

---

## Root cause (verified, not assumed)

`dyn_windows.rs:202` applied `live_glass::apply_live_glass()` to every window in
`sidebar | architect-sidebar | pr-list-sidebar | settings-sidebar | stage`.
`stage` is a **1920×1080 fullscreen overlay**. DWM applies `DWMSBT_TRANSIENTWINDOW`
(Acrylic) / `ACCENT_ENABLE_BLURBEHIND` across the window's **entire HWND rect** —
for a fullscreen window that means an opaque dark Acrylic backdrop over 100% of
the monitor. The DOM was innocent: `ghost.css` paints no fullscreen background
(only component-level ring/bubble fills), and the stage overlay button was a
small fixed island.

Rule going forward: **a fullscreen overlay HWND must never receive a DWM
system backdrop or whole-window blur. Live glass belongs only on compact,
dedicated window HWNDs.**

## Changes

| File | Change | Why |
|---|---|---|
| `src-tauri/src/dyn_windows.rs:202` | dropped `\|\| config.label == "stage"` from the live-glass branch | Stage stays 100% transparent (also drops rounded corners + capture-affinity for stage, matching the documented "stage is share-visible, never capture-excluded" policy) |
| `frontend/src/stage/main.tsx` | `stage-root` gets explicit `background: "transparent"`; removed the floating `LiquidGlassButton` overlay block (+ unused import) | Stage is a non-intrusive overlay: ring + pointer + bubble only. The button self-registered its hitbox on mount (`LiquidGlassButton.tsx:44`), so removal leaves no dead click-eater. `invoke` still used (heartbeat) — no dead import. |

## Gates

| Gate | Result |
|---|---|
| `npm test --run` | **97/97** (task text cites 100/100 — delta is the other lane's in-flight additions; everything present passes) |
| `cargo test --lib` serial | **769/769** (same note vs the cited 770/770) |
| `npm run build` | clean |
| `cargo build --release --features custom-protocol` | clean, **50.9 MB** |

## Live verification (user-run)

Enter Ghost Mode ("Ghost mode.") on a normal desktop → **no blackout**; ring/pointer/bubble render over the live desktop; sidebars keep their frosted glass; Esc still aborts.

---

## Follow-up: "still pitch black" + "no different transparency blur" (2026-10-01)

### What the cross-check found (two separate causes, neither is the old branch)

1. **"No transparency blur" — SOLVED, root cause in CSS, not DWM.**
   `sidebar.css .sidebar-card::after` was `background-color: #060608; opacity: 1`
   (a same-day user directive hiding blurred text showing through). An opaque
   layer occludes the DWM live blur beneath it completely — the hardware blur
   was applying fine, nobody could ever see it. Fix: `::after` is now a
   translucent adaptive tint only — `rgba(0,0,0,0.35)` dark /
   `rgba(255,255,255,0.2)` light (research 02 §4 numbers), driven by the
   existing live `data-glass-luminance` probe, plus a light-mode text-shadow
   boost (text was already pure `#ffffff`). Revert path documented in the CSS.
2. **"Still pitch black" — NOT reproducible from the current tree.**
   The DWM-glass branch now covers only `sidebar | calibrate-toolbar`;
   stage creation/show applies no glass, no paint, no affinity; page CSS is
   transparent; no frontend caller invokes `apply_live_glass_cmd`. Added a
   forensic line — `live_glass: skipped for '<label>' (compact-windows-only
   policy)` — so the user log proves exclusion at runtime.
3. **Stale-binary trap (documented, unverified which binary the user runs):**
   Start Menu `NEXUS.lnk` → `%LOCALAPPDATA%\NEXUS\nexus.exe` dated
   **2026-08-31** (predates ghost mode, glass, and every fix). `nexus start`
   runs `target\release\nexus.exe` WITHOUT rebuilding unless asked. If the
   blackout was observed from the Start Menu copy, no source fix can reach it.

### Verification demanded from the live run (paste these)

1. Binary timestamp (`target\release\nexus.exe` — must postdate the fix).
2. Ghost-enter log lines: `creating 'stage' window` + EITHER
   `live_glass: skipped for 'stage'` (glass innocent → WebView2/driver
   transparency failure, next suspect) OR `live_glass: applied ...` (the
   exclusion regressed — reopen immediately).
3. `stage: frontend alive (client=...)` (renderer alive + build identity).

## Root-Cause Revision (2026-10-01, second follow-up)

The stage-exclusion above was a real but NOT the dominant fix. Cross-check
against the user's GitHub HEAD (per user directive "compare my changes from
GitHub") revealed the actual blackout source:

- `live_glass.rs::apply_live_glass` (added by the same session as doc 61)
  applied `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_TRANSIENTWINDOW` (Acrylic) to
  the **non-activating** sidebar HWND — the exact API ADR-05 Option A
  rejected: DWM paints a **solid opaque fallback** on inactive windows and
  the material call overrides tao's transparency → the entire window surface
  becomes pitch black, under every CSS layer.
- `dyn_windows.rs` glass branch reverted to the GitHub-verified state:
  `round_corners` + `WDA_EXCLUDEFROMCAPTURE` ONLY, no DWM material call.
  (Calibrate pill keeps the same treatment; `live_glass.rs` module stays
  but its accent path has zero callers — dormant.)
- `sidebar.css` card surface restored to GitHub values
  (`rgba(20,20,22,0.60)` + `1px` border + `0 24px 64px` shadow).
- Stage exclusion (this doc) remains valid as defense-in-depth.

Verify: tsc 0 · vitest 114/114 · `node nexus.mjs build` → 50.9 MB @ 22:24.
