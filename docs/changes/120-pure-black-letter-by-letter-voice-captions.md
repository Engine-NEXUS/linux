# Change 120: Pure Black Letter-by-Letter Voice Captions (Feature 105)

**Date**: 2026-10-09  
**Status**: Completed & Verified  
**Scope**: Voice Caption Visual Parity with Orb Capsule, Zero Zoom/Blue Removal, Continuous Letter-by-Letter Progressive Streaming

---

## 1. Problem & Context

Previously, voice response captions used Design #11 ("Liquid Mercury Chrome"), which featured a semi-translucent dark navy backdrop with frosted glass blur, scale zoom on active words (`transform: scale(1.08)` with `@keyframes mercury-pop`), and electric cyan glow (`#00e5ff`). Additionally, text was revealed word-by-word in discrete chunks.

The user directed three strict requirements:
1. **Pure black background**: Identical to `.orb-capsule` in `styles.css` (`#000000 !important`, 10px border radius, 1px subtle highlight border `rgba(255, 255, 255, 0.16)`, and multi-stage drop shadows `0 16px 44px rgba(0, 0, 0, 0.95), 0 4px 14px rgba(0, 0, 0, 0.85)`).
2. **Zero zoom & no blue font**: Steady baseline typography in crisp white (`#ffffff`), with all scale animations and cyan/blue text shadows removed.
3. **Continuous letter-by-letter typing**: Characters stream in and append letter-by-letter in real time as the speech is articulated, preserving the 2.0s reading dwell upon sentence completion.

---

## 2. Changes Made

### A. Progressive Letter-by-Letter Scheduler (`frontend/src/audio/captionScheduler.ts`)
- Replaced block word-by-word reveal scheduling with character-by-character progressive accumulation within each word duration.
- For each word with duration $D$ and length $N$, intra-word character reveals are evenly distributed:
  $$\text{charDelay} = \text{word.start\_ms} + \left(\frac{i}{N}\right) \times \text{word.duration\_ms}$$
- Dynamically accumulates characters and notifies `notifyLine` with `textSnapshot` without dropping words or punctuation.
- Preserved the 2.0s sentence dwell for final clauses and 1.6s for intermediate clauses, along with immediate barge-in cancellation in `clearCaptionSchedule()`.

### B. Steady Baseline & Pure Black Rendering (`frontend/src/stage/ResponseCaption.tsx`)
- Container updated to `.response-caption--black` hosting `.caption-black-capsule`.
- Replaced word-level span mapping with direct progressive rendering of `{currentText}` inside `.caption-black-text`.
- Eliminated per-word span churn, scale popping, and cyan color transitions.

### C. Styling Parity (`frontend/src/stage/ghost.css`)
- Styled `.caption-black-capsule` with `#000000 !important`, `border: 1px solid rgba(255, 255, 255, 0.16)`, `border-radius: 10px`, and shadows matching `.orb-capsule`.
- Set `.caption-black-text` to crisp white `#ffffff` with steady inline display and zero transform.

---

## 3. Verification

1. **TypeScript Typecheck**:
   - `npx tsc --noEmit` ran cleanly with 0 errors.
2. **Unit Tests**:
   - Vitest suite passed: **224 / 224 tests passing across 30 test files**.
3. **Vite Production Build**:
   - `npm run build` succeeded cleanly in 13.65s without warnings.
4. **Rust Backend Check**:
   - `cargo check --features custom-protocol,admin-brain` compiled cleanly with 0 errors.
