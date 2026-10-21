# 131 — How good / how varied is the local voice, and how much RAM can we give back without losing any feature (MEASURED + PLAN, nothing changed in the product)

Date: 2026-10-16. Companion to `129` (one free voice) and `130` (RAM per state). All "M" numbers were measured today on this laptop (i7-1355U, 15.7 GB) with the real app engine (`tts_kokoro.rs`, int8 `model_quantized.onnx`, 88.1 MB), in a **debug** test build, with other apps running. Memory = **working set (resident)**. Speed numbers are pessimistic debug figures.

---

## 1. Voice quality and variety (what the model really offers)

Source: Kokoro-82M model card + VOICES.md (Apache-2.0; 54 voices, 9 languages; trained on permissive/public-domain/synthetic audio). The grade is Hexgrad's own estimate of **how much and how clean the training data of that voice was** - it predicts quality.

| Group | Voice (grade) |
|---|---|
| **US female (11)** | **af_heart (A)**, **af_bella (A-)**, af_nicole (B-), af_aoede / af_kore / af_sarah (C+), af_nova / af_alloy (C), af_sky (C-), af_jessica / af_river (D) |
| **US male (9)** | am_michael / am_fenrir / am_puck (C+), am_echo / am_eric / am_liam / am_onyx (D), am_santa (D-), am_adam (F+) |
| **British female (4)** | **bf_emma (B-)**, bf_isabella (C), bf_alice / bf_lily (D) |
| **British male (4)** | bm_george (C), bm_fable (C), bm_lewis (D+), bm_daniel (D) |
| Other languages | Japanese 5, Mandarin 8, Spanish 3, French 1, Hindi 4, Italian 2, Brazilian Portuguese 3 (G2P/data thinner; NEXUS is English-only by your directive) |

Honest reading:
* **Best quality: af_heart (the NEXUS twin) and af_bella - grade A.** bf_emma and af_nicole are the next best (B-).
* **The JARVIS-style British male is the weak spot:** the best are **bm_george / bm_fable, grade C** (10-100 minutes of training audio). Usable and clear, but not top grade. A custom voice (KVoiceWalk, doc 129) or a blend can improve on it.
* Variety on top of the 28 English voices costs **no RAM**: a voice is a 0.5 MB tensor. Community tools blend voices by averaging tensors (e.g. `af_bella+af_sky` in the Kokoro-FastAPI server; this is a community technique, **not in the model card; unverified here**). A blend of two C+ voices can sound better than either; a KVoiceWalk voice is another route.
* Controls Kokoro has: **speed**. It has **no emotion/style tags and no SSML**. The current "auto emotion" (rate/pitch) exists only for Edge; for Kokoro it would be emulated (speed) or dropped - consistent with the "fixed prosody" decision in 127/129, so nothing the user relies on is lost.
* Versus the current Edge voice: Kokoro's model card says it topped the TTS Spaces Arena at launch (Dec 2024) ahead of Edge TTS and XTTS in a single-voice setting; its 2026 leaderboard position is **unconfirmed**. Evidence you can judge yourself: the **6 real samples I sent** (bm_george, af_heart, bf_emma, bm_fable, af_bella + a long bm_george sentence). Earlier internal ASR round-trip on Kokoro output: WER 0.03-0.10 (recorded, previous session).

## 2. Where Kokoro's RAM actually goes (measured - this is the key finding)

Fresh process per run, `mem_breakdown` ignored test (new, in `tts_kokoro.rs`). Working set:

| Stage | Default build | Cumulative |
|---|---|---|
| 0. test harness before anything | ~10-15 MB | ~15 |
| 1. **Pronunciation dictionary built (G2P, British gold+silver+tagger)** | **+~125 MB** | **139** |
| 2. after first phonemize | +1 | 140 |
| 3. **ONNX session loaded (88 MB file)** | **+135 MB** | **275** |
| 4. **after FIRST synthesis (ONNX working memory / arena)** | **+64 MB** | **339** |
| 5. after 5 syntheses | +29 | **368** |
| 6. **after `unload()`** | **-304 MB** | **64** (memory is returned) |

