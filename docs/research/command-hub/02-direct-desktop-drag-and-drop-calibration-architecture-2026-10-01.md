# Research 02 — Direct Desktop Drag & Scroll-Wheel Calibration Architecture

**Date:** 2026-10-01 · **Status: IMPLEMENTED** (this doc doubles as the implementation record)
**Companion:** `01-interactive-animation-alignment-and-calibration-architecture-2026-10-01.md` (slider design — superseded by this direct-manipulation design), `docs/changes/59-direct-desktop-drag-and-scroll-calibration.md` (per-file ledger)
**Origin:** Gemini antigravity plan `animation_alignment_calibration_plan.md` (session artifact), executed by the opencode session after the collision recovery.

---

## 1. What was built (one paragraph)

A **direct spatial-manipulation calibration system**: the user clicks "✥ Drag & Position on Desktop" in the Command Hub Display tab → the hub hides, a 440×54 translucent companion pill HUD appears top-center, and the real desktop preview of the selected target (Wakeup Orb / Ghost Waves / Loading Indicator) becomes grabbable. Dragging uses Tauri's **native `startDragging()`** (OS DWM moves the window at display refresh rate — zero webview latency); release position is converted to center-anchored percentages by a tested pure-geometry module and reported to Rust. Hovering + scroll-wheel resizes ±10px live (clamped 100–300px for Orb/Waves, 40–160px for Loading) with a pill badge showing the px value. Magnetic snap engages within 20px of the horizontal center (50%) or bottom edge (100%). `[ ✓ Save ]` writes all 9 parameters to settings.json (read-modify-write — unknown fields survive) and re-opens the hub with a success toast; `[ ✕ Cancel ]`/Esc restores the original placement (nothing was written) and re-opens the hub; `↶ Undo` steps back through in-session changes; `↺` resets the active target to defaults.

---

## 2. Architecture decisions (and why)

| # | Decision | Rationale |
|---|---|---|
| 1 | **Drafts live in Rust** (`calibration.rs::CALIBRATION`), both WebViews stay dumb | The HUD and the preview windows are separate WebViews; Rust is the single owner — previews only *report* (position/size) and *render* (badge), no state divergence possible |
| 2 | **Native `startDragging()` + debounced `onMoved`** (150ms trailing) | Pointer-up after a native OS drag loop is unreliable; onMoved fires continuously during the drag so a debounced trailing report captures the settled position |
| 3 | **Center-anchored pct everywhere** (Rust `overlay_xy` ↔ TS `positionToPct` mirror) | The orb placement math that survived live use; a perfect round-trip (pct → px → pct) keeps the element exactly under the cursor |
| 4 | **Waves share the orb window**; `waves_*` is applied by repositioning `main` during ghost sessions (`position_orb` branches on `ghost::session_active()`) | No second runtime window (RAM law — each WebView2 ≈ 250 MB). The waves own the visual during ghost sessions, so the session rect IS the waves rect |
| 5 | **Save = read-modify-write of exactly 9 keys** in settings.json | A struct round-trip would drop unknown fields (research 01 §10 precedent); key-level write preserves whatever else lives in the file |
| 6 | **Cancel needs no snapshot write** | Nothing is written until Save — re-reading the disk IS the restore |
| 7 | **Loading default ≈ historical top-right corner** (0.95/0.05 center-anchored, 80px) | Preserves the old hardcoded placement visually; documented near-identical (center-anchored), not pixel-identical |
| 8 | **One preview at a time** (target switch hides the other window) | Two previews would fight for focus; spec says "switches the desktop preview to that target" |

---

## 3. The 9 persisted parameters

`orbHorizontalPct`, `orbVerticalPct`, `orbSize` (existed) · `wavesHorizontalPct`, `wavesVerticalPct`, `wavesSize` (new, default = orb defaults per the wakeup=waves invariant) · `loadingHorizontalPct`, `loadingVerticalPct`, `loadingSize` (new, default 0.95/0.05/80).

All also live in `NexusSettings` (serde defaults, camelCase) so the settings UI round-trips them, and in the loose readers (`window_manager::read_waves_settings` / `read_loading_settings`) that the runtime show-paths use.

---

## 4. Component map

| Component | Location | Notes |
|---|---|---|
| Calibration session + 8 IPC commands | `src-tauri/src/calibration.rs` (+6 unit tests) | session state, target switch, report position/size, apply-drafts (undo), default, save, cancel |
| Pure placement math | `window_manager::overlay_xy` + readers + `position_loading` | single DPI conversion choke point; `position_orb` branches on session/calibration |
| Companion pill HUD window | `dyn_windows::WindowConfig::calibrate_toolbar()` — 440×54, transparent, topmost, focused | url `companion-hud.html`; registered in vite config + settings-sidebar cap |
| HUD UI | `frontend/src/companion-hud/` (main, CompanionHudApp, css, history + 5 tests) | segmented targets, undo/default, cancel/save, Esc/Enter, subtext |
| Pure geometry | `frontend/src/calibration/geometry.ts` (+9 tests) | `positionToPct` (snap), `wheelResize` (rails), `sizeClamp` |
| Orb preview (drag/wheel/badge) | `Avatar.tsx` calibration effect + handlers; container 100%/100% | waves preview via `calibrationTarget === "waves"` (no ghost session needed) |
| Loading preview (drag/wheel/badge) | `frontend/loading.html` calibration block | mirrors the same math inline (plain module, no React) |
| Store mirror | `assistant.ts` `calibrationTarget`/`calibrationSize` | set from `calibration:state` events in `App.tsx` |
| Command Hub entry + toast | `SettingsSidebarApp.tsx` Display tab card + `settings:toast` listener + css | launch button, success/cancel toast |
| Runtime loading placement | `orchestrator::show_loading` + `commands::show_loading_indicator` → `position_loading` | saved placement takes effect at runtime, both call sites |

---

## 5. Verification

| Gate | Result |
|---|---|
| `cargo test --lib -- --test-threads=1` | **756/756** (includes 6 calibration + 10 window-manager-era tests) |
| `npm test -- --run` | **87/87** (9 geometry + 5 history new) |
| `tsc --noEmit` | clean |
| `npm run build` | clean; `dist/companion-hud.html` emitted |
| `cargo build --release --features custom-protocol` | clean, 50.9 MB |

### Manual acceptance (user-run)
1. Command Hub → Display → **"Drag & Position on Desktop"** → hub closes, pill HUD top-center.
2. Drag the orb natively (smooth, zero lag); release → position persists in the session draft.
3. Wheel over the orb → ±10px with px badge.
4. **Waves** tab → orb window morphs to waves preview at its saved rect, draggable.
5. **Loading** tab → loading window appears, draggable, wheel-clamped 40–160.
6. **✓ Save** → HUD destroyed, hub re-opens with toast, `settings.json` holds the 9 keys.
7. Re-run and **✕ Cancel**/Esc → placement restored, hub re-opens, no disk change.
8. Ghost session → waves render at the saved `waves_*` rect; normal wake → orb at `orb_*`.
