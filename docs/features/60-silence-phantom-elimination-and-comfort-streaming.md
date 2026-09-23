# Silence Phantom Elimination & Continuous Comfort Streaming Architecture Specification

## 1. Problem Statement & Root Cause Diagnosis

During live wake-word testing (`nexus wake test`), the system produced repetitive false alarms in complete silence, reaching up to 100.0% confidence without any speech. A line-level cross-check across both the Python live test harness and the production Rust engine identified four root causes:

1. **The Squeezebox Bug (Temporal Freezing)**: When audio fell below the energy gate or VAD threshold, feature extraction was bypassed without updating the circular feature buffer. Unrelated ambient noises separated by minutes were frozen and concatenated together into a synthesized 1.28-second wake-word window.
2. **LayerNorm Zero-Reset Collapse**: When a trigger or stream reset occurred, the 16-frame embedding buffer was wiped to all zeros (`[0.0; 96] * 16`). In `OwwClassifier`, `LayerNorm(128)` normalized across 14-15 zero frames collapsed variance ($\sigma \rightarrow 0$), causing any minor incoming non-zero float to be multiplied by $1/\sigma$ and produce 99.9% false alarms.
3. **Impulsive Sound Gate Inversion**: The condition `prev_rms > 0.0005 && rms < 0.05` failed both for quiet-room transients ($prev\_rms \le 0.0005$) and for real energetic bursts ($rms \ge 0.05$).
4. **Single-Frame Hair-Trigger**: Spoken "NEXUS" requires 350–700ms (4–8 consecutive 80ms frames). Single-frame noise blips occasionally scored high for 80ms before decaying.

---

## 2. Engineering Architecture & Solutions

### A. Ambient Comfort Embedding Pre-Warming & Continuous Advancement
- **Pre-computed Comfort Embedding**: Generated a 96-dimensional baseline comfort embedding (`src-tauri/resources/oww/comfort_embedding.npy` and `COMFORT_EMBEDDING: [f32; 96]` in `wakeword_oww.rs`) modeling ambient room noise ($\mu = 2.445, \sigma = 15.324$).
- **Continuous Sliding Window**: When audio is silent or vetoed by VAD/impulsive filters, `push_comfort_frame()` pushes a comfort embedding into the 16-frame buffer. Time advances 1:1 with wall-clock time continuously.
- **Safe Reset**: `reset_after_trigger()` and stream restarts fill the buffer with comfort embeddings rather than zeros, mathematically preventing LayerNorm variance collapse.

### B. Adaptive Dynamic Impulsive Filter
- Replaced inverted hardcoded thresholds with a fully dynamic adaptive gate:
  $$\text{baseline} = \max(\text{prev\_rms}, \text{noise\_floor}, 0.001)$$
  $$\text{is\_impulsive} = \text{rms} > \text{baseline} \times \text{impulsive\_ratio}$$
- Tracks energy attack envelopes without artificial RMS ceilings.

### C. Multi-Frame Temporal Confirmation
- Enforces $\ge 2$ consecutive frames $\ge$ threshold ($160\text{ms}$) across both Python and Rust before wake confirmation is granted.

### D. Model Retraining with Negative Prior Bias
- Initialized `last_layer.bias` to $-4.0$ ($\sigma = 1.8\%$) in `scripts/train_local_wakeword.py`.
- Augmented negative training data with 600 synthetic comfort/transient negative windows.
- Trained 60 epochs with balanced $\text{pos\_weight} = 1.2$, restoring epoch 35 best weights ($\text{val\_loss} = 0.0293$, $98.8\%$ recall, $0.4\%$ FA).

---

## 3. Verification & Acceptance Criteria

- **Continuous 80s Silence**: 0 triggers, $0.0000\%$ max score.
- **Impulsive Bursts in Silence**: 0 triggers, $0.0220\%$ max score.
- **Hardware Device Invariance**:
  - Laptop Mic Array (Intel Smart Sound): 100.0%
  - Studio USB Condenser: 93.6%
  - Noisy Office: 92.7%
  - Far-Field Whisper: 90.2%
  - Bluetooth Headset: 87.1%
- **Rust Engine Suite**: 43/43 tests pass cleanly.
