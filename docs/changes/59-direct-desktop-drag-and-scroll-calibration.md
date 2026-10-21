# Direct Desktop Drag & Scroll-Wheel Calibration + Workspace Collision Recovery

**Date:** 2026-10-01
**Plan:** `animation_alignment_calibration_plan.md` (Gemini antigravity artifact) + task message `NEXUS-PLAN` §1–6
**Research:** `docs/research/command-hub/02-direct-desktop-drag-and-drop-calibration-architecture-2026-10-01.md` (written as the implementation record)
**Status:** All automated gates green; manual acceptance = user-run.

---

## 0. INCIDENT — concurrent-session file collision (recovered first)

While starting this task, `cargo check` showed **180 errors**. Forensics:

| Finding | Evidence |
|---|---|
| A second agent session rewrote `lib.rs`, `commands.rs`, `intent_parser.rs`, `screen.rs`, `dyn_windows.rs`, `window_manager.rs`, `orchestrator.rs` from **stale in-context snapshots** at 01:13–01:59 | file mtimes; `lib.rs` lost 16 `mod` declarations + ~50 command registrations; `commands.rs` regressed to a pre-2026-09-27 base (lost my Phase 1 `stop_stt_capture`, Phase 3 ghost settings, Phase D `vision_provider/vision_race`, `tts_emotion`, google/memory/diary/webhook/improve/health/debug_trace/import/export command wrappers, `read_api_key`, `read_speaker_verification`) |
| `intent_parser.rs` / `screen.rs` / `dyn_windows.rs` were NOT stale-restored — the dual-grammar work survived | `parse_type_dictation_command` present; their tests still passed post-recovery |
| HEAD (2026-09-29) also lacks the mods → the correct `lib.rs`/`commands.rs` existed only in the working tree, now overwritten | `git show HEAD` check |
| Recovery source: the Gemini session's transcript at `.gemini/antigravity/brain/3122b61d…/logs/transcript_full.jsonl` contained pre-collision numbered reads of `lib.rs` | extracted the true handler list (`stage::`, `ghost::`, `commands::get_health_status`, `import_settings`, `diary_summary`, `debug_trace`, `stop_stt_capture`…) |

**Recovery (all restored, verified by compiler + 756/756 tests):**
1. `lib.rs`: +16 mods (agent_specs, browser_center, calibration, center, diary, ghost, google, improve, memory, missed_intent_logger, pii_filter, stage, system_center, vision, webhook, youtube_center); handler list rebuilt from transcript fragments + current-tree `#[tauri::command]` inventory + 59 frontend invokes; 4 phantom registrations removed (stt_status, nexus_diagnostics, cancel_architect_analysis, query_impact — all absent from their modules by design); boot hooks restored (note_boot, diary rollup, agent-specs example, improve miner, webhook listener, sentinel poller); `log_diagnostics` 3-arg call fixed with a Send-safe owned `PathBuf` captured pre-spawn.
2. `commands.rs`: struct fields re-added (tts_emotion, vision_provider, vision_race, ghost_depth_ack, ghost_step_timeout_ms, ghost_turn_gap_ms, waves_*, loading_*) + serde defaults + `Default` impl entries; re-added commands: debug_trace, google_* ×5, memory_recall/forget, diary_summary, webhook_token/rotate_token, improvement_report, vision_quota_status, get_health_status (with sysinfo memory + BOOT_INSTANT uptime), export/import_settings, stop_stt_capture; readers: read_api_key, read_ghost_* ×3, read_speaker_verification.
3. `lazy_stt.rs` / `lazy_nlu.rs`: `is_*_responsive` made `pub(crate)` for the health panel.
4. `dyn_windows.rs`: lost `WindowConfig::stage()` restored + new `calibrate_toolbar()`; stray-brace fix.

---

## 1. Calibration implementation — every file, every why

### Rust
| File | Change | Why |
|---|---|---|
| `calibration.rs` (NEW) | session state (`CALIBRATION` mutex), `Target`/`Draft`/`clamp_size`/`wheel_step`/`default_draft` (pure, tested), 8 IPC commands, `apply_main`/`apply_loading`, `emit_state` (+500ms re-sync emit), `end_session` (restore-from-disk + destroy HUD + re-show hub + toast), `write_params` (9-key read-modify-write) | Drafts in Rust = single owner across 2 WebViews; save preserves unknown settings fields; the delayed second emit covers on-demand window creation latency |
| `window_manager.rs` | `overlay_xy` pure fn (pub), `read_waves_settings`/`read_loading_settings`, `position_loading`, `read_orb_settings` made pub, `position_orb` branch: ghost session → waves rect; calibration active → early-return | One conversion choke point; runtime show-paths honor saved placement; drag owns the window during calibration |
| `orchestrator.rs::show_loading` + `commands.rs::show_loading_indicator` | hardcoded top-right → `position_loading` | The user's placement is what actually shows at runtime |
| `commands.rs` | +6 struct fields, +13 serde defaults, +4 readers, +17 restored commands, `stop_stt_capture` | collision recovery + new waves/loading fields |
| `dyn_windows.rs` | `WindowConfig::calibrate_toolbar()` (440×54 pill) + `stage()` restored | task §2 window contract |
| `lib.rs` | `pub mod calibration`, 8 calibration commands registered, boot hooks | wiring |

