# Third-Party Notices (reuse-first work — partial)

This file lists third-party material **copied or adapted into this repository** by the reuse-first plan
(`docs/research/jarvis-landscape/`). It is **not yet a complete inventory**: the full dependency licence
inventory (Rust crates, npm packages) is still to be generated — see doc 02 §"Next actions". Entries below
were added together with the code they cover.

| Material | Where in this repo | Upstream | Licence | Notes |
|---|---|---|---|---|
| Silero VAD v4 model `silero_vad_v4.onnx` (SHA-256 `a35ebf52fd3ce5f1469b2a36158dba761bc47b973ea3382b3186ca15b1f5af28`) | `src-tauri/resources/vad/` | copied from `cjpais/Handy` `src-tauri/resources/models/`, model by `snakers4/silero-vad` | MIT | Silero's own copyright line **not yet verified from source** — confirm before distribution |
| VAD design (Silero v4 driven at 512-sample frames with carried LSTM state; hysteresis idea) | `src-tauri/src/vad.rs` | `cjpais/Handy` `src-tauri/src/audio_toolkit/vad/silero.rs` (+ `vad-rs`) | MIT — Copyright (c) 2025 CJ Pais | Re-implemented on `ort` (no `vad-rs` dependency); structure follows Handy |
| Smart Turn v3.2 model `smart-turn-v3.2-cpu.onnx` (SHA-256 `2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f`) | `src-tauri/resources/smart_turn/` | `pipecat-ai/smart-turn` (HuggingFace `pipecat-ai/smart-turn-v3`) | BSD-2-Clause (per repo page; copyright holder to confirm — Pipecat's own is "Daily") | |
| Whisper log-mel front-end for Smart Turn | `src-tauri/src/turn_detect.rs` | behaviour matched to HuggingFace `WhisperFeatureExtractor` and the reference code in `pipecat-ai/pipecat` `src/pipecat/audio/turn/smart_turn/_whisper_features.py` | BSD-2-Clause — Copyright (c) 2024–2026, Daily (Pipecat) | Independent Rust implementation, parity-tested (max diff 3e-5) |
| Turn-policy design (eagerness low/medium/high) | `src-tauri/src/turn_detect.rs` | concept from OpenAI Realtime `semantic_vad` docs | n/a (documentation of a closed API; no code) | Case 2 in doc 04 |
| Barge-in strategy design | `src-tauri/src/wakeword_oww.rs` (`barge_stop_decision`) | design influence: `pipecat-ai/pipecat` `src/pipecat/turns/*` | BSD-2-Clause | No code copied |
| Kokoro-82M model `model_quantized.onnx` (int8, 92,361,116 B, SHA-256 `fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478`) and voice packs (522,240 B each) | downloaded at runtime into `%APPDATA%/com.nexus.assistant/voices/`; dev copy in git-ignored `src-tauri/resources/kokoro/` | `hexgrad/Kokoro-82M`, ONNX conversion `onnx-community/Kokoro-82M-v1.0-ONNX` at revision `1939ad2a8e416c0acfeecc08a694d14ef25f2231` | Apache-2.0 (model card) | Not committed. Voices ship in the same repo; no separate licence notes found |
| Kokoro vocabulary table (114 symbols), 510-phoneme chunking idea | `src-tauri/src/tts_kokoro.rs` | `pguso/kokoro` (`tokenizer.rs`, `pipeline.rs`) | Apache-2.0 | Table generated from upstream source; synthesis glue re-written on our `ort` |
| `misaki-rs` 0.6 (English G2P, `default-features = false`) | Cargo dependency | `MicheleYin/misaki-rs` | MIT | **Dictionary data** is "based on the original Misaki" and was partly generated with espeak-ng output (its `expand_dicts.py`) — provenance/legal status unresolved for distribution (doc 09 §6). espeak itself is NOT linked |
| (removed) Piper TTS / `piper-rs` / bundled `espeak-ng-data` | — | — | GPL-3 | Removed 2026-10-05 (change 81) |
| Test fixture `sapi_open_whatsapp_16k.wav` | `src-tauri/tests/fixtures/` | generated locally with Windows SAPI | n/a | Synthetic speech, test-only |

## MIT licence text (Handy, Silero)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation the
rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit
persons to whom the Software is furnished to do so, subject to the following conditions: The above copyright notice and
this permission notice shall be included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED
"AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## BSD 2-Clause licence text (Pipecat / Smart Turn)

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
following conditions are met: 1. Redistributions of source code must retain the above copyright notice, this list of
conditions and the following disclaimer. 2. Redistributions in binary form must reproduce the above copyright notice,
this list of conditions and the following disclaimer in the documentation and/or other materials provided with the
distribution. THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED
WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
