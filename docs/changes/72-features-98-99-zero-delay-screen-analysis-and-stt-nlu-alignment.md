# 72 — Features 98+99: Zero-Delay Screen Analysis & Dual-Engine STT-NLU Alignment

**Date:** 2026-10-07
**Status:** Implemented, all gates green ×2.
**Specs:** `docs/features/98-screen-analysis-celebrity-recognition-and-zero-delay-spec.md`, `docs/features/99-dual-engine-stt-nlu-alignment-and-accuracy-fix-spec.md`
**Research:** `docs/research/screen-tour/04-celebrity-recognition-and-zero-delay-architecture-2026-10-07.md`

## Feature 98 — Zero-Delay Screen Analysis (completed across two sessions)

### Phases 1-4 + 6 (previous session, verified in this one)
- **P1 Ladder**: `GEMINI_VISION_MODEL=gemini-3.5-flash-lite` (live 1.27s); `GEMINI_VISION_FALLBACKS=["gemini-3.5-flash","gemini-flash-lite-latest"]`; dead ids (3.8-flash 503, 2.5/2.0-flash 404) removed from `tour_model_ladder`; tests rewritten.
- **P2 Timeouts+Cascade**: screen_tour client 45s→4s (pooled), `TOUR_TOTAL_TIMEOUT_SECS` 75→6; spatial client 20s→4s (pooled); `TourRun::Fallback` no longer cascades into spatial/plain-VLM — speaks local WinRT OCR answer immediately (≤1.5s worst-case failure).
- **P3 Prompts**: rule 4 replaced with the public-figure directive (actively NAME celebrities/YouTubers/politicians; cross-reference facial likeness with on-screen text); test renamed `prompt_is_answer_first_ignores_chrome_and_names_public_figures`; `build_spatial_prompt` instructs full-name identification; `screen_context.rs` gains `ocr_lines` (WinRT OCR, 1.2s bounded) rendered in `prompt_block` as `[On-Screen Context Clues from OS & OCR]`.
- **P4 Speech**: spatial path speaks `payload.overview` ("You're watching …, sir.") with element-count fallback.
- **P6 Cache**: `frame_hash` (FNV-1a) + 60s-TTL bounded cache (`FRAME_CACHE`, 8 entries) in vision.rs; identical screen replays <30ms with zero quota burn.

### This session's leftover
- **P5 capture width**: tour capture 1536px → `VISION_FAST_W` (768px) at `orchestrator.rs:3776` — ~82% smaller base64 upload.

## Feature 99 — Dual-Engine STT ↔ NLU Alignment

- **P1 Vocabulary** (`stt_groq.rs`): `NEXUS_VOCABULARY` expanded to the spec's full command+entity set (ghost control, Brave, YouTube, play/pause/track verbs, memory audit, forget everything, take the mouse, type/press/click/search verbs, Deepgram/Groq entities). Existing budget test (`test_nexus_vocabulary_budget_and_coverage`, ~244-token cap) still passes.
- **P2 Keyterms** (`stt_deepgram.rs`): new `deepgram_listen_url()` appends 13 `&keyterm=` boosts (NEXUS, ghost mode, ghost control, servx, zync, whatsapp, architecture mapper, memory audit, pull request, ghostwriter, deepgram, groq, shopkart) to the Nova-2 listen URL. Unit-tested (all keyterms present, count pinned).
- **P3 Normalizations** (`intent_parser.rs`): new mishearing families — ghost right/goes mode/ghost and go → ghost mode; memory odd it/or dit/odit/order it → memory audit; forget every thing/everthing → forget everything; what's up/what sap/what app + message → whatsapp message; open/launch brave browser → open brave; architecture diagram/digger → architecture mapper (surrounding words preserved). Plus: bare `"memory audit"` / `"audit my memory"` added to `parse_memory_command`'s audit family (the normalized phrase previously matched no pattern). 2 new tests (pure normalization + end-to-end parse).
- **P4 Hallucination guard** (`nlu_client.rs`): `nlu_sanity_check()` — before accepting a ≥0.85 media-family classification, the transcript must contain a semantic anchor (play/pause/track/music/song/video/media/next/previous/spotify/resume/last); failure → None (falls through to brain/Worker instead of misfiring "So you have to list" → MediaPlayPause). Wired into `parse_via_nlu`; non-media intents ungated. Pinned test.
- **P5 Learning loop** (verified, no changes needed): `log_failed_transcript`/`log_successful_transcript` IPC registered (lib.rs) and called from 6 sites in `recorder.ts` (corrected/local/hotkey paths, success+failure) — repeated corrections already promote into `learned_corrections.json` and load at startup.

## Known pre-existing flake (not from these changes)
`nlu_local::test_parse_local_end_to_end_smoke` ("model present but inference failed") fails under default **parallel** test threads (tract model load contention) but passes in isolation and under the repo-standard serial gate. Unaffected by this work (untouched file); the repo gate is serial.

## Verify (each ×2)

- `cargo check --features custom-protocol,admin-brain`: **0 errors, 0 warnings**.
- `cargo test --lib -- --test-threads=1`: **1031 passed, 0 failed** (was 1027; +4 new tests).
- `cargo test --lib vision::` 42, `screen_tour` 33, `orchestrator::` 63, `intent_parser` 205, `nlu` 17, `stt_deepgram` 3.
- Frontend: `npx tsc --noEmit` clean; `npm test -- --run` **200/200** ×2.
- Release: `cargo build --release --features custom-protocol,admin-brain` → `nexus.exe` **91.7 MB** @ 22:58.

## Zero-RAM invariant held
All changes are cloud-side (prompt/URL/timeout edits) or pure string logic — **0 MB local model RAM added**; client baseline unchanged (~177 MB).
