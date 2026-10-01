# STT/TTS Combo Analysis — Low RAM + Accurate + Good Voice

**Date:** 2026-09-05
**Status:** Research only, no code changed
**Scope:** `server/stt_server.py` (faster-whisper), `src-tauri/src/tts.rs` (Piper), `src-tauri/src/lazy_stt.rs`

---

## 1. Measured baseline (this machine, 7GB RAM, no GPU)

| Component | Model | RSS | Notes |
|---|---|---|---|
| STT server | faster-whisper `tiny.en`, CPU `int8` (`server/stt_server.py:50`) | **217MB** (`ps`) | Docstring claims ~150MB + ~0.5s (`stt_server.py:11`). Overhead = Python + fastapi + uvicorn. |
| TTS engine | Piper `en_US-amy-medium` (`tts.rs:34`), in-process via `piper-rs` | ~80MB (claimed, `Cargo.toml`) | ~63MB disk. `CACHED_PHRASES` (`tts.rs:24`) pre-synth acks for instant playback. |
| Wake-word | OWW melspec + embedding + `nexus.onnx` (tract, in-process) | ~100–200MB est | Always-on, unaffected by STT/TTS choice. |

Real accuracy symptom driving this: `tiny.en` misheard as `How is the addition of input?` (2026-09-05 log).

---

## 2. STT candidates

| STT | Params / size | RAM (CPU) | Latency, 3s clip (CPU) | Accuracy |
|---|---|---|---|---|
| faster-whisper `tiny.en` (now) | 39M, ~40MB | **217MB measured** | ~0.5s | Worst. Mishears commands. |
| faster-whisper `base.en` (`WHISPER_MODEL=base.en`) | 74M, ~75MB | ~300–400MB est | ~1.5s | Better, fewer mishears. One env change. |
| faster-whisper `small.en` | 244M, ~250MB | ~600–800MB est | ~3s | Best Whisper here, too slow for wake loop. Skip. |
| **Moonshine tiny ONNX** (UsefulSensors) | ~27M | ~100–150MB est, in-process | ~50–150ms est, ~10x Whisper tiny RT | Beats `tiny.en`. Live-caption built. |
| **Moonshine base ONNX** (UsefulSensors) | 61.5M | ~150–250MB est, in-process | ~100–250ms est, ~10x Whisper base RT | Beats `base.en`. Best low-RAM pick. |
| Parakeet TDT 0.6B (NVIDIA NeMo) | 600M | ~1–2GB+ est + NeMo stack | 60x realtime on GPU, slow on CPU | Most accurate. OOM risk on 7GB CPU box. Skip. |
| Vosk | small | tiny | fast | Worse than `tiny.en`. Skip. |

Why Moonshine wins: native ONNX (runs on `ort`, already in tree via `piper-rs`), streaming-ready, kills the ~217MB Python server. Needs `lazy_stt.rs` rewrite: Rust inference instead of spawning `stt_server.py`. VAD + hotwords + `initial_prompt` logic in `stt_server.py:190-221` must be re-expressed (Moonshine has no Whisper `hotwords` param — replace with post-decode fuzzy map, Worker already fuzzy-matches).

---

## 3. TTS candidates

| TTS | Params / size | RAM | Latency | Voice |
|---|---|---|---|---|
| Piper `amy-medium` (now) | ~63MB disk | ~80MB | ~50–150ms, cached acks instant | Robotic prosody. Fastest, lightest. |
| **Kokoro-82M ONNX** | 82M, ~300MB fp32 / ~80–100MB q8 | ~300–450MB est | First chunk ~200–400ms est, CPU realtime-ish | SOTA naturalness per size, many voices. Needs espeak-ng phonemizer + `ort` inference in `tts.rs` (replaces `piper-rs` call; `ort` already present). Biggest voice upgrade per MB. |
| Chatterbox / expressive clones | — | ~500MB+ est, slow CPU | ~1s+ | Better emotion. Wrong for always-on 7GB box. Skip. |

---

## 4. Cloud APIs (ElevenLabs Turbo v2.5 / Fish Audio S1 / Gemini Live)

| Factor | Detail |
|---|---|
| RAM | 0 (off-device). |
| Voice | Best. ElevenLabs prosody top, Fish clone strong, Gemini native speech + understanding. |
| Latency | ElevenLabs claims <75ms input, P99 <150ms + network 100–300ms. Gemini ~300–600ms roundtrip est. Needs net on every wake. |
| Cost | Per-char / per-min metered, changes fast — pin pricing before shipping. |
| Privacy | **Breaks NEXUS design.** `stt_server.py:1-8`: audio NEVER leaves device, only text goes to worker. Cloud sends mic audio off-device. Needs explicit consent + settings + local fallback. |
| Reliability | Offline dead. No-wifi = no voice. |

Verdict: skip unless user explicitly opts in. Local-first stays default.

---

## 5. Combo totals (STT + TTS; wake ~100–200MB always extra)

| Combo | Total RAM | Latency | Accuracy / voice | Work |
|---|---|---|---|---|
| **A — now** (`tiny.en` + Piper) | ~300MB | fastest | worst accuracy, robotic | none |
| **B — lazy** (`base.en` + Piper) | ~400–500MB | STT ~1.5s | fewer mishears, same voice | one env var |
| **C** (Moonshine base + Piper) | ~230–330MB | STT ~100–250ms | accuracy up, voice same | `lazy_stt.rs` rewrite |
| **D — recommended** (Moonshine base + Kokoro) | ~450–700MB | STT ~100–250ms, TTS first chunk ~200–400ms | accurate + much better voice | `lazy_stt.rs` + `tts.rs` rewrite |
| **E** (Parakeet + Kokoro) | ~1.5GB+ | — | best quality, OOM risk here | skip |

---

## 6. Verdict

- **D wins** low-RAM + accurate + good voice. Net roughly flat vs B (Python ~217MB gone, Kokoro added).
- **C wins** if voice need not change (lightest accurate path).
- **Lazier alternative:** `WHISPER_MODEL=base.en`, keep Piper. One env var, +~100MB, no code.

`ponytail:` D needs `ort` inference paths in `lazy_stt.rs` + `tts.rs`. Upgrade = replace server spawn (`lazy_stt.rs`) + synth fn (`tts.rs`), re-express VAD/hotwords/prompt + `CACHED_PHRASES`.

## 7. Sources

- Local measurements: `ps` RSS 217MB (2026-09-05); `server/stt_server.py`; `src-tauri/src/tts.rs`; `src-tauri/Cargo.toml`.
- Model publishers (figures = vendor claims / community benchmarks, verify before shipping): UsefulSensors Moonshine (tiny ~27M, base 61.5M, live-caption speed claims), Kokoro-82M, OpenAI Whisper model sizes, NVIDIA NeMo Parakeet TDT 0.6B, ElevenLabs Turbo v2.5 latency docs, Fish Audio, Gemini Live API.
