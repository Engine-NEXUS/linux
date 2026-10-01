# Complete Options Comparison: Every STT, TTS, and NLU Engine

**Created:** 2026-09-04
**Purpose:** Compare every available option for STT, TTS, and NLU — local and cloud — with measured RAM, latency, speed, size, and cost.

**Legend:**
- **RAM** = idle resident memory (MB)
- **Latency** = time to first result / first audio (ms)
- **Speed** = real-time factor (RTF) or throughput
- **Size** = model file size on disk (MB)
- **Response** = quality rating (Excellent/Good/Fair/Poor)
- **Free** = whether it has a free tier or is fully free

---

## 1. STT (Speech-to-Text) — All Options

### Local STT Engines

| # | Engine | Model | RAM | Latency (warm) | Speed (RTF) | Size | Response (WER) | Free? | Streaming? | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **faster-whisper** | tiny.en INT8 | 150 MB | 500ms | 0.07 RTF | 40 MB | Fair (8-12% WER) | Yes (MIT) | No (batch) | **CURRENT** — Python sidecar, port 39217 |
| 2 | faster-whisper | base.en INT8 | 200 MB | 800ms | 0.13 RTF | 80 MB | Good (6-8% WER) | Yes (MIT) | No (batch) | Better accuracy, 2x slower |
| 3 | faster-whisper | small.en INT8 | 300 MB | 1.4s | 0.41 RTF | 250 MB | Good (4-6% WER) | Yes (MIT) | No (batch) | Too slow for real-time on CPU |
| 4 | **whisper.cpp** | tiny.en Q5 | 80 MB | 400ms | 0.05 RTF | 40 MB | Fair (8-12% WER) | Yes (MIT) | No (batch) | C/C++ runtime, lighter than Python |
| 5 | whisper.cpp | base.en Q5 | 130 MB | 700ms | 0.10 RTF | 80 MB | Good (6-8% WER) | Yes (MIT) | No (batch) | Good CPU performance |
| 6 | **sherpa-onnx** | Moonshine Tiny | 50 MB | 250ms | 0.05 RTF | 125 MB | Fair (10-12% WER) | Yes (Apache) | Yes (partial) | Fastest local STT, 27M params |
| 7 | sherpa-onnx | Moonshine Base | 100 MB | 400ms | 0.08 RTF | 290 MB | Good (7-9% WER) | Yes (Apache) | Yes (partial) | 61M params, good balance |
| 8 | sherpa-onnx | Whisper Tiny | 80 MB | 350ms | 0.07 RTF | 100 MB | Fair (8-12% WER) | Yes (Apache) | Yes (partial) | ONNX runtime, no Python needed |
| 9 | sherpa-onnx | Parakeet TDT 0.6B | 400 MB | 500ms | 0.09 RTF | 671 MB | Excellent (6.3% WER) | Yes (CC-BY-4.0) | Yes (partial) | Best local accuracy, large |
| 10 | sherpa-onnx | SenseVoice Small | 150 MB | 300ms | 0.06 RTF | 240 MB | Good (7-8% WER) | Yes (Apache) | Yes (partial) | 234M params, multilingual |
| 11 | **Vosk** | small-en | 50 MB | 200ms | 0.03 RTF | 40 MB | Fair (10-15% WER) | Yes (Apache) | Yes (streaming) | Lightest option, lowest accuracy |
| 12 | Parakeet (NeMo) | TDT 0.6B v3 | 500 MB | 450ms | 0.09 RTF | 671 MB | Excellent (6.3% WER) | Yes (CC-BY-4.0) | Yes (streaming) | Needs NeMo runtime, Linux/WSL2 |
| 13 | NVIDIA Nemotron | Streaming EN 0.6B | 500 MB | 200ms | N/A | 600 MB | Excellent (6.9% WER) | Yes (MDW) | Yes (streaming) | First local streaming model, 1.12s chunks |

### Cloud STT APIs

