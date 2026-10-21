# Change 63 — Restore Live Screenshot Blur (ADR-05) & Settings Footer Cleanup

**Date:** 2026-10-01
**Related:** `docs/architecture/06-liquid-glass-screenshot-blur.md` (ADR-05),
`docs/features/21-liquid-glass-sidebar.md`, `docs/features/84-liquid-glass-frosted-desktop-widgets-architecture.md`,
`docs/changes/62-ghost-fullscreen-blackout-fix.md` (follow-up discussion)

## 1. Problem (user report)

- Sidebar / settings *context* (same window, different view) was **pitch black** —
  "no difference at all, exactly the same".
- User directive: the real transparency blur **is already implemented** —
  find it in `docs/` first, then implement from that method.

## 2. Root cause (found in docs, confirmed in code)

The method is **ADR-05 screenshot-capture blur**: Rust captures the desktop
region behind the window (GDI `BitBlt`), blurs it in-process
(`image::fast_blur` / `blur_bgra_to_jpeg_fast`, σ=32), streams JPEG frames via
the `sidebar:backdrop` event → JS sets `--sidebar-backdrop-image` on `<html>` →
CSS `.sidebar-card::after` paints it (`background-image: cover`). A blue
wallpaper reads as **sapphire bokeh** (feature 84 §4.1) — that is the
"blue transparency".

The pipeline was fully wired (all 7 `prepare_sidebar` show paths call
`spawn_sidebar_live_blur`) but **killed by two switches**:

1. `commands.rs`: `const DEV_LIVE_BLUR_DISABLED: bool = true;` — early return,
   no frames ever captured ("TEMPORARY: disabled while the panel is in
   solid-black mode").
2. `sidebar.css` `.sidebar-card::after`: `background-image: none` +
   `background-color: #060608` (the 2026-10-01 solid-black directive) —
   even an arriving frame would have been covered by an opaque black layer.

The DWM live-glass tint added in change 62's follow-up (`rgba(0,0,0,0.35)`
under a transparent window) shows the desktop **sharply** (no blur) — it can
never look like frosted glass. Corrected here.

## 3. Changes

| File | Change |
|---|---|
| `src-tauri/src/commands.rs` | Removed `DEV_LIVE_BLUR_DISABLED` flag + early return; removed its doc-comment. The shared 4 FPS change-gated loop runs on every show path again (7/7 callers verified: `show_sidebar_with_content`, `..._analysis`, `..._confirmation`, `unified_show_sidebar`, `show_settings_sidebar`, 2× `architect.rs`). |
| `frontend/src/sidebar/sidebar.css` | Restored `.sidebar-card::after` to the HEAD-verified rule: `background-image: var(--sidebar-backdrop-image, none)` + `cover`/`center` + pre-first-frame fill `rgba(8,8,10,0.45)` + stable-opacity crossfade. Kept `html[data-glass-luminance="light"] .sidebar-card { text-shadow }` legibility boost. Deleted the change-62 follow-up tint rules. |
| `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` | Removed **Export** and **Import** buttons from the settings footer (user directive). Reset / Save remain; `export_settings` / `import_settings` commands untouched (still available to IPC if needed later). |

Also earlier same session (kept): `dyn_windows.rs` forensic log
`live_glass: skipped for '<label>' (compact-windows-only policy)` on the
non-glass branch — proves at runtime that fullscreen `stage` never receives
DWM glass.

## 4. Verification

- `npx tsc --noEmit` → 0 errors.
- `vitest` → **114/114** (14 files).
- Official `node nexus.mjs build` → frontend dist 21:14, binary
  `target\release\nexus.exe` **50.9 MB @ 21:18** (with `custom-protocol`;
  `sidebar-u6cehHU3.css` contains the backdrop rule — verified by grep).
- Code audit: `prepare_sidebar` callers = spawn callers = 7/7 armed;
  `sidebar:backdrop` emitters present in both single-shot show paths and
  the live loop; `SidebarApp.tsx` listener still sets the CSS variable.

## 5. Known pitfalls (why "no changes" can still appear)

- **Stale installed binary:** Start Menu `NEXUS.lnk` →
  `%LOCALAPPDATA%\NEXUS\nexus.exe` dated **2026-08-31** — predates every
  fix. Launch with `nexus start` (kills existing instances, runs
  `target\release\nexus.exe`).
- `nexus start` does NOT rebuild unless invoked as `nexus start -Build`
  (run.ps1 `if ($Build)`) — run `nexus build` first after source changes.

## 6. User terminology note

When the user says **"window"** they mean the **same window with a different
context/view** (e.g. "settings window" = settings view inside the unified
sidebar) — never a separate/new HWND. Docs and responses must use
"context/view", not "separate window".

## Follow-up 2 — actual root cause (ADR-05 Option A violation)

Cross-check vs GitHub HEAD per user directive: `live_glass.rs` (untracked,
not in the user's GitHub) applied `DWMSBT_TRANSIENTWINDOW` Acrylic to the
non-activating sidebar — the API ADR-05 explicitly rejected — DWM solid
opaque (black) fallback over the whole HWND. Removed the call from
`dyn_windows.rs` (GitHub-verified branch: corners + capture exclusion only)
and restored the GitHub card surface values in `sidebar.css`.
Binary 50.9 MB @ 22:24.

## Follow-up 3 — the two remaining kill-switches + three view ::after layers (2026-10-01)

Root cause was THREE layers of pitch black:
- R1: `DEV_BACKDROP_CAPTURE_DISABLED = true` in `capture_backdrop` (commands.rs:865) — disabled the pre-show GDI capture.
- R2: Live loop (`spawn_sidebar_live_blur`) self-captures transparent pixels → black frames (GDI BitBlt doesn't honor `WDA_EXCLUDEFROMCAPTURE`).
- C2/C3/C4: `settings-sidebar.css`, `architect.css`, `pr-list.css` `::after` layers hard-coded `#060608`.
- C5: `.unified-shell` (untracked shell CSS) `background: #060608 !important` — opaque shell behind cards.

All fixed:
- R1: Removed early-return + doc comment from `capture_backdrop`.
- R2: Re-added `DEV_LIVE_BLUR_DISABLED = true` + early-return with doc comment in `spawn_sidebar_live_blur`.
- C2: Restored `.settings-container` card + `::after` to HEAD (gradient 0.50→0.30 + `--sidebar-backdrop-image`).
- C3: Restored `.architect-app` card (white glass 0.05 + border) + `::after` (gradient + image).
- C4: Restored `.pr-list-container` card (rgba 0.60 + border 0.12) + `::after` (rgba 0.45 + image).
- C5: `.unified-shell` → `background: transparent`.

Gates: tsc 0, vitest 114/114, `node nexus.mjs build` → 51.1 MB binary @ 23:24. Dist CSS contains 4 backdrop variable occurrences across all four views.
