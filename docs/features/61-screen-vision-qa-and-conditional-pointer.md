# Screen Vision Q&A & Conditional Pointer Architecture Specification

## 1. What this is

NEXUS gains clicky-style screen awareness in two parts: **screen Q&A**
("what's on my screen" → screenshot → vision LLM → spoken answer) and a
**conditional pointer** (a marker that appears only for locate answers —
"where is the Save button" — then auto-hides). Point-only v1: the marker
never clicks. This spec covers Route-C phases 1–4 (MVP, pointer, locator
hardening, voice flow, privacy, settings) plus 5a hardening.

## 2. Pipeline

```
hotkey / wake → STT → intent parse (ScreenDescribe/ScreenLocate/
ScreenReadText + StopSpeech/RepeatScreen/PointAgain)
  → privacy exclusion gate (refuse before ANY capture)
  → UIA find_by_name (Windows, ~5ms, no quota) → speak + show_direct
  → screenshot (portal/GDI/screencapture, 1280px JPEG) [10s cache]
  → vision chain: Gemini flash-lite → Groq vision → structured {speak, point?}
  → speak immediately
  → OCR tier (RapidOCR sidecar) → merge_lines → snap_to_text → show_direct
     └─ on OCR miss/failure: raw VLM coords via maybe_show
```

## 3. Locator tiers (accuracy × cost)

| Tier | Method | Latency | Quota | Accuracy |
|---|---|---|---|---|
| 1 | UIA `find_by_name` (Windows) | ~5 ms | none | pixel-perfect |
| 2 | VLM 1-call coords + OCR snap | ~1 s + ~1 s | 1 VLM call | ~96.7% snap hits (harness) |
| 3 | Raw VLM coords | ~1 s | 1 VLM call | best-effort |

`score_match` (shared fuzzy scorer, all tiers): exact 1.0 > substring 0.8
> space-insensitive 0.75 > all-words 0.7 > any-word 0.4; hits need ≥ 0.5.
`merge_lines` fuses OCR word splits ("Sign"+"in") with a 1×-line-height gap
limit (a 3× limit fused adjacent rows — measured, fixed).

## 4. Pointer visibility (Table-B, `pointer::decide`, pure + tested)

Show only for locate-with-point; never for describe/read. Dwell 6s
(clamped 3–15s, setting), 0.4s fade. Instant hide on new turn
(`listening`), Escape, `pointer:hide`. Privacy exclusions refuse capture
up front (Windows fg-title; other OSes unenforced — documented). Fullscreen
suppression (Windows rect check). Off-stage targets clamp with `⤢` badge.
Renders in the always-alive main stage (no new window/RAM); CSS transform
positioning (Wayland-safe); `pointer-events: none` always.

## 5. Voice flow (all LLM-free)

`stop speaking` → Rust TTS stop + hide (bare "stop" stays MediaStop).
`repeat (that)` → re-speak last screen answer + re-show fresh marker.
`point again` → re-emit stored coords <120s, 0 quota. Describe/read clears
stale points so repeats never resurrect old markers.

## 6. Files

`screen.rs` (capture/score), `vision.rs` (chain/contract), `pointer.rs`
(conditions/emit), `ocr.rs` + `lazy_ocr.rs` + `server/ocr_server.py:39220`,
`intent_parser.rs` (6 intents), `orchestrator.rs` (flow + caches),
`overlay/PointerOverlay.tsx` + `pointerMath.ts` + `net/pointer.ts`,
`App.tsx` wiring, `SettingsSidebarApp.tsx` Screen tab,
`scripts/vision_accuracy.py` harness.

## 7. Verification (2026-09-27/28, Linux Wayland)

- Rust 543+/serial green (30+ new), tsc clean, vitest 28/28, vite build green
- Live portal capture: 1920×1080 → 1280×720 JPEG (~1s first run incl.
  permission dialog, instant after)
- Live RapidOCR: 0.98 conf, 977ms warm; harness 96.7% over 150 samples
- CI (frontend/rust/python) green on the PR
- NOT yet verified: end-to-end VLM answer (needs API key), visual marker,
  Windows UIA path, macOS permissions
