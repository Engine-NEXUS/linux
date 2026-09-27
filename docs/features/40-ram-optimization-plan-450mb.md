# RAM Optimization Plan — 800 MB → 450 MB

**Goal:** Save 300-400 MB of idle RAM while maintaining fast latency and the same working state.

**Current state (after Phase 1 pre-warm):** ~800 MB idle
**Target state:** ~450 MB idle (saves ~350 MB)

**Created:** 2026-09-04
**Branch:** `phase-1` (implemented), `phase-2` (this plan)

---

## Current RAM Breakdown (800 MB)

| Component | RAM | File | Status |
|---|---|---|---|
| Baseline (Rust + WebView2 + orb) | ~200 MB | `src-tauri/src/lib.rs`, `tauri.conf.json` | Already optimized |
| STT (faster-whisper tiny.en INT8) | ~150 MB | `server/stt_server.py`, `src-tauri/src/lazy_stt.rs` | Already optimal |
| NLU (BERT-Mini Python sidecar) | ~100 MB | `server/nlu_server.py`, `src-tauri/src/lazy_nlu.rs` | Pre-warmed at startup |
| TTS (Kokoro 82M ONNX) | ~350 MB | `src-tauri/src/tts.rs`, `Cargo.toml` | Pre-warmed at startup |
| **Total** | **~800 MB** | | |

---

## The Two Cuts (Saves ~350 MB)

### Cut 1: Kokoro TTS → Piper TTS (Saves ~250 MB)

| Metric | Kokoro (current) | Piper (target) |
|---|---|---|
| Model | Kokoro 82M ONNX FP16 | Piper VITS ONNX INT8 |
| Model RAM | ~350 MB | ~80-100 MB |
| Disk size | 310 MB + 27 MB voices | 60-80 MB |
| TTFA (warm, new text) | ~200ms | 50-110ms |
| Cached ack latency | ~5ms | ~5ms (same approach) |
| Voice quality | Excellent (af_sky, 82M params) | Good (en_US-amy-medium) |
| Voice character | Natural, expressive | Clear, assistant-style |

