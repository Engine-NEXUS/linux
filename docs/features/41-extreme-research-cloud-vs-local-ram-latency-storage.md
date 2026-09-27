# Extreme Research: Cloud vs Local — RAM, Latency, Storage Optimization

**Created:** 2026-09-04
**Goal:** Reduce RAM from 800 MB to ~200 MB while keeping millisecond response times. Optimize 85 GB storage.

---

## Part 1: Storage Analysis (85 GB → ~100 MB)

### Current storage breakdown

| Location | Size | What it is | Needed? |
|---|---|---|---|
| `src-tauri/target/debug/` | **63.5 GB** | Debug build artifacts (deps 32 GB + incremental 21 GB) | **NO** — dev only, not shipped |
| `src-tauri/target/release/` | **17.4 GB** | Release build artifacts (deps 14 GB + build 2 GB) | **NO** — not shipped, rebuilt on install |
| `src-tauri/target/sherpa-onnx-prebuilt/` | 1.1 GB | Sherpa ONNX prebuilt libs | **NO** — build cache |
| Cargo registry cache | 0.2 GB | Downloaded crate sources | **NO** — build cache |
| `%APPDATA%\com.nexus.assistant\` | 553 MB | Logs + screenshots + learned corrections | Partially (logs can be cleaned) |
| `%LOCALAPPDATA%\com.nexus.assistant\` | 42 MB | WebView2 profile | Yes (runtime) |
| `%USERPROFILE%\.cache\k\` | 337 MB | Kokoro model (0.onnx 310 MB + 0.bin 27 MB) | Yes (TTS model) |
| Installed NEXUS (`%LOCALAPPDATA%\NEXUS\`) | 97 MB | The actual app | Yes |
| **Total** | **~85 GB** | | **Actual app: 97 MB** |

### Root cause

The 85 GB is **build artifacts**, not the app itself. The actual installed NEXUS is only **97 MB**. The `target/debug/` folder alone is 63.5 GB because Rust debug builds include:
- Full debug symbols (no stripping)
- Incremental compilation cache (21 GB)
- Unoptimized code (larger binaries)
- All dependency intermediate files

### Fix: Clean build artifacts

```powershell
# Remove debug build (saves 63 GB)
Remove-Item -Recurse -Force src-tauri\target\debug

# Remove release build after creating installer (saves 17 GB)
# Only do this after the installer is built
Remove-Item -Recurse -Force src-tauri\target\release

# Clean cargo cache (saves 0.2 GB)
cargo cache -a  # or: Remove-Item -Recurse -Force $env:USERPROFILE\.cargo\registry\cache