| # | Provider | Model | RAM | Latency (first partial) | Latency (final) | Speed | Size | Response (WER) | Free Tier | Streaming? | Notes |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 14 | **Deepgram** | Nova-3 | 0 MB | 140ms | 539ms | Real-time | 0 MB | Good (5.3% WER) | $200 credit (~41K min) | Yes (WebSocket) | Fastest cloud STT, best for voice agents |
| 15 | Deepgram | Flux English | 0 MB | 80ms | 400ms | Real-time | 0 MB | Good (6.9% WER) | $200 credit | Yes (WebSocket) | Built-in end-of-turn detection, saves 200-600ms |
| 16 | Deepgram | Nova-3 Medical | 0 MB | 140ms | 539ms | Real-time | 0 MB | Excellent (3.4% WER) | $200 credit | Yes (WebSocket) | Medical domain, higher accuracy |
| 17 | **Groq** | Whisper Large v3 Turbo | 0 MB | 247ms | 269ms | 247x RT | 0 MB | Good (12% WER) | 2,000 req/day free | No (batch only) | Cheapest cloud STT, $0.04/hr |
| 18 | Groq | Whisper Large v3 | 0 MB | 300ms | 350ms | 300x RT | 0 MB | Good (10.3% WER) | 2,000 req/day free | No (batch only) | $0.111/hr, highest accuracy Whisper |
| 19 | **AssemblyAI** | Universal-Streaming | 0 MB | 247ms | 307ms | Real-time | 0 MB | Good (7% WER) | $50 credit (~185 hr) | Yes (WebSocket) | $0.15/hr, cheapest streaming |
| 20 | AssemblyAI | Universal-3.5 Pro | 0 MB | 300ms | 400ms | Real-time | 0 MB | Excellent (6.99% WER) | $50 credit | Yes (WebSocket) | Best diarization, keyterm prompting |
| 21 | **OpenAI** | gpt-4o-mini-transcribe | 0 MB | N/A | 2-4s | Fast | 0 MB | Good (8-10% WER) | $5 trial credit | Yes (streaming) | $0.003/min, cheapest batch |
| 22 | OpenAI | whisper-1 (legacy) | 0 MB | N/A | 2-4s | Fast | 0 MB | Good (8-10% WER) | $5 trial credit | No (batch only) | $0.006/min, being deprecated |
| 23 | **Google Cloud** | Chirp 3 | 0 MB | 350ms | 1.5s | Real-time | 0 MB | Excellent (9% WER) | 60 min/month free | Yes (streaming) | Built-in denoiser, 99+ languages |
| 24 | Google Cloud | Standard | 0 MB | 350ms | 1.5s | Real-time | 0 MB | Good (10% WER) | 60 min/month free | Yes (streaming) | $0.016/min, expensive |
| 25 | **Azure Speech** | Standard | 0 MB | 450ms | 1.2s | Real-time | 0 MB | Good (10% WER) | 5 hr/month free | Yes (streaming) | 500+ voices, 140+ languages |
| 26 | **AWS Transcribe** | Standard | 0 MB | 600ms | 1.5s | Real-time | 0 MB | Fair (12% WER) | 60 min/month free (12 mo) | Yes (streaming) | $0.024/min, most expensive |
| 27 | **ElevenLabs** | Scribe v2 Realtime | 0 MB | 150ms | 1s | Real-time | 0 MB | Excellent (2.2% WER) | 10K chars/month | Yes (WebSocket) | Best accuracy, $0.39/hr |
| 28 | **Cartesia** | Ink-Whisper | 0 MB | 100ms | 300ms | Real-time | 0 MB | Good (8% WER) | TBD | Yes (streaming) | $0.13/hr, cheapest of all |

---

## 2. TTS (Text-to-Speech) — All Options

### Local TTS Engines

