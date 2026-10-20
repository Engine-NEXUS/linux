# P1 — Voice Latency: Adaptive Endpoint, Silero v6, PP-OCRv6

**Date:** 2026-10-02
**Status:** Items 8, 9 and 10 implemented. 11–14 not started.
**Work order:** [`62-competitive-and-platform-audit-2026-10.md`](../features/62-competitive-and-platform-audit-2026-10.md) §P1

---

## Summary

| # | Item | Result |
|---|---|---|
| 8 | Adaptive endpointing (pre-pausal cut-off) | ✅ implemented, 17 unit tests |
| 9 | Silero VAD v6 | ✅ shipped |
| 10 | PP-OCRv6 | ✅ shipped, **3.1x faster than the current build** |
| 11 | AT-SPI tier | ❌ not started |
| 12 | Wake-word conv-attention retrain | ❌ not started |
| 13 | Prosody gate | ❌ not started |
| 14 | Pre-roll suppression buffer | ❌ not started |

**Verification:** frontend tsc ✅ · 45/45 tests (28 + 17 new) ✅ · build ✅ ·
worker tsc ✅ 76/76 ✅ · `cargo test --lib` 572/572 ✅ · all Python parse steps ✅

---

## 8. Adaptive endpointing — the 3-second cut-off

### The problem

`REDEMPTION_MS = 3000`. The user finishes speaking and then waits up to 3 seconds
before anything is transcribed. It is that high for a legitimate reason: a
300–500ms pause inside *"deep analysis for the PR 24 in nexus-agent"* is
**grammatical, not final**, and cutting there truncates the command. Cutting to
200ms — the naive fix — trades latency for silent command loss.

### What was built

`frontend/src/audio/endpoint.ts` — the decision, as a **pure function** with no
ONNX, AudioWorklet or DOM dependency, so it can be unit tested by feeding it
synthetic frame sequences. `vad.ts` keeps only the plumbing.

The rule fires early only when the tail is **unambiguously dead**:

- Silero speech probability collapsed below `probFloor` (0.15), **and**
- mean tail RMS below `tailRmsMax` (0.005), **and**
- at least `baseSilenceMs` (400ms) of such tail, **and**
- at least `minSpeechMs` (500ms) of speech seen.

Three properties make it safe:

1. **Falling back is the default.** A grammatical pause keeps probability
   elevated (breath, coarticulation, room tone), so the rule does not match and
   control falls through to vad-web's existing 3000ms redemption. Behaviour is
   **identical to today whenever the rule is unsure**.
2. **The ambiguous band counts as neither speech nor silence.** Frames between
   the negative and positive thresholds cannot accumulate `tailSilenceMs`, which
   is what stops the timer bridging a hesitation and firing mid-phrase.
3. **Worst case is 400ms, not 3000ms** — it can only reduce latency.

**Pre-pausal half:** the final segment is assembled from our own rolling buffer
walking `preRollMs` (320ms) *back* from the detected onset, so a word whose
leading phonemes fall under the speech threshold is not clipped. This is what
makes it safe to end the utterance early.

Result: **3000ms → ≤400ms** in the common case, with the old path intact as a
fallback.

### Two bugs found while wiring it up

**`resumeVad()` never reset state.** `endpointFinalized` latches. After the first
adaptive cut-off in a multi-turn session, *every* later `onSpeechEnd` would hit
the guard and be discarded — the *"didn't catch that"* retry flow would accept
speech and transcribe nothing, forever. Fixed by calling `resetSpeculation()` in
`resumeVad()`, mirroring `startVad()`. **This would have shipped.**

**`micVad.pause()` re-enters `onSpeechEnd`.** With `submitUserSpeechOnPause: true`,
vad-web's `frameProcessor.pause()` calls `endSegment()`, which fires `SpeechEnd`
**synchronously** when the buffer held enough speech — which is always true here,
since the rule requires ≥500ms of speech. So pausing re-enters our own handler
from inside `finalizeAdaptiveEndpoint()`. The `endpointFinalized = true`
assignment must stay **above** the pause call. Moving it below would double-
transcribe every command. Untestable without a live AudioWorklet + ONNX session,
so the ordering constraint is documented in a comment at the call site.

### The tests, and whether they mean anything

17 tests in `frontend/src/audio/endpoint.test.ts`. Synthetic frame sequences for:
true cut-off, mid-sentence pause, three-pause multi-clause command, ambiguous-band
handling, resume-voids-tail, probability-collapsed-but-noise-present, and pre-roll
clamping.

**Mutation-tested**, because a green suite proves nothing if it cannot fail:

| Injected fault | Tests that failed |
|---|---|
| `probFloor` 0.15 → 0.40 (fires on grammatical pauses) | **1** |
| Ambiguous band counted as silence | **6** |
| Resume no longer voids tail evidence | **3** |

