# Change 81 — Kokoro-82M local TTS replaces Piper (GPL espeak-ng removed)

**Date:** 2026-10-05 · **Feature:** 83 (single-slot offline voice swapping) · **Plan:** `docs/research/jarvis-landscape/09-phase-3-kokoro-in-single-slot-architecture-2026-10-05.md`

## Problem
1. `piper-rs → espeak-rs → espeak-rs-sys` statically linked **GPL-3 espeak-ng** into `nexus.exe` (licence audit, doc 02 finding #1).
2. Feature 83's swap worker downloaded a ~60 MB Piper model on every persona change, from an unpinned `…/resolve/main/…` URL, with **no checksum pinned** (all `sha256: None`), and it **dropped later requests while one download ran** — equip A → B → C could leave the offline voice on A.

## Decisions (from you, 2026-10-05)
Online is always first; the local voice is used only when the internet is gone, and NEXUS keeps probing so it flips back to the cloud voice automatically · slot = one shared model + exactly one voice file · Piper removed entirely · dictionary-provenance question parked · download approved.

## What changed
| Area | Change |
|---|---|
| Engine | New `src-tauri/src/tts_kokoro.rs` — Kokoro-82M on the pinned `ort`, G2P = `misaki-rs` with `default-features=false` (**no espeak**), spell-out fallback for unknown words, product lexicon (NEXUS, WhatsApp, Groq…), sentence-aware chunking (≤ 110 phoneme chars: RAM grows with input length), voice **hot-swap without model reload**, lazy load / unload |
| Catalog | `voice_catalog.rs`: `local_model` (Piper stem) → `kokoro_voice`; slot = `kokoro_model.onnx` + `active_voice.bin` + `manifest.json` (v2); legacy Piper slot files are deleted on first sync (frees ~60 MB) |
| Swap worker | `tts_swap.rs` rewritten: shared model downloaded **once** (88 MB, with progress), thereafter a 0.5 MB voice file; **latest request wins**; `.tmp` → size + **pinned SHA-256** + trial-load/parse → atomic rename; downloads from an **immutable HF commit** (`1939ad2a…`); runs at startup (online) and again when the cloud returns |
| TTS chain | `tts.rs`: edge-tts first, then Kokoro only when the cloud is down/unreachable; offline multi-sentence replies **stream** (first audio = one sentence); streaming producer falls back to local per sentence |
| Network | `tts_network.rs`: Piper lifecycle → local-engine lifecycle; new idle unload (10 min with no local speech) in addition to "10 min of stable network"; cloud-restored watchdog (30 s while down) unchanged and now also re-syncs the offline voice |
| Removed | `tts_piper.rs`, `piper-rs` dep, `resources/piper/` (61 MB), `resources/espeak-ng-data/` (8 MB), `setup_espeak_data_path`, the `piper-amy` voice-picker entry (a stored `piper-*` voice id still means "use the offline voice") |
| Frontend | persona type `kokoroVoice`; pill says "Downloading offline engine (one-time) n %" for the base, "Syncing Offline Voice" for a voice; settings copy updated |
| Also fixed | startup ack-phrase cache used a hardcoded Ava voice instead of the equipped persona's cloud voice |

## Verification
* `cargo test --lib -- --test-threads=1`: **902 passed, 0 failed, 4 ignored** (was 888 before this phase's work). `cargo tree -e normal` → **0** `espeak` packages. No new warnings in touched files.
* Frontend: `tsc --noEmit` clean; vitest **154/154**.
* **Intelligibility** (ASR round-trip: 5 sentences × 5 voices, transcribed with Moonshine small): mean WER 0.10 for `af_heart`, `af_bella`, `bf_emma`, `bm_george`; 0.03 for `bm_fable`. Every residual is either number formatting ("2.30" vs "two thirty") or "Stopped typing" heard as "Stop typing" (4 of 5 voices — a real, minor pronunciation issue on a cached phrase). This measures *intelligibility*, not naturalness.
* **Hashes:** the SHA-256 Hugging Face advertises (`X-Linked-ETag`) matched the locally computed hash for the model, `af_heart` and `bm_george`; the other four catalog voices were pinned from the same headers (not downloaded).

## Measured performance (honest)
| | Result | How |
|---|---|---|
| Speed | **≈ real-time**: RTF 0.69–0.75 in the Rust test profile; 0.99 (best, 4 threads) up to 1.7 in a Python onnxruntime check — noisy | i7-1355U laptop, int8 model. **First audio for a 3 s sentence ≈ 2–3 s**, hence offline streaming. Piper's quoted "~40 ms" was never re-measured here |
| RAM | +128 MB on model load; **259 MB** after a short sentence, 369 MB at 120 tokens, 654 MB at 300 tokens | Python harness (working set); this is why chunks are capped at 110 phoneme chars |
| Disk | 88.1 MB model + 0.5 MB voice ≈ **89 MB** (was ~63 MB Piper) | |

## Known limitations / not done
* **Release build and a live offline/online run were not performed** — swap download, engine hot-swap, cloud↔local flip are covered by unit tests of their pure logic (plan, latest-wins queue, verify-and-install, unload policy, URL/pin tables) but not by an end-to-end run with real network loss.
* **Voice mapping is unaudited** (no listening test yet); Kokoro's British male voices are its weakest (bm_george C, bm_fable C) and there is no Irish voice for FRIDAY.
* **Model variant: chosen = int8** (measured 2026-10-05, see below).
* **Installer does not bundle the model** (downloaded on first online run). A user whose very first launch is offline has no local voice until they have been online once.
* Dictionary-data provenance (misaki-rs's espeak-expanded entries) remains an open distribution-time question (doc 09 §6).

## Model-variant comparison (accuracy per RAM)

| Variant | File | Model RAM (+MB) | Peak RAM (+MB) | Median RTF (4 threads) | WER vs reference text | Transcripts identical to fp32 |
|---|---|---|---|---|---|---|
| **int8 `model_quantized` (chosen)** | 92.4 MB | **129** | **330** | 0.98 | 0.042 | **16/16** |
| q8f16 | 86.0 MB | 152 | 366 | 1.38 | 0.042 | 16/16 |
| fp16 | 163.2 MB | 347 | 553 | 0.74 | **0.250** | 10/16 |
| fp32 `model` | 325.5 MB | 351 | 560 | 0.62 | 0.042 | 16/16 (reference) |

Method: 8 sentences × (US `af_heart`, GB `bm_george`) = 16 clips per variant, identical G2P phonemes, one process per variant (clean RAM), 4 ORT threads on the i7-1355U, audio transcribed back with Moonshine small. WER 0.042 for the three good variants is entirely number formatting ("2.30" vs "two thirty") and one "Stopped"→"Stop"; mean duration difference vs fp32 ≤ 0.04 s.

int8 matches fp32 on intelligibility at 37 % of the model RAM and 28 % of the download; fp16 is rejected (worse accuracy on this CPU); q8f16 is slower and uses more RAM than int8. fp32 is ~1.6× faster but 2.7× the RAM. Not measured: naturalness (a listening A/B is still advisable).