Why the dictionary is so large: `misaki-rs` embeds ~14 MB (US) / ~14.7 MB (GB) of JSON dictionaries (`us_gold` 3.2 MB + `us_silver` 11.9 MB; `gb_gold` 3.0 + `gb_silver` 11.7) plus a 5.7 MB tagger-weights JSON, and **parses them into Rust `HashMap<String, ...>` at start** (`data.rs`: `include_str!` + `serde_json`). JSON text turned into millions of small heap strings costs ~8x its size.
Why the ONNX session is large: it is built with the default options (`Session::builder().commit_from_file`): all cores, **default CPU memory arena** and memory-pattern caching.

### Variant results (one run each, fresh process)
| ONNX setting | After 5 syntheses (WS) | Saved vs default | Debug speed (RTF) |
|---|---|---|---|
| default (all threads, arena on) | **368 MB** | - | 2.70 |
| 2 threads | 367 MB | 1 MB | 3.16 |
| 2 threads, **arena off, memory-pattern off** | **280 MB** | **-88 MB (-24 %)** | 3.26 |
| 4 threads, arena off, memory-pattern off | **279 MB** | **-89 MB** | 3.42 |
Speed numbers are **noisy** (debug build, other apps running; 4 threads came out slower than 2); they show the arena-off setting does not collapse speed, but **the real cost must be measured in a quiet release build** (plan R0). Virtual-size numbers in the raw output are not commit and are ignored.

## 3. RAM-reduction plan - no feature removed

Every item keeps the feature; the risk column says what could go wrong. Savings are against the measured numbers above or marked E.

| # | Change | Saves (resident) | Evidence | Risk / what protects it |
|---|---|---|---|---|
| R1 | **Kokoro ONNX session tuning**: CPU arena off, memory pattern off, intra threads 2-4 (`ort::ep::CPU::default().with_arena_allocator(false)`, `with_memory_pattern(false)`; the APIs exist in the pinned ort 2.0.0-rc.12) | **-88 MB while speaking** | M | speed; measure RTF in release on this and a low-end PC; keep the default as a fallback setting |
| R2 | **Compact pronunciation dictionary**: build once into a binary string table (sorted keys + offsets, or an FST) memory-mapped from disk; no JSON parse, no HashMap; same output | **-100 to -115 MB** | M (G2P = 125 MB) | exact-output equality: golden corpus of 20 000 words/sentences phonemized by old vs new must be identical (the repo already has `dump_phonemes_for_variant_eval`) |
| R3 | **ORT-format model with bytes used in place** (`.ort` + `session.use_ort_model_bytes_directly`, no second copy of the weights) and `with_prepacking(false)` | **-40 to -90 MB** | E (session = +135 MB for an 88 MB file) | build step to convert the model (Python `onnxruntime` tool, offline); check output equality |
| R4 | **Idle unload of the voice** (already exists; tune to ~60-120 s). Measured: unload returns **304 MB** | **-300 MB when idle** | M | first line after idle pays a 1-2 s load -> hidden by the clip bank of stock lines (doc 129 T1/T3) |
| R5 | **Clip bank**: frequent lines never load the model | lowers the *average*, not the peak | E | none |
| R6 | **Local STT fallback**: replace the Python/Moonshine sidecar (130-340 MB, plus a Python install) with an in-process ONNX recognizer (sherpa-onnx Moonshine-tiny or Whisper-tiny.en, Q8 weights; parameters 27 M / 39 M), loaded only when Groq is unreachable, unloaded after ~60 s | **-100 to -250 MB peak offline** | E (no RAM figure found in public docs; **must measure**) | accuracy (WER) and latency; keep Groq as primary; ship behind a setting until verified |
| R7 | **NLU model**: unload after ~5 min idle; int8-quantize (two 16.8 MB files -> ~8 MB each) | **-30 to -60 MB** | E | accuracy regression: run the existing NLU eval gates |
| R8 | **Orb GPU process**: release the WebGL context when the orb has been hidden > 30 s and recreate it on show (the orb is hidden most of the time; GPU process = 83 MB commit idle) | **-30 to -50 MB idle (commit)** | E | 100-200 ms re-creation on wake; must not delay the first frame (pre-create on wake word) |
| R9 | **Hand mode on the existing WebView2 environment** (same user-data dir already shares the browser/GPU processes; avoid a second heavy renderer or host MediaPipe in a worker of the stage page) | **-80 to -150 MB when hand mode is on** | E (not measured) | camera permission and the "hand window must stay visible" pitfall in AGENTS.md; **measure first** |
| R10 | Rust core heap (70 MB commit): profile (`dhat`) and trim obvious buffers (stock-line cache f32 -> i16 = ~2.5 MB; app-registry strings) | -10 to -20 MB | E | low value, do last |
| R11 | Working-set trim when idle (`SetProcessWorkingSetSize`) on top of EcoQoS | cosmetic for Task Manager; helps 8 GB PCs | M (idle private WS already 65 MB) | none |

