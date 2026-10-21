# Research 01 — User-Positionable Stage Architecture (Command Hub Future Plan)

**Date:** 2026-09-30 · **Status: RESEARCH ONLY — nothing implemented.**
Companion: `02-user-positionable-stage-execution-plan-2026-09-30.md` (phases).

**User's future vision (verbatim intent, preserved):** from the NEXUS Command
Hub, the user positions the animation anywhere on screen and saves it — it
then always appears there perfectly. Slide-in follows the edge: top → slides
top-to-bottom, left → left-to-right, right → right-to-left, etc. Not only
`wakeup.json`: the user also controls the loading indicator and the size of
the sidebar windows. `wakeup.json` = `waves.json`: wherever wakeup is set,
waves appear at the same position.

---

## 1. What exists today (complete inventory — the foundation)

### 1.1 Orb position + size persistence (the pattern to clone)

| Piece | Location | Behavior |
|---|---|---|
| Settings fields | `commands.rs:1560-1571` (`orb_horizontal_pct` 0–1, `orb_vertical_pct` 0–1, `orb_size` 100–300px, all `#[serde(default)]` + camelCase) | Persist in `%APPDATA%/com.nexus.assistant/settings.json`, survive restarts |
| Reader + clamps | `window_manager.rs:14-39` (`read_orb_settings`, safe fallbacks 0.5/1.0/200, range clamps) | Corrupt/missing file → center-bottom 200px, never a crash |
| Positioner | `window_manager.rs:44-68` (`position_orb`) | % → physical px via `current_monitor` size × `scale_factor`, center-anchored, **clamped fully on-screen**; also sets size |
| Call sites | `lib.rs:629` (startup), wake, hotkey, `show_overlay` (`window_manager.rs:105-119`) | Position re-applied on **every show** — drift impossible |
| Live preview IPC | `window_manager.rs:132-168` (`set_orb_position`) | Moves/resizes **without saving**; frontend saves separately on confirm |
| Designer UI | `SettingsSidebarApp.tsx:274-388` (Display tab) | H/V sliders + size slider + live mini-map preview + reset-to-defaults; `liveUpdate` → `set_orb_position` while dragging |

**Lesson for the plan:** every future positionable surface should copy this
exact five-piece shape — persisted fields → clamped reader → positioner →
re-apply-on-show → preview-without-save IPC → designer tab. It is the only
positioning pattern in the codebase that has survived live use.

### 1.2 Stage geometry map (the second pattern to clone)

`frontend/src/stage/geometry.ts` (+ 6 tests) mirrors Rust math in CSS px:
`orbRect` (same pct/size/clamp formulas), `dockRect` (bottom-right dock,
12px gap, 48px taskbar reserve), `loadingRect` (80×80 top-right, 7px/9px
physical insets), `PANEL_SIZES` (sidebar 400×1000, pr-list 500×1000,
architect 900×1000). Hitbox reporter multiplies by `devicePixelRatio`
before sending physical px to Rust. **Any new positionable surface needs its
rect function here first**, with tests, before Rust moves anything.

### 1.3 Window catalog (all 8 configs, `dyn_windows.rs:35-156`)

| Window | Size | Deco | Topmost | Focus | Notes for positioning |
|---|---|---|---|---|---|
| `main` (orb) | 200 (user 100–300) | none, transparent | yes | no (non-activating, `set_focusable(false)`) | user-positioned today |
| `loading-indicator` | 80×80, top-right | none, transparent | yes | no | HARDCODED corner — future: user position + size |
| `sidebar` (response) | 400×1000, right dock | none, transparent | yes | no | HARDCODED dock — future: user zone + size |
| `pr-list-sidebar` | 500×1000, right dock | none, transparent | yes | no | same |
| `architect-sidebar` | 900×1000, right dock | none, transparent | yes | no | same |
| `settings-sidebar` (Command Hub) | 520×1000, right dock | none, transparent | yes | no (+`set_focus` at open) | same; designer UI lives HERE |
| `setup` / `settings` | 520×680 / 600×720, centered | decorated, opaque | no | yes | normal windows — out of scope (OS manages) |
| `stage` | fullscreen per-monitor | none, transparent | yes | no | future: could host ALL positioned layers as DOM (see §9) |