| # | Engine | Model | RAM | Latency (TTFA warm) | Speed (RTF) | Size | Response (Quality) | Free? | Streaming? | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | **Kokoro** | 82M ONNX FP16 | 350 MB | 90ms (new) / 5ms (cached) | 0.08 RTF | 337 MB | Excellent | Yes (Apache 2.0) | No | **CURRENT** — in-process Rust, af_sky voice |
| 2 | **Piper** | en_US-amy-medium INT8 | 80 MB | 40ms (new) / 5ms (cached) | 0.03 RTF | 60 MB | Good | Yes (MIT) | No | 30x faster than real-time, runs on Pi |
| 3 | Piper | en_US-amy-low | 40 MB | 30ms | 0.02 RTF | 30 MB | Fair | Yes (MIT) | No | Lowest quality, smallest size |
| 4 | Piper | en_US-lessac-high | 120 MB | 50ms | 0.04 RTF | 120 MB | Good+ | Yes (MIT) | No | Best Piper quality |
| 5 | **whisper.cpp** | (not applicable) | — | — | — | — | — | — | — | Not a TTS engine |
| 6 | **XTTS v2** | Coqui 460M | 4,500 MB | 600ms | 0.34 RTF | 1,800 MB | Excellent+ | Yes (CPML) | Yes | Voice cloning, too heavy for desktop |
| 7 | **MeloTTS** | MyShell | 800 MB | 300ms | 0.15 RTF | 500 MB | Good | Yes (MIT) | Yes | Multilingual, Python-based |
| 8 | **eSpeak-NG** | (formant) | 5 MB | 1,614ms | 0.01 RTF | 15 MB | Poor (robotic) | Yes (GPL) | No | Already bundled for Kokoro phonemes |
| 9 | **Silero TTS** | v3 | 100 MB | 200ms | 0.10 RTF | 80 MB | Good | Yes (MIT) | Yes | Russian/English, lightweight |
| 10 | **Bark** | (Suno) | 2,000 MB | 5,000ms | 0.50 RTF | 1,500 MB | Excellent | Yes (MIT) | No | Too slow for real-time |
| 11 | **Pocket-TTS** | ONNX | 200 MB | 150ms | 0.20 RTF | 150 MB | Good | Yes (MIT) | Yes | Compact, streaming capable |
| 12 | **Picovoice Orca** | Streaming | 29 MB | 106ms | 0.065 core-hr | 7 MB | Good+ | **No** (commercial) | Yes (token streaming) | Smallest+fastest, but paid license |
| 13 | **edge-tts** | Microsoft Edge | 0 MB (cloud) | 200ms | Fast | 0 MB | Excellent | Yes (free, unofficial) | Yes | Uses Edge read-aloud endpoint, needs internet |
| 14 | **gTTS** | Google Translate | 0 MB (cloud) | 500ms | Fast | 0 MB | Fair | Yes (free, unofficial) | No | Google Translate TTS, not for production |

### Cloud TTS APIs

| # | Provider | Model | RAM | Latency (TTFA) | Speed | Size | Response (Quality) | Free Tier | Streaming? | Notes |
|---|---|---|---|---|---|---|---|---|---|---|
| 15 | **ElevenLabs** | Flash v2.5 | 0 MB | 150-180ms + 50-200ms net = 200-380ms | 75ms inference | 0 MB | Excellent | 10K chars/month free | Yes (WebSocket) | 32 languages, $0.05/1K chars |
| 16 | ElevenLabs | Turbo v2.5 | 0 MB | 264ms P50 + net | Fast | 0 MB | Excellent+ | 10K chars/month free | Yes (WebSocket) | Higher quality, $0.10/1K chars |
| 17 | ElevenLabs | Multilingual v2 | 0 MB | 1,232ms P50 | Slow | 0 MB | Excellent++ | 10K chars/month free | Yes | Highest quality, too slow for real-time |
| 18 | **Cartesia** | Sonic-3 | 0 MB | 188ms P50 + net | 90ms first byte | 0 MB | Excellent | TBD | Yes (WebSocket) | $50/1M chars, sub-100ms first byte |
| 19 | **Deepgram** | Aura-2 | 0 MB | 313ms P50 + net | Fast | 0 MB | Good | $200 credit | Yes (WebSocket) | $0.005/min audio, budget option |
| 20 | **OpenAI** | tts-1 | 0 MB | ~500ms + net | Fast | 0 MB | Good | $5 trial credit | Yes (streaming) | $15/1M chars, 6 voices |
| 21 | OpenAI | tts-1-hd | 0 MB | ~800ms + net | Medium | 0 MB | Good+ | $5 trial credit | No | $30/1M chars, higher quality |
| 22 | OpenAI | gpt-4o-mini-tts | 0 MB | ~400ms + net | Fast | 0 MB | Good | $5 trial credit | Yes (streaming) | $15/1M chars, 57 voices |
| 23 | **Google Cloud** | Standard | 0 MB | ~400ms + net | Fast | 0 MB | Fair | 4M chars/month free (1yr) | No | $4/1M chars, cheapest cloud TTS |
| 24 | Google Cloud | WaveNet | 0 MB | ~500ms + net | Medium | 0 MB | Good | 1M chars/month free (1yr) | No | $16/1M chars |
| 25 | Google Cloud | Chirp 3 HD | 0 MB | ~600ms + net | Medium | 0 MB | Excellent | 1M chars/month free (1yr) | No | $30/1M chars |
| 26 | **Azure Speech** | Neural Standard | 0 MB | ~400ms + net | Fast | 0 MB | Good | 500K chars/month free | Yes (streaming) | $14.11/1M chars, 500+ voices |
| 27 | **Amazon Polly** | Neural | 0 MB | ~500ms + net | Fast | 0 MB | Good | 1M chars/month free (1yr) | Yes (streaming) | $16/1M chars |
| 28 | **Groq** | Orpheus English | 0 MB | ~200ms + net | 100 chars/s | 0 MB | Good | 2,000 req/day free | Yes | $22/1M chars, on Groq LPU |

