# Voice Stack State of the Art — Wake Word, VAD, STT, TTS

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** Wake word / VAD / STT / TTS state of the art with concrete upgrade paths and measured numbers.


**Date:** 2026-10-02
**Scope:** Whether our current choices are still current. Concrete upgrade paths
with measured numbers, licenses, and migration notes.

**Summary:** Our VAD and STT choices are good. Our wake-word *head* is quietly
outdated. Our TTS model is fine but upstream is dead.

---

## 0. Repo maintenance status (verified via GitHub API)

| Repo | Stars | Last push | Latest release | Verdict |
|---|---|---|---|---|
| `dscripka/openWakeWord` | 2,805 | **2025-12-30** (9 mo) | v0.6.0 (2024-02-11) | Effectively dormant |
| `moonshine-ai/moonshine` | 11,167 | 2026-10-01 | v0.1.5 (2026-08-24) | Very active |
| `snakers4/silero-vad` | 10,338 | 2026-09-29 | v6.2.3 (2026-09-23) | Very active |
| `hexgrad/kokoro` | 9,108 | **2025-08-06** (14 mo) | none | Effectively dormant |
| `ggml-org/whisper.cpp` | 54,078 | 2026-09-28 | v1.9.3 (2026-08-20) | Very active |
| `Picovoice/porcupine` | 4,944 | 2026-10-01 | v4.0.2 (2026-02-13) | Active |
| `k2-fsa/sherpa-onnx` | 15,072 | 2026-09-22 | — | Very active |
| `mudler/parakeet.cpp` | 796 | 2026-10-01 | — | Active |
| `livekit/livekit-wakeword` | 278 | 2026 (new, ~Feb) | crate v0.1.3 (2026-09-11) | Early but the best architecture |

---

## 1. Wake word — the highest-leverage change in this document

### 1.1 🔴 `livekit-wakeword` replaces the classifier head

**Apache-2.0** code. Rust crate `livekit-wakeword` **v0.1.3**, published 2026-09-11.

It keeps openWakeWord's audio front-end **verbatim** (frozen Google
speech-embedding + openWakeWord mel/embedding ONNX → `(16, 96)` matrix) and only
replaces the classifier head:

```
openWakeWord:        Flatten(16×96=1536) → Dense → Dense → Sigmoid
livekit-wakeword:    Conv1D(k=3) → MultiheadAttention → MeanPool → Linear(1) → Sigmoid
```

Measured on their "hey livekit" validation set (15,000 positives, 45,084
negatives, 25 h audio):

| Metric | openWakeWord DNN | livekit DNN | livekit **conv-attention** |
|---|---|---|---|
| AUT (↓) | 0.0720 | 0.0423 | **0.0012** |
| FPPH (↓) | 8.50 | 3.07 | **0.08** |
| Recall (↑) | 68.6% | 85.3% | **86.1%** |
| Optimal threshold | 0.01 | 0.01 | **0.68** |

### 🔴 Read the last row carefully

**Our threshold is already 0.68.** That is a remarkable convergence — our existing
calibration is not a quirk of our custom training, it is the operating point the
field settles on when training a *temporally-aware* head. openWakeWord's flat DNN
can only reach 0.08 FPPH at a threshold of 0.01; conv-attention reaches it at 0.68
with 17% higher recall.

**Why this targets our exact failure class.** Our `AGENTS.md` documents four
2026-09-23 fixes: speech-onset decapitation, dual-buffer squeezebox, AGC pre-gain
overdrive, LayerNorm zero-reset collapse — plus a 3,300-file benchmark proving
0.2% FA. Those are all symptoms of a model that learns *presence* of phonetic
content rather than *order*. LiveKit's stated mechanism for FA reduction is
exactly temporal structure: *"reducing false triggers from phonetically similar
but differently ordered phrases."* Our soundalike negatives (`hello`, `next`,
`neckus`) and vocal-friction negatives (throat clearing, gargle, cough) are the
same class.

**Caveats, honestly:**
- The benchmark is **vendor self-reported**, on their own phrase. **No independent
  reproduction exists yet.** Treat the 60×/100× as directional.
- Our 3,000+ commits show the flat DNN head has a real capacity ceiling. Don't
  expect to fully close remaining negatives with the current head.
