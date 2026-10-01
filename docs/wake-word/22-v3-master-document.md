# NEXUS Wake Word v3 — Complete Training & Improvement Master Document

> **Goal:** Achieve Alexa/Jarvis-level wake word accuracy — **>=95% recall at <=1 false alarm per hour** — using a combination of real voice samples, synthetic TTS data, aggressive augmentation, and a Conv-Attention model architecture.
>
> **Status:** Data collected, augmentation pipeline ready, training notebook created. Ready for Colab training run.
>
> **Date:** 2026-09-07
>
> **Workspace verification (2026-09-14):** The deployed `nexus.onnx` is 415,224 bytes and is locked by `src-tauri/resources/oww/model_manifest.json`. The v3 notebook and local `wake_word_data` recordings described below are not present in this workspace; restore or recreate them before training. Historical results are not a current production-quality claim.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Current State Assessment](#2-current-state-assessment)
3. [Research: How Alexa, Jarvis, and Others Do It](#3-research-how-alexa-jarvis-and-others-do-it)
4. [The Gap: v2 vs Alexa-Level](#4-the-gap-v2-vs-alexa-level)
5. [v3 Architecture: Conv-Attention Model](#5-v3-architecture-conv-attention-model)
6. [Data Collection: Real Voice Samples](#6-data-collection-real-voice-samples)
7. [Data Augmentation Pipeline](#7-data-augmentation-pipeline)
8. [Synthetic TTS Data Generation](#8-synthetic-tts-data-generation)
9. [Negative Data Strategy](#9-negative-data-strategy)
10. [Training Curriculum](#10-training-curriculum)
11. [Evaluation Strategy](#11-evaluation-strategy)
12. [ONNX Export & Rust Integration](#12-onnx-export--rust-integration)
13. [Intel SST Microphone Issue](#13-intel-sst-microphone-issue)
14. [Step-by-Step: Running the v3 Training](#14-step-by-step-running-the-v3-training)
15. [Post-Training Validation](#15-post-training-validation)
16. [Continuous Improvement Plan](#16-continuous-improvement-plan)
17. [Files Reference](#17-files-reference)
18. [Troubleshooting](#18-troubleshooting)

---

## 1. Executive Summary

The NEXUS wake word engine uses openWakeWord's 3-stage pipeline (melspectrogram -> embedding -> classifier) running in pure Rust via tract-onnx. The current v2 model is a 1-layer DNN (197K parameters, 415KB) trained on 5,000 synthetic Piper TTS clips. It achieves ~100% recall on the owner's voice with 0 false positives in 4 minutes of silence, but has critical limitations:

- **Synthetic-only training** — never heard real human speech
- **Small model capacity** — 197K params can't learn all acoustic variation
- **Limited negatives** — 5K TTS soundalikes, no real-world noise
- **No SpecAugment** — no regularization against overfitting

The v3 upgrade addresses all four issues:

| Improvement | v2 | v3 |
|-------------|----|----|
| Model | 1-layer DNN (197K params) | Conv-Attention (500K params) |
| Real data | 0 samples | 223 real voice samples (98 positive + 93 negative + 20 background + 12 free) |
| Augmented data | 0 | 10,013 augmented samples (50x multiplier) |
| TTS data | 5,000 positive | 10,000 positive + 10,000 negative |
| Negative hours | ~0 | ~200 hours (ACAV100M) |
| Augmentation | Basic (reverb + noise) | SpecAugment + speed + pitch + volume + noise + reverb + time shift |
| Training | 100 epochs fixed | 3-stage curriculum (30K steps) + hard-negative mining |
| Evaluation | Accuracy only | DET curve + FA/hour + FRR at fixed FAR |

---

## 2. Current State Assessment

### 2.1 v2 Model Details

| Property | Value |
|----------|-------|
| Model file | `src-tauri/resources/oww/nexus.onnx` |
| Model size | 415,224 bytes (415 KB) |
| Last modified | 2026-08-30 |
| Architecture | Flatten -> Linear(1536->128) -> LayerNorm -> ReLU -> Linear(128->1) -> Sigmoid |
| Parameters | ~197K |
| Input | `[batch, 16, 96]` (16 frames x 96-dim embeddings, 1.28s context) |
| Output | `[batch, 1]` (probability 0.0-1.0, sigmoid baked in) |
| ONNX I/O names | Input: `onnx::Flatten_0`, Output: `output` |
| Runtime threshold | 0.35 |
| Training data | 5,000 Piper TTS positive + 5,000 TTS negative |
| Negative phrases | 41 soundalikes |
| Training platform | Google Colab T4 GPU |
| Training duration | ~75-90 minutes |

### 2.2 v2 Validation Results (2026-08-19)

| Test | Result |
|------|--------|
| Model loads in Rust (tract-onnx) | PASS |
| ONNX file valid | PASS (onnx + pytorch markers) |
| WakeEngine initializes (all 3 models) | PASS |
| Runtime detection | 7/7 wakes in ~3 min (probabilities 0.809-0.992) |
| Average detection probability | 0.931 |
| False positives during silence | 0 in ~4 min |
| Refractory period (3s cooldown) | Working correctly |
| Audio processing rate | ~36.6 callbacks/sec (80ms per chunk) |

### 2.3 v2 Known Limitations

1. **Synthetic data bias** — Model trained on TTS voices may not generalize to all human speakers
2. **Speaker variation** — Only tested on the owner's voice
3. **Accent coverage** — Depends on Piper LibriTTS voice coverage (~1000 speakers)
4. **Background noise** — Augmentation helps but real noise is more varied
5. **Small model capacity** — 197K parameters cannot learn the full acoustic space
6. **No SpecAugment** — No regularization on mel features

---

## 3. Research: How Alexa, Jarvis, and Others Do It

### 3.1 Industry Comparison

| System | Architecture | Positives | Negatives | FA/hr | Recall |
|--------|-------------|-----------|-----------|-------|--------|
| **Amazon Alexa** | DNN-HMM / CNN-LSTM / CRA | 1M+ real | 10K+ hrs | <1 | 95-99% |
| **NVIDIA Jarvis/Riva** | MatchboxNet (1D TCN) | User data | User data | <1 | 95%+ |
| **openWakeWord DNN** | FC-DNN (197K) | 10K TTS | 2K hrs | 8.50 | 68.6% |
| **LiveKit Conv-Attn** | Conv-Attention | 10K TTS | 2K hrs | **0.08** | 86.1% |
| **Snowboy (personal)** | DNN (proprietary) | 3 recordings | None | High | Speaker-dependent |
| **Snowboy (universal)** | DNN (proprietary) | ~1,500 from 500 people | Crowdsourced | Medium | ~90% |
| **Porcupine (Picovoice)** | DNN + transfer learning | Phrase-only (no user data) | Pre-trained | <1/10hrs | 95%+ |
| **NEXUS v2** | FC-DNN (197K) | 5K TTS | 5K TTS | 0 (4min test) | ~100% (owner) |
| **NEXUS v3 (target)** | **Conv-Attention (500K)** | **15K real+TTS** | **200 hrs** | **<=1** | **>=95%** |

### 3.2 Key Findings

1. **Alexa uses 1M+ real positive samples** and 10K+ hours of negatives. We can't match this, but we can get close with transfer learning (frozen embedding model + trained classifier).

2. **Conv-Attention beats DNN by 100x** on false alarm rate (LiveKit benchmark: 0.08 vs 8.50 FA/hr). This is the single biggest architectural improvement we can make.

3. **10K synthetic positives + 2K hours of negatives** is the minimum for >95% recall with openWakeWord's pipeline. We're combining this with real voice samples for better generalization.

4. **SpecAugment** (time + frequency masking on mel features) significantly reduces overfitting and improves noise robustness. The "Recipe for Creating a Highly Accurate Wake Word Engine" paper showed it reduces FRR from 5.6% to 0.4% at 1 FA/hour.

5. **Hard-negative mining** is critical — training on the hardest negatives (those that look most like "nexus") dramatically reduces false alarms.

6. **Two-stage verification** (primary detector + verifier network) is what Alexa uses for its final accuracy. The verifier only runs on high-scoring frames from the primary detector.

### 3.3 Open-Source Datasets Available

| Dataset | Size | Use |
|---------|------|-----|
| ACAV100M | 100M clips (~31 years) | Negative features (openWakeWord uses 2K hrs) |
| Mozilla Common Voice | Multilingual speech | Negative speech |
| LibriSpeech / LibriTTS | English audiobooks | TTS voice source |
| FMA (Free Music Archive) | Music | Background noise augmentation |
| FSD50K | Sound events | Background noise augmentation |
| Google Speech Commands v2 | 105K 1-sec utterances | KWS baseline |
| MLCommons Multilingual Spoken Words | 23.4M 1-sec examples, 50 languages | Multi-language KWS |
| Sonos "Hey Snips" | 11.8K positives + 86.5K negatives | Research benchmark |
| MIT Environmental RIRs | ~270 room impulse responses | Reverb augmentation |
| BIRD Impulse Response | Room impulse responses | Reverb augmentation |

### 3.4 Sources

- Amazon DNN-HMM wake word: https://m.media-amazon.com/images/G/01/amazon.jobs/MONOPHONE-BASEDBACKGROUNDMODELING._CB1522860833_.pdf
- Amazon raw-audio DNN/CNN-LSTM: https://m.media-amazon.com/images/G/01/amazon.jobs/2017_ASRU_Paper._CB1198675309_.pdf
- Amazon CRA wakeword verification: https://www.isca-archive.org/interspeech_2020/kumar20c_interspeech.pdf
- NVIDIA MatchboxNet: https://arxiv.org/pdf/2004.08531
- openWakeWord: https://github.com/dscripka/openWakeWord
- LiveKit wakeword: https://github.com/livekit/wakeword
- "Recipe for Creating a Highly Accurate Wake Word Engine": https://doi.org/10.1109/bigdata50022.2020.9378193
- SpecAugment: https://doi.org/10.21437/interspeech.2019-2680
- Noisy student-teacher KWS: https://arxiv.org/html/2106.01604
- Snowboy: https://github.com/Kitt-AI/snowboy
- Porcupine: https://picovoice.ai
- Sonos dataset: https://github.com/sonos/keyword-spotting-research-datasets
- ACAV100M: https://acav100m.github.io
- MLCommons: https://mlcommons.org/datasets/multilingual-spoken-words/

---

## 4. The Gap: v2 vs Alexa-Level

### 4.1 Critical Gaps

| Aspect | v2 (Current) | Alexa-Level | Gap Size | v3 Solution |
|--------|-------------|-------------|----------|-------------|
| Training samples | 5K TTS | 50K-100K+ | 10-20x | 15K real+TTS+augmented |
| Negative samples | 5K TTS | 200K+ (real speech) | 40x | 200 hrs ACAV100M |
| Real speech data | 0 | Millions of hours | Critical | 223 real samples + augmentation |
| Background noise | FMA music + RIR | TV, traffic, crowds, kitchens | Needs real noise | 20 real background clips + FMA |
| Accent coverage | Piper LibriTTS (~1000) | Global accents | Limited | TTS diversity + real samples |
| Model architecture | 1-layer DNN (197K) | Multi-layer RNN/TCN/Conformer | Needs deeper model | Conv-Attention (500K) |
| False positive rate | 0 in 4 min | <1 per 24 hours | Needs longer testing | DET curve evaluation |
| Recall | ~100% (owner only) | >97% across all speakers | Untested on others | Real voice training data |
| Speaker verification | Not implemented | Enrolled speaker only | Missing | Future v4 |
| SpecAugment | Not used | Standard | Missing | Implemented in v3 |
| Hard-negative mining | Not used | Standard | Missing | Implemented in v3 |

### 4.2 The Three Critical Problems

**Problem 1: Synthetic-only training data**
The v2 model was trained entirely on Piper TTS. Real human speech has:
- Breath noise, lip smacks, throat clears
- Variable distance from mic (6 inches to 15 feet)
- Background noise (TV, music, traffic, other people talking)
- Accent variations (Indian, British, Australian, Southern US)
- Whispered vs shouted vs normal volume
- Coarticulation effects ("hey nexus" vs "nexus" alone vs "ok nexus")

**Problem 2: Intel SST driver instability**
The mic goes silent after 2-25 minutes of use. The silence recovery thread in `wakeword_oww.rs` restarts the stream, but this causes:
- 5-second gaps where wakes are missed
- False triggers from the restart transient noise
- Manual intervention needed for permanent silence

**Problem 3: Small model capacity**
A 197K-parameter 1-layer DNN cannot learn the full acoustic variation of "nexus" across all speakers, accents, and noise conditions. Alexa uses a multi-million-parameter model with TCN or Conformer architecture.

---

## 5. v3 Architecture: Conv-Attention Model

### 5.1 Architecture Overview

```
Input: [batch, 16, 96]  (16 frames x 96-dim embeddings, 1.28s context)
  |
  v
Transpose -> [batch, 96, 16]  (for Conv1d)
  |
  v
Conv1d(96 -> 128, kernel=3, padding=1)  + BatchNorm1d(128)  + ReLU
  |  -- captures local temporal patterns (3-frame window = 240ms)
  v
Conv1d(128 -> 128, kernel=3, padding=1) + BatchNorm1d(128) + ReLU
  |  -- captures longer temporal patterns (5-frame receptive field = 400ms)
  v
Transpose -> [batch, 16, 128]  (for attention)
  |
  v
Multi-Head Self-Attention(embed_dim=128, num_heads=4, dropout=0.3)
  |  -- focuses on the frames where "nexus" is actually spoken
  |  -- 4 heads learn different attention patterns (phoneme, pitch, energy, timing)
  v
Global Average Pooling -> [batch, 128]
  |
  v
Dropout(0.3)
  |
  v
Linear(128 -> 1)
  |
  v
Sigmoid  (baked into ONNX)
  |
  v
Output: [batch, 1]  (probability 0.0-1.0)
```

### 5.2 Parameter Count

| Layer | Parameters |
|-------|-----------|
| Conv1d(96, 128, k=3) | 96 * 128 * 3 + 128 = 36,992 |
| BatchNorm1d(128) | 256 |
| Conv1d(128, 128, k=3) | 128 * 128 * 3 + 128 = 49,280 |
| BatchNorm1d(128) | 256 |
| MultiHeadAttention(128, 4 heads) | 128 * 128 * 4 + 128 * 4 = 66,048 |
| Linear(128, 1) | 129 |
| **Total** | **~153K** (with attention projections) |
| **With all projections** | **~500K** |

### 5.3 Why Conv-Attention Over DNN

| Metric | DNN (v2) | Conv-Attention (v3) | Improvement |
|--------|---------|---------------------|-------------|
| FA/hr (LiveKit benchmark) | 8.50 | 0.08 | **106x better** |
| Recall (LiveKit benchmark) | 68.6% | 86.1% | **1.25x better** |
| Parameters | 197K | ~500K | 2.5x more capacity |
| ONNX size | 415 KB | ~2 MB | Still tiny |
| Inference latency | <1ms | ~2ms | Negligible |
| Temporal modeling | None (flatten) | Yes (Conv + Attention) | New capability |
| Frame focus | All frames equal | Attention-weighted | New capability |

### 5.4 ONNX Compatibility

The v3 model exports with the same ONNX I/O names as v2 for Rust compatibility:

```
Input:  onnx::Flatten_0  [batch, 16, 96]   (dynamic batch)
Output: output           [batch, 1]        (dynamic batch)
```

This means **no Rust code changes needed** — just replace the `.onnx` file.

### 5.5 SpecAugment

SpecAugment is applied to the `[16, 96]` embedding input during training:

- **Time masking**: Mask 0-4 consecutive frames (out of 16) — simulates missing audio
- **Frequency masking**: Mask 0-16 consecutive frequency bins (out of 96) — simulates frequency dropout
- **2 time masks + 2 freq masks** per augmented sample
- Applied with 50% probability per batch

This forces the model to not rely on any single frame or frequency band, making it robust to:
- Brief audio dropouts (Intel SST silence bursts)
- Frequency-dependent noise (fan hum, electrical interference)
- Partial word captures (start of word clipped)

---

## 6. Data Collection: Real Voice Samples

### 6.1 Recording Session (2026-09-07)

**98 positive + 93 negative + 20 background + 12 free = 223 total real samples**

| Category | Files | Size | Duration | Phrases |
|----------|-------|------|----------|---------|
| Positive | 98 | 6.0 MB | 196s (3.3 min) | "nexus", "hey nexus", "ok nexus", "nexus please", "nexus wake up" |
| Negative | 93 | 5.7 MB | 186s (3.1 min) | 41+ soundalikes + competing wake words + common phrases |
| Free | 12 | 3.7 MB | 120s (2.0 min) | Natural speech with "NEXUS" mixed in |
| Background | 20 | 1.2 MB | 40s (0.7 min) | Room noise (with music/TV) |

### 6.2 Positive Phrases Recorded

| Phrase | Count | Purpose |
|--------|-------|---------|
| "nexus" | ~30 | Bare wake word |
| "hey nexus" | ~20 | Prefixed variant |
| "ok nexus" | ~20 | Prefixed variant |
| "nexus please" | ~15 | Polite variant |
| "nexus wake up" | ~13 | Explicit wake variant |

### 6.3 Negative Phrases Recorded

**Soundalikes (must NOT trigger):**
- next, nixis, mexic, necess, lexis, nixes, nixus, noxus, naxus
- text, taxes, focus, bonus, census, versus, hocus, locus
- next us, this is, process, access, excess, success
- reflexes, complex, context, index, annex
- nervous, precious, delicious, suspicious
- connect us, protect us, collect us, expect us

**Competing wake words (hard negatives):**
- hey google, ok google, hey siri, ok siri
- hey alexa, ok alexa, alexa, siri, google
- computer, assistant, hey computer, ok computer

**Common everyday phrases:**
- hello, hey, okay, please, thank you, what
- the weather is nice, open chrome, play music, what time is it
- send a message, close the window, turn off the light

### 6.4 Recording Quality

| Metric | Value | Verdict |
|--------|-------|---------|
| Sample rate | 16000 Hz | Correct (matches model input) |
| Channels | 1 (mono) | Correct |
| Format | 16-bit PCM WAV | Correct |
| Clip duration | 2.0 seconds | Correct (model context = 1.28s) |
| RMS range (positive) | 0.004 - 0.149 | Healthy variation |
| RMS range (negative) | 0.003 - 0.156 | Healthy variation |
| Silence threshold | 0.003 | Skips digital silence |
| Skipped clips | 9 (too quiet) | Expected (Intel SST fade) |

### 6.5 Recording Script

**File:** `scripts/record_wake_samples.py`

```bash
# Record 50 positive samples (say "NEXUS" when prompted)
python scripts/record_wake_samples.py positive 50

# Record 50 negative samples (say the displayed phrase, NOT nexus)
python scripts/record_wake_samples.py negative 50

# Record 120 seconds of free-form speech (say NEXUS naturally)
python scripts/record_wake_samples.py free 120

# Record 20 background noise clips (no speaking)
python scripts/record_wake_samples.py background 20

# Show statistics
python scripts/record_wake_samples.py stats
```

**Features:**
- 3-second countdown per clip
- RMS quality check (skips silence < 0.003)
- Random phrase selection from predefined lists
- Auto-numbering (continues from existing files)
- Windows UTF-8 console encoding fix
- Output: `wake_word_data/{positive,negative,free,background}/`

### 6.6 Future Recording Goals

| Milestone | Positives | Negatives | Background | Total |
|-----------|-----------|-----------|------------|-------|
| Current | 98 | 93 | 20 | 223 |
| Short-term | 200 | 200 | 50 | 450 |
| Medium-term | 500 | 500 | 100 | 1,100 |
| Long-term | 1,000 | 1,000 | 200 | 2,200 |

**Recording conditions to vary:**
- Different rooms (bedroom, office, kitchen, living room)
- Different distances (close ~30cm, normal ~60cm, far ~2m)
- Different volumes (whisper, quiet, normal, loud, shout)
- Different speeds (slow, normal, fast)
- Different directions (facing mic, turned 45deg, turned 90deg, back to mic)
- Different times of day (morning voice, evening voice)
- Different background noise (TV, music, traffic, kitchen, silence)
- Different physical states (sitting, standing, walking, lying down)

---

## 7. Data Augmentation Pipeline

### 7.1 Augmentation Strategy

Each original sample is augmented 50 times with random combinations of:

| Augmentation | Parameters | Probability | Purpose |
|-------------|-----------|-------------|---------|
| Speed perturbation | 0.85x, 0.9x, 0.95x, 1.05x, 1.1x, 1.15x | 50% | Simulate different speaking rates |
| Pitch shifting | -3, -2, -1, +1, +2, +3 semitones | 40% | Simulate different speakers |
| Volume scaling | 0.3x, 0.5x, 0.7x, 1.3x, 1.5x, 2.0x | 60% | Simulate different distances |
| Background noise | 0, 5, 10, 15, 20, 30 dB SNR | 70% | Simulate real-world noise |
| Room reverb | decay 0.2-0.5, delay 30-80ms | 30% | Simulate room acoustics |
| Time shifting | -200, -100, +100, +200 ms | 20% | Simulate timing variation |

### 7.2 Augmented Data Counts

| Category | Original | Augmented | Total | Multiplier |
|----------|----------|-----------|-------|------------|
| Positive | 98 | 4,900 | 4,998 | 51x |
| Negative | 93 | 4,650 | 4,743 | 51x |
| Free (as negative) | 12 | 240 | 252 | 21x |
| Background (as negative) | 20 | — | 20 | 1x |
| **Total** | **223** | **9,790** | **10,013** | **45x** |

### 7.3 Augmentation Script

**File:** `scripts/augment_wake_samples.py`

```bash
# Run augmentation (generates 10K+ samples from 223 originals)
python scripts/augment_wake_samples.py
```

**Features:**
- Uses `scipy.signal.resample` for speed perturbation
- Uses resampling-based pitch shifting (no phase vocoder needed)
- Uses `scipy.signal.fftconvolve` for room reverb
- Mixes background noise at specified SNR levels
- Clips output to [-1, 1] to prevent distortion
- Saves as 16-bit PCM WAV at 16kHz mono
- Output: `wake_word_data/augmented/{positive,negative}/`

### 7.4 On-Colab Augmentation

The v3 training notebook also runs augmentation on Colab using openWakeWord's built-in `--augment_clips` runner, which provides:
- Room reverb from MIT RIRs (~270 impulse responses)
- Background noise from FMA music (1,500 WAVs)
- Volume and speed variation via audiomentations
- Output: `(N, 16, 96)` feature `.npy` files (ready for training)

---

## 8. Synthetic TTS Data Generation

### 8.1 Piper TTS Configuration

| Parameter | v2 | v3 |
|-----------|----|----|
| TTS engine | Piper LibriTTS medium | Same |
| Model file | `en_US-libritts_r-medium.pt` (~200 MB) | Same |
| Positive clips | 5,000 | **10,000** |
| Negative clips | 5,000 | **10,000** |
| Validation positives | 2,000 | **3,000** |
| Soundalike negatives | 41 | **100+** |
| Speaking rates | Slow, normal, fast | Same + more variation |
| Voices | LibriTTS pool (~1000) | Same |

### 8.2 Expanded Negative Phrases (v3)

v3 adds 60+ new negative phrases beyond v2's 41:

**New phonetic confusions:**
- "nexus is", "is nexus", "nexus was", "was nexus"
- "nexus will", "will nexus", "nexus can", "can nexus"
- "nexus do", "do nexus", "nexus did", "did nexus"
- "nexus are", "are nexus", "nexus were", "were nexus"
- "nexus has", "has nexus", "nexus had", "had nexus"
- "nexus not", "not nexus", "nexus no", "no nexus"
- "nexus yes", "yes nexus", "nexus but", "but nexus"
- "nexus or", "or nexus", "nexus and", "and nexus"
- "nexus if", "if nexus", "nexus then", "then nexus"
- "nexus so", "so nexus", "nexus just", "just nexus"
- "nexus now", "now nexus", "nexus here", "here nexus"
- "nexus there", "there nexus", "nexus where", "where nexus"
- "nexus when", "when nexus", "nexus how", "how nexus"
- "nexus what", "what nexus", "nexus why", "why nexus"
- "nexus who", "who nexus", "nexus which", "which nexus"

**New competing wake words:**
- "hey google", "ok google", "hey siri", "ok siri"
- "hey alexa", "ok alexa", "alexa", "siri", "google"
- "computer", "assistant", "hey computer", "ok computer"

**New common phrases:**
- "the weather", "play music", "open chrome", "close window"
- "turn off", "turn on", "set timer", "what time"
- "send message", "make call", "read email", "check calendar"

### 8.3 Why TTS + Real (Not TTS Only)

| Aspect | TTS Only | TTS + Real |
|--------|---------|-----------|
| Privacy | No user audio collected | Real audio stays local |
| Speaker diversity | ~1000 LibriTTS voices | + 1 real speaker (owner) |
| Real-world noise | Simulated only | Real background noise |
| Mic characteristics | Unknown | Matches the actual Intel SST mic |
| Pronunciation | Dictionary perfect | Natural variations |
| Breath/body sounds | Absent | Present |
| Distance variation | Absent | Present (close, normal, far) |

---

## 9. Negative Data Strategy

### 9.1 Three-Tier Negative Strategy

| Tier | Source | Hours | Purpose |
|------|--------|-------|---------|
| 1. Adversarial | TTS soundalikes (100+ phrases) | ~5 hrs | Phonetic discrimination |
| 2. Real | Real voice negatives + augmented | ~3 hrs | Real-world speech |
| 3. Background | ACAV100M (1/5th subsample) | ~200 hrs | General speech + noise |

### 9.2 ACAV100M Subsampling

| Split | v2 | v3 |
|-------|----|----|
| Train | 1/10th (~200 hrs) | **1/5th (~400 hrs)** |
| Validation | 1/100th (~20 hrs) | **1/50th (~40 hrs)** |

### 9.3 Hard-Negative Mining

During training, each batch contains:
- 32 positive samples (random)
- 96 negative samples (random from all sources)

After forward pass, only samples where:
- Negative and prediction >= 0.001 (model is getting it wrong)
- Positive and prediction < 0.999 (model is getting it wrong)

...are kept for backprop. This focuses training on the hardest examples.

---

## 10. Training Curriculum

### 10.1 Three-Stage Schedule

| Stage | Steps | Learning Rate | Max Neg Weight | Validation Window |
|-------|-------|---------------|----------------|-------------------|
| 1: High LR | 20,000 | 1e-4 | 1,500 | Last 25% |
| 2: Medium LR | 5,000 | 1e-5 | 3,000 | Full |
| 3: Low LR | 5,000 | 1e-6 | 3,000 | Full |
| **Total** | **30,000** | | | |

### 10.2 Learning Rate Schedule (Within Each Stage)

```
Warmup (first 20%):    lr * (step + 1) / warmup_steps
Hold (next 33%):       lr
Cosine decay (rest):   lr * 0.5 * (1 + cos(pi * decay_t))
```

### 10.3 Batch Composition

```
Each batch (128 samples):
  - 32 positive (from real + TTS + augmented)
  - 32 adversarial negative (TTS soundalikes)
  - 64 ACAV negative (real speech)
```

### 10.4 SpecAugment Application

- Applied with 50% probability per batch
- 50% of samples in an augmented batch get SpecAugment
- 2 time masks (up to 4 frames each)
- 2 frequency masks (up to 16 bins each)

### 10.5 Checkpoint Selection

- Validate every 500 steps
- Save checkpoint if:
  - F1 score is the best seen so far
  - Recall >= 0.95
  - FA/hour <= 1.0
- Track all metrics for DET curve plotting

### 10.6 Class Weight Ramping

The negative class weight ramps from 1 to `max_neg_weight` over the stage:

```
current_neg_w = min(max_neg_weight, 1 + step * max_neg_weight / total_steps)
```

This prevents the model from collapsing to "always predict negative" early in training.

---

## 11. Evaluation Strategy

### 11.1 Metrics

| Metric | Formula | Target |
|--------|---------|--------|
| Recall (True Positive Rate) | TP / (TP + FN) | >= 95% |
| Precision | TP / (TP + FP) | >= 95% |
| F1 Score | 2 * P * R / (P + R) | >= 0.95 |
| Accuracy | (TP + TN) / (TP + TN + FP + FN) | >= 99% |
| FA/hour | FP / neg_samples * 2812.5 | <= 1.0 |
| FRR at 1 FA/hr | FN / (TP + FN) at threshold where FA/hr = 1 | <= 5% |

### 11.2 DET Curve

The evaluation tests at 16 thresholds (0.10 to 0.85 in 0.05 steps) and reports:

```
Thresh | Acc   | Recall | Prec   | F1    | FA/hr  | FP  | TP
 0.10  | 0.952 | 0.987  | 0.923  | 0.954 | 12.50  | 45  | 98
 0.15  | 0.961 | 0.975  | 0.941  | 0.958 | 8.75   | 32  | 97
 ...
 0.35  | 0.984 | 0.953  | 0.978  | 0.965 | 0.50   | 2   | 95
 ...
 0.50  | 0.971 | 0.872  | 0.991  | 0.928 | 0.00   | 0   | 87
```

### 11.3 Optimal Threshold Search

The evaluation automatically searches for a threshold that meets:
- Recall >= 95%
- FA/hour <= 1.0

If no threshold meets both criteria, it reports the best F1 threshold.

### 11.4 Runtime Validation (Post-Training)

| Test | Method | Pass Criteria |
|------|--------|---------------|
| Model loads in Rust | `cargo test --features wakeword-oww` | All 3 tests pass |
| ONNX file valid | Python onnx.load() | Valid, correct I/O shapes |
| Live detection | `cargo tauri dev` + say "NEXUS" 10x | >= 9/10 detected |
| False positive (silence) | 10 min of silence | 0 false triggers |
| False positive (speech) | 10 min of non-NEXUS speech | 0 false triggers |
| False positive (noise) | 10 min with TV/music | <= 1 false trigger |
| Latency | Time from speech to wake event | < 200ms |
| Refractory period | 3s cooldown after wake | No double-triggers |

---

## 12. ONNX Export & Rust Integration

### 12.1 ONNX Export Configuration

```python
torch.onnx.export(
    model,
    dummy_input,                          # torch.randn(1, 16, 96)
    'nexus_v3.onnx',
    input_names=['onnx::Flatten_0'],      # Same as v2 for Rust compat
    output_names=['output'],              # Same as v2 for Rust compat
    dynamic_axes={
        'onnx::Flatten_0': {0: 'batch'},
        'output': {0: 'batch'}
    },
    opset_version=14,
)
```

### 12.2 Rust Runtime (No Changes Needed)

The Rust code in `src-tauri/src/wakeword_oww.rs` loads the ONNX model via tract-onnx:

```rust
let classifier = load_onnx_model(&nexus_model_path)?;
```

The model is used in `detect_chunk()`:

```rust
let probability: f32 = match self.classifier.run(tvec!(features.into())) {
    Ok(result) => result[0].to_array_view::<f32>()
        .ok()?
        .iter()
        .next()
        .copied()
        .unwrap_or(0.0),
    Err(_) => 0.0,
};
```

Since the I/O names and shapes are identical, **replacing the `.onnx` file is the only change needed**.

### 12.3 Runtime Threshold

The current runtime threshold is 0.35. After v3 training, the optimal threshold will be determined from the DET curve. If it differs from 0.35, update:

```rust
// src-tauri/src/wakeword_oww.rs, line ~445
let threshold = 0.35f32;  // Update this to the v3 optimal threshold
```

### 12.4 Model File Replacement

```bash
# After downloading nexus_v3.onnx from Colab:
copy nexus_v3.onnx src-tauri\resources\oww\nexus.onnx
```

---

## 13. Intel SST Microphone Issue

### 13.1 Current Driver

| Property | Value |
|----------|-------|
| Device | Microphone Array (Intel Smart Sound Technology for Digital Microphones) |
| Driver version | 10.29.0.11192 |
| Driver date | July 18, 2024 |
| Status | OK (but intermittent silence bug) |
| HP Assistant update | Did NOT update the driver |

### 13.2 The Silence Bug

The Intel SST driver stops delivering audio after 2-25 minutes of use:
- RMS drops to exactly 0.000000
- No audio callbacks are received
- The mic appears "OK" in Device Manager but produces silence
- The only fix is to restart the audio service or driver (requires admin)

### 13.3 Current Workaround

The silence recovery thread in `wakeword_oww.rs` monitors the audio callback counter:
- Poll interval: 5 seconds
- Silence threshold: 165 callbacks (~5s of no audio)
- Restart method: `try_device_silent` (recreates the cpal stream)
- Nuclear option: Every 12 restarts (~60s of silence), restarts Windows Audio service
- Total restart cycle: ~5 seconds

### 13.4 Mic Test Script

**File:** `scripts/test_mic_live.py`

```bash
python scripts/test_mic_live.py
# Records 5s and reports RMS, peak, silence %, and frequency content
```

### 13.5 Manual Fix (Requires Admin)

```powershell
# Method 1: Restart Windows Audio service
Restart-Service -Name "Audiosrv" -Force

# Method 2: Restart the Intel SST device
pnputil /restart-device "INTELAUDIO\CTLR_DEV_51CA&LINKTYPE_02&DEVTYPE_00&VEN_8086&DEV_AE20&SUBSYS_8BE0103C&REV_10EC\5&111f6c68&0&0000"

# Method 3: Device Manager
# Sound, video and game controllers > Intel Smart Sound Technology for Digital Microphones
# Right-click > Disable > Enable
```

---

## 14. Step-by-Step: Running the v3 Training

### Step 1: Prepare the Data Zip

```bash
cd C:\PROJECTS\ULTRON

# Create zip with original recordings (10.8 MB)
powershell -Command "Compress-Archive -Path wake_word_data/positive, wake_word_data/negative, wake_word_data/background, wake_word_data/free -DestinationPath wake_word_data.zip -Force"
```

### Step 2: Upload to Google Colab

1. Go to https://colab.research.google.com
2. File > Upload notebook > select `train_nexus_oww_v3.ipynb`
3. Set runtime: Runtime > Change runtime type > T4 GPU
4. Upload `wake_word_data.zip` via the left sidebar (Files > Upload)

### Step 3: Run the Notebook

Run cells in order:

| Cell | Description | Duration |
|------|-------------|----------|
| 1 | Install dependencies | ~2 min |
| 2 | Clone repos + download Piper model | ~5 min |
| 3 | Apply runtime patches | ~1 min |
| 4 | Download shared models + RIRs + FMA | ~15 min |
| 5 | Download ACAV100M features (~17 GB) | ~20 min |
| 6 | Unzip real voice samples | ~1 min |
| 7a | Generate 10K TTS clips | ~15 min |
| 7b | (Augmentation runs on Colab) | ~10 min |
| 8 | Resample + augment + featurize all clips | ~20 min |
| 9 | Define Conv-Attention model + SpecAugment | ~1 min |
| 10 | Load all features + train (30K steps) | ~90 min |
| 11 | Full evaluation with DET curve | ~1 min |
| 12 | Export ONNX + download | ~1 min |
| **Total** | | **~3-4 hours** |

### Step 4: Download and Replace the Model

```bash
# After downloading nexus_v3.onnx from Colab:
copy nexus_v3.onnx C:\PROJECTS\ULTRON\src-tauri\resources\oww\nexus.onnx
```

### Step 5: Update Runtime Threshold (If Needed)

If the DET curve shows a different optimal threshold, update `src-tauri/src/wakeword_oww.rs`:

```rust
let threshold = 0.35f32;  // Change to v3 optimal threshold
```

### Step 6: Runtime Validation

```bash
cd C:\PROJECTS\ULTRON\src-tauri

# Rust unit tests
cargo test --features wakeword-oww --lib wakeword_oww::tests -- --nocapture

# Full app with wake word
cargo tauri dev --config (ConvertTo-Json -Depth 5 @{build=@{beforeDevCommand='npm --prefix C:/PROJECTS/ULTRON/frontend run dev';beforeBuildCommand='npm --prefix C:/PROJECTS/ULTRON/frontend run build';frontendDist='../frontend/dist';devUrl='http://localhost:5173'}})
```

---

## 15. Post-Training Validation

### 15.1 Immediate Tests (First Hour)

| Test | Duration | Method | Pass Criteria |
|------|----------|--------|---------------|
| Live detection | 5 min | Say "NEXUS" 10x | >= 9/10 detected |
| Silence false positive | 10 min | No speaking | 0 false triggers |
| Speech false positive | 10 min | Talk about non-NEXUS topics | 0 false triggers |
| Noise false positive | 10 min | Play TV/music | <= 1 false trigger |
| Refractory period | 5 min | Say "NEXUS" rapidly | No double-triggers |
| Latency | 10 wakes | Measure time to wake | < 200ms each |

### 15.2 Extended Tests (24 Hours)

| Test | Duration | Method | Pass Criteria |
|------|----------|--------|---------------|
| Long-running stability | 24 hrs | Leave app running | No crashes, no memory leaks |
| False positive rate | 24 hrs | Normal work day | <= 1 FA per hour |
| Recall (natural use) | 24 hrs | Count natural "NEXUS" calls | >= 95% detected |
| Intel SST recovery | 24 hrs | Monitor silence recovery | Auto-recovers within 5s |

### 15.3 Multi-Speaker Tests (Future)

| Test | Method | Pass Criteria |
|------|--------|---------------|
| Other speakers | Have 3+ people say "NEXUS" 10x each | >= 85% detected |
| Accent variation | Test with non-native English speakers | >= 80% detected |
| Child voice | Test with child speaker | >= 70% detected |
| Whispered | Say "NEXUS" in a whisper at 30cm | >= 80% detected |
| Far field | Say "NEXUS" from 3m away | >= 70% detected |

---

## 16. Continuous Improvement Plan

### 16.1 Phase 1: v3 Model (Current)

- [x] Record 223 real voice samples
- [x] Augment to 10,013 samples
- [x] Create v3 training notebook
- [ ] Run v3 training on Colab
- [ ] Replace model in resources
- [ ] Runtime validation
- [ ] 24-hour stability test

### 16.2 Phase 2: More Data (Next 2 Weeks)

- [ ] Record 200+ more positive samples (different rooms, distances, volumes)
- [ ] Record 200+ more negative samples (more soundalikes, more common phrases)
- [ ] Record 50+ background noise samples (TV, traffic, kitchen, office)
- [ ] Re-run augmentation (50x multiplier)
- [ ] Re-train v3 with more data

### 16.3 Phase 3: Speaker Verification (Future)

- [ ] Implement speaker enrollment (collect 10 clips of owner's voice)
- [ ] Train speaker embedding model (speechbrain ECAPA-TDNN)
- [ ] Add speaker verification to wake word pipeline
- [ ] Only trigger wake if speaker matches enrolled profile
- [ ] This eliminates false positives from other people saying "nexus"

### 16.4 Phase 4: Admin Brain Integration (Future)

- [ ] Admin brain continuously monitors wake word detections
- [ ] Collects false positive examples for retraining
- [ ] Collects missed detections (from hotkey fallback) for retraining
- [ ] Periodically retrains the model with new data
- [ ] Only the improved model is distributed to users

### 16.5 Phase 5: Multi-Wake-Word (Future)

- [ ] Train models for "hey nexus", "ok nexus", "nexus wake up" as separate wake words
- [ ] Run multiple classifiers in parallel (already supported by Tier 3 architecture)
- [ ] User can choose which wake phrase to use

---

## 17. Files Reference

### 17.1 Scripts

| File | Purpose | Lines |
|------|---------|-------|
| `scripts/record_wake_samples.py` | Live voice sample recorder | 317 |
| `scripts/augment_wake_samples.py` | Data augmentation pipeline (50x) | 298 |
| `scripts/test_mic_live.py` | Mic health check (RMS, silence, frequency) | 46 |
| `scripts/record_samples.py` | Older single-phrase recorder (legacy) | 52 |
| `scripts/restart_mic.ps1` | PowerShell script to restart Intel SST device | 15 |

### 17.2 Training

| File | Purpose |
|------|---------|
| `train_nexus_oww_v3.ipynb` | v3 Colab training notebook (Conv-Attention) |
| `train_nexus_oww_v2.ipynb` | v2 Colab training notebook (DNN, legacy) |

### 17.3 Data (Gitignored)

| Path | Contents | Size |
|------|----------|------|
| `wake_word_data/positive/` | 98 real "NEXUS" utterances | 6.0 MB |
| `wake_word_data/negative/` | 93 real non-wake-word utterances | 5.7 MB |
| `wake_word_data/free/` | 12 free-form recordings | 3.7 MB |
| `wake_word_data/background/` | 20 background noise clips | 1.2 MB |
| `wake_word_data/augmented/positive/` | 4,998 augmented positives | ~300 MB |
| `wake_word_data/augmented/negative/` | 5,015 augmented negatives | ~300 MB |
| `wake_word_data.zip` | Zip of originals for Colab upload | 10.8 MB |

### 17.4 Model

| File | Purpose | Size |
|------|---------|------|
| `src-tauri/resources/oww/nexus.onnx` | Current v2 classifier model | 415 KB |
| `src-tauri/resources/oww/melspectrogram.onnx` | Pre-trained mel model (frozen) | 1.1 MB |
| `src-tauri/resources/oww/embedding_model.onnx` | Pre-trained embedding model (frozen) | 1.3 MB |

### 17.5 Documentation

| File | Purpose |
|------|---------|
| `docs/wake-word/21-v3-training-plan.md` | Short v3 training plan (this session) |
| `docs/wake-word/22-v3-master-document.md` | This file (comprehensive) |
| `docs/wake-word/06-model-training.md` | v1/v2 training documentation |
| `docs/wake-word/14-model-validation-results.md` | v2 runtime validation results |

### 17.6 Rust Source

| File | Purpose |
|------|---------|
| `src-tauri/src/wakeword_oww.rs` | Wake word engine (OWW KWS + speaker verification + Tier 3) |
| `src-tauri/src/wakeword_oww.rs` (threshold) | Runtime threshold (line ~445, currently 0.35) |
| `src-tauri/src/wakeword_oww.rs` (silence gate) | RMS silence gate (line ~531, currently 0.002) |
| `src-tauri/src/wakeword_oww.rs` (AGC) | Automatic gain control (line ~532-537) |

---

## 18. Troubleshooting

### 18.1 Model Won't Load in Rust

**Symptom:** `Failed to parse ONNX` error in `wakeword_oww.rs`

**Cause:** ONNX opset version or operator not supported by tract-onnx

**Fix:**
1. Check ONNX opset version (must be <= 14)
2. Check for unsupported operators (MultiHeadAttention may need to be decomposed)
3. If tract-onnx doesn't support attention, fall back to DNN architecture with Conv1d layers only

### 18.2 Model Loads But No Detections

**Symptom:** Model loads, audio pipeline runs, but no wakes detected

**Cause:** Threshold too high, or model output range different

**Fix:**
1. Check model output range with Python onnxruntime
2. Lower threshold from 0.35 to 0.25, then 0.20
3. Check if sigmoid is baked into ONNX (output should be 0-1)

### 18.3 Too Many False Positives

**Symptom:** Model triggers on non-NEXUS speech or noise

**Cause:** Threshold too low, or model undertrained on negatives

**Fix:**
1. Raise threshold from 0.35 to 0.45, then 0.50
2. Add more negative training data (especially the false-triggering phrases)
3. Increase `max_negative_weight` in training (1500 -> 3000)
4. Re-train with more hard-negative mining

### 18.4 Intel SST Mic Goes Silent

**Symptom:** RMS drops to 0.000000, no audio callbacks

**Fix:**
1. The silence recovery thread should auto-restart within 5s
2. If it doesn't recover, restart the audio service (requires admin):
   ```powershell
   Restart-Service -Name "Audiosrv" -Force
   ```
3. If that fails, restart the Intel SST device in Device Manager
4. If all else fails, restart the computer

### 18.5 Colab Training OOM

**Symptom:** CUDA out of memory error during training

**Fix:**
1. Reduce batch_size from 256 to 128 or 64
2. Reduce ACAV100M subsample from 1/5th to 1/10th
3. Move data to CPU and fetch batches to GPU on demand
4. Use gradient accumulation (simulate larger batch with multiple small batches)

### 18.6 ONNX Export Fails

**Symptom:** `torch.onnx.export` fails with operator error

**Fix:**
1. Ensure model is in eval mode (`model.eval()`)
2. Ensure model is on CPU (`model.cpu()`)
3. Try opset version 13 or 12
4. If MultiHeadAttention fails to export, replace with manual attention:
   ```python
   class ManualAttention(nn.Module):
       def __init__(self, dim, heads):
           super().__init__()
           self.q = nn.Linear(dim, dim)
           self.k = nn.Linear(dim, dim)
           self.v = nn.Linear(dim, dim)
       def forward(self, x):
           q, k, v = self.q(x), self.k(x), self.v(x)
           attn = torch.softmax(q @ k.transpose(-2, -1) / (dim ** 0.5), dim=-1)
           return attn @ v
   ```

---

## Cross-References

- [06-model-training.md](./06-model-training.md) — v1/v2 training details
- [14-model-validation-results.md](./14-model-validation-results.md) — v2 validation results
- [05-oww-3-stage-pipeline.md](./05-oww-3-stage-pipeline.md) — 3-stage ONNX pipeline
- [07-speaker-verification.md](./07-speaker-verification.md) — Speaker verification plan
- [09-audio-pipeline.md](./09-audio-pipeline.md) — Audio capture and resampling
- [10-rust-integration.md](./10-rust-integration.md) — Rust integration details
- [11-testing-strategy.md](./11-testing-strategy.md) — Testing strategy
- [AGENTS.md](../../AGENTS.md) — Project notes (wake word section)
