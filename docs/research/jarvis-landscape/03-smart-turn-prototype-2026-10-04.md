# Smart Turn v3.2 Prototype — Status & Findings (2026-10-04)

**Status: built and verified standalone; NOT wired into capture. Accuracy benefit is unproven.**

## What was added

| Item | Path | Notes |
|---|---|---|
| Model (BSD-2) | `src-tauri/resources/smart_turn/smart-turn-v3.2-cpu.onnx` | 8,679,182 B, SHA-256 `2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f`, from `huggingface.co/pipecat-ai/smart-turn-v3` (downloaded with your approval) |
| Module | `src-tauri/src/turn_detect.rs` (`pub mod turn_detect;` in `lib.rs`, `#[allow(dead_code)]`) | Whisper log-mel front-end in pure Rust + ONNX runner + pure endpoint policy `should_end_turn` |
| Dependency | `ort = "=2.0.0-rc.12"` in `Cargo.toml` | Same version/features `piper-rs` already resolves — `Cargo.lock` changes by 1 line |
| Tests | `cargo test --lib turn_detect` → 3/3 | mel parity, load+run+latency, policy table |

The existing energy endpointer in `wakeword_oww.rs` (`STT_SILENCE_CHUNK_LIMIT` 10 / patient 15 chunks of 80 ms) is **unchanged**.

## Verified

* **Feature parity:** Rust log-mel vs HuggingFace `WhisperFeatureExtractor(chunk_length=8, do_normalize=True)`:
  max abs difference **3.0e-5** over all 80×800 values (test pins 8 sample points + 2 region sums).
* **Preprocessing matches the official `inference.py`:** last 8 s, normalise over real samples, right-pad zeros,
  output is already a probability (no sigmoid), threshold 0.5.
* **Runtime:** tract 0.23 **cannot** run this model (`Failed analyse for node "node_conv1d" ConvHir` — quantized Conv1d), so it runs on ONNX Runtime (`ort`).
* **Rust vs Python runtimes** agree to ≈0.01 on 12 speech-like clips (e.g. 0.9486 vs 0.9357, 0.9750 vs 0.9777).

## NOT verified / caveats

1. **Discrimination on real human speech is untested.** I only had Windows SAPI TTS clips. Complete ("open whatsapp") and
   cut-off ("send a message to") phrases both scored 0.92–0.99 — expected, because TTS reads every phrase with
   finished-sentence prosody and the model keys on human intonation/hesitation. This says nothing for or against the model.
2. **Quantized model is unstable on out-of-distribution input:** a pure tone gave 0.118 (Python onnxruntime 1.29) vs 0.171
   (Rust `ort` rc.12) from byte-identical features. Do not pin probabilities in tests.
3. **Zero right-padding saturates the model at ≈0.98 for *any* input** in my probes, including noise and digital silence;
   left-padding (audio right-aligned) did respond to content. The official code right-pads, so I followed it — but this
   asymmetry should be settled with real recordings before shipping (try both padding modes in the evaluation).
4. **Latency:** see §Latency below.

## Latency

Measured with `CARGO_PROFILE_TEST_OPT_LEVEL=3` (optimized), 8 s window, **first call: 129 ms** — that includes the
one-time twiddle/filter table build and ONNX warm-up, so steady state should be lower (not separately measured).
Unoptimized debug builds took 710 ms, so never judge latency from a debug build.
129 ms is acceptable for an endpoint check on a worker thread (the existing silence window is 800 ms), but the
naive O(n²) DFT is the dominant cost; swapping in a real FFT is an easy win if needed.
The optimized build is a separate profile, so it left extra artifacts in `src-tauri/target` — clean if disk matters.

## Proposed integration (when evidence supports it)

In the capture callback, once `silence >= MIN (3 chunks ≈ 240 ms)` and `< HARD (10/15 chunks)`, hand the last ≤8 s of the capture
buffer to a worker thread (never run the model in the audio callback), and stop early only if
`TurnDetector::predict(..) ≥ 0.5` — `turn_detect::should_end_turn(silence, 3, hard, prob, 0.5)`.
If the model is missing/slow/errors, behaviour is identical to today (wait for the hard limit). A settings flag
(default **off**) gates it.

Expected benefit: *shorter* latency when the user is clearly done (stop at ~240–400 ms instead of 800 ms) and *fewer cut-offs*
when they trail off mid-thought (keep listening past 800 ms). Neither effect is measured yet.

## Evaluation needed from you (≈10 minutes)

Record ~30 real utterances through the normal mic path, tagged:
* 10 complete commands ("open WhatsApp", "what's the weather")
* 10 mid-thought pauses ("send a message to… <1 s pause> …mom saying hi")
* 10 trailing-off/filler ("open the… um…")

Then run the model on each prefix + trailing silence and compare against today's 800 ms rule. A recording helper can reuse
`scripts/record_wake_samples.py`'s capture path; I haven't written it yet.
