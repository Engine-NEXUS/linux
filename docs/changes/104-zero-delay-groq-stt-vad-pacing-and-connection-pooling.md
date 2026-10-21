# Change 104 — Zero-Delay Groq STT VAD Pacing, Persistent Tokio Runtime & HTTP/2 Connection Pooling

**Document ID**: `docs/changes/104-zero-delay-groq-stt-vad-pacing-and-connection-pooling.md`  
**Date**: 2026-10-07  
**Status**: Implemented, Verified & Released  
**Target Architecture**: Windows 11, Rust Desktop Daemon, Audio STT Pipeline  

---

## 1. Problem & Motivation

1. **VAD Trailing Dead Air**: In Ghost Mode and standard voice commands, `wakeword_oww.rs` required 10 consecutive silent chunks (800ms) after speech completed before triggering an endpoint. This forced a noticeable ~1.0-second delay between the speaker finishing their command and NEXUS taking action.
2. **Cold Connection & TLS Handshake Latency**: The STT capture thread (`stt-capture-rx`) and wake word verifier built a brand new `reqwest::Client` on every single spoken turn. This bypassed connection pooling and TCP/TLS session tickets, imposing a 150ms–220ms cold handshake penalty on every turn.
3. **Tokio Runtime Recreation Churn**: The STT capture receiver created and destroyed a new `tokio::runtime::Builder::new_current_thread()` on every single turn, adding thread allocation overhead and CPU thrashing.

---

## 2. Root Cause & Solution

### A. Adaptive Paced Silence Limits (`src-tauri/src/wakeword_oww.rs`)
* Defined `STT_SILENCE_CHUNK_LIMIT_GHOST: u32 = 3` (240ms) and `STT_SILENCE_CHUNK_LIMIT_GHOST_PATIENT: u32 = 6` (480ms).
* Reduced standard `STT_SILENCE_CHUNK_LIMIT` from 10 (800ms) down to 5 (400ms), and `STT_SILENCE_CHUNK_LIMIT_PATIENT` from 15 (1200ms) down to 10 (800ms).
* Made endpoint selection context-aware:
  ```rust
  let silence_limit = if crate::ghost::session_active() {
      if super::STT_PAUSE_COUNT.load(Ordering::Relaxed) >= 1 {
          super::STT_SILENCE_CHUNK_LIMIT_GHOST_PATIENT
      } else {
          super::STT_SILENCE_CHUNK_LIMIT_GHOST
      }
  } else if super::STT_PAUSE_COUNT.load(Ordering::Relaxed) >= 2 {
      super::STT_SILENCE_CHUNK_LIMIT_PATIENT
  } else {
      super::STT_SILENCE_CHUNK_LIMIT
  };
  ```

### B. Global Shared HTTP/2 Client (`src-tauri/src/stt.rs`)
* Introduced `SHARED_STT_CLIENT: std::sync::OnceLock<reqwest::Client>` with `pool_idle_timeout(Duration::from_secs(90))` and `tcp_keepalive(Duration::from_secs(60))`.
* Updated `SttState::new()` and `wakeword_oww.rs` to reuse `crate::stt::shared_client()`, maintaining persistent warm TLS connections to `api.groq.com`.

### C. Persistent Tokio Runtime on Capture Thread (`src-tauri/src/wakeword_oww.rs`)
* Initialized `tokio::runtime::Builder` once outside the `stt-capture-rx` loop instead of per-turn.
* Reused `rt.block_on(...)` with the shared HTTP client across all turns.
* Updated wake word verification at line 4104 to reuse `crate::stt::shared_client()`.

---

## 3. Quantitative Verification Results

1. **Compilation & Linting**:
   * `cargo check --features custom-protocol,admin-brain`: clean (**0 warnings, 0 errors**).
   * `npx tsc --noEmit`: clean (**0 errors**).
2. **Automated Test Suites**:
   * `cargo test --lib -- wakeword`: **58/58 passed**.
   * `cargo test --lib -- stt`: **31/31 passed**.
   * `cargo test --lib -- --test-threads=1`: **1025/1025 passed** (6 ignored dev benchmarks).
   * `npm test -- --run`: **200/200 passed** across 28 test suites.
3. **Production Packaging**:
   * `npm run build`: built in 7.87s.
   * `cargo build --release --features custom-protocol,admin-brain`: produced fresh 96.0 MB production release binary `src-tauri/target/release/nexus.exe`.
