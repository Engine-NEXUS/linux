# Orb Presence Upgrade — 2× Particles, 400px Box, Thinking-Entry Screen Burst (2026-10-04)

**User asks:** (1) the orb still reads as a "small window/particle box" — old OS windows verified GONE, what remains is the 200px `OrbFrame` div; (2) double the particle size; (3) particles should form from the entire screen, not just the box.
**Scope:** particle scale + bigger box + fullscreen converge on transformation. Deferred: fullscreen ambient layer (perf/theme cost — needs its own plan).

## What changed

**1. Particle size ×2 (`frontend/src/avatar/voice-orb.js`):** the whole GL size term doubled (`density*2.0`, pop boost `1.8→3.6`, floor `1.8→3.6`) + 2D fallback `dot … *2.0` — relative depth/rim/ambient shading preserved, only absolute size changes. Halo/core radii untouched (normalized `gl_PointCoord` space auto-scales). ~4× overdraw per covered pixel; the `_slow≥45` auto-degrade (800 particles) is the safety net. Glyph text gets chunkier — verify short acks live.

**2. Orb box max 100–300 → 100–400 (user-calibratable, default still 200):**
- Rust: `window_manager.rs` (`read_orb_settings`, `read_waves_settings`, `set_orb_position` clamps) + `calibration.rs::clamp_size` rails + `commands.rs` doc.
- Mirrors: `calibration/geometry.ts::sizeClamp` (+3px/step slider comment), `stage/geometry.ts::orbRect`.
- Tests updated: `calibration.rs::test_clamp_size_rails`, `geometry.test.ts` (rails, slider map 100→400, 3px/step, on-grid round-trip `[100,160,220,280,340,400]`, thumb-seat 250px→50), `stage/geometry.test.ts` (clamp →400). Companion HUD slider follows automatically (generic `sliderToPx`).
- Spec note: `features/82` orbSize/wavesSize rows.

**3. Fullscreen converge burst on thinking entry (`OrbFrame.tsx` + existing `EntranceBurst`):** new `thinkBurstSeq` counter bumps on the false→thinking edge (visible + not calibrating only); a second `<EntranceBurst>` converges screen-edge particles into the orb rect as the knot contracts — the fullscreen counterpart to the contract-then-stretch. Wake burst untouched. Reduced-motion respected (component-level). Speaking entry deliberately excluded (every-reply bursts = visual noise).

## What was NOT changed (research verdicts)
- **Old OS windows are gone** — `dyn_windows.rs` has only setup/sidebar/stage, `tauri.conf.json` `"windows": []`, zero `get_webview_window("main")`. The "small window" is the transparent 200px `OrbFrame` div (now up to 400px). **If you still see a real OS window: you launched the stale Start-Menu binary — always `nexus start`.**
- **Doc 73 verified already-current** (status + OrbFrame §2 landed in `ff42fa7`) — no annotation edit needed.
- Historical `100–300` mentions in changes/research docs left as-is (records); only the living spec (`features/82`) updated.

## Verify
- tsc 0; vitest **154/154** (fixed 2 rail-math expectations I missed on first pass: clamp-cap + thumb-seat); Rust **874/874** serial (1 new clamp test); release binary **52.7 MB**.
- Warnings 34→35: the +1 is pre-existing dead param `window_manager.rs:240` (`app` unused in `set_orb_hitbox_interactive` — Claude-session code, untouched by this change). Left alone.
- Uncommitted. Live matrix (`nexus start`): wake → edge-burst converges fullscreen → orb at calibrated size with 2× particles → ask → thinking-entry burst + knot → ack glyphs chunky-readable → calibrate slider reaches 400px.
