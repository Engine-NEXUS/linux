# Phase 2 Implementation Plan — Groq STT + edge-tts TTS + Fallbacks

**Branch:** `phase-2` (off `phase-1`)
**Created:** 2026-09-04
**Architecture:** Cloud-first with local fallbacks

---

## Architecture: 3-Tier Fallback Chain

```
PRIMARY (cloud, $0 forever):
  STT → Groq Whisper Large v3 Turbo (user's free API key, 2,000 req/day)
  TTS → edge-tts Microsoft Neural (free, no key, 400+ voices)
  NLU → Deterministic Rust parser (local, <5ms)

FALLBACK 1 (local, lazy-loaded on failure):
  STT → faster-whisper tiny.en (Python sidecar, 150 MB, 8s cold / 500ms warm)
  TTS → Piper INT8 (local ONNX, 80 MB, 40ms warm)
  NLU → BERT-Mini ONNX (Python sidecar, 100 MB, 50ms)

FALLBACK 2 (local, always available):
  TTS → eSpeak-NG (5 MB, robotic but never fails)
  NLU → Deterministic Rust (always works, 90-95% coverage)
```

## RAM: 800 MB → 202 MB (saves 598 MB, 75% reduction)

| Component | Phase 1 (current) | Phase 2 (primary) | Phase 2 (fallback active) |
|---|---|---|---|
| STT | 150 MB (pre-warmed) | 0 MB (Groq cloud) | 150 MB (faster-whisper) |
| NLU | 100 MB (pre-warmed) | 2 MB (deterministic) | 100 MB (BERT-Mini) |
| TTS | 350 MB (Kokoro) | 0 MB (edge-tts cloud) | 80 MB (Piper) |
| Baseline | 200 MB | 200 MB | 200 MB |
| **TOTAL** | **800 MB** | **202 MB** | **530 MB** |

**Normal operation: 202 MB.** Fallbacks only load if cloud fails.

## Latency: 555ms → 257ms (2.2x faster)

| Metric | Phase 1 | Phase 2 (cloud) | Phase 2 (fallback) |
|---|---|---|---|
| Cold start | 32s | **0s** | 8s (STT only) |
| STT | 500ms | **247ms** (Groq) | 500ms (whisper) |
| TTS ack (cached) | 5ms | **5ms** | 5ms |
| TTS new text | 90ms | **~200ms** (edge-tts) | 40ms (Piper) |
| **Wake → "On it sir"** | **555ms** | **257ms** | **757ms** |

## Cost: $0/month forever (10 users, per-user Groq keys)

| Component | Cost | Free Tier |
|---|---|---|
| Groq STT | $0 | 2,000 req/day per user, forever |
| edge-tts TTS | $0 | No key, no limit, no account |
| Piper fallback | $0 | Local |
| faster-whisper fallback | $0 | Local |
| Deterministic NLU | $0 | Local |
| Cloudflare Worker | $0 | Free tier |
| **Total** | **$0** | **Forever** |

## Storage: 410 MB → 133 MB (saves 277 MB)

| Model | Phase 1 | Phase 2 |
|---|---|---|
| Kokoro (0.onnx + 0.bin) | 337 MB | 0 MB (removed) |
| Piper (en_US-amy-medium.onnx) | 0 MB | 60 MB (fallback) |
| faster-whisper tiny.en | 40 MB | 40 MB (fallback, cached) |
| NLU BERT-Mini | 18 MB | 18 MB (fallback) |
| espeak-ng-data | 15 MB | 15 MB (shared) |
| **Total** | **410 MB** | **133 MB** |

---

## Implementation Phases

### Phase 2A: Groq Cloud STT (saves 150 MB, -253ms latency)

**Steps:**
1. Add `groq_api_key` to `NexusSettings` struct + Settings UI
2. Create `src-tauri/src/stt_groq.rs` — Groq Whisper API client
3. Update `src-tauri/src/stt.rs` — route to Groq if key present, fallback to local
4. Remove STT pre-warm from `src-tauri/src/lib.rs`
5. Register `stt_groq` module in `lib.rs`
6. Build + test

### Phase 2B: edge-tts Cloud TTS (saves 350 MB, better quality)