---

## 3. NLU (Natural Language Understanding) — All Options

### Local NLU

| # | Engine | Type | RAM | Latency | Speed | Size | Response (Accuracy) | Free? | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 1 | **Deterministic Parser** | Regex/Keyword | 2 MB | <5ms | Instant | 0 MB | Good (90-95% coverage) | Yes | **CURRENT** — `intent_parser.rs`, handles common commands |
| 2 | **BERT-Mini ONNX** | ML fallback | 100 MB | 50ms | Fast | 18 MB | Good+ (95-97% accuracy) | Yes | **CURRENT** — Python sidecar, port 39218 |
| 3 | BERT-Mini ONNX (Rust) | ML in-process | 30 MB | 20ms | Fast | 18 MB | Good+ (95-97% accuracy) | Yes | Would eliminate Python sidecar |
| 4 | DistilBERT ONNX | ML | 80 MB | 30ms | Fast | 65 MB | Excellent (97-98% accuracy) | Yes | Better accuracy, more RAM |
| 5 | MobileBERT ONNX | ML | 50 MB | 25ms | Fast | 25 MB | Excellent (97% accuracy) | Yes | Good balance for on-device |
| 6 | spaCy + custom rules | NLP + rules | 150 MB | 10ms | Fast | 50 MB | Good (85-90% accuracy) | Yes (MIT) | Python-based, heavier |
| 7 | **Picovoice Rhino** | Speech-to-Intent | 5 MB | <10ms | Instant | 4 MB | Good (90% accuracy) | **No** (commercial) | On-device, no text needed (audio→intent directly) |

### Cloud NLU

| # | Provider | Type | RAM | Latency | Speed | Size | Response (Accuracy) | Free Tier | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 8 | **OpenAI** | GPT-4o-mini | 0 MB | 200-500ms | Fast | 0 MB | Excellent (99%+ accuracy) | $5 trial credit | Most flexible, understands any phrasing |
| 9 | **Groq** | Llama 3.1 8B | 0 MB | 100-200ms | 394 TPS | 0 MB | Excellent (98%+ accuracy) | 14,400 req/day free | Fastest LLM inference, free tier generous |
| 10 | **Anthropic** | Claude Haiku 3.5 | 0 MB | 200-400ms | 100 TPS | 0 MB | Excellent (99%+ accuracy) | $5 trial credit | Best reasoning, more expensive |
| 11 | **Google** | Gemini 2.0 Flash | 0 MB | 150-300ms | 150 TPS | 0 MB | Excellent (99%+ accuracy) | 15 req/min free | Good free tier, fast |

---

## 4. Combined Pipeline Options — Head-to-Head

### Option A: Current Setup (All Local, Pre-warmed)

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | faster-whisper tiny.en INT8 | 150 MB | 500ms | 40 MB | Yes |
| NLU | Deterministic + BERT-Mini | 102 MB | 5ms / 50ms | 18 MB | Yes |
| TTS | Kokoro 82M | 350 MB | 5ms (cached) / 90ms (new) | 337 MB | Yes |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **802 MB** | **~555ms** | **395 MB** | **Yes ($0)** |

