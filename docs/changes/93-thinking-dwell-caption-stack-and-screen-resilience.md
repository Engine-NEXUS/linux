# Change 93: 1.0s Thinking Dwell, 2-Line Subtitle Stack, Screen Ack & Resilient Fallback

## Date: 2026-10-06

### Summary of Changes

1. **Thinking Animation 1.0s Minimum Dwell**:
   - `frontend/src/store/assistant.ts`: Added `THINKING_MIN_DWELL_MS = 1000` with dwell lock preventing instant transition to speaking until the continuous 3D woven luminous violet ribbon knot has completed its 700ms expansion and at least 300ms of visible 3D spatial rotation.
   - Handled delayed state transitions cleanly with `thinkingPendingTimer` and `clearThinkingDwell()` during state resets.

2. **Calm, Legible 2-Line Subtitle Stack**:
   - `frontend/src/audio/captionScheduler.ts`:
     - Refined sentence partitioning to break exclusively on natural terminal punctuation (`[.?!]`) or speech pause boundaries (>650ms), eliminating jarring arbitrary 3–5 word chunk splits.
     - Enforced cognitive readability dwell floor: intermediate sentences dwell for at least 2,500ms; final sentences dwell for at least 3,500ms.
     - Added `previousText` tracking to retain the preceding sentence in faded form above the newly emerging sentence.
   - `frontend/src/stage/ResponseCaption.tsx` & `frontend/src/stage/ghost.css`:
     - Rendered `.caption-line--prev` above active `.caption-line` with 72% opacity, 18px font size, and 400ms cross-dissolve with subtle blur.

3. **1:1 Color Palette Parity with `v2.mp4`**:
   - `frontend/src/avatar/voice-orb.js`:
     - **Listening**: Warm amber / champagne gold (`vec3(0.95, 0.72, 0.35)` to `vec3(1.0, 0.88, 0.58)`).
     - **Thinking**: Luminous violet to electric magenta (`vec3(0.72, 0.14, 0.98)` to `vec3(0.92, 0.28, 0.95)`).
     - **Speaking**: Organic plum droplet with warm pink shimmer (`vec3(0.68, 0.22, 0.48)` to `vec3(0.90, 0.45, 0.65)`).
     - **Captions**: Silver-lavender stardust (`#d8d0ea`).

4. **Screen Tour Immediate Acknowledgment & 503 Resilient Fallback**:
   - `src-tauri/src/screen_tour.rs`: Expanded `SCREEN_ACKS` with varied acknowledgments ("On it, sir.", "Ok, sir.", "Sure, sir.", "Right away, sir.", "Analyzing your screen now, sir.").
   - `src-tauri/src/orchestrator.rs`:
     - Immediately speaks an acknowledgment before kicking off the background screen analysis.
     - Resiliently detects upstream 503 Service Unavailable / 429 errors from vision models and speaks a polite notification before automatically falling back to local Windows OCR extraction.

### Verification (Dual-Pass Rigorous Testing)
- **Pass 1**:
  - `npx tsc --noEmit`: 0 errors (clean)
  - `npm test -- --run`: 189/189 tests passed
  - `cargo check`: clean
  - `cargo test --lib -- --test-threads=1`: 997/997 passed (6 ignored dev benches)
- **Pass 2**:
  - `npx tsc --noEmit`: 0 errors (clean)
  - `npm test -- --run`: 189/189 tests passed
  - `cargo test --lib -- --test-threads=1`: 997/997 passed (6 ignored dev benches)
- **Release Build**:
  - Vite production build clean.
  - Tauri release binary compiled cleanly.