### Resulting budget (resident MB; M = measured today, other = plan targets)
| State | Today | After R1-R4 (+R7) | After all items |
|---|---|---|---|
| Idle, warm, voice unloaded | 285-325 | 255-295 | 225-265 |
| **Voice loaded / speaking (Kokoro)** | **650-730** | **~400-470** (368 -> ~175 with R1+R2; plus R3 -> ~110-135) | **~340-400** |
| Offline STT fallback active (on top) | +130 to +340 | +130 to +340 | +60 to +150 |
| Hand mode on (on top) | +150 to +300 (E) | same | +50 to +150 |
| **Everything on** | **1.1-1.5 GB** | **~0.85-1.1 GB** | **~0.6-0.85 GB** |
These are targets built from measured pieces (R1 and R2 are measured; R3-R10 are estimates). The numbers stay honest only after each step is measured.

## 4. Order of work (each step has a gate)
1. **R0 - release-build baseline (0.25 day):** repeat `mem_breakdown` with `--release` on a quiet machine and on the lowest-spec PC available; record WS and RTF per variant. *Gate: arena-off keeps first audio <= 1.0 s for a 6-word line.*
2. **R1 (0.25 day)** + **R4 tuning (0.25 day).**
3. **R2 (1-1.5 days):** compact dictionary + golden-corpus equality test + mutation check (flip an entry, test must fail). Largest single saving.
4. **R3 (0.5 day).**
5. **R9 and R6 measurements (0.5 day each, need your camera / offline test),** then implement only what measures well.
6. **R8, R7, R10, R11** as time allows.

## 5. Decisions needed
1. Is a **C-grade British male** (bm_george / bm_fable) acceptable as JARVIS for now, with a better custom/blended voice built later (doc 129 T4)? Or should the default persona be the A-grade `af_heart`?
2. OK to run R0-R4 (no feature change, only memory/speed settings plus the dictionary format)?
3. Do you want me to measure hand mode, the Hub window and ghost mode while you open them (needs you at the machine)?

## 6. Not verified
Release-build speed and the speed cost of arena-off; the real saving of R3, R6-R10 (estimates); blend quality; the 2026 TTS-Arena ranking; the grade list reflects training data, not a listening test. One run per variant (no repeats). The memory test runs inside a unit-test process (~10-15 MB harness overhead), so absolute numbers are slightly high.

## Sources
- Kokoro-82M model card + voice grades: https://huggingface.co/hexgrad/Kokoro-82M , https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md
- ONNX Runtime memory options (arena, memory pattern): https://www.mintlify.com/microsoft/onnxruntime/performance/memory-optimization (third-party mirror; check onnxruntime.ai)
- sherpa-onnx (Moonshine tiny / Whisper tiny.en support): https://github.com/k2-fsa/sherpa-onnx
- misaki-rs data loading: crate source `misaki-rs-0.6.0/src/data.rs`, `data/*.json` (local)