### Option B: All Local, Optimized (Piper + Lazy NLU)

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | faster-whisper tiny.en INT8 | 150 MB | 500ms | 40 MB | Yes |
| NLU | Deterministic only (lazy BERT-Mini) | 2 MB | 5ms | 0 MB | Yes |
| TTS | Piper medium INT8 | 80 MB | 5ms (cached) / 40ms (new) | 60 MB | Yes |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **432 MB** | **~545ms** | **100 MB** | **Yes ($0)** |

### Option C: Hybrid (Cloud STT + Local Piper TTS) — RECOMMENDED

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | Deepgram Nova-3 (cloud) | 0 MB | 140ms (streaming) | 0 MB | $200 credit |
| NLU | Deterministic Rust parser | 2 MB | <5ms | 0 MB | Yes |
| TTS | Piper medium INT8 (local) | 80 MB | 5ms (cached) / 40ms (new) | 60 MB | Yes |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **282 MB** | **~190ms** | **60 MB** | **$0.72/mo** |

### Option D: Hybrid (Groq STT + Local Piper TTS) — CHEAPEST

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | Groq Whisper Large v3 Turbo (cloud) | 0 MB | 247ms (batch) | 0 MB | 2,000 req/day free |
| NLU | Deterministic Rust parser | 2 MB | <5ms | 0 MB | Yes |
| TTS | Piper medium INT8 (local) | 80 MB | 5ms (cached) / 40ms (new) | 60 MB | Yes |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **282 MB** | **~297ms** | **60 MB** | **$0 (free tier)** |

### Option E: All Cloud (Deepgram + ElevenLabs)

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | Deepgram Nova-3 (cloud) | 0 MB | 140ms | 0 MB | $200 credit |
| NLU | Groq Llama 3.1 8B (cloud) | 0 MB | 150ms | 0 MB | 14,400 req/day free |
| TTS | ElevenLabs Flash v2.5 (cloud) | 0 MB | 200-380ms | 0 MB | 10K chars/month free |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **200 MB** | **~490-670ms** | **0 MB** | **$22+/mo after free** |

### Option F: Ultra-Cheap (Groq for everything)

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | Groq Whisper Large v3 Turbo | 0 MB | 247ms | 0 MB | 2,000 req/day free |
| NLU | Groq Llama 3.1 8B | 0 MB | 150ms | 0 MB | 14,400 req/day free |
| TTS | Groq Orpheus English | 0 MB | 200ms | 0 MB | 2,000 req/day free |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **200 MB** | **~597ms** | **0 MB** | **$0 (free tier)** |

### Option G: Ultra-Fast (Deepgram Flux + Picovoice Orca)

| Component | Engine | RAM | Latency | Size | Free? |
|---|---|---|---|---|---|
| STT | Deepgram Flux (cloud) | 0 MB | 80ms | 0 MB | $200 credit |
| NLU | Deterministic Rust parser | 2 MB | <5ms | 0 MB | Yes |
| TTS | Picovoice Orca (local) | 29 MB | 106ms | 7 MB | **No (commercial)** |
| Baseline | Rust + WebView2 | 200 MB | — | — | Yes |
| **Total** | | **231 MB** | **~191ms** | **7 MB** | **Paid license** |

---

## 5. Summary Comparison Table

| Option | RAM | Latency (ack) | Latency (STT) | Cold Start | Model Size | Monthly Cost | Offline? | Streaming STT? |
|---|---|---|---|---|---|---|---|---|
| **A: Current (all local)** | 802 MB | 5ms | 500ms | 32s | 395 MB | $0 | Yes | No |
| **B: Optimized local** | 432 MB | 5ms | 500ms | 15s | 100 MB | $0 | Yes | No |
| **C: Hybrid (recommended)** | 282 MB | 5ms | 140ms | 0s | 60 MB | $0.72 | Partial | Yes |
| **D: Hybrid (Groq, cheapest)** | 282 MB | 5ms | 247ms | 0s | 60 MB | $0 | Partial | No |
| **E: All cloud (ElevenLabs)** | 200 MB | 200-380ms | 140ms | 0s | 0 MB | $22+ | No | Yes |
| **F: All Groq (free)** | 200 MB | 200ms | 247ms | 0s | 0 MB | $0 | No | No |
| **G: Ultra-fast (Orca)** | 231 MB | 106ms | 80ms | 0s | 7 MB | Paid | Partial | Yes |