**Steps:**
1. Add `edge-tts-rust` to `Cargo.toml`, remove `kokoro-micro`
2. Rewrite `src-tauri/src/tts.rs` — edge-tts primary, Piper fallback, eSpeak last resort
3. Create `src-tauri/src/tts_edge.rs` — edge-tts client (WebSocket to Microsoft)
4. Create `src-tauri/src/tts_piper.rs` — Piper fallback (lazy ONNX load)
5. Keep same `speak_text`, `speak_cached`, `stop_tts` Tauri commands
6. Keep same `CACHED_PHRASES` pre-generation (edge-tts at boot, stored as PCM)
7. Update `tauri.conf.json` — add Piper model to resources, remove Kokoro
8. Download Piper voice model to `src-tauri/resources/piper/`
9. Build + test

### Phase 2C: Remove NLU Pre-warm (saves 100 MB)

**Steps:**
1. Remove NLU pre-warm block from `src-tauri/src/lib.rs`
2. Keep deterministic parser as primary (handles 90-95% of commands)
3. NLU Python sidecar stays as lazy fallback (only starts on ambiguous command)
4. Build + test

### Phase 2D: Settings UI + User Onboarding

**Steps:**
1. Add "Cloud STT" section to Settings → Backend tab
2. Add Groq API key input field (password type)
3. Add "Get Free Key" button → opens console.groq.com
4. Add voice selection dropdown (edge-tts voices)
5. Add connection status indicator (Groq + edge-tts)
6. Build + test

### Phase 2E: Testing + Verification

**Steps:**
1. Run all Rust tests (130+ existing)
2. Run frontend build + TypeScript check
3. Run Worker tests (28 existing)
4. Manual test: boot RAM, first command latency, ack quality
5. Manual test: Groq fallback (disable internet, verify local STT kicks in)
6. Manual test: edge-tts fallback (disable internet, verify Piper kicks in)
7. Build installer via GitHub Actions
8. Test on fresh laptop

---

## File Changes Summary

| File | Change | Phase |
|---|---|---|
| `src-tauri/src/commands.rs` | Add `groq_api_key`, `tts_voice_edge` to NexusSettings | 2A, 2D |
| `src-tauri/src/stt_groq.rs` | NEW: Groq Whisper API client | 2A |
| `src-tauri/src/stt.rs` | Route to Groq, fallback to local | 2A |
| `src-tauri/src/tts_edge.rs` | NEW: edge-tts client | 2B |
| `src-tauri/src/tts_piper.rs` | NEW: Piper fallback | 2B |
| `src-tauri/src/tts.rs` | Rewrite: edge-tts primary, Piper fallback | 2B |
| `src-tauri/src/lib.rs` | Remove STT + NLU pre-warm, add new modules | 2A, 2C |
| `src-tauri/Cargo.toml` | Add edge-tts-rust, piper deps; remove kokoro-micro | 2B |
| `src-tauri/tauri.conf.json` | Add Piper model to resources | 2B |
| `src-tauri/resources/piper/` | NEW: Piper voice model files | 2B |
| `frontend/src/settings/SettingsApp.tsx` | Add Groq key field, voice selector | 2D |
| `frontend/src/audio/stt.ts` | Update comments (Groq primary) | 2A |
| `frontend/src/audio/ttsPlayer.ts` | Update voice list (edge-tts voices) | 2D |

---

## Acceptance Criteria

- [ ] Idle RAM ≤ 250 MB (target ~202 MB) with Groq key set
- [ ] First command latency ≤ 300ms (target ~257ms) with Groq
- [ ] Cached "On it sir" plays in <10ms
- [ ] edge-tts voice quality ≥ Kokoro quality
- [ ] Groq STT produces correct transcripts
- [ ] Fallback to local STT works when Groq fails (internet off)
- [ ] Fallback to Piper TTS works when edge-tts fails (internet off)
- [ ] Fallback to eSpeak works when Piper fails
- [ ] No Python processes start at boot (unless fallback triggered)
- [ ] Groq API key saves/loads from settings.json
- [ ] Voice selection dropdown shows edge-tts voices
- [ ] All 130+ Rust tests pass
- [ ] All 28 Worker tests pass
- [ ] Frontend build passes
- [ ] Release build succeeds
- [ ] Monthly cost = $0 (Groq free tier + edge-tts free)
- [ ] Installer builds on GitHub Actions