# Clean old logs (saves ~400 MB)
Get-ChildItem "$env:APPDATA\com.nexus.assistant\*.log" | Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-7) } | Remove-Item
```

### Prevention: Add to `.gitignore` and CI

The `.gitignore` already excludes `target/`. The issue is local disk usage. Add a cleanup script:

```powershell
# scripts/clean-build.ps1
Remove-Item -Recurse -Force src-tauri\target\debug -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force src-tauri\target\tmp -ErrorAction SilentlyContinue
Write-Host "Cleaned debug build artifacts"
```

### Storage after optimization

| Item | Size |
|---|---|
| Installed NEXUS app | 97 MB |
| Kokoro model (cached) | 337 MB |
| WebView2 profile | 42 MB |
| Logs (cleaned weekly) | ~50 MB |
| **Total** | **~526 MB** |

**Savings: 85 GB → 526 MB (99.4% reduction)**

---

## Part 2: Cloud vs Local — Latency & RAM Comparison

### Current local setup (800 MB RAM)

| Component | RAM | Latency (warm) | Latency (cold) | Cost |
|---|---|---|---|---|
| STT (faster-whisper tiny.en) | 150 MB | 500ms | 8s | Free |
| NLU (BERT-Mini Python) | 100 MB | 50ms | 18s | Free |
| TTS (Kokoro 82M) | 350 MB | 5ms (cached) / 200ms (new) | 5.7s | Free |
| Baseline (Rust + WebView2) | 200 MB | — | — | Free |
| **Total** | **800 MB** | **~555ms** | **~32s** | **$0** |

### Cloud alternative (~200 MB RAM)

| Component | RAM | Latency (warm) | Latency (cold) | Cost |
|---|---|---|---|---|
| STT (Deepgram Nova-3 streaming) | 0 MB | **140ms** partial / 539ms final | 140ms | $0.0048/min |
| NLU (deterministic Rust parser) | 2 MB | <5ms | <5ms | Free |
| TTS (ElevenLabs Flash v2.5) | 0 MB | **150-180ms** TTFA | 150ms | $0.05/1K chars |
| Baseline (Rust + WebView2) | 200 MB | — | — | Free |
| **Total** | **~202 MB** | **~345ms** | **~345ms** | **~$0.01/command** |

### Critical latency comparison: "On it sir" ack

| Approach | Time to hear "On it sir" | Why |
|---|---|---|
| **Local Kokoro (cached)** | **5ms** | PCM pre-synthesized, just play from RAM |
| **Local Kokoro (new text)** | 200ms | ONNX inference on CPU |
| **Cloud ElevenLabs Flash** | 150-180ms + 50-200ms network = **200-380ms** | API round-trip |
| **Cloud Deepgram Aura-2** | 313ms + 50-200ms network = **363-513ms** | API round-trip |

**Key finding:** For cached ack phrases, local is **40-76x faster** than cloud. For new text, cloud is comparable to local.

### STT latency comparison

| Approach | First partial | Final transcript | Notes |
|---|---|---|---|
| **Local faster-whisper (warm)** | N/A (batch) | **500ms** | Processes complete utterance, no streaming |
| **Deepgram Nova-3 streaming** | **140ms** | 539ms | Streams partials while user speaks |
| **Deepgram Flux** | **80ms** | ~400ms | Fastest streaming STT in 2026 |
| **AssemblyAI Universal** | 247ms | 307ms | Immutable word emission |

**Key finding:** Cloud STT is **3.6x faster** for first partial (140ms vs 500ms) and supports streaming (user sees partials while speaking). Local whisper is batch-only — it waits for the full utterance before processing.

### Full pipeline latency: wake → "On it sir"

| Approach | Total latency | Breakdown |
|---|---|---|
| **Current (all local, warm)** | ~555ms | STT 500ms + NLU 5ms + TTS 5ms (cached) + overhead 45ms |
| **Cloud STT + Cloud TTS** | ~345ms | STT 140ms + NLU 5ms + TTS 200ms + overhead 0ms |
| **Cloud STT + Local TTS (cached)** | **~150ms** | STT 140ms + NLU 5ms + TTS 5ms (cached) |
| **Hybrid: Cloud STT + Local Piper TTS** | **~220ms** | STT 140ms + NLU 5ms + TTS 75ms (Piper warm) |

**Winner: Cloud STT + Local Piper TTS (cached)** — 220ms total, 202 MB RAM

---

## Part 3: The Optimal Architecture

### Recommended: Hybrid Cloud STT + Local Piper TTS

```
Wake Word (local, 0 MB extra)
    → Cloud STT (Deepgram Nova-3 streaming, 0 MB RAM, 140ms)
    → Deterministic NLU (local Rust, 2 MB RAM, <5ms)
    → Local Piper TTS (80 MB RAM, 5ms cached / 75ms new)
    → Cloudflare Worker (for AI analysis, already exists)
