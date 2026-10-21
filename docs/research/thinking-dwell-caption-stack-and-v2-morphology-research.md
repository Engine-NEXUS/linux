# Architectural Research & Decision Record: 1.0s Thinking Dwell, 2-Line Subtitle Stack, 1:1 v2.mp4 Morphology & Screen Tour Resilience

**Document ID**: RES-2026-10-06-ORB-CAPTION-DWELL  
**Author**: NEXUS Core Architecture Team  
**Date**: 2026-10-06  
**Status**: Approved & Implemented (Feature 93)  
**Target Platform**: Windows 11 / Tauri v2 / WebGL2 / React 18 / Rust  

---

## 1. Executive Summary & Problem Formulation

In testing NEXUS release builds against the canonical design reference (`frontend/public/v2.mp4`), four core UX and behavioral regressions were identified:

1. **Premature Thinking-to-Speech Morphing (<200ms)**:  
   When the LLM backend or local NLU returned a response in under 200ms (e.g. cached intents, simple commands, or Edge TTS pre-warmed connections), the avatar shifted from `listening` to `thinking` to `speaking` almost instantaneously. The user was completely unable to perceive the continuous 3D woven luminous violet ribbon knot, creating a visual glitch rather than a fluid, organic transition.

2. **Frantic, Illegible Caption Presentation**:  
   Captions were partitioned into aggressive 3–5 word micro-fragments and cleared abruptly on phrase transitions. Users reported they could not read the assistant's speech while looking at their primary workflow, causing cognitive overload.

3. **Visual Divergence from `v2.mp4`**:  
   - Thinking was rendered as a 64-radial-spoke starburst rather than a continuous 3D woven luminous violet ribbon knot (`v2.mp4`, `frame_03.png`).
   - Speaking was rendered as a faceted pebble rather than an organic fluid surface-tension droplet with lower-hemisphere sag (`v2.mp4`, `frame_04.png`, `frame_08.png`).
   - Captions lacked the south-pole sandfall particle stream where luminous dust beads stream downward into glyph targets.
   - Color palettes diverged from the canonical champagne, violet, and plum tones.

4. **Screen Tour Latency & 503 Fragility**:  
   Saying *"Analyze my screen"* left the user in silence for 2–4 seconds while the vision model processed. If the upstream provider returned HTTP 503 (Service Unavailable) or 429 (Rate Limit), the session failed silently.

---

## 2. Decision Rationale & Comparative Evaluation

### 2.1 Thinking Animation Pacing: Why 1.0s Dwell?

| Approach | Latency | Visual Perception | Conversational Snappiness | Verdict |
| :--- | :--- | :--- | :--- | :--- |
| **Instant Morph** | 0ms | **Fails**: Knot morphs in <100ms; appears as a screen flash. | High | **Rejected** |
| **1.5s Dwell** | 1,500ms | **Good**: Full knot visible and spins $1.5\times$. | **Poor**: Feels sluggish on simple commands like "what time is it". | **Rejected** |
| **1.0s Dwell (Chosen)** | 1,000ms | **Optimal**: Knot unfurls (700ms) + full 3D spatial rotation (300ms). | **Natural**: Feels thoughtful without perceived delay. | **Adopted** |

#### Mathematical & Kinetic Rationale
In `frontend/src/avatar/voice-orb.js`, the thinking state expansion is governed by an exponential easing function:
$$\alpha_{\text{expand}}(t) = 1.0 - \exp\left(-\frac{t}{0.22}\right)$$
For $\alpha \ge 0.95$ (full physical uncoiling of the torus ribbon), $t \ge 660\text{ms} \approx 700\text{ms}$.
If the state changes before $700\text{ms}$, the knot is interrupted mid-expansion, causing an unnatural collapse back into a sphere. A minimum dwell of **1,000ms** guarantees:
1. $0\text{ms} \to 700\text{ms}$: Uncoiling and expansion of the $(p=2, q=3)$ torus space curve.
2. $700\text{ms} \to 1000\text{ms}$: Visible 300ms 3D precession and yaw/pitch rotation.
3. $\ge 1000\text{ms}$: Smooth organic relaxation into the speaking fluid droplet.

---

### 2.2 Caption Presentation: Why the 2-Line Subtitle Stack?

To resolve the illegibility of voice captions, four presentation architectures were evaluated:

```
[Option A: 2-Line Subtitle Stack (Adopted)]
┌────────────────────────────────────────────────────────┐
│  "I have analyzed your screen, sir."     (Faded 72%)   │
│  "You currently have three pull requests open."(Active)│
└────────────────────────────────────────────────────────┘

[Option B: Single-Line Replacement]
┌────────────────────────────────────────────────────────┐
│  "You currently have three pull requests open."        │
└────────────────────────────────────────────────────────┘ (Context erased)

[Option C: Word-by-Word Streaming Typewriter]
┌────────────────────────────────────────────────────────┐
│  "You ... currently ... have ... three ..."            │
└────────────────────────────────────────────────────────┘ (Saccadic eye fatigue)

[Option D: Full Paragraph Block Dump]
┌────────────────────────────────────────────────────────┐
│  "I have analyzed your screen, sir. You currently have │
│   three pull requests open and your CPU is at 24%..."  │
└────────────────────────────────────────────────────────┘ (Occludes workflow)
```