RAM law (why windows are on-demand, `dyn_windows.rs:1-8`): each WebView2 ≈
7 processes ≈ 250 MB. Positioning work must never keep extra windows alive —
move/resize, never duplicate.

### 1.4 Show/hide mechanics today (what slide-in must compose with)

- **Native show/hide required** (`window_manager.rs:105-130`): CSS
  opacity/transform alone can't reliably hide WebView2 transparent windows
  (GPU frame cache) — `win.show()`/`hide()` own visibility; CSS owns motion.
- **Frontend slide-down**: 500ms delayed pause on hide (Avatar.tsx
  visibility effect) so the Lottie stays alive during the exit beat.
- **Entry/exit beats**: ghost pinch 220ms, leave 200ms, waves bloom 650ms —
  timer-owned, phase-gated. A directional slide must slot into this beat
  system, not replace it.
- **No directional motion today**: entries are fade/scale/zoom only. Slide
  vectors are greenfield.
- **hideOrbAfterSpeech** (`net/orchestrator.ts`): state-aware hide (never
  mid-speech/ghost, re-arms 1s while speaking). Slide-out must consult the
  same gates or it will cut speech.

### 1.5 Sidebar dock math + ordering constraint (hard-won, do not regress)

`commands.rs:703-729`: fresh windows spawn at physical (0,0) — possibly
off-screen/wrong-monitor — so the code **pre-positions before backdrop
capture** (`set_position` works hidden), because `capture_backdrop` needs
`current_monitor()` for BitBlt and must not capture the window itself.
Order is law: **position → capture → show**. Any user-positioned sidebar
must reuse this exact order or backdrop blur breaks on multi-monitor.

### 1.6 Monitor/DPI model today (gaps the plan must close)

- All math: `current_monitor()` → `scale_factor()` → physical px; fallback
  `primary_monitor()` when homeless (`commands.rs:711-714`).
- `available_monitors()` is **never enumerated** — no monitor picker, no
  per-monitor memory; unplugging a monitor silently re-homes windows.
- Conversions: `to_logical`/`PhysicalSize`/`LogicalPosition` scattered per
  call site (80+ hits) — no single `to_physical_rect` choke point. The plan
  should introduce one (testable, §11).

---

## 2. Coordinate & persistence design for user positioning

### 2.1 Representation (recommendation)

- **Position: fractional anchor + zone.** Store `{ h_pct, v_pct }` (0–1,
  monitor-relative, like the orb) PLUS a derived `edge_zone` (9-zone grid:
  top-left, top, top-right, left, center, right, bottom-left, bottom, bottom-right)
  computed at save time. Fractions survive resolution/DPI changes; the zone
  drives slide direction without re-deriving on every show.
- **Size: logical px + min/max clamps** per surface (orb 100–300 precedent;
  sidebars 320–900 wide, 600–1200 tall; loading 48–160 square).
- **Monitor: `monitor_id` (device name hash) + fallback chain**
  saved-id → same-size monitor → primary. Never strand a window off-screen:
  re-clamp on every show (position_orb precedent).
- **Schema:** new `NexusSettings` fields, all `#[serde(default)]` with
  `default_*` fns (missing-field = old behavior, zero migration breakage).
  Grouped: `orb_*` (exists), `loading_*`, `sidebar_*` (+ per-sidebar
  overrides falling back to shared), `slide_*` (direction mode, duration,
  enable flag), `waves_lock` (bool, default true).

### 2.2 Preview-without-save (mandatory pattern)

`set_orb_position` proves the UX: sliders/drag call preview IPCs live;
**Save** calls `save_settings`; **Reset** restores defaults + previews.
Every new surface gets `set_<surface>_geometry` with identical semantics,
or the designer will write half-dragged states to disk.

### 2.3 Designer UI (lives in the Command Hub = settings-sidebar)

