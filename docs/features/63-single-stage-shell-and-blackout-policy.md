# Single-Stage Shell + Blackout Policy (Step 1, 2026-09-25)

User requirement: the user must never experience a full blackout. If the
stage dies entirely, it must auto-shutdown, inform the user, and fix
itself in the background. Implemented in step 1 (`src-tauri/src/stage.rs`).

## 1. Definitions

- **Blackout** = stage marked visible AND (window missing OR renderer
  heartbeat stale >6s). Checked every 2s. Cold-boot grace: no verdict
  within 8s of (re)show.
- **Heartbeat** = stage frontend invokes `stage_heartbeat` every 2s
  (first beat 500ms after mount). No payload — presence is the signal.

## 2. The four moves (in order, every incident)

1. **DETECT** — window-gone or stale-beat (either suffices; both logged
   with which fired and the incident number).
2. **SHUTDOWN** — destroy the window immediately and mark not-visible.
   Rationale: a black fullscreen surface traps the user worse than no
   surface; the orb + voice + all backends keep running (separate
   window/process).
3. **INFORM** — one orb-spoken line per incident via a dedicated
   `stage:notice` channel (deliberately NOT `orchestrator:event`
   Result — a Result would overwrite the orb's in-flight request id and
   clear the long-running flag, orphaning a real turn's `done`
   handshake if a wake lands mid-announcement; caught in rebuild
   cross-check). Skipped when `has_active_request()` is true — the
   watchdog never talks over a live user turn (the turn's own speech
   covers the moment).
4. **AUTO-FIX** — exponential backoff (2s/4s/8s) → recreate +
   show → wait up to 10s for a *fresh heartbeat* (window existing is not
   enough; only renderer-alive counts). Healthy → fail counter resets.
   After 3 failed fixes: stop recreating, one final orb message
   ("switched visuals off until restart — voice still works"), stay
   hidden. The loop keeps watching so manual `stage_show` still recovers.

## 3. Escape hatches (layered, cheapest first)

| Layer | Trigger | Effect |
|---|---|---|
| Ctrl+Space | any visual visible (stage included) | closes stage via `stage_hide` (flag-aware, not raw destroy) |
| **Ctrl+Alt+X** | anytime, even with wedged renderer | `stage_hide_kill`: destroy + session-disable; watchdog and hitbox loop go quiet |
| Auto give-up | 3 failed fixes | hidden until restart/manual show; voice product fully alive |
| Legacy windows | untouched by this plan | full fallback path still exists in code |

The hotkey close path deliberately routes through `stage_hide`, never raw
`destroy_window` — a raw destroy with VISIBLE still set would read as a
blackout on the next tick and rebuild what the user just closed (caught
during implementation review).

## 4. Why this shape (alternatives rejected)

- **Restart-the-app on blackout**: destroys voice sessions + STT state;
  window-level recovery is sufficient (renderer hangs, backends don't).
- **Silent auto-fix without informing**: violates the voice-first
  contract — the user sees visuals vanish; an unexplained disappearance
  reads as a bigger failure than a narrated one.
- **Instant recreate without backoff**: a poisoned page (bad asset in
  dist) would hot-loop create/destroy; backoff + budget caps it.
- **Heartbeat with payload/state sync**: over-engineering for step 1;
  presence-only is enough to distinguish alive from black. Revisit when
  panels carry state worth restoring (step 4+).

## 5. Verification (step-1 scope)

- Unit: hitbox edge inclusion (`stage.rs` tests); geometry mapping
  (`geometry.test.ts`: dock math, orb clamp, loading size).
- Suites: Rust 524/524 serial, frontend 26/26, tsc clean, production
  build emits `dist/stage.html` — each twice.
- Live (needs built app): `stage_show` → empty fullscreen, clicks pass
  through everywhere, video keeps playing; kill renderer (devtools
  crash) → window vanishes ≤4s + orb speaks + background rebuild beats
  resume; Ctrl+Alt+X → stays dead silent; Ctrl+Space closes without
  rebuild.
