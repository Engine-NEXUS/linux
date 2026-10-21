# Phase D Close-Out — Shipped, Deferred, and Why (2026-09-27)

## Shipped

- **D2 declarative specs** (`agent_specs.rs`): `intents.yaml` user intents
  (open + say actions only), fallback-slot priority (built-ins win),
  strict validation, first-run example, 11 tests.
- **D3 edge-case miner** (`improve.rs`): clusters missed intents +
  compound failures into `suggested_phrases.json`, `improvement_report`
  command, boot run. Record + suggest ONLY — no auto-training, no
  spec writes, no actions (safety invariant holds). 6 tests.

## Deferred (deliberately, not slipped)

### D1 composable backends (gRPC) — DEFERRED
A tonic/prost gRPC layer would add a heavy dependency tree for a
single-device assistant that already has 2-tier fallbacks on every
engine (edge→Piper, Groq→Moonshine, deterministic→BERT→Qwen→Worker).
The marginal value is low until a second device class exists (the
Flutter companion app, which speaks nexus-json-v1 over HTTPS — no
gRPC needed). If multi-device local inference ever lands, the seam
map is: `tts.rs::synthesize_with_fallback`, `stt.rs::transcribe_*`,
`router.rs::route_question` — these three functions are the backend
boundaries to trait out. No refactor for its own sake before then.

### D4 on-device wake word (ESP32) — DEFERRED (hardware-gated)
Requires ESP32-S3 hardware + Espressif tooling to verify; cannot be
tested in this environment. The path is documented for when hardware
exists: export the `nexus.onnx` classifier + mel frontend via
openWakeWord's ESP32 pipeline (microWakeWord-compatible footprint),
keeping the 0.68 threshold and 80ms windowing. No unverifiable code
shipped.

## Scoreboard

| Phase | Items | Tests added | Rust total |
|-------|-------|-------------|------------|
| A (quick wins) | 5 | 9 (PII) | 549 → 558 |
| B (high-impact) | 5 | 29 (mem 8, tts 6+6, cmds 5, cc 4) | 558 → 587 |
| C (strategic) | 5 | 26 (vision 8, diary 4, proto 3+3, piper 5, webhook 6) | 587 → 613 |
| D (ecosystem) | 2 shipped, 2 deferred | 17 (specs 11, miner 6) | 613 → 630 |
| **Total** | **17 shipped** | **+81** | **549 → 630** |

All green twice (serial), zero new clippy warnings, FE 33/33 + tsc
throughout, Worker 52/52.
