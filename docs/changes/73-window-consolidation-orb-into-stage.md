# Window Consolidation — Orb + Loading Retire into the Single Stage (Claude session, 2026-10-03)

**Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\c-users-chitkul-lakshya-downloads-phone-idempotent-snail.md` (Wakeup Orb Redesign — Shapes, No-Slide Entrance, Above-Orb Captions, Single-Window Host; 5 phases, built one at a time).
**Status:** Phase 1 (window consolidation) **complete — Rust + frontend** and verified (tsc clean, `npm run build` clean, 154/154 vitest, `cargo check --lib` clean, 865/865 `cargo test --lib`). Phases 2–5 pending.
**Commit:** checkpoint `d61c517` (426 files, +74,372/−18,245) plus follow-up fixes `ac249e4`, `e5a221e`, `380dcc3`; this doc's own commit lands the consolidation's Rust refinements + the entire frontend half (§2 below, now done).

## 0. Context (why this change exists)

AGENTS.md (2026-09-25, Single-Stage Shell) planned the orb to eventually move into the fullscreen `stage` overlay ("step 2 orb moves in… then loading layer… then panels") — never executed until now. The orb lived in its own ~200px OS window (`main`), the loading spinner in another 80px window (`loading-indicator`), while `stage` hosted only the ghost ring + annotation layer. Phase 1 retires both small windows so the whole animation renders in ONE always-fullscreen, click-through, watchdog-protected surface.

Accepted tradeoffs (user-confirmed in the plan):
- **Orb drag** must be reimplemented as in-page pointer tracking (a div can't `startDragging()` without dragging the whole stage).
- A rare `stage` recovery glitch now also briefly hides the orb (the existing watchdog auto-recovers in seconds).

## 1. What was done — Rust side (verified against the working tree)

### 1.1 `window_manager.rs` — orb is now an event, not an OS window
- No more `WIN = "main"` window grabs. New API: `orb_rect` (`:228`), `emit_orb_rect` (`:250`), `show_orb_interactive` (`:276`), `wake_orb` (`:291`).
- New events: **`stage:orb_rect`** `{x,y,w,h}` (physical px — same Rust-emits-physical / JS-divides-dpr convention as `ghost:ring`), **`stage:orb_visible`**, **`orb:wake`**.
- Race-free pending pull: `get_pending_orb_rect` (`:36`, registered `lib.rs:946`) — matches the codebase's existing pending pattern for dynamically-created windows.
- `overlay_xy` / `read_orb_settings` / `read_loading_settings` / `position_orb` math kept as-is (pure, unit-tested) — only the *consumer* changed: `stage:orb_rect` emissions instead of `win.set_position()/set_size()`.
- **Deleted commands:** `show_overlay`, `hide_overlay`, `set_click_through` (replaced by `show_orb_interactive` / hitbox registration).

### 1.2 `stage.rs` — source-keyed hitboxes + boot show
- `set_hitbox_source(source, rects)` (`:71`) — hitboxes are now keyed by source so the **orb**, **loading indicator**, and **spatial annotations** can each register interactive rects without clobbering each other (previously one global list).
- `stage` is shown synchronously at boot (near the startup `position_orb` call) instead of only on ghost/annotation requests — it now hosts the always-present orb.

### 1.3 `calibration.rs` — preview retargeted to stage rects
- Wakeup/Waves/Loading calibration preview retargeted from real window moves to `stage:orb_rect` emit + `set_hitbox_source` registration (`:144-159`); cleanup paths clear the sources (`:247-248`, `:345-349`).

### 1.4 Every `"main"` / `"loading-indicator"` call site migrated

| File | Change |
|---|---|
| `hotkey.rs:109,167` | wake → `window_manager::wake_orb` (was `get_webview_window("main")` + eval) |
| `wakeword_oww.rs:3649` | identical wake pattern → `wake_orb` |
| `tray.rs:46,161` | tray click → `wake_orb` |
| `lib.rs` | single-instance relaunch wake, startup positioning, first-run hide → stage-internal visibility/events |
| `commands.rs` | `show_loading_indicator` / `hide_loading_indicator` / `show_overlay` / `hide_overlay` **deleted** |
| `orchestrator.rs` | orb-window grabs → event-based functions |
| `mic_permissions.rs:42` | WebView2 mic-permission priming label iteration updated |

### 1.5 Window configs + capabilities
- `dyn_windows.rs`: `WindowConfig::main()` and `WindowConfig::loading_indicator()` **deleted** — only `setup()`, `sidebar()`, `stage()` remain (`:42,65,79`).
- `tauri.conf.json`: top-level `"windows": []` — no window is created from config at startup.
- `capabilities/loading-cap.json` **deleted**; `capabilities/main.json` + `capabilities/global-shortcut.json` updated (working tree).

## 2. What was done — frontend (now complete)

- **§1.1 Voice runtime relocated:** `frontend/src/stage/orbRuntime.ts` (new) carries the entire bootstrap that used to live in `main.tsx` — the `__NEXUS_WAKE__` / `__NEXUS_FIRST_RUN_GREETING__` / `__NEXUS_RELEASE_MIC__` / `__NEXUS_GET_MIC_STREAM__` globals, mic warmup, and every `stt:*`/`ghost:*`/`audio:*`/Tier-3 listener. `initOrbRuntime()` is idempotent (guarded by a module-level flag) and is called once from `OrbFrame`'s mount effect. `triggerFollowupListen()` (used by `ghostHotMic.ts`, `recorder.ts`, `wsBridge.ts`) now lives here too — all three call sites, plus the `ghostHotMic.test.ts` mock, were repointed from `../main` to `../stage/orbRuntime` (a `tsc --noEmit` pass caught all of them, including two in `recorder.ts` the first sweep missed).
- **§1.2 `OrbFrame.tsx` (new):** listens `stage:orb_rect` (+ `get_pending_orb_rect` pull-fallback for race-free mount), hosts `<Avatar/>` in an absolutely-positioned div, mirrors `calibration:state`/`stage:orb_visible`/`stage:notice`/`ghost:session`/sentinel listeners that used to live in `App.tsx`, and drives `set_orb_interactive` instead of the deleted `set_click_through`.
- **§1.3 No-slide entrance/exit:** `OrbFrame` tracks the visible-false→true and true→false edges itself (`entered`/`dispersing` state, independent of ghost mode) and passes both into `Avatar`→`VoiceOrb`. `VoiceOrb.tsx` gained a `dispersing` prop whose effect calls the orb's existing (previously-unwired) `disperse()`. `Avatar.tsx`'s `entered` prop is now `ghostActive || enteredProp` so a normal wake gets the same particle-assemble entrance ghost sessions already had, without suppressing ghost's own trigger.
- **§1.5 Calibration drag reimplemented:** `Avatar.tsx`'s old `startDragging()` + native `win.onMoved()` pair is gone. Pointer-down now records the offset from `#orb-frame`'s `getBoundingClientRect()`; pointer-move/pointer-up on `window` compute the new top-left directly from `clientX/clientY` (stage is the fullscreen window at the primary monitor's origin, so CSS px × dpr already is the physical-px coordinate space `window_manager.rs` works in) and report through the unchanged `calibration_report_position` IPC + `positionToPct` pure helper. Same pattern applied to the new `LoadingIndicator.tsx` for the "loading" calibration target.
- **§1.6 Loading indicator merged:** `frontend/src/stage/LoadingIndicator.tsx` (new) renders the `loading.json` Lottie spinner (served from `public/loading.json`, already copied to every page's `dist/` root) at `stage:loading_rect`/`stage:loading_visible`, with the same in-page pointer-drag + wheel-resize calibration handling `loading.html` used to do via native window drag.
- **Cleanup:** `frontend/src/main.tsx`, `App.tsx`, `index.html`, `loading.html` deleted; `vite.config.ts`'s `main`/`loading` rollup inputs removed; `stage/main.tsx` now imports `../styles.css` (whole-file — the dead `#app` slide rules are harmless no-ops under `#stage-root` and are left for the Phase 5 cleanup pass) and mounts `<OrbFrame/>` + `<LoadingIndicator/>` alongside the existing ghost ring/pointer/annotation layer.

Phases 2–5 (shape/texture redesign, response caption, live-speech caption, cleanup) remain pending.

## 3. Verification status

- `cargo check --lib` — zero errors, only pre-existing dead-code warnings.
- `cargo test --lib -- --test-threads=1` — **865/865 passed** (1 ignored), including all `window_manager`/`calibration`/`stage` unit tests, unchanged.
- `npx tsc --noEmit` (frontend) — zero errors.
- `npm run build` (frontend) — clean; `dist/` contains exactly `settings.html`, `setup.html`, `sidebar.html`, `stage.html`, `companion-hud.html` + `loading.json` (no `index.html`/`loading.html`).
- `npx vitest run` (frontend) — **154/154 passed** across 21 test files.
- **Not done:** a live manual run (wake via hotkey/wake-word, confirm the particle-form-in entrance, calibration drag, loading spinner) — deliberately left to the user, since launching the real background assistant registers global hotkeys and opens mic capture on the live machine.

## 4. Next steps

Phase 2 — shape/texture/glitch redesign in `voice-orb.js` per the reference video (grainy amber listening → flowing ribbon thinking → bumpy magenta speaking), followed by Phase 3 (response caption), Phase 4 (live-speech caption via streaming STT), Phase 5 (cleanup — including the dead `.transcript, .caption` CSS rule and a final `styles.css` split).

## 5. Ghost command/cursor split (research note, plan context)

The Ghost Mode division of labor (documented in `docs/features/ghost-mode-architecture-reference.md` + AGENTS.md): **keyboard/Win-search flows need zero grounding** (registry-first open, primitive atoms); **mouse use is always free** (never judged, never ends the session); the ring rides **commanded glides only**; **Esc = the cancel button**. Phase 1's consolidation does not change any of these semantics — it only changes where the orb visual lives.
