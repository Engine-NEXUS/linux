# Response Caption — Word-by-Word Reveal Above the Orb — Phase 3 (Claude session, 2026-10-04)

**Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\c-users-chitkul-lakshya-downloads-phone-idempotent-snail.md` (Wakeup Orb Redesign; 5 phases). Phases 1 ([doc 73](73-window-consolidation-orb-into-stage.md)) and 2 ([doc 74](74-orb-shape-color-and-glitch-redesign.md)) landed first. This is **Phase 3 — the spoken reply's response caption, rendered word-by-word above the orb, timed off the TTS engine's own word-boundary data.**

## 1. Rust — real word timing from edge-tts, estimated timing from Piper

### `tts_edge.rs`
- `Boundary::Sentence` → `Boundary::Word` (the two request sites) — this crate's `Boundary` is mutually exclusive per call, and nothing in the codebase reads sentence boundaries.
- Extracted a shared `synthesize_raw(text, voice, emotion: Option<TtsEmotion>)` returning the crate's own `SynthesisResult { audio, boundaries }`, consolidating what were two near-duplicate functions. `synthesize_to_mp3`/`synthesize_to_mp3_with_emotion` keep their existing `Result<Vec<u8>, String>` shape (unchanged — still used by `tts_bench.rs`/`pipeline_bench.rs`) and just discard `.boundaries`. `synthesize_to_pcm`/`synthesize_to_pcm_with_emotion` now return a 3-tuple `(samples, sample_rate, Vec<BoundaryEvent>)` instead of 2 — their only callers are in `tts.rs` (updated) plus one benchmark destructure (updated to `Ok((samples, sr, _boundaries))`).
- `BoundaryEvent { kind, offset_ticks, duration_ticks, text }` ticks are confirmed 100ns units (`TICKS_PER_SECOND = 10_000_000` in the crate's `constants.rs`) — `ms = ticks / 10_000`.

### `tts.rs` — new `CaptionWord`/`CaptionTrack` types + pure helpers
- `CaptionWord { text, start_ms, duration_ms }` and `CaptionTrack { words, total_ms, estimated }` (both `Serialize`, emitted as-is to the frontend).
- `estimate_words(text, total_ms)` — evenly distributes whitespace-split words across a known PCM duration (the Piper fallback path: local VITS ONNX has no word-boundary events). Pure + unit-tested.
- `boundaries_to_words(boundaries, cumulative_ms)` — converts real edge-tts boundaries to `CaptionWord`s, ticks→ms, offset by `cumulative_ms` (non-zero for streamed chunk 2+). Pure + unit-tested (including the offset case).
- `CachedAudio` gained a `caption: CaptionTrack` field — `pregenerate_cache` computes it once at boot (real boundaries from edge-tts, or `estimate_words` if the Piper cache-generation fallback triggered) so the **instant <5ms cache-hit path also gets a caption**, not just freshly-synthesized replies.
- `synthesize_with_fallback` now returns `(Vec<f32>, u32, CaptionTrack)` — all three call sites (`speak_text`, `preview_voice`, `speak_cached`) and the streaming chunk functions (`synthesize_chunk`/`synthesize_chunk_owned`) updated to carry it through.

### Emission point: `play_audio`/`play_audio_streamed`, not synthesis
Per the plan's explicit instruction, `tts:caption` is emitted at **playback start** (`sink.append()`), not at synthesis time — this is what makes cache hits, Piper, and edge-tts all go through one consistent emission point regardless of where the audio came from. Both functions now take an owned `tauri::AppHandle` and only emit if `TTS_GENERATION` still matches `my_generation` (a barge-in that happened during synthesis must never flash a stale caption).

**Streaming risk (explicitly flagged in the plan) — handled**: `play_audio_streamed` tracks a running `cumulative_ms`, incremented by each chunk's `total_ms` — which is computed from the chunk's **own decoded PCM sample count** (`samples.len() * 1000 / sample_rate`), never any engine-reported duration. Chunk 2+'s `CaptionWord.start_ms` values are offset by `cumulative_ms` *before* emitting, so the frontend receives one utterance's words already on a single absolute timeline — it never needs to know synthesis was chunked.

## 2. Frontend

- **`frontend/src/audio/captionScheduler.ts`** (new): listens for `tts:caption`, anchors to `performance.now()` on the first chunk of an utterance, and `setTimeout`s each word's reveal at its `start_ms`. A later chunk's words get scheduled against the **same anchor** (never resets mid-utterance) — this is what makes the Rust-side cumulative-offset design transparent to the frontend. `clearCaptionSchedule()` cancels every pending timer and immediately notifies "cleared" — wired into `ttsPlayer.ts`'s `stopTts()` so a barge-in kills the caption exactly like it kills the audio.
- **`frontend/src/stage/ResponseCaption.tsx`** (new): mounted in `stage/main.tsx` as a sibling of `OrbFrame`/`LoadingIndicator`. Listens to `stage:orb_rect` independently (same self-contained-component pattern `SpatialAnnotationLayer`/`AnnotationCanvas` already use in this file, rather than threading the rect through props) and positions itself a fixed 28px gap above the orb, horizontally centered on it. Each revealed word is its own `<span>` keyed by index, so React only mounts (and CSS-animates in) genuinely new words — already-revealed ones never re-trigger the fade-in. Lingers 1.4s after the "done" signal, then fades; a barge-in clears it immediately instead of waiting for that timer.
- **CSS** (`stage/ghost.css`): `.response-caption`/`.response-caption-word` + a `response-caption-word-in` keyframe (opacity + 5px rise, 260ms).

## 3. Verification

- `cargo check --lib` clean; `cargo test --lib` **870/870** (5 new caption tests: empty text, single word gets full duration, even distribution, ticks→ms conversion, cumulative-offset application).
- `npx tsc --noEmit` clean; `npm run build` clean.
- `npx vitest run`: hit one failure (`orchestrator.test.ts`'s ghost-turn test, an `invokeMock` call-count assertion) on a single run. Bisected via `git stash` + re-running the **pre-Phase-3** code through the full suite, which failed too — on a *different* test (`identityBanner.test.ts`, owned by a concurrent uncommitted session, not touched by this work) — confirming this is pre-existing cross-file test-isolation flakiness (shared module-level state across parallel test workers; AGENTS.md already documents one instance of this class of flake), not a regression from this phase. Re-ran 4 more times with Phase 3's code in place: **154/154 every time**.

## 4. Not done / next

- Phase 4 (live-speech caption via streaming STT — the user's own words growing while they're still talking) and Phase 5 (cleanup) remain pending.
- Not yet manually verified against the real TTS pipeline with actual audio (no live run in this session) — the caption's exact visual timing against real speech is worth a quick live check, same caveat as Phases 1-2.
