# Screen Tour — Implementation Record and Test Results (v1 2026-10-05, v2 2026-10-06)

## A. v1 — narrated tour (change 85)
### Files
Rust: `screen_tour.rs` (new pure core) · `tts.rs` (`narrate`, `NarrationTracker`, generic read helpers) · `orchestrator.rs` (`run_screen_tour`, `apply_tour_actions`, gate in `run_screen_analysis`) · `vision.rs` (`gemini_json_with_image`, plain capture) · `stage.rs` (`set_capture_excluded`, `is_shown`) · `intent_parser.rs` · `commands.rs` · `lib.rs`.
Frontend: `stage/TourOverlay.tsx`, `tour.css`, `tourGeometry.ts`, `tourState.ts`, `net/screenTour.ts`; edits in `stage/main.tsx`, `ResponseCaption.tsx` (hidden during a tour), `OrbFrame.tsx`, `audio/ttsPlayer.ts` (`setNarrationPlaying`).

### Key mechanics
* **Sync**: `narrate` appends one rodio source per line (+250 ms silence); every 20 ms `NarrationTracker.update(appended, sink.len())` emits `Started(i)`/`Ended(i)`; `TourEngine` turns them into `Show/Clear/End`. A synthesis underrun clears the callout instead of leaving it up.
* **Cancel**: `TTS_GENERATION` bump (stop/Esc/wake/hotkey) or the request cancel flag (a watcher calls `stop_tts`) → `End(Cancelled)`; no sidebar, no `Done`; frontend closes the orb after 1.2 s unless a newer turn took over.
* **No audio**: timed dwell `clamp(words×330+1200, 2500, 9000)` ms.
* **Overlay (CSP-safe)**: SVG attributes + classes + ref/CSSOM; ring, leader (`pathLength`=1 dash draw-on), arrowhead at the target, dot at the callout; `placeCallout` prefers right/left/below/above, avoids target and orb, inside-corners for screen-sized targets.
* **Orb**: `net/screenTour.ts` hides the orb after the ack audio (the shared Ack handler is untouched); `tour_phase{fallback}` cancels a pending hide.

### Results (v1)
Rust 961/961 (6 ignored dev helpers, +34); frontend tsc clean, vitest 182/182 (+23: `tourGeometry` 15, `screenTour` 8). One test-first bug caught: callout placement initially chose "nearest" side (below) instead of the documented preference (right); fixed to first-valid-in-preference-order.

## B. v2 — main content, context, stronger model (change 91)
### Files
New `screen_context.rs`. Changed: `screen_tour.rs`, `vision.rs`, `browser_url.rs` (`foreground_window_title/process_name/rect`, `browser_document_rect`, `is_browser_process`), `orchestrator.rs`, `commands.rs`, `lib.rs`. Frontend untouched.

### Behaviour
| Aspect | Before | After |
|---|---|---|
| Capture | whole monitor | page content region (UIA Document → window−inset → full) at 1536 px |
| Context to model | none | app, tab title, trimmed URL, YouTube title+channel, "image is cropped" |
| Overview | ≤20 words | ≤35 words, the direct answer; lists name every item |
| Item speech | ≤17 | ≤22; callout ≤24; total ≤120; `answer` ≤120 words → sidebar overview |
| Output tokens | 4096 | 8192 |
| Model | lite only | `gemini-3.8-flash → 3.5-flash → 3.5-flash-lite → 2.5-flash` (`tourModel` overrides first) |
| 429 | first 429 → provider "exhausted" | advance ladder; exhausted only if every failure was a 429 |
| Daily cap | constant 500 | `geminiVisionDailyLimit` setting (default 500) |
| Sensitive windows | analysed | refused via `live::safety::is_target_blocked` |
| People | model free-guessing | prompt forbids face identification; text evidence only |
| Settings | unknown keys erased on save | `screenTour`, `tourModel`, `geminiVisionDailyLimit` in `NexusSettings` |

### Privacy decisions
URL sent as scheme+host+path only; YouTube reduced to `watch?v=<id>`; credentials/query/fragment dropped; sensitive windows skip capture, lookup and model; oEmbed receives only the clean video URL.

### Results (v2)
* Rust **996/996** passed, 0 failed, 6 ignored dev helpers (baseline 971 from the concurrent sessions; +25 here): `screen_context` (YouTube ids for 8 URL shapes, rejects 6 non-video URLs, URL trimming, oEmbed parsing, classification, region clamp/inset/fallback, sensitive flags, nuts-page and video context assembly), `screen_tour` (cropped-region→screen-px mapping, answer→sidebar overview, answer cap, `tourModel` parsing, prompt is answer-first/ignores chrome/forbids face ID, new budgets), `vision` (ladder order/override/dedupe, historic ladder, `ladder_end` rule, daily-limit parsing, configured limit drives `exhausted`/`mark_exhausted`/`quota_status`).
* Frontend vitest 188/188. `tsc` shows 2 errors in `audio/captionScheduler.test.ts` belonging to another session's in-progress caption work — not part of this change.
* No new compiler warnings in touched files.

## C. Not verified (live run required) — honest list
1. UIA exposes the browser `Document` element on the user's Brave (Chromium enables accessibility lazily). Check with `NEXUS_TOUR_DUMP_DIR` → `tour_capture.jpg`.
2. `WDA_EXCLUDEFROMCAPTURE` toggled at runtime is honoured by GDI BitBlt for the stage (precedent only).
3. `gemini-3.8-flash` / `3.5-flash` accept the request on this project; latency; real daily limits (AI Studio).
4. Answer quality: nuts page names ~7 kinds with pointers on bowls, no tab/URL callouts; a YouTube video states title/channel and what the frame shows; people described not named.
5. Crop alignment at 125/150 % scaling; stage window vs monitor size; oEmbed reachability.
6. `Sink::len()` boundary vs audible start (WASAPI latency); offline per-line Kokoro latency; release-build CSP rendering of the SVG overlay.
7. No Settings UI for `tourModel`/`geminiVisionDailyLimit`.

## D. Live test checklist
Nuts page · YouTube video · VS Code/non-browser app · bank/password-manager window (refused) · say "stop"/Esc mid-tour · cut the network · meeting active · two tours back-to-back · confirm no callout ever precedes its sentence · 429 behaviour after the daily limit.