Extend the Display tab (`SettingsSidebarApp.tsx:274+`):
- **Mini-map**: screen rectangle with draggable orb dot + panel rects
  (reuse `.position-preview` CSS, L414+). Drag → preview IPC; release →
  dirty flag (Save lights up).
- **Zone presets**: 9-zone grid buttons (snap h/v pct to 0/0.5/1 combos).
- **Size sliders** per surface (orb exists; add loading square, sidebar W/H).
- **Slide controls**: enable toggle, duration slider (150–600ms), direction
  readout (auto from zone, manual override select).
- **Waves lock toggle**: "Waves follow wakeup" (default ON) — the user's
  `wakeup = waves` invariant as a visible setting, not tribal knowledge.
- **Reset all** (per-surface + global), mirroring L386-388.

---

## 3. Slide-direction engine

### 3.1 Edge → vector mapping

From `edge_zone` (or manual override): top → (0,+d) entering downward
("slides top-to-bottom"); bottom → (0,−d); left → (+d,0); right → (−d,0);
corners → diagonal; center → scale/fade (no slide — nowhere to come from).
Distance d ≈ 24–40px for the orb (small surface), ≈ panel width/3 for
sidebars (large surfaces read better with longer travel), duration 150–600ms
user-set, `ease-out` enter / `ease-in` exit.

### 3.2 Native vs CSS (decision: CSS owns motion, native owns visibility)

- WebView2 transparent windows + per-frame native moves = smear risk; CSS
  transforms inside an already-shown window are composited cleanly.
- Implementation: `win.show()` at final rect → CSS keyframe from
  `translate(vector) + fade` → rest. Exit: CSS slide-out → `win.hide()` on
  `animationend` (or the existing 500ms-delay pattern).
- The 500ms hide-delay, pinch/leave beats, and `hideOrbAfterSpeech` gates
  must wrap slide-out: slide consults state (never mid-speech/ghost).
- `prefers-reduced-motion`: hard requirement — reduce to fade when set
  (accessibility; also saves a support ticket class).

### 3.3 What slides where (scope)

| Surface | Slide | Notes |
|---|---|---|
| Orb (`main`) | edge vector + fade | wake/hotkey entries; must not fight pinch (ghost enter keeps pinch, slide only in normal mode) |
| Waves | **never slides independently** — rides inside `main` (absolute inset-0 today). `wakeup = waves` falls out free as long as they share the window. If waves ever move to `stage`/own window, the lock setting forces identical rect + mirrored slide |
| Loading | edge vector, short (80px surface) | long-running entries only |
| Sidebars | horizontal slide from docked edge (right → from +x; if user docks left, from −x) + backdrop blur already captured pre-show | position→capture→show order preserved |
| Settings/Setup | none (decorated OS windows) | out of scope |

---

## 4. Waves ↔ wakeup coupling (user invariant)

Today the coupling is **structural**: waves render inside the `main` window
(Avatar.tsx `ghost-waves` absolute inset-0; rest dots at file geometry).
Threats to preserve against:
1. Any "separate waves window" refactor (perf or z-order) must copy the orb
   rect + slide vector frame-for-frame — hence the `waves_lock` setting.
2. Rest-dot geometry (`REST_DOT_X/Y`) is file-derived; if the user resizes
   the orb window, dots scale with it (percent-based ✓ — already safe).
3. The scrub driver is resolution-independent (frames, not px) ✓.
4. Ghost ring (`ghost:ring`) rides the **cursor**, not the orb — independent
   by design; must not snap to saved positions (user directive territory).

---

## 5. Sidebar + loading control design

### 5.1 Sidebars (response / PR / architect / settings)

