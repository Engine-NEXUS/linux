# Changelog 44 — Screen Vision Q&A & Conditional Pointer

## Summary
NEXUS can now answer questions about what's on screen and point at named
UI targets with a conditional overlay marker (Route-C phases 1–4 + 5a).
Point-only v1: the marker never clicks.

## Key Changes

### 1. Screenshot capture (`screen.rs`)
- Cross-platform, zero new crates: Windows GDI (reuses `sidebar_backdrop`
  helper), Linux xdg-portal Screenshot over existing `zbus`, macOS
  `screencapture` with TCC-specific guidance on denial.
- Portal fast-fail when no portal exists (no 25s timeout on bare X11).
- 1280px JPEG encode with byte-cap retry; original dims kept for
  coordinate mapping (0–1000 normalized maps linearly — verified).

### 2. Vision chain (`vision.rs`)
- Gemini flash-lite (per-user free key, thinking off) → Groq vision
  (`qwen3.6-27b`, `llama-4-scout`) with `{speak, point?}` JSON contract,
  fence-tolerant parsing, 0–1000 validation.

### 3. Locator hardening
- UIA `find_by_name` (Windows): exact boxes, ~5ms, zero quota; wired
  locate-first in the orchestrator.
- RapidOCR sidecar (`server/ocr_server.py:39220` + `lazy_ocr.rs` + `ocr.rs`):
  live-verified 0.98 conf / 977ms warm; `merge_lines` + `snap_to_text`
  (96.7% harness over 150 synthetic samples).
- Shared `score_match` fuzzy scorer (exact 1.0 → any-word 0.4, ≥0.5 hits)
  with space-insensitive containment for OCR space-drops ("Closewindow").

### 4. Conditional pointer + voice flow
- `pointer.rs` Table-B matrix (pure `decide()`); marker in main stage,
  dwell 6s, instant hide on new turn/Escape; 4 settings + Screen tab UI.
- LLM-free `stop speaking` / `repeat` / `point again`; 10s screenshot
  cache; privacy exclusion gate refuses capture before any pixels.

### 5. Verification
- Rust 543+/serial, tsc, vitest 28/28, vite build, CI green.
- Live: Wayland portal capture + RapidOCR + harness (96.7%).
- Pending: VLM end-to-end (needs key), visual marker check, Windows/macOS.

## Files
`screen.rs`, `vision.rs`, `pointer.rs`, `ocr.rs`, `lazy_ocr.rs`,
`server/ocr_server.py`, `intent_parser.rs`, `orchestrator.rs`,
`commands.rs`, `lib.rs`, `tauri.conf.json`, `App.tsx`,
`overlay/PointerOverlay.tsx`, `overlay/pointerMath.ts`,
`net/pointer.ts`, `settings-sidebar/SettingsSidebarApp.tsx`,
`scripts/vision_accuracy.py`.
