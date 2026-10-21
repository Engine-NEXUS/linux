# Design #11 Liquid Mercury Stream: Incremental Word-by-Word Voice Captions (2026-10-08)

## Motivation & Architecture
- **User Directive**:
  1. User selected Design #11 (Liquid Mercury Chrome) for voice captions.
  2. Strict constraint: Do **NOT** display the entire pre-rendered sentence and highlight words.
  3. Words must **ONLY** materialize on screen as they are spoken out in real-time (incremental reveal with zero future words rendered in the DOM).
  4. Mode: **Accumulating Clause Stream** — words stream in one-by-one as spoken up to sentence/clause breaks, hold for 2.0s reading dwell, then smoothly dissolve for the next sentence.
- **Zero-RAM & CSP Compliance**:
  - Maintained zero local model RAM overhead (~177 MB baseline).
  - Maintained full CSP compliance in the transparent stage window: transforms and opacities are applied via direct DOM mutation on `el.style.transform` and `el.style.opacity`, while CSS classes handle transition timing, blur, chrome highlights, and `@keyframes mercury-pop`.

## Implementation Details
1. **Frontend Caption Scheduler (`frontend/src/audio/captionScheduler.ts`)**:
   - `CaptionLineEvent` extended with `words?: string[]` and `activeWordIndex?: number`.
   - `partitionIntoLines` preserves individual `CaptionWord` structures per line.
   - `scheduleChunk` schedules word reveal timers anchored to `cw.start_ms`. Each word timer appends the spoken word into `lineRevealed` and emits a line update containing *only* words spoken so far.
   - Final reading dwell extended to 2.0s after the final word before triggering sentence cross-dissolve.
2. **Response Caption Component (`frontend/src/stage/ResponseCaption.tsx`)**:
   - Upgraded to render `.response-caption--mercury` with `.mercury-pill`.
   - Maps over revealed `words`:
     - Spoken words: `<span className="mercury-word mercury-word--spoken">{w}</span>`
     - Active word: `<span className="mercury-word mercury-word--active">{w}</span>`
   - Dynamically centered via `transform: translate(${centerX}px, ...) translateX(-50%)`, allowing the pill to smoothly expand outwards as words are spoken.
   - Returns `null` if no words have arrived yet, guaranteeing zero pre-rendered text.
3. **Stage Styling (`frontend/src/stage/ghost.css`)**:
   - Added `.response-caption--mercury`, `.mercury-pill`, `.mercury-pill--active`, `.mercury-pill--fading`, `.mercury-pill-text`, `.mercury-word`, `.mercury-word--active`, `.mercury-word--spoken`.
   - Liquid mercury aesthetic: `rgba(16, 20, 30, 0.82)` background, `border: 1px solid rgba(255, 255, 255, 0.22)`, `backdrop-filter: blur(36px) saturate(200%)`, pill radius (`border-radius: 9999px`), glowing cyan chrome accent on active word with subtle `@keyframes mercury-pop` spring bounce.
4. **Interactive Showcase (`frontend/caption-showcase.html`)**:
   - Option #11 interactive simulation on `http://localhost:5173/caption-showcase.html` with real-time audio playback and dynamic word reveal.

## Verification
- `npx tsc --noEmit` in `frontend/`: clean (0 errors).
- `npm test -- --run` in `frontend/`: **210/210 passed** across 29 test files.
- `npm run build` in `frontend/`: built cleanly in 12.74s.
- `cargo check --features custom-protocol,admin-brain`: clean (0 warnings, 0 errors).
- `cargo build --release --features custom-protocol,admin-brain`: clean release binary (93.37 MB) compiled and deployed to `%LOCALAPPDATA%\NEXUS\nexus.exe`.