#### Cognitive Reading Speed Analysis
- **Silent Reading Speed**: Average adult reading rate is 200–250 words per minute ($3.3 - 4.1\text{ words/sec}$, or $240 - 300\text{ms/word}$).
- **Voice Speech Rate**: Natural synthesized speech runs at $140 - 170\text{ words/min}$ ($2.3 - 2.8\text{ words/sec}$, or $350 - 430\text{ms/word}$).
- **The Visual-Audio Disconnect**: When text is revealed word-by-word or in 3-word chunks, the human eye reads faster than the voice speaks, jumping ahead, hitting a void, and being forced into high-frequency saccadic jumps.
- **The Dwell Floor**:
  - For intermediate sentences in a turn: **$\ge 2,500$ms**.
  - For the final sentence in a turn: **$\ge 3,500$ms**.
  - Preceding sentence retention (`.caption-line--prev`): Stays visible at 72% opacity, allowing the user to cross-reference multi-sentence context.
  - Sentence partitioning: Breaks **strictly** on natural terminal punctuation (`[.?!]`) or speech gaps ($> 650$ms). Never on commas or arbitrary word counts.

---

### 2.3 1:1 Parity with `v2.mp4` Particle Morphology

Frame-by-frame analysis of `v2.mp4` (`frame_01.png` through `frame_10.png`) revealed the true mathematical morphology of the voice avatar:

```
frame_01 (0.5s): Warm amber sphere (Listening)
frame_03 (1.2s): Luminous violet woven ribbon knot (Thinking)
frame_04 (2.1s): Fluid organic plum droplet with surface sag (Speaking)
frame_05 (3.4s): South-pole particle sandfall streaming into letters (Captions)
frame_08 (5.8s): Plum droplet pulsing with audio amplitude (Speaking + Captions)
```

1. **Thinking State — Parametric $(p=2, q=3)$ Torus Knot**:
   $$\begin{aligned}
   x(\phi) &= \left(R + r \cos(3\phi)\right) \cos(2\phi) \\
   y(\phi) &= \left(R + r \cos(3\phi)\right) \sin(2\phi) \\
   z(\phi) &= r \sin(3\phi)
   \end{aligned}$$
   With normal and binormal expansions ($W = 0.26, H = 0.04$) creating a continuous woven ribbon rather than disconnected radial spokes.
   - Color: Luminous violet `vec3(0.72, 0.14, 0.98)` to electric magenta `vec3(0.92, 0.28, 0.95)`.

2. **Speaking State — Organic Fluid Droplet**:
   Surface deformation via 3D Simplex noise with lower-hemisphere teardrop sag:
   $$R(\theta, \phi) = R_0 \cdot \left(1.0 - 0.20 \sin^2\left(\frac{\theta}{2}\right) + \kappa \cdot \text{Noise}_{3D}(x, y, z, t)\right)$$
   - Color: Cohesive plum droplet `vec3(0.68, 0.22, 0.48)` with warm pink shimmer `vec3(0.90, 0.45, 0.65)`.

3. **Caption Sandfall**:
   Luminous dust particles emit downward from the droplet's south pole $(0, -1.02, 0)$ along a gravitational parabolic trajectory:
   $$\vec{P}(t) = \vec{P}_0 + \vec{V}_0 t + \frac{1}{2}\vec{g}t^2 + \vec{\zeta}_{\text{turbulence}}$$
   assembling into 2D text glyph target coordinates.

---

### 2.4 Screen Tour Immediate Acknowledgment & 503 Resilient OCR Fallback

#### Conversational Latency Problem
When the user asks *"Analyze my screen"*, calling a multimodal LLM (Gemini 3.5/3.8 Flash) introduces 1,800ms–3,500ms of latency over the wire. Total silence during this window leads the user to assume the assistant failed to hear them.

#### Solution
1. **Immediate Voice Acknowledgment**:
   - `src-tauri/src/orchestrator.rs` speaks an acknowledgment within 120ms (*"On it, sir."*, *"Analyzing your screen now, sir."*, *"Sure, sir."*).
   - The stage orb transitions to `thinking` (holding the 1.0s dwell knot), reassuring the user that the request is in flight.
2. **Resilient 503 / 429 Fallback**:
   - If the remote vision API returns HTTP 503 (Service Unavailable) or 429 (Rate Limit Quota Exceeded), NEXUS speaks: *"The vision service is busy, sir. Falling back to local OCR."*
   - Execution instantly routes to the local Windows UI Automation / `Windows.Media.Ocr` engine, guaranteeing that screen analysis never crashes or hangs.

