# Build Warnings Zero-Out, Stage Cold-Boot Resilience & Daemon Exit Prevention (2026-10-07)

> Provenance note: this file was reconstructed from the AGENTS.md entry of the same date after a
> filename collision during parallel-session work briefly replaced it. Content below is faithful to
> that entry; the owning session should diff and amend anything lost.

## Problem & Motivation
1. Build logs emitted 23 Rust compiler warnings (`unused_imports`, unregistered IPC commands, dormant helper functions) and Vite chunk warnings (>500 kB).
2. Launching via `nexus start` triggered a premature blackout (`stage: blackout detected (window_gone=false, stale_beat=true) — incident #1`) because `SHOW_GRACE_SECS` was only 8s, expiring before WebView2 completed cold-boot initialization (~8.1s).
3. Watchdog window recreation destroyed the `stage` window; because 0 windows remained, Tauri v2's default exit behavior shut down the entire daemon.

## Root Cause & Fixes
- `frontend/vite.config.ts`: Set `build.chunkSizeWarningLimit: 2000` to silence chunk warnings for the unified sidebar.
- `src-tauri/src/window_manager.rs` & `ocr.rs`: Removed unused `Emitter` and `IAsyncOperation` imports.
- `src-tauri/src/lib.rs`: Registered `tts::restore_tts_volume`, `architect::cancel_architect_analysis`, and `architect::query_impact` in `generate_handler!`; updated runner to intercept `RunEvent::ExitRequested` and call `api.prevent_exit()`.
- `src-tauri/src/browser_center.rs`, `system_center.rs`, `youtube_center.rs`, `improve.rs`, `pii_filter.rs`, `vision.rs`: Added `#[allow(dead_code)]` annotations to dormant utility functions and structs.
- `src-tauri/src/stage.rs`: Increased `SHOW_GRACE_SECS` from 8s to 20s and `HEARTBEAT_STALE_SECS` from 6s to 10s.

## Verify
- `npm run build`: clean (0 errors, 0 warnings).
- `npm test -- --run`: 200/200 pass.
- `cargo check --features custom-protocol,admin-brain`: clean (**0 warnings, 0 errors**).
- `cargo test --lib -- --test-threads=1`: 1020/1020 pass.
- `cargo build --release --features custom-protocol,admin-brain`: fresh release binary (96.0 MB) produced cleanly.