- **Zone**: reuse the 9-zone grid, default bottom-right (today's dock).
  Dock math (`dockRect` + 12px gap + 48px taskbar) becomes zone-parametrized:
  each zone = anchor corner + inward offsets. Taskbar reserve follows the
  docked edge (bottom zones reserve 48px; assume top/side OS bars 0 + clamp).
- **Size**: per-sidebar W/H sliders within min/max; `WindowConfig` widths
  become defaults, not constants. `set_size` on live window for preview;
  backdrop re-capture after any geometry change (blur is position-baked).
- **Per-sidebar override vs shared**: shared `sidebar_*` zone/size +
  optional per-label override (architect users love full-height; PR users
  love narrow). Override empty = inherit.
- **Focus behavior unchanged**: non-activating (`focus:false`) everywhere;
  Command Hub keeps its open-time `set_focus`.

### 5.2 Loading indicator

- **Position**: zone grid (default top-right, today's corner); size 48–160.
- **Coupling**: loading shows during long runs while the orb hides
  (`hideOrbAfterSpeech`) — their zones must not overlap confusingly; the
  designer mini-map renders both rects so collisions are visible at design
  time (warn, don't forbid).
- `loading.html` input already exists in vite config (window contract intact).

---

## 6. Ghost + stage interplay (do-not-break list)

1. **Ghost fullscreen sessions**: explicit exits only (Esc / voice / stage
   hide). Saved orb positions must not yank the window mid-session;
   geometry changes apply on next normal-mode show.
2. **Exclusive-fullscreen auto-pause** (ghost Phase 4): overlay pauses over
   fullscreen apps — positioning code must query, not assume, visibility.
3. **Stage shell** (`WindowConfig::stage`, fullscreen/transparent/topmost):
   long-term, ALL positioned layers could become DOM inside `stage`
   (geometry.ts already maps every rect). That migration is OUT of scope for
   this plan's phases but the rect functions must stay stage-compatible
   (pure, tested, CSS-px in / physical-px out).
4. **Hitboxes**: stage 30ms cursor poll toggles `set_ignore_cursor_events`
   around frontend rects — repositioned visuals MUST re-report rects via the
   existing reporter or clicks fall through (or worse, block input).
5. **UIPI/elevation**: topmost cannot cover elevated windows; spoken reroute
   precedent (ghost_click) applies — positioning never promises z-order over
   admin windows.
6. **Screen sharing**: orb is share-visible by design (stage NOT capture-
   excluded); sidebars ARE excluded. Moving the orb does not change capture
   policy — document so nobody "fixes" it during the refactor.

---

## 7. Click-through, focus & input (constraints)

- Non-activating overlays (`focus:false`, `set_focusable(false)`,
  click-through toggling) are load-bearing for "never steals typing".
  Position changes must not flip focusability (Tauri resets some flags on
  `set_position` on some backends — re-assert `always_on_top` after moves,
  per the `set_click_through` precedent `window_manager.rs:99-101`).
- Slide animations run with click-through ON (ignore=true) until settled,
  then restore — a sliding window must not eat clicks mid-flight.
- Decorated windows (setup/settings) excluded: OS owns their geometry.

---

## 8. Voice control (future intents — parser + center routing)

New deterministic intents (pattern → `Subsystem::AppCenter`-style local run):
`move orb to <zone>`, `orb to top left`, `bigger/smaller orb`,
`move sidebar left/right`, `reset positions`. Each previews immediately +
says the cached confirm line; positions persist only on explicit "save (it)".
Slot lexicon: 9 zones + synonyms (top = "top/up/north", etc.),
relative ("a bit left" = −10% h). Almond: these ride the SAME preview IPCs
as the designer — one code path, two triggers. Out of scope for phases 0–3;
Phase 4.

---

## 9. Multi-monitor, DPI & edge cases

- **Monitor picker**: enumerate `available_monitors()`, store device-hash;
  fallback chain saved → same-geometry → primary; re-clamp every show.
- **DPI change mid-session** (drag window across mixed-DPI monitors — Windows
  moves it, scale changes): recompute on `ScaleFactorChanged` event / each
  show; never cache physical px across shows (only fractions + logical px).
- **Unplug/resize/rotation**: clamp-on-show covers all three; designer
  mini-map re-reads screen size on open.
- **Taskbar positions**: bottom assumption (48px) is US-centric; read
  work-area if Tauri exposes it, else keep 48px + clamp (documented
  approximation — same as today).
- **Fullscreen apps**: auto-pause precedent stands; no positioning work
  while paused.

---

## 10. Schema, migration & safety rails

- All new fields `#[serde(default)]` + `default_*` (today's behavior when
  absent). Version stamp `settings_version: u32` (default 1) for future
  migrations; unknown fields preserved on save (don't drop what we don't
  know — read-modify-write the JSON value, not the struct).
- **Clamp rails** (never trust input): pct 0–1, orb 100–300, loading 48–160,
  sidebar W 320–900 / H 600–1200, slide 0/150–600ms, monitor hash string.
- **Atomic save**: write temp + rename (power-loss safety for the file the
  whole app reads at boot).
- **Audit**: log geometry changes at debug (positions leak screen layout —
  keep out of any cloud-bound payload; PII-filter precedent).

---

## 11. Test strategy (gates per phase)

- **Rust**: rect math pure fns (`zone_rect`, `slide_vector`, clamp) —
  table tests incl. 0-size screens, 200% DPI, ultrawide, portrait;
  settings-default tests (missing/corrupt → defaults); ordering test
  (position→capture→show sequence mock).
- **Frontend**: `geometry.ts` additions (mirror of Rust, same vectors);
  designer reducer tests (drag → pct, presets → pct, dirty-flag logic);
  `transition()`-style gate tests for slide (never mid-speech/ghost).
- **E2E/manual**: 9-zone matrix on 100%/150% DPI (screenshot each);
  unplug-monitor drill; slide direction checklist per zone; voice intents
  (Phase 4) against the EN fixture.
- **Dual-gate**: every phase twice (serial Rust + vitest + tsc), matching
  program convention.

---

## 12. Risks & open questions

| # | Risk / question | Current read |
|---|---|---|
| 1 | Moving visible transparent WebView2 smears? | Mitigate: move-while-hidden where possible; small live moves only in designer preview; test on Intel iGPU (weakest path) |
| 2 | `set_position` resetting topmost/focus on some backends? | Re-assert flags after every move (precedent exists) + test |
| 3 | `available_monitors` stability of device IDs across reboots? | Spike first: log IDs across sleep/reboot; fallback chain covers flakiness |
| 4 | Work-area (taskbar rect) API in Tauri? | If absent, keep 48px+clamp approximation |
| 5 | Slide + backdrop blur ordering for sidebars? | Slide is post-show CSS; blur captured pre-show at final rect — compatible, but verify no blur smear during slide on weak GPUs |
| 6 | Should `stage` absorb all layers long-term? | Tempting (one window, DOM positioning, zero native moves) but RAM/compositing + capture-policy + fullscreen-ghost implications need their own spike — explicitly NOT this plan |
| 7 | Voice zone synonyms across accents (Whisper)? | Phonetic-alias map precedent (`intent_parser.rs` Layer 3); EN-only per program directive |
| 8 | Scope creep into setup/settings windows? | Hard no — decorated OS windows stay OS-managed |

---

## 13. File inventory (for the implementer — verify freshness before use)

| Area | Files |
|---|---|
| Rust positioning | `src-tauri/src/window_manager.rs` (orb), `src-tauri/src/commands.rs` (sidebars/loading/settings struct/save), `src-tauri/src/dyn_windows.rs` (configs), `src-tauri/src/stage.rs`, `src-tauri/src/architect.rs` (own dock math ×3) |
| Frontend geometry | `frontend/src/stage/geometry.ts` (+ tests), `frontend/src/avatar/Avatar.tsx` (slide/scale owners), `frontend/src/styles.css` (pulses, pinch/leave, ghost transitions) |
| Designer UI | `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` (Display tab L274-388), `settings-sidebar.css` (preview grid L414+) |
| State/events | `frontend/src/store/assistant.ts` (`transition()`), `frontend/src/net/orchestrator.ts` (hide gates, done handshake) |
| Docs | `docs/features/63-*` (stage shell), `docs/features/80-*` (orb pipeline), `docs/changes/56-*`, `docs/research/orb-waves/*` |
