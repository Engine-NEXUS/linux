# Screen Tour v2 — Root-Cause Analysis, Research and Plan (2026-10-06)

## Live test (user)
* Page: Women's Health "Top 10 healthiest nuts" in Brave. User said "analyse my screen". Result: it explained the **tab strip and URL bar**, and said only "this is the kinds of nuts". Expected: how many kinds and which ones (7 bowls: pumpkin seeds, hazelnuts, sunflower seeds, cashews, pistachios, walnuts, almonds).
* YouTube video: it said "this is the person / this is the product" — not who, not what the product is, not what the video is about (the URL/title should say).

## Root causes (each traced to code)
| # | Cause | Evidence |
|---|---|---|
| 1 | Whole-monitor capture | `run_screen_tour` → `vision::capture_plain_jpeg_base64_w` = full primary monitor; screenshot shows tabs (y 0–40), URL bar (40–80), taskbar |
| 2 | Prompt had no notion of "main content" | v1 `build_tour_prompt`: "pick the 3–5 most important things" → the model picks the most legible text, i.e. browser chrome |
| 3 | Zero context | only pixels + the literal transcript; `browser_url::get_active_browser_url` and the (private) `get_foreground_window_title` existed but the tour never called them → "what is this page / video" unanswerable |
| 4 | Output shape forces labels | overview ≤20 words, spoken ≤17, callout ≤18, total ≤90, ≤5 items, 2–5 detail rows, hard `limit_words` cuts, no `answer` field, no instruction to answer the implicit question or enumerate. Seven bowls cannot be named in 20 words |
| 5 | Weakest model | `GEMINI_VISION_MODEL = gemini-3.5-flash-lite`, temp 0.2, no stronger fallback |
| 6 | Face identification | a vision model will not (and NEXUS must not) name a person from their face; "who is that" is answerable only from text evidence (title, channel, captions, on-screen text) |
| 7 | (found while planning) Settings erase unknown keys | `save_settings` rewrites settings.json from the `NexusSettings` struct → hand-added keys (`screenTour`, any new tour settings) vanished on the next Settings save |

## Research
* **Gemini video understanding**: public YouTube URLs can be passed as `file_data.file_uri`; free tier limited (8 h of YouTube video/day); only public videos. Offered to the user as an optional v3 ("let Gemini watch the video"); not built.
* **YouTube oEmbed** (`https://www.youtube.com/oembed?url=…&format=json`): returns title and `author_name`, no API key → chosen for video context (only the clean video URL is sent).
* **Model ids** (Google model page, fetched 2026-10-06): Flash family `gemini-3.8-flash`, `3.7`, `3.6`, `3.5-flash` (stable, image input yes); Flash-Lite `gemini-3.5-flash-lite`, `3.1-flash-lite`. 3.8 Flash = "most intelligent Flash".
* **Rate limits** (Google rate-limits page): applied **per project**, per model; dimensions RPM/TPM/RPD; tiers Free/1/2/3; **the docs publish no per-model numbers — "view in AI Studio" (aistudio.google.com/rate-limit)**; "not guaranteed". The pricing page's "500 RPD" figures are for Search/Maps *grounding*, not model requests. Consequence: NEXUS's `GEMINI_VISION_RPD = 500` is **our own assumption**; limits per model mean a fallback ladder adds capacity.

## User decisions
| Question | Decision |
|---|---|
| Stop analysing tabs/URL bar/taskbar | **crop to the page content** (not "full screen + ignore it") |
| Learn what a page/video is about | **window title + URL, plus YouTube title/channel lookup** |
| Model | user asked for the daily limit, then said "proceed with more powerful model" → stronger-first ladder |
| Daily limit | told: Google doesn't publish per-model free numbers; per project; see AI Studio; NEXUS's 500 is an assumption → made configurable |

## Plan (as approved)
1. `screen_context.rs`: app, title, privacy-trimmed URL, YouTube meta, content kind, capture region, sensitive-window flag.
2. Crop via UI Automation `Document` → window rect minus toolbar inset → full screen; map boxes back to screen px.
3. Answer-first prompt + new budgets + `answer` field.
4. Model ladder (strong → lite), 404/429 advance, quota only if all-429; configurable daily limit.
5. Safety: refuse sensitive windows; never identify faces.
6. Add `screenTour`, `tourModel`, `geminiVisionDailyLimit` to `NexusSettings`.
