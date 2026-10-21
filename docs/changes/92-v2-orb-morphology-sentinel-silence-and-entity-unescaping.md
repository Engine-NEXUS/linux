# Change 92: 1:1 v2.mp4 Orb Morphology, Sentinel Poller Guard, XML Unescaping & Wake Default

## Date: 2026-10-06

### Summary of Fixes

1. **Sentinel Email Poller Silence**:
   - Stale synthetic watch `screen_thread_1790701281` was purged from `%APPDATA%\com.nexus.assistant\memory\mail_watches.json`.
   - `src-tauri/src/google/sentinel.rs` now checks for active Google OAuth authentication before printing poll cycle logs. If unauthenticated, it emits a single startup notice and sleeps silently without repeating every 15 seconds.
   - Synthetic `screen_thread_*` targets now automatically expire after 2 hours.

2. **XML Entity Decoding (`&apos;`)**:
   - `src-tauri/src/tts.rs`: Added `unescape_ssml_entities()` decoding `&apos;` -> `'`, `&quot;` -> `"`, `&amp;` -> `&`, `&lt;` -> `<`, `&gt;` -> `>` in `boundaries_to_words()`.
   - `frontend/src/audio/captionScheduler.ts`: Added `unescapeXml()` in line partitioning and word reveal handlers.

3. **Wake Verification Default**:
   - `src-tauri/src/commands.rs`: `read_verify_wake` now defaults to `true` (as documented), enforcing Stage-2 acoustic verification with Moonshine/Whisper to prevent ambient noise blips from triggering false wakes.

4. **1:1 Visual Parity with `v2.mp4`**:
   - `frontend/src/avatar/voice-orb.js`:
     - **Thinking**: Continuous 3D woven luminous violet ribbon knot (parametric $p=2, q=3$ torus space curve with ribbon normal/binormal width expansion $W=0.26, H=0.04$, soft neon violet gradient `vec3(0.72, 0.14, 0.98)` to `vec3(0.92, 0.28, 0.95)`, 3D yaw/pitch precession). Zero radial spokes, zero solid nucleus.
     - **Speaking**: Organic fluid droplet with 3D Simplex noise surface tension, lower-hemisphere teardrop sag, and speech amplitude/onset bulging.
     - **Caption Sandfall**: South-pole downward particle stream ($(0, -1.02, 0) \to \text{target}$) with downward gravity arc and turbulence assembling into 2D text glyphs, dissolving clause-by-clause before each phrase.
     - Parity maintained across WebGL and 2D canvas CPU fallback.

5. **Release Binary**:
   - Recompiled fresh release executable to `src-tauri/target/release/nexus.exe` (88.0 MB) embedding updated frontend and backend.

### Verification
- `npx tsc --noEmit`: 0 errors (clean)
- `npx vitest run`: 189/189 passed
- `cargo check`: 0 errors (clean)
- `cargo test --lib -- --test-threads=1`: 997/997 passed
- Fresh release binary built at `06-10-2026 20:44:33`.
