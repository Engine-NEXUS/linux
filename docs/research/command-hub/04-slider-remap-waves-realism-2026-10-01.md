# Plan 04 — Size Slider 0–100 Remap, Carbon-Copy Slider, Waves Realism

**Date:** 2026-10-01 · **Status: IMPLEMENTED 2026-10-01 (gates green).**
**Trigger:** user review — (1) size slider must read 0–100 with px managed underneath, thumb must sit where the current size maps; (2) Waves preview "box doesn't move / isn't realistic" on select; (3) slider must be a carbon copy of the supplied Radix+NumberFlow component in light + dark; (4) simple naming.
**Audited baseline:** packages present (`@number-flow/react@0.6.2`, `@radix-ui/react-slider@1.4.7`, `clsx`; NO Tailwind); pill already 540×92 2-row, Rust + CSS aligned; `Slider.tsx` already ~95% of the supplied code; `CompanionHudApp` already wires `value={[activeSize]}` reactively.

---

## 1. Button/position-editing audit (current HUD, what stays)

| Control | Current state | Verdict |
|---|---|---|
| 3 option tabs (Wakeup/Waves/Loading) | clickable, highlight + dirty ✓ dot, `1/2/3` keys | KEEP |
| ↶ Undo / ↺ Default | working, history-capped, coalesced | KEEP |
| Size badge ("200 px") | static text button under element | KEEP (stays the px readout) |
| Size slider row | domain = raw px (100–300 / 40–160), tooltip = px | **REMAP to 0–100 (§2)** |
| Cancel / Save | Esc/Enter + click, scoped toast | KEEP |
| Arrow-key nudge, wheel resize | working, coalesced history | KEEP; extend coalescing to slider drags |

## 2. Slider 0–100 remap (the core change)

**Pure mapping module** (`calibration/geometry.ts` or HUD-local `sizeScale.ts`):

```ts
sliderToPx(target, s01: 0–100) = round(min + (s01/100) * (max - min))
pxToSlider(target, px)         = round((px - min) / (max - min) * 100)
```

- Orb/Waves (100–300): 1 slider step = 2px exact, no dead steps.
- Loading (40–160): 1 step = 1.2px → `Math.round` causes occasional duplicate px for adjacent steps. Mitigation: invoke Rust only when the mapped px **changes** (guard in `handleSliderChange`); duplicates collapse naturally. Documented, accepted.
- **Init/switch:** `value={[pxToSlider(target, activeSize)]}` — derived from the live draft (already reactive via `calibration:state`), so target switch, wheel, drag, nudge, and undo all re-seat the thumb automatically. No extra sync code.
- **Commit:** `onValueChange` → live `calibration_report_size({size: sliderToPx(...)})` (same immediacy as wheel today); extend the nudge burst-coalescing to slider drags (same 800ms/target rule → one undo entry per drag; `onValueCommit` clears the mark).
- **Keyboard:** Radix thumb natively handles arrows/Home/End in the 0–100 domain — free, no code.
- **Tooltip content decision:** NumberFlow shows the **0–100 slider value** (literal spec: "slider should only show 0 to 100"); px lives in the under-element badge + Rust drafts ("controlled in the background"). One-line flip available if the user prefers px in the tooltip.
- **Simple name:** the row gets the single-word label **"Size"** (answers "give me a simple name"); entry button unchanged.

## 3. Carbon-copy Slider, light + dark (gap analysis vs supplied code)

| Supplied line | Current `Slider.tsx` | Gap → fix |
|---|---|---|
| Root Tailwind layout | `.hud-slider-root` custom class | none (equivalent, keep custom — no Tailwind in repo) |
| Track `bg-zinc-100 dark:bg-zinc-800` | hard `rgba(255,255,255,0.15)` | **add tokens** `--slider-track` (+ `@media (prefers-color-scheme: light)` override + `.hud--light` hook for later) |
| Range `bg-black dark:bg-white` | blue→green gradient | keep gradient (house style), add light-theme token |
| Thumb `bg-white ring` | white, shadow, no ring | **add hairline ring** so the white thumb reads on light tracks |
| Tooltip `text-lg font-semibold` + NumberFlow | 11px custom tooltip + unit span | keep custom tooltip (fits 92px pill; `text-lg` would clip — WebView clips overflow); value becomes the 0–100 number |
| `continuous` prop | **absent** | **drop it: not in installed 0.6.2 API** (verified in `dist/index.d.ts`; only `isolate/willChange/format/prefix/opacityTiming/transformTiming`). Upgrade path: bump to ^0.7 if the flag is wanted later |
| `opacityTiming/transformTiming` | present verbatim | keep verbatim |

