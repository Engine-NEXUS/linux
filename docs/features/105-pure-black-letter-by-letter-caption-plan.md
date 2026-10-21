# Feature Plan: 105 — Pure Black Letter-by-Letter Voice Captions

**Date**: 2026-10-09  
**Status**: Plan Only (Awaiting User Review)  
**Scope**: Response Caption Visual Redesign, Pure Black Capsule Alignment, Zero Zoom/Blue Removal, Continuous Letter-by-Letter Typewriter Streaming

---

## 1. Executive Summary & Design Directives

The user has specified three exact design requirements for voice captions:
1. **Pure Black Background Matching the Orb**:
   - The caption background must be 100% solid pure black (`#000000 !important`), identically matching `.orb-capsule` in `styles.css`.
   - Replaces the previous semi-transparent `rgba(16, 20, 30, 0.82)` and frosted backdrop blur.
   - Retains the orb's subtle border (`1px solid rgba(255, 255, 255, 0.16)`), 10px corner radius, and deep shadow (`box-shadow: 0 16px 44px rgba(0, 0, 0, 0.95), 0 4px 14px rgba(0, 0, 0, 0.85)`).
2. **No Zoom In & No Blue Font**:
   - Completely remove the cyan/blue glowing font (`#00e5ff`, `text-shadow: 0 0 16px #00e5ff`).
   - Completely remove the scale zoom and spring bounce (`transform: scale(1.08)`, `@keyframes mercury-pop`).
   - Use clean, solid, high-contrast crisp white (`#ffffff` / `#f8fafc`) with uniform typography across all characters.
3. **Continuous Letter-by-Letter Addition**:
   - Instead of words popping in block-by-block with individual word highlights, the text continuously appends **letter-by-letter (character-by-character)** in real time as the voice speaks.
   - Pacing is dynamically synchronized with the speech cadence (interpolated smoothly across word timestamps).
   - Upon sentence/clause completion, the full line dwells for 2.0s before smoothly dissolving for the next sentence.

---

## 2. Visual Architecture & Styling Parity

### Comparison: Previous Mercury vs. New Pure Black Stream