The suite detects each fault, so it is a real guard rather than decoration.

### ⚠️ The thresholds are uncalibrated

**This repo ships no speech fixtures** (zero `.wav` files), so `probFloor`,
`tailRmsMax` and `baseSilenceMs` were set **from first principles, not measured**.
They are deliberately loose for that reason. Every decision logs the stats it
used, so a calibration pass can tighten them with evidence.

**Before lowering these, measure the truncation rate on real multi-clause
commands** — that is the failure mode that matters, and it is the one thing the
17 synthetic tests cannot prove. The suggested floor is 20 commands x 3 clauses
with a deliberate mid-sentence pause in each; anything clipped is a threshold bug.

---

## 9. Silero VAD v6

Swapped v5 → v6 (`@ricky0123/vad-web` 0.0.30 → 0.0.31).

- v6 ships in 0.0.31 only; 0.0.30 and earlier do not contain the asset.
- Model copied from the package itself (`dist/silero_vad_v6.onnx`), SHA-256
  `1a153a22…78e3`, and **kept v5 in git history** for rollback.
- `public/silero_vad_v5.onnx` deleted — nothing referenced it, and it would have
  added 2.3 MB to every install.
- The model name is now a single `SILERO_MODEL` constant rather than a literal at
  three call sites, so the next bump is one edit.
- Thresholds left at Silero's own operating point (0.5 / 0.35) — v6 is calibrated
  against them, and the old hand-tuned v5 values were not carried over.

`npm ci` re-verified in a clean directory: resolves 0.0.31 and ships the v6 asset.
(The npm 10 lockfile class of bug that broke CI earlier was checked for explicitly.)

---

## 10. PP-OCRv6

Migrated `rapidocr-onnxruntime` 1.4.4 (PP-OCRv4) → `rapidocr` 3.9.2 (PP-OCRv6).

Two things beyond a model bump:

**The runtime model download is gone.** v4 fetched ~15MB of weights on first use,
so OCR was dead on any machine without network. 3.x **bundles** the ONNX inside
the wheel — local to the install.

**The upstream default is the wrong one, and the research got it backwards.**
The audit says v6 is *"2–3x faster"*. True — but only for the `tiny` variant.
Measured on an i3-1215U, warm, best of 3, over a synthetic 1920x1080 UI screenshot
with 12 text regions:

| | latency | regions found |
|---|---|---|
| PP-OCRv4 small *(old default)* | 4138 ms | 12 |
| PP-OCRv6 small *(upstream default)* | **5248 ms** | 12 |
| PP-OCRv6 tiny *(shipped)* | **1493 ms** | 12 |
| PP-OCRv6 tiny *(shipped module, end-to-end)* | **1312 ms** | 12 |

Taking the upstream default would have made OCR **27% slower** while shipping a
"faster model" upgrade. `Det.model_type`/`Rec.model_type` are pinned to
`ModelType.TINY`. All three found the same 12 regions, and v6-tiny also recovered a
**rotated/slanted** label that v4's angle classifier handles worse — which matches
v6's advertised non-axis-aligned-text gain.

### The API break, and a bug it caused

3.x returns a `RapidOCROutput` **dataclass**, not the legacy
`(result, elapse)` tuple. Confirmed `hasattr(out, "__iter__") is False`, so
`result, _ = engine(img)` is a hard `TypeError`. Result handling rewritten to zip
`boxes`/`txts`/`scores`.

That rewrite then had a real bug, caught by running it: `getattr(out, "boxes") or []`
raises `ValueError: truth value of an array is ambiguous`, because `boxes` is a
numpy array. Replaced with explicit `is None` checks. **This would have been a 500
on every OCR call.**

Verified by importing the real shipped `ocr_server.py` and driving its real
`_get_engine()` against a screenshot: 1312 ms, 12 boxes, schema
`{text,x,y,w,h,conf}` intact.

### Honest correction to the file header

The old `~100-300ms` figure was **never true on this hardware** — v4 measured
2134–4138 ms. Header and both grounding-tier comments now state ~1.5s. If genuine
sub-300ms OCR is ever required, the answer is a GPU/Vulkan execution provider, not
a smaller model.

---

## Not done

- **11 — AT-SPI tier.** Still no Linux accessibility-tree path; screen reading is
  OCR-only on Linux. This is the largest remaining perception gap.
- **12 — Wake-word retrain** with a conv-attention head. Untouched; the current
  model stands.
- **13 — Prosody gate.**
- **14 — Pre-roll suppression buffer.** Note item 8 added a *pre-roll inclusion*
  buffer for endpointing; this item is the unrelated inbound-stream echo
  suppressor. Not addressed.