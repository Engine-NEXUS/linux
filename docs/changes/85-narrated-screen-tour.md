# Change 85 — Narrated screen tour ("Nexus, analyse my screen")

**Date:** 2026-10-05 · **Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\no-lets-plan-how-shiny-cherny.md` · **Flag:** `screenTour` in `settings.json` (default **on**; legacy chain is the fallback)

## What the user asked for
Say "analyse my screen / explain what I can see / what am I seeing / what is on my screen". The full-screen overlay then points at one thing (ring + arrow) and a callout box explains it **while TTS speaks — one callout at a time, never ahead of the speech**. TTS is an overview only; the detailed analysis goes to the sidebar. Orb flow: thinking after STT → short ack ("On it sir") → orb hides while we fetch → orb returns while narrating → hides at the end.

Decisions: orb hides + spinner during the fetch · in a meeting: sidebar only (no overlay, no speech, no ack) · sidebar opens at the END and the overlay clears · "what is on my screen / what am I seeing" use the vision tour; "read / scan / copy my screen" stay on free OCR.

## Problems found in the existing code (and what we did)
| Finding | Action |
|---|---|
| "what am I seeing", "explain what I can see" did not parse | parser family 3 extended (+ misfire tests) |
| "what is on my screen" was routed to OCR-only | new `wants_narrated_tour` sends it to the tour; pure transcription verbs stay OCR |
| Capture contained the stage (orb/spinner/old pins) | stage temporarily `WDA_EXCLUDEFROMCAPTURE` around the capture (`stage::set_capture_excluded`), not permanent (would hide NEXUS from the user's recordings) |
| Red axis grid on the screenshot (can occlude small text) | tour sends a clean 1280-px capture (`capture_plain_jpeg_base64_w`); grid unchanged for click grounding |
| Spatial vision output cap 2048 tokens | tour call uses 4096; `gemini_json_with_image` is now the shared Gemini ladder (spatial path refactored onto it) |
| `speak()` stops previous audio, no queue; end signal ~0.5 s late | Rust-side narration player `tts::narrate` — one rodio queue, one source per line, queue position == line index |
| Ack handler parks the orb in `speaking`; `hideOrbAfterSpeech` only hides on `idle` | isolated fix in `net/screenTour.ts` (after the ack audio ends, state→idle + hide); shared Ack handler untouched |
| `run_screen_analysis` skipped State/Ack/Loading and never checked cancel | tour runner emits them and checks the cancel flag after every step; a watcher stops TTS when a newer request supersedes |
| Stage CSP drops React `style` props (existing `SpatialAnnotationLayer` likely broken in release) | new overlay uses classes, SVG attributes and ref+CSSOM only |

## How it works
`orchestrator::run_screen_analysis` → (flag on and `wants_narrated_tour`) → `run_screen_tour`:
1. `State{Thinking}` → `Ack` ("On it sir." / "Sure sir." / "I'll get back to you sir.") → `screen:tour_phase{fetching}` → loading spinner. Frontend hides the orb once the ack has finished.
2. Capture with the stage excluded → Gemini (`screen_tour::analyze_tour_image`, shared quota ledger) → `validate_script` (pure): fenced/prose/truncated JSON salvage, ≤5 items, duplicate (IoU>0.7) and degenerate boxes dropped, full-screen boxes lose the pointer ring, **word budgets enforced deterministically** (overview ≤20, spoken ≤17, callout title ≤4 / text ≤18, total spoken ≤~90), ids renumbered so sidebar card N == callout N.
3. Wait for the ack to finish → `screen:tour_start` (orb returns, speaking) → `tts::narrate([overview, item 1…n, "Full breakdown is in the sidebar, sir."])`.
4. `NarrationTracker` turns (items appended, items still queued) into `Started(i)/Ended(i)`; `TourEngine` (pure, injected clock) turns those into `Show(item) / Clear / End`. **Invariants (tested, including a random event soup): `Show` only after `Clear`; nothing after `End`; `End` exactly once.** Only the CURRENT callout is ever sent to the frontend (`screen:callout`), so nothing can be drawn ahead of speech.
5. End: `screen:tour_end` → sidebar Spatial view (existing; zero sidebar changes) → `Done` (orb resets/hides). Cancel (barge-in, Esc, "stop", hotkey, new request) → overlay clears, no sidebar, no `Done`; frontend closes the orb unless a newer turn took over.
6. No audio (offline TTS failure / no output device) → timed silent tour: dwell = clamp(words×330+1200, 2500, 9000) ms; the callouts carry the text.
7. Any failure before narration (no key, quota, capture failure, unusable script) → `screen:tour_phase{fallback}` and the existing chain (spatial → OCR → UIA) continues unchanged.

## Files
Rust: `screen_tour.rs` (new, pure core + Gemini glue) · `tts.rs` (`narrate`, `NarrationTracker`, generic read helpers) · `orchestrator.rs` (`run_screen_tour`, gate in `run_screen_analysis`) · `vision.rs` (`gemini_json_with_image`, plain capture, helper visibility) · `stage.rs` (`set_capture_excluded`, `is_shown`) · `intent_parser.rs` · `commands.rs` · `lib.rs`.
Frontend: `stage/TourOverlay.tsx`, `stage/tour.css`, `stage/tourGeometry.ts`, `stage/tourState.ts`, `net/screenTour.ts` · edits: `stage/main.tsx`, `stage/ResponseCaption.tsx` (hidden during a tour — the callout already shows the text), `stage/OrbFrame.tsx` (init), `audio/ttsPlayer.ts` (`setNarrationPlaying`).

## Verification
- `cargo test --lib -- --test-threads=1`: **961 passed, 0 failed** (6 ignored dev helpers; baseline 927 → +34): script validation (clean/fenced/prose/truncated-salvage/garbage/degenerate/duplicate/full-screen/cap/word budgets/total budget), narration plan, dwell clamps, engine (happy path, duplicates, jump-forward, cancel in every state, timed mode, clear-before-next sweep), routing positives + OCR negatives, setting default, `NarrationTracker` (gapless, underrun gap, missed polls, ordering walk), parser phrases + misfire guards. No new compiler warnings in touched files.
- Frontend: `tsc --noEmit` clean; vitest **182/182** (+23: `tourGeometry` 15, `screenTour` orb flow 8).

## Not verified — needs a live run (honest list)
* That GDI BitBlt honours `WDA_EXCLUDEFROMCAPTURE` toggled at runtime for the stage (the sidebar backdrop already relies on it for a permanently excluded window — strong precedent, not proof). Debug aid: set `NEXUS_TOUR_DUMP_DIR` to dump the exact JPEG sent to the model as `tour_capture.jpg`.
* Gemini box accuracy on real screens (clean vs gridded; 1280 px), latency, and whether 4096 tokens ever truncates (salvage handles it, but fewer items).
* `Sink::len()` boundary vs audible start (WASAPI latency may need a constant 50–150 ms offset), per-line synthesis latency on the offline Kokoro voice.
* Stage window size vs monitor at 125/150 % scaling (the stage is created 1920×1080 logical and never fitted to the monitor; the overlay maps by viewport/monitor ratio so a mismatch degrades proportionally rather than offsetting, but this is unconfirmed).
* SVG rendering under the release CSP, orb hide/show timing, overlay over fullscreen video, primary display only.
* The `screenTour` flag has no Settings UI toggle (edit `settings.json`).

## Deviations from the plan
* No Gemini `responseSchema` (risk of silent 400s → permanent fallback); JSON mime type + prompt contract + tolerant validation, as the existing spatial path.
* The ack-finish wait happens just before narration (capture + model call overlap the ack audio) instead of before capture.
* Deferred to v2/v3 as planned: OCR/UIA snap-to-element, persistent numbered pins, `stage_fit_to_primary`, follow-ups ("tell me more about number two"), multi-monitor.