| Visual Attribute | Previous (Design #11 Liquid Mercury) | New Directive (Pure Black Letter Stream) |
| :--- | :--- | :--- |
| **Background** | `rgba(16, 20, 30, 0.82)` with 36px blur | **`#000000` (Pure pitch black, exact match to `.orb-capsule`)** |
| **Border & Corners** | 9999px pill radius, `rgba(255,255,255,0.22)` | **`10px` radius, `1px solid rgba(255, 255, 255, 0.16)`** |
| **Box Shadow** | Standard glow `rgba(0,0,0,0.65)` | **`0 16px 44px rgba(0, 0, 0, 0.95), 0 4px 14px rgba(0, 0, 0, 0.85)`** |
| **Active Word Zoom** | `transform: scale(1.08) translateY(-1px)` + `@keyframes mercury-pop` | **`transform: none` (Zero zoom, perfectly steady text baseline)** |
| **Active Word Color** | Neon Cyan `#00e5ff` + 28px blue glow | **Crisp White `#ffffff` (Zero blue, no glow)** |
| **Reveal Granularity** | Word-by-word discrete chunks | **Continuous Letter-by-Letter Typewriter stream** |

### CSS Specification (`frontend/src/stage/ghost.css`)
```css
/* Pure Black Response Caption Capsule — Identical to .orb-capsule */
.response-caption--black {
  width: auto;
  max-width: 680px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}

.caption-black-capsule {
  background: #000000 !important;
  background-color: #000000 !important;
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 10px;
  padding: 12px 24px;
  max-width: 620px;
  min-height: 48px;
  box-shadow: 
    0 16px 44px rgba(0, 0, 0, 0.95), 
    0 4px 14px rgba(0, 0, 0, 0.85);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: opacity 300ms ease, transform 300ms ease;
  will-change: opacity, transform;
}

.caption-black-capsule--active {
  opacity: 1;
  transform: translateY(0);
}

.caption-black-capsule--fading {
  opacity: 0;
  transform: translateY(-4px);
}

/* Crisp White Typography — Steady Baseline, No Zoom */
.caption-black-text {
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", sans-serif;
  font-size: 18px;
  font-weight: 550;
  letter-spacing: -0.2px;
  line-height: 1.4;
  color: #ffffff;
  text-shadow: 0 1px 2px rgba(0, 0, 0, 0.9);
  white-space: pre-wrap;
  word-break: break-word;
  display: inline;
}

/* Optional subtle blinking trailing cursor while speaking */
.caption-typing-cursor {
  display: inline-block;
  width: 2px;
  height: 18px;
  margin-left: 3px;
  background-color: rgba(255, 255, 255, 0.8);
  vertical-align: middle;
  animation: cursor-blink 800ms infinite;
}

@keyframes cursor-blink {
  0%, 100% { opacity: 0; }
  50% { opacity: 1; }
}
```

---

## 3. Letter-by-Letter Streaming Mechanics

### A. The Challenge with Word Boundaries vs Characters
Kokoro and Edge TTS emit word boundaries (`start_ms`, `duration_ms` per word). If we reveal text strictly letter-by-letter, the letters within each word must be smoothly spaced out over that word's duration so the typing matches the audio articulation:
- Let word $W$ have string length $L$ and duration $D = cw.\text{duration\_ms}$.
- Each letter index $k \in [0, L-1]$ takes duration $\Delta t = \frac{D}{L}$ (typically 20ms–40ms per character).
- The space character following the word is added immediately when the word completes.

### B. Two-Tier Typewriter Architecture

```
Audio Clock (ms):  0 -------- 200 ------ 350 ----------- 650 -------- 900
Words:             "Hi"       "there"    "I"             "am"         "NEXUS"
Letter Timeline:   H-i-(space)-t-h-e-r-e-(space)-I-(space)-a-m-(space)-N-E-X-U-S
Display:           H -> Hi -> Hi  -> Hi t -> Hi th -> Hi the -> Hi ther -> Hi there...
```

### C. Component Implementation (`ResponseCaption.tsx`)
Rather than creating hundreds of `setTimeout` timers (which can cause micro-stutters and garbage collection pressure), `ResponseCaption.tsx` uses a lightweight, audio-anchored interpolation loop:
1. `captionScheduler.ts` delivers the active line data (full target sentence and word timing metadata).
2. `ResponseCaption.tsx` tracks `revealedCharCount`:
   - Every `requestAnimationFrame` tick (or 25ms timer), it calculates how many characters should be visible based on `performance.now() - anchorMs`.
   - `revealedText = fullSentence.slice(0, targetCharCount)`.
3. If user interrupts (barge-in), `clearCaptionSchedule()` instantly wipes the text.
4. When all characters of the sentence have been revealed, it holds for the 2.0s dwell before smoothly fading out.

---

## 4. Implementation Steps (When Approved to Execute)

### Step 1: Update Styling in `frontend/src/stage/ghost.css`
- Add `.response-caption--black`, `.caption-black-capsule`, `.caption-black-text`.
- Set `background: #000000 !important` and border/shadow identical to `.orb-capsule`.
- Remove `.mercury-word--active` scale zoom and blue cyan glow.

### Step 2: Implement Letter-by-Letter Streaming in `ResponseCaption.tsx`
- Replace word mapping array (`words.map(...)`) with smooth `revealedText` character slice.
- Add character interpolation loop matching word timestamps.
- Render clean white text on pure black capsule.

### Step 3: Verify and Test
- Verify in Vite playground and dev server.
- Run `npm test -- --run` (ensure all 213 tests pass).
- Run `npx tsc --noEmit` (ensure 0 TypeScript errors).
- Compile release binary and test visual alignment with the voice orb.

---

## 5. Decision & Confirmation
This document is **PLAN ONLY** as requested. Once you review and approve, we will proceed directly with the implementation.