No new dependencies. `clsx` already used. tsc-clean by construction (props already typed).

## 4. Waves "box not moving / not realistic" (diagnosis + fix)

**Why it looks broken today (verified in code):**
1. **Defaults are identical** (`waves_*` = `orb_*` = 0.5/1.0/200): switching targets relocates the window to the *same rect* — zero visible motion is **correct but reads as broken**. There is no defect in `calibration_set_target`/`apply_main` (window moves iff drafts differ).
2. **Realism gap:** the preview Lottie plays at 0.6x with the full drive loop gated off (by design — no mic in calibration), and if the asset ever fails, only static dots show. It cannot match live ghost waves 1:1, and nothing tells the user the switch registered.

**Fix (all preview-scoped, ghost paths untouched):**
1. **Switch pulse:** on `calibration:state` target change, the preview window plays a 220ms scale pulse + the HUD flashes the target name (uses the existing `hud--dirty`-style dot mechanism + a `preview-pulse` CSS keyframe on `.avatar-wrap--calibrating` / loading root). Identical rects now *read* as switched.
2. **Blank-proof fallback:** after mount, assert `wavesContainerRef.current.childElementCount > 0` within 800ms; on failure, force the procedural-bars branch (free-running CSS loop — `@keyframes ghost-wave` verified present in `styles.css:185`) so the preview can never be empty dots again.
3. **Motion parity:** keep 0.6x Lottie loop; add the rest-dot shimmer to preview mode (currently static — the shimmer rAF is drive-gated). Cheap, closes the "frozen" read.
4. **Live diagnostic (if it still looks wrong after 1–3):** `[CALIB-DIAG]` lines already in the build + `nexus start` Rust logs (`calibration: waves → h=… v=…`) prove whether the window relocated; if drafts differ and the window doesn't move, the bug is in Rust `apply_main`, not the preview.

## 5. Tests & gates (per change)

- Mapping: round-trip property tests (`pxToSlider(sliderToPx(x))` stable), rails, loading-quantization guard, init-from-draft test.
- Coalescing: slider-drag burst → single undo entry (extends existing test).
- Fallback: mount-assert forces bars branch (component-testable via the extracted pure gate if feasible; otherwise live-checklist item).
- Full gates: Rust serial, vitest, tsc, `npm run build`, release `--features custom-protocol` (Rust untouched, but the binary ships the bundle).

## 6. Manual acceptance (user-run)

1. Each target at a known px → slider thumb sits at the mapped 0–100 mark; drag thumb → preview resizes live, badge + tooltip agree.
2. Switch Wakeup→Waves at identical rects → pulse + HUD flash confirm the switch; drag waves elsewhere → window follows.
3. Bright + dark wallpaper/system theme → slider legible both (tokens switch).
4. Keyboard-only: Tab to thumb, arrows move it, undo reverses the drag in one step.

---

## 7. Implementation outcomes (2026-10-01)

- §2 slider remap: `sliderToPx`/`pxToSlider` in `calibration/geometry.ts` (+6 tests: rails, exact 2px orb steps, round-trip, loading quantization guard, clamps, init-from-draft); HUD `value={[pxToSlider(target, activeSize)]}` domain 0–100 step 1; px-change guard skips loading duplicate steps; slider drags share the nudge burst-coalescing (one undo per drag; `onValueCommit` lets the final event coalesce). Row labeled **Size**.
- §3 carbon copy: tooltip now shows the 0–100 value (`px` unit span removed — badge keeps `NNN px`); `continuous` confirmed absent from installed 0.6.2 API (dropped, upgrade path noted); timings kept verbatim; theme tokens (`--slider-*`) + thumb ring + `@media (prefers-color-scheme: light)` + `.hud--light` hook.
- §4 waves: `calibrationPulse` store counter + `calib-pulse-0/1` toggle animation (220ms) on desktop previews incl. loading entry-gated pulse; blank-proof fallback (`wavesLottieFailed` → free-running bars, reset on leaving preview); rest-dot CSS shimmer in preview only (ghost rAF untouched).
- Gates: Rust 788/788 serial · vitest 114/114 (one pre-existing timer flake hit once mid-build, green on rerun ×2) · tsc clean · clippy zero-new · `npm run build` clean. Release link pending exe unlock.