- **Multilingual numbers are explicitly worse** ("Multilingual models currently
  achieve lower accuracy than English models") because the frozen Google
  speech-embedding is English-dominant. **Multilingual is out of scope for us, so
  this caveat does not apply.**

**Migration:** our `wakeword_oww.rs` already does mel → embedding → 16-frame
buffer → classifier in tract-onnx. Swap the classifier ONNX and its input shape
(`[1,16,96]` → `[1,16,96]` conv+attn, likely unchanged), plus retrain. The crate
does the whole front-end if we'd rather delete `melspectrogram.onnx` handling.

### 1.2 openWakeWord's training pipeline is broken on modern Python

Confirmed by maintainers, not inferred —
`dscripka/openWakeWord#317` (2026-02-04). A user reverse-engineered a working
pipeline; the maintainer replied:
> *"no PRs, because the project has merged exactly 0 community PRs in the past 18
> months, has 10 open community PRs (some over 2 years old), and 90 open issues.
> **The training pipeline has been broken on modern systems since mid-2024.**"*

Third-party confirmation (`alfiedennen/openwakeword-colab-2026`) documents **8
distinct breakages** on Python 3.12: no `piper-phonemize` wheels for 3.12,
`torchaudio` 2.x removed `set_audio_backend`, YAML config keys moved to
`KeyError`, `piper-sample-generator` layout changed. Their fix replaces
`auto_train` with a hand-rolled PyTorch loop.

→ **Our inference stack is fine and will keep working. Our retraining path is on
life support** — which is exactly what our training pipeline depends on.

### 1.3 Porcupine — works, but commercially worse

v4.0.0 (2025-12-11) added GPU/multi-core, dropped Unity. Current v4.0.2,
2026-02-13. Self-reported: 97.3% acceptance at 1 FA/10 h, 10 dB SNR, **0.6% CPU
on Raspberry Pi 5**.

⚠️ **The free tier was reportedly deprecated 2026-06-30**, replaced by 7-day
trial → paid. **Source is a competitor's blog (VoxRT), not Picovoice's own
announcement**; Picovoice's pricing page 404s. Treat as likely-but-unverified.

### 1.4 Licensing note on weights

**openWakeWord code is Apache-2.0 but pretrained weights are CC-BY-NC-SA 4.0
(non-commercial).** We sidestepped this by training our own. **livekit-wakeword
is Apache-2.0 all the way through** — a genuine improvement.

### 1.5 Dead ends (for the record)

| Engine | Status |
|---|---|
| Mycroft Precise | Dormant since 2019 |
| Snowboy (KITT.AI) | **Archived 2020** |
| PocketSphinx | 48.0% acceptance |
| Spokestack | **Shut down 2022**, all repos archived |
| microWakeWord | Maintained but ESP32-S3 microcontroller class — wrong power band |
| sherpa-onnx KWS | Zipformer transducer keyword *spotting*, not a general trainer |

---

## 2. VAD — one config flag from current

### 2.1 🔴 We are one release line behind

Silero latest: **v6.2.3, released 2026-09-23**. Repo pushed 2026-09-29, only
**13 open issues**.

- **v6.0** (2025-08-26): **16% fewer errors on noisy real-life data**, 11% fewer
  on multi-domain validation, changed training algorithm, 6000+ languages.
- **v6.2** (2025-11-06): fixed **child voices, cartoon voices, muted voices,
  muted speech, low-quality phone calls**.

**And `@ricky0123/vad-web` already ships `silero_vad_v6.onnx`.** From the package
docs:
> `model`: `"legacy"` (default), `"v5"` (512 samples), `"v6"` (512 samples).
> *"Using `"v6"` is recommended."*

We ship **Silero v5** in `frontend/public/`. One release line behind, and the fix
is already in the package we depend on.

⚠️ **Threshold caveat:** switching model versions **invalidates** tuned
`positiveSpeechThreshold` / `redemptionMs` / `speechSeconds`. Budget an hour to
re-tune.

Our package is current: `@ricky0123/vad-web` **0.0.31**, published 2026-09-12.
(`vad-node` was discontinued Oct 2024 — browser-only, irrelevant.)

### 2.2 Architecture unchanged

v5/v6 is still `STFT → 4× Conv1d+ReLU → LSTM(128) → Conv1d → Sigmoid`. ~2 MB
JIT. <1 ms per 30+ ms chunk single-threaded. **There is no successor to Silero.**

### 2.3 Optional: pure-Rust v6

- **`silero-vad-pure`** (crates.io) — pure-Rust v6.2 reimplementation, ~1400
  lines, weights embedded at compile time (~2.2 MB), no ONNX/PyTorch. Claims
  **3–6× faster than official runtimes** single-threaded (Apple M5 Pro),
  ~1191× real-time at 16 kHz even on the portable fallback. Validated
  bit-faithful (max prob error ≤1.2e-7 vs onnxruntime).
  ⚠️ **Tiny project, no adoption, no independent benchmark.**
- `nipponjo/silero-vad-c` — standalone C port with AVX2/NEON SIMD, 4 stars.

### 2.4 Alternatives assessed — all downgrades

| Option | Verdict |
|---|---|
| Picovoice Cobra VAD | Claims 99%, commercial, closed. Same vendor that reportedly killed its wake-word free tier. |
| **NOVA-VAD** | Claims 99.79% vs Silero 94.87%. **We do not trust this.** 7 stars, single author, pure scikit-learn MFCC ensemble. Its own README concedes the headline *reversed* between rounds, and its own model sat at 94.0% before tuning. Different task framing too (file-level noise classification, not frame-level streaming segmentation). |
| TenVAD | Silero v6.0 release notes include a direct comparison. Not a drop-in winner. |
| Word-level timestamps | Still no free, local, accurate word-level aligner that beats forced alignment. WhisperX is the usual answer and it is heavy. |

---

## 3. Streaming STT — already right

### 3.1 Moonshine v2 published a peer-reviewed paper and a new architecture

**arXiv:2602.12241**, "Moonshine v2: Ergodic Streaming Encoder ASR for
Latency-Critical Speech Applications," Kudlur et al., **2026-02-12**.

Architecture change: full-attention encoder → **ergodic streaming encoder** with
sliding-window self-attention, position-free. First and last two encoder layers
use window **(16 past, 4 future) = 80 ms algorithmic lookahead**; middle layers
strictly causal. Result: **TTFT is bounded and constant regardless of utterance
length.** The old full-attention Moonshine's TTFT grew linearly.

Empirical response latency (end-of-utterance → transcript, **MacBook M3**):

| Model | Latency | Compute load | Params | Avg WER |
|---|---|---|---|---|
| Moonshine Tiny (v1) | 27 ms | 5.91% | 27.1M | 12.65% |
| Moonshine Base (v1) | 44 ms | 7.34% | 61.5M | 9.99% |
| Moonshine v2 Tiny | 50 ms | 8.03% | — | — |
| **Moonshine v2 Small** | **148 ms** | 17.97% | 123M | ~7.8% |
| **Moonshine v2 Medium** | **258 ms** | 28.95% | 245M | **6.65%** |
| Whisper Tiny | 289 ms | 8.46% | 39M | 12.81% |
| Whisper Small | 1,940 ms | 56.84% | 244M | 8.59% |
| Whisper Large v3 | 11,286 ms | 330.65% | 1550M | 7.44% |

**Moonshine v2 Medium (245M) beats Whisper Large v3 (1.55B) on accuracy at 1/6 the
parameters and 43.7× lower latency.** Our default is `medium_streaming`.

🔴 **Action: verify we have the v2 sliding-window checkpoint (released ~2026-02),
not v1.** Everything else is already correct.

⚠️ **Moonshine v2 is English-only as of March 2026.** Japanese models remain on
v1. Multilingual is out of scope for us, so no impact.

Independent multi-domain WER check: Moonshine v2 medium ≈ **6.7%**, Parakeet TDT
0.6B ≈ **6.3%**, Canary-Qwen 2.5B ≈ **5.6%**, Whisper large-v3 ≈ **7.4%**.

Another independent benchmark (Open ASR, 8 English test sets): Moonshine
streaming-medium **6.66%**, and notes the inversion that matters — **on AMI
(multi-speaker meeting audio) Moonshine medium is 10.68%, the best of all models
tested**, ahead of Voxtral Mini 3B (16.30%) and Whisper large-v3 (15.95%).

### 3.2 Parakeet is now inside whisper.cpp — this changes the landscape

**whisper.cpp v1.9.0 (2026-06-17)** merged PR #3735 *"parakeet: add support for
NVIDIA Parakeet"* — **+8,733 lines / 38 files.** Adds `--enable-parakeet`,
`parakeet-cli`, conversion and quantize tooling. Model:
`ggml-parakeet-tdt-0.6b-v3.bin`.

Standalone `parakeet.cpp` (originally aicatalyst-team, archived) lives on as
**`mudler/parakeet.cpp`**.

Self-reported benchmarks (20-core x86, 8 threads, vs NeMo 2.7.3 PyTorch CPU):
**byte-identical transcripts** (agreement WER 0.0000% on 5 of 10 models), median
**1.40× faster than NeMo** on CPU, **~27× faster than whisper.cpp turbo on CPU**
at equal accuracy. RSS roughly halved.

**The killer architectural property:** Parakeet is FastConformer + Token-and-Duration
**Transducer**, not an encoder-decoder. It predicts token *durations* and skips
ahead — no per-token autoregressive cost. **Transducers emit nothing on silence**,
so Whisper's entire `[BLANK_AUDIO]` / "thanks for watching" hallucination class
disappears at the source.

Measured desktop CPU (i5-1035G1 4C/8T, 11 s clip): **Parakeet TDT v2 = 1,239 ms
(RTF 0.113)** vs Moonshine Base 534 ms, Moonshine Tiny 435 ms, Whisper Small
21,260 ms.

Quantization: q8_0 = **37% of f32 size, up to 1.86× speedup, near-lossless**.
NVIDIA's card says ≥2 GB RAM; the **110M** variant runs 563 MB RSS.

Also: NVIDIA ships `nemotron-3.5-asr-streaming-0.6b`, **multilingual (40+
locales), streaming, 0 agreement WER vs NeMo, 2.40× NeMo on CPU at f32**. Not
needed (multilingual out of scope).

**Recommendation:** keep Moonshine v2 medium as the streaming primary. **Add a
Parakeet tier for hard audio** (noisy, accented, far-field) — no Python, ~600 MB
RSS, and it would let us retire the hand-rolled hallucination filter in `stt.rs`.

### 3.3 sherpa-onnx — the true-streaming alternative nobody mentions

Zipformer transducers are the mature *true* streaming answer: **RTF 0.04** on a
single thread (`sherpa-onnx-streaming-zipformer-bn-vosk-2026-02-09`: 7 s audio in
0.28 s, 1 thread). Models as small as **20M English** (~46 MB int8). arXiv
2506.14434 shows right-context training closes the streaming/offline WER gap by
~7.9%.

Desktop x86: Zipformer 20M streaming = 1,775 ms for 11 s (RTF 0.161) — **slower
than Moonshine for short utterances**, but it emits genuine partials.

**Honest assessment:** technically better streaming architecture, loses the
latency benchmark for our 1–5 s commands. Keep as a future option.

### 3.4 Distil-Whisper / faster-whisper status

- faster-whisper: 25,666 stars, pushed 2026-10-01. Very active. Fine.
- **Distil-Whisper Large v3** (756M, 5–6× faster, English-only, ~5 GB): superseded
  for English by Parakeet TDT 0.6B v3 on every axis. **Would not adopt in 2026.**
- whisper.cpp v1.9.3 (2026-08-20), flash attention default since v1.8.0, native VAD
  since v1.7.6. Best *runtime* for Whisper models, but Whisper models are the
  wrong choice for us.

---

## 4. TTS

### 4.1 Kokoro is still the right default, but upstream is dead

`hexgrad/kokoro`: last push **2025-08-06**, 212 open issues, no GitHub releases.
OpenSSF Scorecard: **unmaintained**. Health index 38/100, bus factor 1, 168
unanswered issues.

**Pin the community fork instead:** **`thewh1teagle/kokoro-onnx`** — 2,680 stars,
MIT, PyPI `kokoro-onnx` **0.6.1**, has **Kokoro v1.0 and v1.1** including a v1.1
Chinese model. ONNX Runtime, no PyTorch.

| Quant | Size |
|---|---|
| fp32 | 325.5 MB |
| fp16 | 163.2 MB |
| **int8** | **92.4 MB** |
| q8f16 | 86.0 MB |

**Why Kokoro still wins for us:** it is **non-autoregressive** (StyleTTS2 + ISTFTNet
decoder-only). One forward pass over phonemes produces the whole waveform and
parallelizes across cores. Every AR engine — XTTS-v2, Bark, Orpheus, IndexTTS-2,
Chatterbox, Fish Speech, VoxCPM — emits audio tokens one step at a time and
**cannot be made fast on CPU by quantization.** That architectural split predicts
CPU behaviour better than parameter count does.

Our `AGENTS.md` notes "+350 MB → ~582 MB active" for Kokoro; **int8 at 86 MB would
roughly halve that.**

Apache-2.0, commercially clean. 54 voices / 8 languages. No cloning — fine for us.

### 4.2 Piper — licensing regression

`rhasspy/piper` (MIT) **archived October 2025**. Development moved to
**`OHF-Voice/piper1-gpl`, now GPL-3.0** (espeak-ng embedded).

Current: **v1.8.0, 2026-09-04**. v1.5.0 added the C++ `libpiper` CLI.

Still the best low-power option: ~63 MB medium voices, 174 voices / 55 language
codes, real-time on Pi 5, <1 GB RAM, non-AR. **But GPL-3.0 copyleft matters if we
embed in a project with that licence expectation.**

Piper v1.3.0+ supports **raw phonemes via `[[ ]]`** — potentially useful for
forced pronunciation control.

### 4.3 Quality ladder (all available now)

| Model | Params | License | Cloning | Latency | Notes |
|---|---|---|---|---|---|
| **Kokoro-82M** | 82M | **Apache-2.0** | No | ~RT on CPU | Our current pick |
| **Chatterbox-Nano** | 110M | **MIT** | No | **3× real time on 8 CPU cores** | New; CPU-capable with cloning lineage |
| **Chatterbox-Turbo** | 350M | **MIT** | Yes (5–20 s) | ~200 ms | 1-step decoder distilled from 10; tags `[laugh]` `[cough]` |
| Chatterbox Multilingual V3 | 500M | MIT | Yes | — | PerTh watermark mandatory, cannot be removed |
| Orpheus 3B | 3B | Apache-2.0 | Yes | real-time | Broadest deployment ecosystem |
| IndexTTS-2.5 | 0.8B | **bilibili (restricted)** | Yes | RTF 0.207 | 2026-08-10; zh/en/es/ja/ar |
| Fish Speech S2 Pro | 4.4B | **Research licence** | Yes | <150 ms H200 | Highest scores; paid commercially |
| CosyVoice 3 | 0.5B | Apache-2.0 | Yes | ~150 ms | Multilingual zero-shot, heavy deployment |
| F5-TTS | ~336M | Code MIT / **weights CC-BY-NC** | Yes | fast | Research-grade only |
| XTTS v2 | ~460M | **CPML — non-commercial** | Yes | ~600 ms TTFA | The licence kills it |

⚠️ Chatterbox's 63.75–65.3% blind-preference win over ElevenLabs is
**vendor-run by Resemble AI**. Directionally striking, not a verdict.

**Hard rule:** shippable = Kokoro (Apache), Chatterbox (MIT), Orpheus (Apache).
Not shippable = XTTS v2 (CPML), F5-TTS weights (CC-BY-NC), Fish Speech
(research), IndexTTS (bilibili), Higgs Audio v3 (non-commercial).

### 4.4 Cloud

edge-tts is free, no API key, neural quality — but needs internet, which breaks
the offline story. **ElevenLabs `with-timestamps`** returns character-level
alignment mappable to visemes, but cloud + paid. Given local-first, keep cloud
strictly as a fallback tier.

**Note our actual default is edge-tts-first with Piper as fallback**
(`tts.rs:1`) — which contradicts our README's "generated entirely on-device"
claim. See the security/privacy doc for why this matters.

---

## 5. Announced / not yet usable

### 5.1 Meta Muse Voice Transcribe — the wildcard

[research.meta.ai/blog](https://research.meta.ai/blog/introducing-muse-voice-transcribe),
announced **2026-09-01**. Claims:

- Real-time streaming ASR, **diarization with 20+ speakers**, endpointing built in
- Multilingual, seamless code-switching, 70+ languages trained / **25 verified**
- **"Adaptive delay"** — RL-trained, dynamically varies per-word lookahead trading
  accuracy against latency. **This is exactly the axis our latency budget lives on.**
- Native >1 hour audio, 20+ speakers, no post-processing
- Claims **#1 on Artificial Analysis streaming STT** and #1 on public diarization
  benchmarks

**Not open weights. No release date. No API announced.** Meta Superintelligence
Labs.

**If this ships with weights it obsoletes our entire STT tier** — it does
endpointing (our VAD job), streaming, and diarization in one model. **Watch it.
Do not plan around it.**

### 5.2 livekit-wakeword ESP32 end-to-end

Their blog (2026-04-06): *"We're building an end-to-end model that removes the
need for a separate embedding model, making it small enough to run directly on
ESP32."* No release. **If it lands, our 3-ONNX pipeline collapses to 1 file** and
possibly our Rust front-end too.

### 5.3 Sensory commercial on-device STT

Announced 2026-05-18. 37 languages. 2.7 MB command-and-control model (787 KiB peak
SRAM) or 13 MB general. Targets Arm Ethos-U NPU, Cortex-M, ESP32, Snapdragon Wear.

**Not usable for us** — wrong power band (MCU/wearable), commercial licence, no
desktop x86 path.

### 5.4 Academic wake-word work — real but not deployable

| Paper | Contribution |
|---|---|
| **EdgeSpot** (arXiv 2601.16316, ICASSP 2026) | BC-ResNet + trainable PCEN + temporal self-attention, **29.4M MACs, 128k params**. 10-shot accuracy at 1% FAR: 73.7% → 82.0%. **Validates the same temporal-attention thesis** |
| **MALEFA** (arXiv 2604.03689) | Zero-shot KWS, **650K params / 93M FLOPs**, FAR 0.007% on AMI. If matured, our entire per-word training pipeline becomes unnecessary |
| **Typman-KWS** (arXiv 2509.07051) | 14.4k params, 92.4% F1 on STM32 — reminder that feature extraction matters as much as the model |

All research-stage. No drop-in pretrained weights.

### 5.5 Moonshine is quietly building a TTS + avatar stack

- **`moonshine-ai/moonshine-tts`** — C++ TTS across multiple languages, pushed
  2026-04-02. **9 stars, 50 commits, no README.** Early.
- **`moonshine-ai/tts-avatar`** — *"Javascript animation library for avatars that
  sync with text to speech,"* pushed 2026-04-13. **0 stars.** Vaporware.

Worth watching. If Moonshine ships phoneme-timed TTS alongside their streaming
ASR, that's a first-party end-to-end voice stack. **Do not plan around it today.**

---

## 6. Verdict table

| Component | Ours today | Verdict | Action |
|---|---|---|---|
| **Wake word engine** | openWakeWord DNN head | 🟡 **Outdated head** | Retrain with livekit-wakeword conv-attention. Apache-2.0. **Highest leverage here.** |
| **Wake word training** | openWakeWord `auto_train` | 🔴 **Broken on 3.12** | Move to a maintained pipeline. |
| **VAD** | Silero **v5** | 🟡 **One release behind** | Switch to v6 — already in our package. 16% fewer errors. Re-tune thresholds. |
| **VAD package** | vad-web 0.0.x | 🟢 Current (0.0.31) | Nothing. |
| **STT** | Moonshine medium_streaming | 🟢 **Still right** | **Verify the v2 checkpoint**, not v1. |
| **STT cloud** | Groq large-v3-turbo | 🟢 Good | Keep. 247 ms beats anything local. |
| **STT fallback tier** | — | ⚪ **Missing** | Add **Parakeet TDT 0.6B** via `whisper.cpp --enable-parakeet`. No silence hallucinations → could retire our `stt.rs` filter. |
| **TTS** | Kokoro | 🟡 **Good model, dead upstream** | Pin `kokoro-onnx` 0.6.1. Consider int8 (86 MB vs 325 MB). |
| **Lip-sync** | — | 🔴 **Missing intentionally** | **Do not build.** See `03` §9. |
| **Wake word prosody** | — | ⚪ **Missing** | Add the **Aware** gate: F1 0.93 vs Echo 0.56. |
| **Pre-roll suppression** | — | ⚪ **Missing** | 300–500 ms buffer + NLU check (Google patent 11,557,293). |

---

## 7. What could not be verified

- **Picovoice free-tier deprecation** — from a competitor's blog, not Picovoice.
  Their pricing page 404s. Likely but unconfirmed.
- **livekit-wakeword's benchmark** is entirely self-reported on their own phrase.
  No independent reproduction. The architecture argument is sound and matches the
  academic literature (EdgeSpot, MALEFA), which is why we'd bet on it — but the
  **100× FPPH figure is not independently confirmed**.
- **All WER numbers** come from Open ASR aggregates or vendor tables on different
  datasets. Cross-table comparison is indicative, not exact. ⚠️ **CORAAL caveat:**
  every model roughly quintuples its error rate on African American English vs
  broadcast English (Parakeet 2.77% TEDLIUM → **15.57% CORAAL**). **Test on our
  users' audio before trusting any of this.**