**Why Piper is the right choice:**
- Fastest CPU TTS in 2026 benchmarks (RTF ~0.07-0.12, 59x real-time on modern CPU)
- Time-to-first-audio: 50-110ms warm (faster than Kokoro's 200ms for new text)
- Widely used in production edge devices
- ONNX Runtime compatible (can share runtime with NLU if we move NLU in-process later)
- INT8 quantization reduces memory ~50-70% with negligible quality impact

**Voice quality trade-off:**
- Kokoro 82M produces more natural, expressive speech (better prosody, intonation)
- Piper medium voices are clear and professional but less expressive
- For short ack phrases ("On it sir", "Didn't catch that sir"), the difference is minimal
- For longer responses (analysis summaries), Piper is still clear but less natural
- **Recommendation:** Test both side-by-side with your actual phrases before committing

**Existing work:**
- Unapplied patch: `patches/0002-feat-replace-Kokoro-TTS-with-Piper-TTS-270-MB-RAM-re.patch`
- This patch adds `piper-rs 0.2` as a dependency and rewrites `tts.rs`
- The patch needs to be reviewed, updated, and tested against the current codebase

### Cut 2: Stop NLU Pre-warm → Keep Lazy (Saves ~100 MB)

| Metric | Pre-warmed (current) | Lazy (target) |
|---|---|---|
| Idle RAM | ~100 MB | 0 MB |
| First ambiguous command | ~0.1s | ~18s (cold start) |
| Subsequent commands | ~0.1s | ~0.1s (after warm) |
| Idle timeout | 300s | 300s |
| Deterministic parser coverage | 90-95% | 90-95% (unchanged) |

**Why this is safe:**
- The deterministic parser in `intent_parser.rs` already handles 90-95% of commands
- NLU is only a fallback for paraphrased/ambiguous commands the regex parser can't match
- The 18s cold start only happens on the FIRST ambiguous command per session
- After that, NLU stays resident for 300s (5 min) and responds in ~50ms
- Most users will never hit the NLU fallback in a typical session

**What stays pre-warmed:**
- STT: stays pre-warmed (~150 MB) — needed for every voice command, 8s cold start is unacceptable
- TTS: stays pre-warmed (~100 MB with Piper) — needed for "On it sir" ack, must be instant

---

## Target RAM Breakdown (450 MB)

| Component | RAM | Change |
|---|---|---|
| Baseline (Rust + WebView2 + orb) | ~200 MB | No change |
| STT (faster-whisper tiny.en INT8) | ~150 MB | No change (stays pre-warmed) |
| NLU (BERT-Mini Python) | 0 MB | Lazy (saves ~100 MB) |
| TTS (Piper INT8) | ~100 MB | Replaces Kokoro (saves ~250 MB) |
| **Total** | **~450 MB** | **Saves ~350 MB** |

---

## Implementation Steps

### Step 1: Apply and Update the Piper TTS Patch

**File:** `src-tauri/src/tts.rs`, `src-tauri/Cargo.toml`

1. Review `patches/0002-feat-replace-Kokoro-TTS-with-Piper-TTS-270-MB-RAM-re.patch`
2. Update the patch to work with the current codebase (it was written before Phase 1 changes)
3. Add `piper-rs` to `Cargo.toml` (or use `ort` crate directly with Piper ONNX models)
4. Rewrite `tts.rs` to use Piper instead of Kokoro:
   - Keep the same `TtsState` struct
   - Keep the same `speak_text` and `speak_cached` Tauri commands
   - Keep the same `CACHED_PHRASES` array
   - Keep the same `ensure_engine_loaded()` pattern
   - Keep the same parallel cache pre-generation
   - Replace `kokoro_micro::TtsEngine` with Piper engine
5. Download Piper voice model:
   - Recommended: `en_US-amy-medium.onnx` (~60 MB)
   - Place in `src-tauri/resources/piper/`
   - Update `tauri.conf.json` resources to bundle it
6. Update `espeak-ng-data` path (Piper uses the same espeak-ng backend)
7. Remove `kokoro-micro` from `Cargo.toml` (or keep as optional feature)

**Testing:**
- Build Rust release
- Verify TTS pre-warm logs show Piper loading
- Verify cached phrases play correctly
- Verify "On it sir" sounds clear
- Verify longer responses are intelligible
- Compare latency: Piper TTFA vs Kokoro TTFA

### Step 2: Stop NLU Pre-warm

**File:** `src-tauri/src/lib.rs`

1. Remove the NLU pre-warm `std::thread::spawn` block (lines ~520-530)
2. Keep the TTS and STT pre-warm blocks
3. NLU will revert to lazy startup via `lazy_nlu::ensure_nlu_running()` on first unparseable command
4. Keep the increased readiness wait (30s) and idle timeout (300s) from Phase 1

**Before:**
```rust
// NLU pre-warm (currently in lib.rs)
std::thread::spawn(|| {
    tracing::info!("nlu: startup pre-warm starting...");
    lazy_nlu::ensure_nlu_running();
    lazy_nlu::mark_nlu_request();
    tracing::info!("nlu: startup pre-warm complete");
});
```

**After:**
```rust
// NLU stays lazy — deterministic parser handles 90-95% of commands.
// NLU cold-starts on first ambiguous command (~18s) but stays resident
// for 300s after that. Saves ~100 MB idle RAM.
```

**Testing:**
- Verify deterministic commands work instantly (open, close, analyse, architect)
- Verify first ambiguous command triggers NLU cold start (check logs for "starting NLU server")
- Verify subsequent ambiguous commands are fast (~50ms)

### Step 3: Update AGENTS.md and Docs

**Files:** `AGENTS.md`, `QUICKSTART.md`

1. Update RAM table in `AGENTS.md`:
   - Idle: ~450 MB (was ~800 MB)
   - TTS: Piper ~100 MB (was Kokoro ~350 MB)
   - NLU: 0 MB idle, ~100 MB when active (was pre-warmed ~100 MB)
2. Update `QUICKSTART.md` troubleshooting if voice sounds different
3. Document the Piper voice model location and how to swap voices

### Step 4: Verify RAM and Latency

1. Launch NEXUS
2. Wait 5 minutes for warm state
3. Check `nexus.exe` RSS in Task Manager (should be ~350 MB)
4. Check `python.exe` (STT) RSS (should be ~150 MB)
5. Total should be ~450 MB (±50 MB)
6. Test voice command latency:
   - Say "Nexus" → "open architecture mapper"
   - Measure time from end of speech to "On it sir"
   - Should be ~1-2s (TTS pre-warmed + cached, STT pre-warmed)
7. Test ambiguous command (forces NLU cold start):
   - Say "Nexus" → "can you look at the codebase for me"
   - First time: ~18s (NLU cold start)
   - Second time within 5 min: ~1-2s (NLU warm)

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Piper voice quality worse than expected | Medium | Medium | Test side-by-side before committing; keep Kokoro as fallback feature |
| Piper patch doesn't apply cleanly | High | Low | Manual reimplementation; patch is a reference, not a drop-in |
| NLU cold start frustrates users | Low | Medium | Deterministic parser handles 90-95% of commands; NLU rarely needed |
| Piper model download fails on fresh clone | Low | High | Bundle model in `resources/piper/` via Tauri resources config |
| espeak-ng data path wrong for Piper | Medium | Low | Already configured for Kokoro; Piper uses same espeak-ng backend |

---

## Future-Proofing (Optional, Low Priority)

### Trait Abstractions

Add traits so models can be swapped via config without code changes:

```rust
// src-tauri/src/engines.rs
#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn synthesize(&self, text: &str) -> Result<Vec<f32>, String>;
    async fn is_ready(&self) -> bool;
}

pub trait NluEngine: Send + Sync {
    fn parse(&self, text: &str) -> Option<ParsedIntent>;
}

#[async_trait]
pub trait SttEngine: Send + Sync {
    async fn transcribe(&self, audio: &[f32]) -> Result<String, String>;
}
```

Implementations:
- `PiperTts` (primary), `KokoroTts` (optional high-quality profile)
- `DeterministicNlu` (primary), `OnnxNlu` (fallback)
- `FasterWhisperStt` (only implementation needed)

Config file (`config.toml`):
```toml
[tts]
engine = "piper"  # or "kokoro" for high-quality mode
model = "en_US-amy-medium"
cache_phrases = true

[nlu]
primary = "deterministic"
fallback = "bert_mini"  # or "none" to disable ML fallback
lazy_start = true

[stt]
engine = "faster_whisper"
model = "tiny.en"
compute_type = "int8"
prewarm = true
```

This allows swapping engines without recompilation and is the "solve it forever" layer.

---

## Acceptance Criteria

The plan is complete when:

- [ ] Idle RAM ≤ 500 MB (target ~450 MB) measured via Task Manager after 5 min warm
- [ ] First voice command latency ≤ 3s (TTS pre-warmed + cached, STT pre-warmed)
- [ ] Cached ack phrases ("On it sir") play in <10ms
- [ ] Piper TTS voice is clear and intelligible for all ack phrases
- [ ] Deterministic commands work instantly (no NLU needed)
- [ ] Ambiguous commands trigger NLU cold start (check logs)
- [ ] Frontend build passes
- [ ] Rust release build passes
- [ ] All 130+ tests pass
- [ ] NEXUS launches and runs for 10+ minutes without crashes

---

## Comparison Summary

| Metric | Phase 1 (current) | Phase 2 (this plan) | Delta |
|---|---|---|---|
| Idle RAM | ~800 MB | ~450 MB | **-350 MB** |
| First command latency | ~2-3s | ~2-3s | Same |
| Cached ack latency | ~5ms | ~5ms | Same |
| STT latency | ~0.5s | ~0.5s | Same |
| NLU latency (warm) | ~0.1s | ~0.1s | Same |
| NLU latency (cold) | 0s (pre-warmed) | ~18s (lazy) | Worse, but rare |
| TTS voice quality | Excellent (Kokoro) | Good (Piper) | Slight downgrade |
| TTS new-text TTFA | ~200ms | 50-110ms | **Faster** |