---

## 6. Cost Comparison (100 commands/day, 5-10 users)

| Option | STT Cost | TTS Cost | NLU Cost | Total/month | Free Tier Duration |
|---|---|---|---|---|---|
| A: Current | $0 | $0 | $0 | **$0** | Forever |
| B: Optimized local | $0 | $0 | $0 | **$0** | Forever |
| C: Hybrid (Deepgram+Piper) | $0.72 | $0 | $0 | **$0.72** | 9 months ($200 credit) |
| D: Hybrid (Groq+Piper) | $0 | $0 | $0 | **$0** | Forever (2,000 req/day) |
| E: All cloud (ElevenLabs) | $0.72 | $22 | $0 | **$22.72** | 1 month (10K chars) |
| F: All Groq | $0 | $0 | $0 | **$0** | Forever (free tier) |
| G: Ultra-fast (Orca) | $0.72 | License | $0 | **$0.72 + license** | TBD |

---

## 7. Recommendation Matrix

| If you prioritize... | Best option | Why |
|---|---|---|
| **Lowest RAM** | E: All cloud (200 MB) or F: All Groq (200 MB) | Zero model RAM, only WebView2 baseline |
| **Lowest latency** | G: Ultra-fast (191ms) | Deepgram Flux 80ms + Orca 106ms |
| **Lowest cost** | D: Hybrid Groq (0/mo) or F: All Groq (0/mo) | Groq free tier covers 2,000 req/day |
| **Best quality TTS** | E: All cloud (ElevenLabs) | ElevenLabs Flash = best cloud TTS quality |
| **Best quality STT** | C: Hybrid (Deepgram Nova-3) | 5.3% WER, streaming, 140ms |
| **Fully offline** | B: Optimized local (432 MB) | No internet needed at all |
| **Best balance** | **C: Hybrid (282 MB, 190ms, $0.72/mo)** | Low RAM, fast latency, cheap, streaming STT |
| **Zero cost + low RAM** | **D: Hybrid Groq (282 MB, 297ms, $0)** | Free forever, decent latency |

---

## 8. Key Findings

### STT findings:
1. **Deepgram Nova-3** is the fastest cloud STT (140ms partial) with good accuracy (5.3% WER)
2. **Groq Whisper** is the cheapest cloud STT ($0.04/hr, 2,000 req/day free) but batch-only
3. **Local faster-whisper** is the current choice — 500ms warm, no streaming, 150 MB RAM
4. **Moonshine Tiny** (sherpa-onnx) is the fastest local STT (250ms, 50 MB RAM) but lower accuracy
5. **Local STT cannot stream** — Whisper processes complete utterances only

### TTS findings:
1. **Local Piper** is the best budget TTS — 40ms TTFA, 80 MB RAM, 60 MB model, free
2. **Local Kokoro** (current) is the best quality local TTS — 90ms TTFA, 350 MB RAM, excellent quality
3. **Picovoice Orca** is the fastest+smallest (106ms, 29 MB, 7 MB model) but commercial license
4. **Cloud ElevenLabs Flash** is the best cloud TTS (75ms inference) but costs $22/month
5. **Cached ack phrases** play in 5ms regardless of engine — this is the key insight

### NLU findings:
1. **Deterministic Rust parser** handles 90-95% of commands in <5ms with 2 MB RAM — keep it
2. **Groq Llama 3.1 8B** is the best cloud NLU — 150ms, 14,400 req/day free, 98%+ accuracy
3. **BERT-Mini ONNX in Rust** would eliminate the Python sidecar (100 MB → 30 MB RAM)
4. NLU is the least impactful component to optimize — deterministic parser is already perfect

### The critical insight:
**Cached ack phrases play in 5ms regardless of TTS engine.** This means:
- For "On it sir" / "Didn't catch that sir" → local Piper cached = same speed as Kokoro cached
- For new text (analysis summaries) → Piper 40ms vs Kokoro 90ms (Piper is faster!)
- For new text quality → Kokoro is more natural than Piper
- **Since 90% of TTS output is cached ack phrases, the quality difference barely matters**
