# Plan 02 — User-Positionable Stage Execution Plan (Command Hub Future)

**Date:** 2026-09-30 · **Status: PLAN ONLY — no code until the user says go.**
Research: `01-user-positionable-stage-architecture-2026-09-30.md` (§ references
below like R§2.1 point at research sections).

**Goal restated:** Command Hub designer → user positions orb/waves, loading,
sidebars anywhere → saved → every appearance slides from the correct edge;
waves locked to wakeup; sidebar + loading sizes user-controlled.

**Program rules inherited:** dual-gate every phase (serial Rust tests +
vitest + tsc, each twice); no unverifiable code; design freeze on orb/waves
content (positions move windows, never pixels inside files); ghost explicit-
exit semantics untouched.

---

## Phase 0 — Schema, geometry core, gates (foundation; no visible change)

**Enter when:** user says go. **Exit when:** all new pure fns tested, settings
round-trip green, zero UI change.

| # | Change | Files |
|---|---|---|
| 0.1 | New `NexusSettings` fields (all `#[serde(default)]` + `default_*`): `loading_h_pct/v_pct/size`, `sidebar_h_pct/v_pct/width/height` (+ per-label override struct, empty = inherit), `slide_enabled/duration_ms/direction_override`, `waves_lock` (default true), `settings_version: 1` | `commands.rs:1528` |
| 0.2 | Atomic save (temp + rename) + unknown-field preservation in `save_settings` | `commands.rs:1837` |
| 0.3 | Pure rect core: `zone_rect(zone, w, h, screen, taskbar)` (9 zones + clamp), `slide_vector(zone, override)` → (dx, dy), `to_physical_rect` choke point (single DPI conversion) | new `src-tauri/src/stage_geometry.rs` (+ 20 tests: zero screens, 200% DPI, ultrawide, portrait, off-grid pct) |
| 0.4 | Mirror in TS: `zoneRect`, `slideVector`, preset→pct table | `stage/geometry.ts` (+ tests, mirror-parity test vs Rust vectors) |
| 0.5 | Docs: feature stub number reserved; research §12 spikes 3–4 (monitor-ID stability, work-area API) time-boxed 1 day | docs |

**Gate:** Rust serial green, vitest green, tsc clean ×2. Settings file with
new fields loads; without them behaves exactly as today.

## Phase 1 — Rust setters + ordering (still no UI)

| # | Change | Files |
|---|---|---|
| 1.1 | `set_sidebar_geometry(label, h, v, w, h)` + `set_loading_geometry(h, v, size)` — preview-without-save, clamp rails (R§10), re-assert topmost/focus after moves (R§7) | `commands.rs` (mirrors `set_orb_position`, `window_manager.rs:132`) |
| 1.2 | Refactor the three copy-pasted dock maths (sidebar/architect/PR) + settings-sidebar + loading onto `zone_rect` (bottom-right zone = today's numbers, pixel-identical) | `commands.rs:677-730, 918-1040, 1178-1240, 1321-1400, 1470+`, `architect.rs:567-600, 792-810`, `orchestrator.rs:3826-3850` |
| 1.3 | Enforce position→capture→show in every refactored path (R§1.5); ordering unit test with mocked windows | same sites |
| 1.4 | `monitor_id` capture on show + fallback chain (saved → same-geometry → primary) + re-clamp every show | `window_manager.rs`, `stage_geometry.rs` |

**Gate:** pixel-diff test (old vs new dock math identical), full suite ×2.
Visible behavior unchanged.

## Phase 2 — Command Hub designer tab (the user-facing core)

| # | Change | Files |
|---|---|---|
| 2.1 | Display-tab extension: mini-map with draggable orb dot + panel rects (orb/loading/sidebar layers), 9-zone preset grid, per-surface size sliders, slide enable + duration + direction readout, waves-lock toggle, per-surface + global reset | `SettingsSidebarApp.tsx` (extends L274-388), `settings-sidebar.css` (extends L414+) |
| 2.2 | Dirty-flag flow: drag/slider → preview IPCs (`set_orb_position` exists; 1.1 adds the rest) → Save calls `save_settings` → Reset restores + previews | same + `commands.rs` save path |
| 2.3 | Collision readout: mini-map draws orb + loading + sidebar rects together; overlap → warning badge (never forbid) | designer only |
| 2.4 | Designer reducer unit tests (drag→pct, presets→pct, dirty logic, reset) | new test file, 15+ tests |

**Gate:** vitest + tsc ×2; manual mini-map drill on 100%/150% DPI
(screenshot matrix); unplug-monitor drill keeps windows visible.

## Phase 3 — Directional slide engine

| # | Change | Files |
|---|---|---|
| 3.1 | Slide runtime: `win.show()` at final rect → CSS keyframe `translate(vector)+fade` → rest; exit: slide-out → hide on `animationend`/existing 500ms pattern; `prefers-reduced-motion` → fade only | `Avatar.tsx` (orb), `styles.css` (keyframes per edge), sidebar roots, loading root |
| 3.2 | Gate integration: slide-out consults `hideOrbAfterSpeech` conditions (never mid-speech/ghost); ghost enter keeps pinch (no slide in ghost) | `net/orchestrator.ts`, `Avatar.tsx` ghost effects |
| 3.3 | Click-through ON during flight, restore on settle (R§7) | `window_manager.rs` `set_click_through` call sites |
| 3.4 | Slide-gate tests (`transition()`-style: speech/ghost veto) + vector tests per zone (corners diagonal, center fade) | frontend suite |

**Gate:** full suite ×2; per-zone slide checklist (9 zones × orb/loading/sidebar, video or screenshots); weak-iGPU smear check.

## Phase 4 — Voice control + acceptance (program close-out)

| # | Change | Files |
|---|---|---|
| 4.1 | Deterministic intents: `move orb to <zone>`, `bigger/smaller orb`, `move sidebar left/right`, `reset positions` (+ Layer-3 phonetic aliases, EN-only) → preview IPCs + cached confirm line; persist only on explicit "save" | `intent_parser.rs`, center routing, `command_center.rs` if compounds |
| 4.2 | EN voice fixture additions (zone synonyms: top/up/north…) | NLU fixture, e2e count unchanged-or-better |
| 4.3 | Full acceptance: 9-zone matrix × DPI × monitor drill × slide checklist × voice intents × ghost-session non-interference (geometry changes deferred to next normal show mid-session) | manual + recorded |
| 4.4 | Docs: feature doc (numbered), changes ledger entry, AGENTS.md session note, research §12 questions closed or carried | docs/ |

**Gate:** Rust serial ×2, vitest ×2, tsc, e2e fixture green, user live acceptance.

---

## Cross-phase invariants (violations stop the line)

1. `wakeup = waves`: one rect, one slide, one window (until/unless a future
   migration moves layers — then the lock forces mirroring).
2. Position → capture → show ordering in every window path.
3. Re-clamp + re-apply on every show (never cache physical px).
4. Ghost explicit-exit + fullscreen-pause semantics untouched.
5. Non-activating flags re-asserted after every move.
6. No new windows kept alive (RAM law); move/resize only.
7. Reduced-motion always honored.
8. Screen-layout data never leaves the device (no positions in cloud logs).

## Effort sketch (for planning, not commitment)

Phase 0: S–M · Phase 1: M (5 dock sites) · Phase 2: M–L (designer UI dominates) ·
Phase 3: M (beats integration fiddly) · Phase 4: S–M. Biggest risks: R§12
rows 1–3 (smear, flag resets, monitor-ID stability) — spike early in Phase 0.