### Frontend
| File | Change | Why |
|---|---|---|
| `calibration/geometry.ts` (+9 tests) | `positionToPct` (center-anchored inverse + 20px magnetic snap center/bottom), `wheelResize` (±10, rails 100–300 / 40–160), `sizeClamp` | mirror of Rust math; pure = testable |
| `companion-hud/` (NEW: html, main, CompanionHudApp, css, history +5 tests) | pill HUD | task §2 full spec |
| `Avatar.tsx` | container 180px → **100%/100%** (fluid scaling); calibration effect (startDragging on pointerdown, debounced onMoved → `calibration_report_position`, wheel → `calibration_report_size`); px badge; waves preview gate (`calibrationTarget === "waves"`, smile hidden, no ghost session); halo class | task §3/§4; native drag = zero latency |
| `App.tsx` | `calibration:state` listener → store mirror + orb visibility | preview + cleanup on session end |
| `loading.html` | calibration block (pointerdown/wheel/onMoved/badge, same math inline) | loading preview is a plain module window (no React) |
| `store/assistant.ts` | `calibrationTarget`/`calibrationSize` + setter | cross-window mirror |
| `SettingsSidebarApp.tsx` | Display-tab entry card ("✥ Drag & Position on Desktop" → `show_calibration_hud` + `hide_settings_sidebar`), `settings:toast` listener + UI, 6 new round-trip keys | task §1/§5 |
| `settings-sidebar.css` | toast styles | |
| `vite.config.ts` | `companionHud` rollup input | release-build window contract |
| capabilities | `settings-sidebar-cap.json` +`companion-hud` window (events already granted); `loading-cap.json` +start-dragging/set-size/events | HUD + loading-preview input paths |

### Waves-runtime semantics (important)
The waves render inside the orb window (`Avatar.tsx` `ghost-waves`, inset-0). `waves_*` placement is applied to that **same window during ghost sessions** — `position_orb` reads `read_waves_settings` when `ghost::session_active()`, else `read_orb_settings`. Calibration's Waves target previews by repositioning the same window; on save, ghost sessions land on the saved waves rect. No new runtime window (RAM law).

## 2. Gates
| Gate | Result |
|---|---|
| Rust serial | **756/756** (collision recovery + calibration included) |
| Frontend vitest | **87/87** |
| tsc | clean |
| `npm run build` | clean, `dist/companion-hud.html` emitted |
| `cargo build --release --features custom-protocol` | clean, **50.9 MB** |
| clippy | zero NEW warnings in touched ranges (window_manager `.max().min()` style is pre-existing) |

## 3. Known limitations
1. Loading default (0.95/0.05 center-anchored) is visually ≈ the old top-right corner, not pixel-identical.
2. Wheel resize recenters via Rust's center-anchored formula (spec: "re-centers smoothly") — the element stays centered on its pct anchor.
3. `waves_preview` renders the waves layer without ghost choreography (silent, paused Lottie at rest) — intentional, calibration is a calm context.
4. Multi-monitor: placement math uses the window's current monitor (same as the orb); monitor-picker/out-of-scope per research 01 §9.

---

## 4. Follow-up: switched-target drag report + TEMP live-check tooling (2026-10-01)

- **User report:** after switching HUD target (Waves/Loading), the preview sits at its position but will not drag.
- **Confirmed bug (Loading):** `loading.html` called bare `listen(...)` with no import in scope — the whole calibration block threw at module eval, so the loading window never received `calibration:state`, never armed drag/wheel. Fixed: single module-scope registration (dynamic imports), enable/disable only flips badge/cursor; also fixes duplicate-handler stacking on repeated switches (`listenFn` guard never assigned).
- **Waves (main window) path audited clean:** `calibration_set_target` sets `ignore_cursor_events(false)` + shows + emits state; App listener mirrors to store; Avatar attaches handlers when `calibrationTarget` is wakeup/waves. No defect found in code — needs the live observation below to close.
- **TEMP diagnostics (remove before release):** `[CALIB-DIAG]` console lines in `Avatar.tsx` (pointerdown / startDragging ok+FAIL / moved-listener armed / report values) and `loading.html` (same + init/report failures). Frontend console shows them; Rust already logs every `calibration_report_*`.
- **TEMP dev-persist (remove before release):** `calibration_dev_persist(enabled)` writes `calibrationDevPersist` to settings.json; boot auto-opens the HUD + wakeup preview after 3s; Command Hub Display tab has a "Dev persist ON/OFF" toggle. Purpose: keep the calibrator on display across rebuilds/restarts for live cross-checks.
- **Gates:** Rust 769/769 serial; vitest 97/97; tsc clean; `npm run build` clean.

---

## 5. Drag dead-zone fix + glass pill restyle + on-demand pill preview (2026-10-01)

- **User report:** only Wakeup drags; Waves/Loading previews sit frozen.
- **Root cause (verified):** transparent overlay pixels never receive mouse
  events (OS per-pixel hit-testing). Wakeup paints ~90% of its window so it
  feels fine; the Waves preview (empty slot + 3 dots) and the Loading window
  (60px art in up to 160px) are mostly unhittable transparency. Fix: a
  `~1%`-alpha full-window wash (`.calibration-hitcatcher` / `#hitcatcher`)
  while calibrating — imperceptible, fully hittable. (Also fixed en route:
  `loading.html` had a second copy of the module-scope registration bug.)
- **HUD restyle (user-supplied GlassFilter):** new `GlassFilter.tsx`
  (feTurbulence → displacement `radio-glass` filter, Tailwind-free) applied
  to a rebuilt minimalist 3-option segmented control — `[ 1 Wakeup ]
  [ 2 Waves ] [ 3 Loading ]`, equal thirds, centered, emoji removed.
- **On-demand pill preview:** new TEMP command `preview_companion_hud`
  (pill only, no session) + "Preview pill only" button in the Command Hub;
  fast style loop = frontend-only `npm run build` + relaunch (no 5-min
  Rust rebuild for CSS/TSX changes).
- **Gates:** Rust 769/769 serial; vitest 97/97; tsc + `npm run build` clean.
