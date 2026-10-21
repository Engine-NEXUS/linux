# Change 91 — Screen tour v2: explain the MAIN content, with context and a stronger model

**Date:** 2026-10-06 · **Builds on:** change 85 (narrated screen tour) · **Plan:** `C:\Users\Chitkul Lakshya\.claude\plans\no-lets-plan-how-shiny-cherny.md`

## Live finding (user test)
On the Women's Health "Top 10 healthiest nuts" page the tour talked about the **tab strip and URL bar** and only said "this is the kinds of nuts" (no count, no names). On a YouTube video it said "this is the person / this is the product" — not who/what, not what the video was about.

## Root causes
1. **Whole-screen capture** — tabs, URL bar and taskbar were in the image; the prompt asked for "the most important things" with no notion of main content.
2. **No context** — the model only had pixels and "analyse my screen"; the tab title and URL (already readable via `browser_url`) were never passed.
3. **Output shape forced labels** — overview ≤20 words, `spoken` ≤17, callout ≤18, total ≤90, no `answer` field, no instruction to answer the implicit question or enumerate.
4. **Weakest model** — `gemini-3.5-flash-lite`, no stronger fallback.
5. **People** — a vision model will not (and NEXUS should not) identify someone from their face; "who is that" can only come from text evidence (title, channel, captions, on-screen text).

## What changed
| Area | Change |
|---|---|
| Context (`screen_context.rs`, new) | Foreground app, window/tab title, **privacy-trimmed URL** (scheme+host+path; YouTube collapses to the id-only watch URL), and for YouTube the **video title + channel via the public oEmbed endpoint** (no key, 3 s timeout, only the clean video URL leaves the machine, cached). Gathered with a 3.5 s cap; failure = less context, never a failed tour. |
| Crop | Browsers: UI Automation `Document` rect (the web view); fallback window rect minus a toolbar inset; other apps: window rect; anything invalid/tiny/off-monitor → full screen. `vision::capture_region_jpeg_base64` (1536 px wide). `TourScript.region` + `callout_json` map the model's 0–1000 boxes back to real screen pixels. If the cropped capture fails the whole screen is captured and the prompt says so. |
| Prompt | Answer-first and content-aware: decide what the content is; **answer the implicit question** (lists: count + name them all; video: what it is about from title/channel; product: what it is + facts; code/error: what/why); items are the *subjects* of the content, never browser/OS chrome; **never identify anyone from their face** — only text evidence, otherwise "the presenter" worded as inference. |
| Budgets | overview 20→**35** words (it *is* the answer), `spoken` 17→**22**, callout text 18→**24**, total spoken 90→**120**; new `answer` field (≤120 words) becomes the sidebar overview; `details` 3–6 rows; `maxOutputTokens` 4096→**8192** (stronger models spend some on thinking; `validate_script` still salvages truncation). |
| Model | Tour ladder **`gemini-3.8-flash` → `gemini-3.5-flash` → `gemini-3.5-flash-lite` → `gemini-2.5-flash`** (`vision::tour_model_ladder`; `tourModel` setting overrides the first slot). 404 (unknown id) and 429 advance the ladder; the provider is marked exhausted **only if every failure was a 429** (`ladder_end`). Google limits are per model per project, so falling back also adds capacity. |
| Daily limit | New `geminiVisionDailyLimit` setting (default 500, clamped) replaces the hard-coded constant in `exhausted` / `mark_exhausted` / `quota_status`. The 500 was **our assumption**: Google's docs publish no per-model free-tier numbers — the real figures are per project in AI Studio (aistudio.google.com/rate-limit). One tour = 1 request. |
| Settings struct | `screenTour`, `tourModel`, `geminiVisionDailyLimit` added to `NexusSettings`: `save_settings` rewrites settings.json from that struct, so keys outside it were erased on the next Settings save (this also affected `screenTour` from change 85). |
| Safety | `live::safety::is_target_blocked` on app/title/URL (banks, password managers, wallets): nothing is captured or sent, NEXUS says "I won't analyse this window, sir." |

## Files
`src-tauri/src/screen_context.rs` (new) · `screen_tour.rs` · `vision.rs` · `browser_url.rs` (`foreground_window_title/process_name/rect`, `browser_document_rect`, `is_browser_process`) · `orchestrator.rs` (`run_screen_tour`: context → sensitive gate → region capture) · `commands.rs` (settings) · `lib.rs`. No frontend change.

## Verification
- `cargo test --lib -- --test-threads=1`: **996 passed, 0 failed** (6 ignored dev helpers; +25 over the 971 baseline): URL/YouTube-id parsing for every URL shape, privacy trimming, oEmbed parsing, content classification, region clamp/inset/fallback, sensitive-window flags, context assembly for the nuts page and a video, cropped-region → screen-pixel mapping, answer→sidebar overview, new budgets, prompt contract (answer-first, ignores chrome, no face identification), model ladder order/override, `ladder_end` quota rule, settings-driven daily limit. No new warnings in touched files.
- Frontend untouched. `vitest` 188/188. `tsc` reports 2 errors in `audio/captionScheduler.test.ts` — **another session's in-progress caption work**, not part of this change.

## Not verified — needs a live run
* That UI Automation exposes the browser `Document` element on the user's Brave (Chromium enables accessibility lazily; first query can be empty). Fallback = window rect minus a toolbar inset. Check `tour_capture.jpg` (`NEXUS_TOUR_DUMP_DIR`) is cropped to the page.
* Whether `gemini-3.8-flash` / `3.5-flash` accept the request as sent (model ids per Google's model page; a 404 just advances the ladder) and their latency/quota on the user's project.
* Answer quality: the nuts page should name ~7 kinds with callouts on the bowls; a YouTube video should state its title/channel and what the frame shows. People are described, not named, unless the title/channel/on-screen text says so.
* Crop alignment at 125/150 % scaling; oEmbed reachability; the 3-model latency when the first model is rate-limited.
* No Settings UI fields yet for `tourModel` / `geminiVisionDailyLimit` (edit settings.json; values now survive saves).