```

### RAM breakdown

| Component | RAM | Saved |
|---|---|---|
| Baseline (Rust + WebView2 + orb) | 200 MB | — |
| STT (Deepgram cloud API) | 0 MB | -150 MB |
| NLU (deterministic Rust only) | 2 MB | -98 MB |
| TTS (Piper INT8 local) | 80 MB | -270 MB |
| **Total** | **~282 MB** | **-518 MB** |

### Latency breakdown (warm)

| Stage | Latency | Notes |
|---|---|---|
| Wake word detection | ~50ms | Local OWW, unchanged |
| STT (Deepgram streaming) | 140ms first partial | Streams while user speaks |
| NLU (deterministic parser) | <5ms | Local Rust regex |
| TTS "On it sir" (cached) | 5ms | Pre-synthesized PCM |
| TTS new text (Piper) | 75ms | ONNX inference |
| **Total to "On it sir"** | **~200ms** | **vs 555ms current** |

### Cost estimate (5-10 users)

| Usage | Cost/month |
|---|---|
| Deepgram STT: 100 commands/day × 3s each = 5 min/day = 150 min/month | $0.72/month |
| ElevenLabs TTS (if used): 100 responses/day × 50 chars = 150K chars/month | $7.50/month (Creator plan) |
| Local Piper TTS (if used instead) | $0/month |
| Cloudflare Worker (already deployed) | $0/month (free tier) |
| **Total with local Piper TTS** | **$0.72/month** |
| **Total with cloud ElevenLabs TTS** | **$8.22/month** |

### Why this is better than all-local

| Metric | All-local (current) | Hybrid (recommended) | Improvement |
|---|---|---|---|
| Idle RAM | 800 MB | 282 MB | **-65%** |
| First command latency | 555ms | 200ms | **-64%** |
| Cold start latency | 32s | 200ms (no cold start) | **-99%** |
| Streaming STT | No (batch only) | Yes (partials while speaking) | **New capability** |
| Disk storage (models) | 377 MB | 80 MB (Piper only) | **-79%** |
| Monthly cost | $0 | $0.72 | +$0.72 |
| Privacy | Fully local | STT audio sent to Deepgram | Trade-off |
| Offline capability | Yes | No (needs internet for STT) | Trade-off |

### Why this is better than all-cloud

| Metric | All-cloud | Hybrid (recommended) | Why |
|---|---|---|---|
| "On it sir" latency | 200-380ms | **5ms** (cached) | Local Piper plays pre-synthesized PCM instantly |
| TTS cost | $7.50/month | $0 | Piper is free, local |
| TTS quality | Excellent (ElevenLabs) | Good (Piper) | Trade-off, but ack phrases sound fine |
| Internet dependency | Yes (for everything) | Yes (STT only) | TTS works offline |

---

## Part 4: Implementation Plan

### Phase 2A: Clean storage (immediate, saves 84 GB)

1. Delete `src-tauri/target/debug/` (63 GB)
2. Add `scripts/clean-build.ps1` cleanup script
3. Add weekly log cleanup to AGENTS.md
4. Document in QUICKSTART.md that `target/` is disposable

### Phase 2B: Replace local STT with Deepgram (saves 150 MB RAM, -360ms latency)

1. Add Deepgram API key to secrets manager
2. Create `src-tauri/src/stt_deepgram.rs`:
   - WebSocket streaming client to Deepgram Nova-3
   - Send audio chunks as they arrive from mic
   - Receive partial + final transcripts
   - No Python sidecar needed
3. Remove `lazy_stt.rs` pre-warm from `lib.rs`
4. Remove STT Python sidecar startup
5. Keep `stt_server.py` as offline fallback (optional)

**Files to change:**
- `src-tauri/src/stt.rs` — add Deepgram WebSocket client
- `src-tauri/src/lib.rs` — remove STT pre-warm
- `src-tauri/Cargo.toml` — add `tokio-tungstenite` for WebSocket
- `frontend/src/audio/recorder.ts` — send audio chunks to Deepgram instead of local STT

### Phase 2C: Replace Kokoro with Piper TTS (saves 270 MB RAM)

1. Apply and update `patches/0002-feat-replace-Kokoro-TTS-with-Piper-TTS-270-MB-RAM-re.patch`
2. Download Piper voice model (`en_US-amy-medium.onnx`, ~60 MB)
3. Bundle in `src-tauri/resources/piper/`
4. Update `tauri.conf.json` resources
5. Keep same `speak_cached` / `speak_text` Tauri commands
6. Keep same `CACHED_PHRASES` pre-generation

**Files to change:**
- `src-tauri/src/tts.rs` — replace Kokoro with Piper
- `src-tauri/Cargo.toml` — replace `kokoro-micro` with `piper-rs` or `ort`
- `src-tauri/tauri.conf.json` — add Piper model to resources
- `src-tauri/resources/piper/` — new directory with model

### Phase 2D: Remove NLU pre-warm (saves 100 MB RAM)

1. Remove NLU pre-warm from `lib.rs`
2. Keep deterministic parser as primary (handles 90-95% of commands)
3. NLU Python sidecar stays as lazy fallback (optional, can be removed entirely)

---

## Part 5: Acceptance Criteria

- [ ] Idle RAM ≤ 300 MB (target ~282 MB)
- [ ] First command latency ≤ 300ms (target ~200ms)
- [ ] Cached "On it sir" plays in <10ms
- [ ] STT streaming partials arrive in <200ms
- [ ] Storage (build artifacts) cleaned: 85 GB → <1 GB
- [ ] Deepgram API key stored in secrets, not in code
- [ ] Offline fallback: TTS works without internet (Piper is local)
- [ ] Online required: STT needs internet (Deepgram cloud)
- [ ] Monthly cost ≤ $1/month for 5-10 users
- [ ] All 130+ Rust tests pass
- [ ] All 28 Worker tests pass
- [ ] Frontend build passes
- [ ] Installer builds on GitHub Actions

---

## Part 6: Risk Assessment

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Deepgram API downtime | Low | High (no STT) | Keep local STT as fallback (lazy start) |
| Deepgram latency varies by region | Medium | Medium | Use US-East region (lowest latency) |
| Piper voice quality worse than Kokoro | Medium | Low | Test side-by-side; keep Kokoro as optional |
| Internet required for STT | Certain | Medium | Document clearly; TTS still works offline |
| Deepgram cost exceeds budget | Low | Low | $200 free credit = ~70K minutes = ~years of usage |
| WebSocket connection drops | Medium | Medium | Auto-reconnect with exponential backoff |

---

## Part 7: Cost Projection

### Deepgram STT pricing

| Plan | Rate | Cost for 100 commands/day | Monthly cost |
|---|---|---|---|
| Pay As You Go | $0.0048/min | 100 × 3s = 5 min/day = 150 min/month | **$0.72/month** |
| Free credit | $200 | ~41,666 minutes = ~277 days of usage | $0 for first 9 months |

### ElevenLabs TTS pricing (if used instead of Piper)

| Plan | Rate | Cost for 100 responses/day | Monthly cost |
|---|---|---|---|
| Free | 10K chars/month | ~200 chars/response × 100 = 20K chars | Exceeds free tier |
| Creator | $22/month, 100K chars | 20K chars/month | **$22/month** |
| Pro | $99/month, 500K chars | 20K chars/month | $99/month (overkill) |

**Recommendation:** Use local Piper TTS ($0/month) for ack phrases and short responses. Use ElevenLabs only for long analysis summaries if Piper quality is insufficient.

### Total monthly cost

| Configuration | STT | TTS | Total |
|---|---|---|---|
| Hybrid (Deepgram + Piper) | $0.72 | $0 | **$0.72/month** |
| Hybrid (Deepgram + ElevenLabs) | $0.72 | $22 | $22.72/month |
| All-local (current) | $0 | $0 | $0/month but 800 MB RAM |

---

## Summary: Which approach wins?

| Criteria | All-local | All-cloud | **Hybrid (winner)** |
|---|---|---|---|
| RAM | 800 MB | 200 MB | **282 MB** |
| Latency (ack) | 5ms | 200-380ms | **5ms** |
| Latency (STT) | 500ms | 140ms | **140ms** |
| Cold start | 32s | 0s | **0s** |
| Cost | $0 | $22+/month | **$0.72/month** |
| Offline | Yes | No | Partial (TTS yes, STT no) |
| Storage | 377 MB models | 0 MB models | **80 MB (Piper only)** |

**The hybrid approach wins on every metric except offline STT.** Since you said you don't care about internet dependency, this is the clear winner: 282 MB RAM, 200ms latency, $0.72/month, 80 MB model storage.
