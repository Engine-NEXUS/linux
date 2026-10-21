# Change 90: Master Volume Restoration, Orb Disappearance Fix, Pure Black Capsule & Line-by-Line Captions

## Context & Motivation
Following user feedback across three core areas:
1. **System Master Volume Restoration**: NEXUS would boost system volume to 70% during speech but fail to return it to the user's previous baseline (e.g. 20–30%), trapping system master volume at 70%.
2. **Voice Orb Disappearance & Observability**:
   - The Voice Orb was disappearing inside the black slider capsule while the capsule remained active onscreen, and disappearing in Ghost Mode while captions continued.
   - Captions and the orb were out of sync.
   - The unified log monitor (`scripts/run.ps1`) dropped `[ORB]` logs due to missing regex matchers and lack of an `else` fallback.
3. **Pure Black Capsule & Line-by-Line Captions**:
   - The user requested removing any pillar bars or status tabs in the capsule, leaving strictly the Voice Orb in an obsidian black slider capsule with 10px minimalist corners.
   - Assistant captions must replace line-by-line / clause-by-clause (e.g. "hi lakshya i am nexus" → dissolves → "how can i help u today" → dissolves → "this is the analyssi result") rather than displaying full paragraphs or overflowing.
   - Live user captions display a clean 5-word FIFO moving window.
   - The Voice Orb stays visible and active until the final caption line has completely dissolved.

---

## Changes Implemented

### 1. Zero-Leak Master Volume Restoration (Rust Core)
- **`src-tauri/src/volume.rs`**:
  - Implemented RAII `TtsVolumeLease` with reference-counted active speaker counter `ACTIVE_TTS_COUNT`.
  - Locked baseline invariant on `0 -> 1` transition: captures initial master volume only once. If current volume already matches target, the existing baseline is preserved.
  - Implemented 15-second safety watchdog (`check_volume_watchdog`) to prevent any leaked audio state from leaving system volume elevated.
  - Verified with `cargo test --lib test_save_and_restore`: volume reliably restored from 0.70 back to 0.30.
- **`src-tauri/src/tts.rs`**:
  - Bound all speech playback scopes to `acquire_volume_lease()`.
  - Added `force_restore_volume()` call in `stop_tts()` for instantaneous restoration on user barge-in.

### 2. Full Observability & Zero-Leak Log Streaming
- **`scripts/run.ps1`**:
  - Always enabled `--remote-debugging-port=9222` and started CDP monitor by default.
  - Added regex matchers for `[ORB]`, `[ORB-FRAME]`, `[STAGE]`, `[AVATAR]`, `[VOICE-ORB]` (Cyan), and `[CAPTION]` (Green).
  - Added catch-all `else` fallback so no console message is dropped.
- **`frontend/src/avatar/VoiceOrb.tsx`**:
  - Added `[VOICE-ORB] WebGL loop RESUMED (play)` and `[VOICE-ORB] WebGL loop PAUSED (pause)` lifecycle logs.
  - Added `color?: string;` prop forwarding to `<voice-orb>`.
- **`frontend/src/stage/OrbFrame.tsx`**:
  - Added `[ORB-FRAME] state: isShown=... pos=...` lifecycle logs.

### 3. Voice Orb Disappearance & Sync Fix
- **`frontend/src/avatar/Avatar.tsx`**:
  - Fixed visibility logic: `isOrbVisible = Boolean(enteredProp || visible || ghostActive || dispersing)`.
  - Passed `visible={isOrbVisible}` to `<VoiceOrb>` so the WebGL loop never pauses or blanks inside the onscreen black box.
- **`frontend/src/net/orchestrator.ts`**:
  - In `hideOrbAfterSpeech`, added `if (s.captionActive) { hideOrbAfterSpeech(350); return; }`.
  - The Voice Orb remains visible in the black box until the final caption clause has completely finished fading out.

### 4. Pure Black Capsule & Line-by-Line Captions
- **`frontend/src/stage/OrbFrame.tsx`**:
  - Pure black capsule: wraps strictly `<Avatar>` inside `.orb-sphere-wrapper` inside `.orb-capsule` within `.orb-slider`.
  - Zero pillar bars, zero status tabs.
  - Deliberate `0.72s cubic-bezier(0.22, 1, 0.36, 1)` hardware-accelerated slide motion.
- **`frontend/src/audio/captionScheduler.ts`**:
  - Exported `suppressNextCaption` to prevent duplicate DOM captions when particle text is rendered.
  - Implemented `partitionIntoLines(words)`: partitions words into clean 5-word / clause units by sentence punctuation (`.`, `?`, `!`), comma boundaries, pauses (>350ms), and 5-word limits with lookahead.
  - Implemented line-by-line phase scheduling: `active` → `fading` → `cleared`.
  - Dynamically updates `captionActive` in `useAssistant` store.
- **`frontend/src/stage/ResponseCaption.tsx`**:
  - Subscribed to `onCaptionLineUpdate` to render discrete active and fading lines.
  - Adaptive docking coordinates: renders below the capsule for Top docking (`bottomY + 24px`) and above the capsule for Bottom docking (`topY - 24px, translateY(-100%)`).
- **`frontend/src/stage/LiveCaption.tsx`**:
  - Chunks live user partial transcripts into a 5-word FIFO moving window.
- **`frontend/src/stage/ghost.css`**:
  - Added `.caption-line`, `.caption-line--active`, and `.caption-line--fading` CSS transitions.

---

## Verification
- **Rust compilation**: `cargo check` clean (0 errors).
- **Rust unit tests**: `cargo test --lib -- --test-threads=1` passed: **971/971 tests pass** (6 ignored dev benches).
- **TypeScript compilation**: `npx tsc --noEmit` clean (0 errors).
- **Frontend unit tests**: `npx vitest run` passed: **188/188 tests pass** across 26 test files.
- **Git status**: `frontend/public/v2*` verified ignored in `.gitignore`.