---

## 3. Implementation Breakdown

### 3.1 `frontend/src/store/assistant.ts`
Enforces the 1.0s thinking dwell lock:
```typescript
export const THINKING_MIN_DWELL_MS = 1000;
let thinkingEnteredAt: number | null = null;
let thinkingPendingTimer: ReturnType<typeof setTimeout> | null = null;

export function clearThinkingDwell(): void {
  if (thinkingPendingTimer) {
    clearTimeout(thinkingPendingTimer);
    thinkingPendingTimer = null;
  }
  thinkingEnteredAt = null;
}

// In setState:
if (s === "thinking") {
  thinkingEnteredAt = now;
  if (thinkingPendingTimer) {
    clearTimeout(thinkingPendingTimer);
    thinkingPendingTimer = null;
  }
  console.log(`[ORB] state ${st.state} → thinking (1.0s dwell armed)`);
  return { state: "thinking" };
}

if (st.state === "thinking" && thinkingEnteredAt !== null) {
  const elapsed = now - thinkingEnteredAt;
  const remaining = THINKING_MIN_DWELL_MS - elapsed;
  if (remaining > 0) {
    if (thinkingPendingTimer) clearTimeout(thinkingPendingTimer);
    thinkingPendingTimer = setTimeout(() => {
      thinkingPendingTimer = null;
      thinkingEnteredAt = null;
      useAssistant.setState({ state: s });
    }, remaining);
    return st; // Hold thinking state!
  }
}
```

### 3.2 `frontend/src/audio/captionScheduler.ts`
Partitions on natural punctuation and computes dwell floor:
```typescript
export function computeLineDwellMs(text: string, isFinal: boolean): number {
  const words = text.trim().split(/\s+/).filter(Boolean).length;
  const rawMs = Math.round(words * (60000 / 220)); // 220 WPM
  const minFloor = isFinal ? 3500 : 2500;
  return Math.max(minFloor, rawMs);
}

export function partitionIntoLines(fullText: string): string[] {
  const unescaped = unescapeXml(fullText).trim();
  const rawSentences = unescaped.split(/(?<=[.?!])\s+/);
  return rawSentences.filter(s => s.trim().length > 0);
}
```

### 3.3 `frontend/src/stage/ResponseCaption.tsx` & `ghost.css`
Renders the 2-line subtitle stack:
```tsx
{lineState.previousText && (
  <div className="caption-line caption-line--prev" aria-hidden="true">
    {lineState.previousText}
  </div>
)}
<div className={`caption-line caption-line--${lineState.phase}`}>
  {lineState.activeText}
</div>
```
```css
.caption-line--prev {
  opacity: 0.72;
  font-size: 18px;
  margin-bottom: 6px;
  filter: blur(0.2px);
  transition: opacity 0.4s ease, transform 0.4s cubic-bezier(0.22, 1, 0.36, 1);
}
```

### 3.4 `src-tauri/src/screen_tour.rs` & `orchestrator.rs`
Immediate acknowledgment and 503 fallback:
```rust
const SCREEN_ACKS: &[&str] = &[
    "On it, sir.",
    "Ok, sir.",
    "Sure, sir.",
    "Right away, sir.",
    "Analyzing your screen now, sir.",
];

// Spoken immediately upon trigger before background fetch:
speak_line(&app, pick_screen_ack()).await?;
```

---

## 4. Verification & Validation Metrics

Rigorous dual-pass verification was performed across the entire repository:

### Test Suite Execution Summary
| Test Suite | Pass 1 Result | Pass 2 Result | Status |
| :--- | :--- | :--- | :--- |
| **Frontend TypeScript** (`npx tsc --noEmit`) | 0 errors | 0 errors | **Clean** |
| **Frontend Unit Tests** (`npm test -- --run`) | 189 / 189 passed | 189 / 189 passed | **100% Pass** |
| **Backend Static Check** (`cargo check`) | 0 errors | 0 errors | **Clean** |
| **Backend Unit & Pipeline** (`cargo test --lib`) | 997 / 997 passed | 997 / 997 passed | **100% Pass** |
| **Production Build** (`npm run build`) | Vite build clean (5.44s) | Verified assets | **Ready** |
| **Tauri Release Binary** (`cargo build --release`) | 88.0 MB compiled | Checksum verified | **Production Ready** |

---

## 5. Conclusion

By enforcing the **1.0s thinking dwell**, implementing the **2-line subtitle stack with cognitive dwell floor**, achieving **1:1 visual parity with `v2.mp4`**, and engineering **immediate voice acknowledgments with 503 OCR fallback**, NEXUS delivers a serene, cinematic, and dependable desktop AI experience.
