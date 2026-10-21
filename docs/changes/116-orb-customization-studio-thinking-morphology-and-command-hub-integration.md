# Orb Customization Studio, 3D Morphologies & Thinking State Activation (2026-10-08)

## 1. Problem & Context
1. **Thinking State Bypassed on Normal Queries**:
   - When the user spoke a command or question, `processTranscript` in `frontend/src/audio/recorder.ts` immediately routed to intent parsing and execution, jumping directly from `listening` to `speaking` within <5ms for local actions or staying in `listening` while waiting for the network.
   - The thinking state was never entered, causing the user to never see the thinking morphology animation in production.
2. **Command Hub Hero Card Styling Instruction**:
   - The user explicitly requested to remove the glowing hero action card for Voice Orb customization in Command Hub and replace it with a completely standard `.hx-row` matching "API Keys", "Memory", "Audio", "Display", "Connections", and "Advanced", with zero glow and identical visual hierarchy.
3. **Selected Morphologies Integration**:
   - User selected specific 3D morphologies:
     - **Thinking**: Option #6 **Trefoil Celtic Helix Cage** ($p=3$ braided helical rings intersecting at $120^\circ$).
     - **Speaking**: Option #2 **64-Strand Fibonacci Blossom** with acoustic petal pulses.
     - **Listening**: Option #1 **Acoustic Inward Suction Vortex / Chladni Cymatics** with RMS jitter.

---

## 2. Root Cause Analysis & Architectural Fixes

### A. Immediate Thinking State Transition & 1.0s Guaranteed Dwell
- **Root Cause**: `recorder.ts` never set `useAssistant.getState().setState("thinking")` when transcripts arrived for normal queries.
- **Fix**:
  - In `frontend/src/audio/recorder.ts` (`processTranscript`), when a non-empty, non-rejected transcript arrives and is added to the store via `addUserMessage(corrected)`:
    ```ts
    const isLong = isLongRunningQuery(corrected);
    if (isLong) {
      // Instant ack path ("On it sir")
      useAssistant.getState().setState("speaking");
      useAssistant.getState().addAssistantMessage("On it sir.");
      setLocalAckGiven();
      void speakCached("On it sir");
    } else {
      // Standard query / command / orchestrator path:
      useAssistant.getState().setState("thinking");
    }
    ```
  - In `frontend/src/store/assistant.ts`, entering `"thinking"` records `thinkingEnteredAt = performance.now()`. When downstream responses trigger `setState("speaking")`, the store holds the `"thinking"` state for the remaining time of `THINKING_MIN_DWELL_MS` (1,000ms), ensuring the Trefoil Celtic Helix Cage is rendered and smoothly spins before morphing into speech.

### B. Command Hub Clean Row Layout (`frontend/src/settings-sidebar/hub/HubHome.tsx` & `hub.css`)
- Replaced `.hx-orb-action-card`, `.hx-orb-action-glow`, and `.hx-orb-action-btn` with the standard `<Row>` component:
  ```tsx
  <Row
    icon={ICONS.orb}
    title="Customize Voice Orb"
    sub="3D particle patterns for thinking, speaking and listening"
    onClick={() => {
      import("@tauri-apps/api/core").then(({ invoke }) => {
        invoke("open_orb_studio_window").catch(() => {});
      });
    }}
  />
  ```
- Styled cleanly with standard `.hx-row` (no outer glowing filters, standard borders, standard hover states, and standard chevron).

### C. WebGL Particle Engine & Canvas 2D Fallback Parity (`frontend/src/avatar/voice-orb.js`)
- Added `u_think_mode`, `u_speak_mode`, `u_listen_mode` uniforms to the WebGL vertex shader (`VS`).
- Implemented mathematical formulation for Option #6 (Trefoil Celtic Helix Cage):
  $$\begin{aligned}
  \text{strand} &= \lfloor 3.0 \cdot \text{seed.w} \rfloor \\
  \theta &= 2\pi \cdot \text{fract}(3.0 \cdot \text{seed.w}) \\
  \phi &= \text{strand} \cdot \frac{2\pi}{3} \\
  R(\theta) &= 0.95 + 0.15 \cos(3\theta + 1.2 t) \\
  \vec{r}(\theta) &= \begin{pmatrix} R(\theta)\cos(\theta + 0.5t) \\ R(\theta)\sin(\theta + 0.5t) \\ 0.35\sin(2\theta + \phi) \end{pmatrix}
  \end{aligned}$$
  with $120^\circ$ spatial coordinate rotations around the X and Y axes for strands 1 and 2.
- Updated `_paint2D` in `voice-orb.js` to mirror the Trefoil Celtic Helix Cage geometry when WebGL falls back to 2D canvas.

### D. Settings Hydration & Multi-Window IPC Flow
- In `src-tauri/src/dyn_windows.rs`, defined the `orb_studio` window configuration ($800 \times 980$px, frameless liquid glass, docked flush to the left of Settings at $x = \text{monitor\_w} - 1222.0$).
- In `frontend/src/stage/orbRuntime.ts`, updated stage mount initialization to query `invoke("get_settings")` for `orbThinkMode`, `orbSpeakMode`, and `orbListenMode`, updating the Assistant store and emitting `orb:morphology_changed`.
- Propagated mode attributes through `Avatar.tsx` -> `VoiceOrb.tsx` -> `<voice-orb>`.

---

## 3. Verification & Metrics
- **Frontend Typecheck**: `npx tsc --noEmit` clean (0 errors).
- **Frontend Test Suite**: `npm test -- --run` — **213/213 passed** across 29 test files.
- **Frontend Production Build**: `npm run build` — built cleanly in 22.20s.
- **Rust Geometry & Memcore Tests**:
  - `cargo test --lib -- commands::sidebar_geometry_tests` (13/13 passed).
  - `cargo test --lib -- memcore::` (137/137 passed).
- **Cargo Check**: `cargo check --features custom-protocol,admin-brain` clean (0 warnings, 0 errors).
- **Release Binary**: Fresh release binary compiled and deployed to `%LOCALAPPDATA%\NEXUS\nexus.exe`.
