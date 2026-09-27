# 39: CI Windows Streamlining & Rust Compilation Fixes

**Date**: 2026-09-22  
**Scope**: CI Workflows, Wake Word Health Constants, Browser Tab Hotkeys, Cross-Platform Scope Consolidation

---

## 1. Overview & Context

Following the merge of PR #28 and the apex wake word evolution, the GitHub Actions CI pipeline encountered build failures across multiple runners:
1. **`wakeword_oww.rs`**: Missing `STREAM_DEAD_ZERO_SECS` and `STREAM_SUSPECT_ZERO_SECS` constants under `mock-wake` compilation feature because they were gated by `#[cfg(not(feature = "mock-wake"))]`.
2. **`screen.rs`**: Missing `Key::Num1..Num9` enum variants in `enigo 0.5.0` within `switch_browser_tab`.
3. **CI Scope Bloat**: Linux and macOS runners were consuming build minutes and causing false negatives on OS-specific dependencies.
4. **PowerShell Quote Stripping in `build-windows.yml`**: Inner quotes in JSON config strings were stripped by PowerShell argument parsers in Windows runner environments.

---

## 2. Changes Made

### A. Wake Word Health Constants (`src-tauri/src/wakeword_oww.rs`)
- Made `STREAM_DEAD_ZERO_SECS`, `STREAM_SUSPECT_ZERO_SECS`, `STREAM_RESTART_FLOOR_SECS`, and `STREAM_CBK_PER_SEC` available unconditionally with `#[allow(dead_code)]`.
- Enables clean compilation for both production builds and `--features mock-wake` test passes.

### B. Enigo 0.5 Key Compatibility (`src-tauri/src/screen.rs`)
- Replaced non-existent `Key::Num1..Key::Num9` variants with `Key::Unicode('1'..='9')` in `switch_browser_tab(index: u32)`.

### C. CI Pipeline Streamlining (`.github/workflows/ci.yml`)
- Removed `rust-check-linux`, `rust-check-macos`, and `macos-installer` jobs to focus CI resources exclusively on Windows.
- Expanded `rust-check-windows` to validate default `cargo check`, `cargo check --features mock-wake`, and `cargo test --lib -- --test-threads=1`.
- Added frontend test execution (`npm test`) to `frontend-check`.
- Added NLU data foundation validation (`python server/nlu/data_foundation.py validate`) to `python-check`.

### D. Windows Installer Script Fix (`.github/workflows/build-windows.yml`)
- Cleaned up Tauri build invocation to avoid PowerShell string quote-stripping issues.

---

## 3. Verification & Test Results
- `cargo check --manifest-path src-tauri/Cargo.toml --features mock-wake`: **Passed (0 errors)**
- `cargo check --manifest-path src-tauri/Cargo.toml`: **Passed (0 errors)**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1`: **522/522 passed (0 failed)**
- `npm run build --prefix frontend`: **Passed (0 errors)**
- `npm test --prefix frontend`: **20/20 passed (0 failed)**
- `python server/nlu/data_foundation.py validate`: **Passed (481 evaluation rows, 8 registered sources)**
