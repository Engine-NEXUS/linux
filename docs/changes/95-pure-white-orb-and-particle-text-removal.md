# Change 95: Pure White Orb Color Invariant & Particle-Text Inside Black Box Removal

## Problem & Motivation

1. **Particle Text Inside Black Box**:
   - The user noticed that during short speech responses (e.g., "On it sir.", "Ok sir."), text letters were forming inside the black capsule slider from the orb's particles: *"there is a text being formed inside the black box using particales i dont want that remove that"*.
   - This occurred because `ttsPlayer.ts` had a short-phrase gate (`phrase.trim().length <= 22 && !phrase.includes("\n")`) that suppressed regular DOM captions via `suppressNextCaption()` and emitted `orb:show_text`, causing `voice-orb.js` to sample canvas glyph points and target particles into letters inside the orb.
   - The user explicitly requested removing this: the orb particles must remain an intact avatar (sphere, ribbon knot, fluid droplet) at all times and not detach into letters inside the capsule. Spoken responses must render exclusively as subtitles outside the capsule.

2. **White Particle Color Invariant**:
   - The user requested: *"make the orb color white color all particles should be white at any state it might have been make sure of it"*.
   - Previously, particles had state-dependent color palettes (amber for listening, violet/magenta for thinking ribbon knot, plum/pink for speaking droplet).
   - All particles across every state (idle, listening, thinking, speaking) must now render in pure, clean, luminous white (`vec3(1.0, 1.0, 1.0)` / `#ffffff`).

3. **Compilation & Resilience Fixes**:
   - Resolved `E0061` in `src-tauri/src/orchestrator.rs`: passed `dialog_context` to `run_counsel_turn`.
   - Resolved `E0063` in `src-tauri/src/commands.rs`: provided default `persona_mode: default_persona_mode()` in `NexusSettings::default()`.
   - Resolved slice bounds panic in `src-tauri/src/intent_parser.rs`: safe prefix stripping for `"tell me honestly"`.

---

## Architectural Changes

### 1. Particle-Text Trigger Elimination
- **`frontend/src/audio/ttsPlayer.ts`**:
  - Removed short-phrase interception (`text.trim().length <= 22`) in both `speak()` and `speakCached()`.
  - Removed calls to `suppressNextCaption()` and `emit("orb:show_text")`.
  - All spoken utterances (short acknowledgments and long sentences alike) now flow naturally to `captionScheduler.ts` and display as readable HTML subtitles below the capsule in `ResponseCaption.tsx`.

### 2. Orb Web Component De-coupling & Invariant
- **`frontend/src/avatar/VoiceOrb.tsx`**:
  - Removed `orb:show_text` event listener and `text` prop effect.
  - Kept `text: _text` destructured prop for backwards compatibility without TypeScript lint errors.
- **`frontend/src/avatar/voice-orb.js`**:
  - Disarmed `setText(text, holdMs)` to immediately `return false;`.
  - In vertex shader `VS`, removed `streamPos` text-assembly interpolation and simplified `pos = cloudPos;` so particles stay strictly within their organic geometry (listening sphere, thinking ribbon knot, speaking fluid droplet).
  - Hardcoded `tint = vec3(1.0, 1.0, 1.0); sparkTint = vec3(1.0, 1.0, 1.0);` in vertex shader.
  - Hardcoded `vec3(1.0, 1.0, 1.0)` in fragment shader `FS`.
  - Hardcoded `PALETTE` and `parseColorHex` to pure white (`[1.0, 1.0, 1.0]`).
  - Updated 2D CPU fallback `_paint2D` `stateColor` and `ctx.fillStyle` to `rgb(255, 255, 255)`.
  - Updated store default in `frontend/src/store/assistant.ts` from `#f2b859` to `#ffffff`.

---

## Verification Results

1. **Frontend Type Check**:
   - `npx tsc --noEmit`: 0 errors (clean).
2. **Frontend Test Suite**:
   - `npm test -- --run` (Vitest): **189 / 189 tests passed** (clean).
3. **Rust Codebase Validation**:
   - `cargo check`: clean (0 errors).
   - `cargo test --lib -- --test-threads=1`: **1002 / 1002 tests passed** (clean, 0 failures, 6 ignored dev benches).
4. **Production Build**:
   - `npm run build`: built production dist in 10.03s.
   - `cargo build --release --features custom-protocol`: cleanly compiled fresh 95.8 MB release binary `src-tauri/target/release/nexus.exe`.
