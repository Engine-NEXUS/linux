# Feature Spec 98: Screen Analysis — Celebrity Recognition & Zero-Delay Pipeline

**Target Systems**: `src-tauri/src/vision.rs`, `src-tauri/src/screen_tour.rs`, `src-tauri/src/orchestrator.rs`, `src-tauri/src/screen_context.rs`, `src-tauri/src/ocr.rs`  
**Related Docs**: `docs/features/97-screen-analysis-zero-delay-plan.md`, `docs/research/screen-tour/04-celebrity-recognition-and-zero-delay-architecture-2026-10-07.md`  
**Status**: Ready for Implementation by Muse Spark

---

## 1. Executive Summary & Live Diagnostics

### The 2 Primary Problems
1. **Fails to identify celebrities/YouTubers/influencers/politicians**: NEXUS identifies clothing attributes (*"person in a black t-shirt"*) but never names the person.
2. **Dead-air delay is ~30 seconds**: Instead of answering in 1–2s, turns hang for 30+ seconds before falling back or speaking.

### Root Causes & Live Probe Findings (Verified on Gemini API)

#### A. Dead & Failing Models in the Ladder (`src-tauri/src/vision.rs`)
Live tests with the active Gemini key on `https://generativelanguage.googleapis.com/v1beta/models/...` revealed:
- `gemini-3.8-flash`: Returning **HTTP 503 Service Unavailable** (high demand spike, hung for 4.1s).
- `gemini-2.5-flash`: Returning **HTTP 404 Not Found** (deprecated by Google).
- `gemini-2.0-flash`: Returning **HTTP 404 Not Found** (deprecated by Google).
- **`gemini-3.5-flash-lite`**: Returning **HTTP 200 OK in 1.27s** (Healthy, lightning fast, but buried 3rd in the ladder!).
- **`gemini-3.5-flash`**: Returning **HTTP 200 OK in 2.37s** (Healthy, superior entity recognition).
- **`gemini-flash-lite-latest`**: Returning **HTTP 200 OK in 1.37s** (Healthy official alias).

#### B. The Triple-Engine Cascading Death Spiral (`src-tauri/src/orchestrator.rs`)
In `run_screen_analysis` (lines 4190–4350):
1. **Engine 1 (`run_screen_tour`)**: Tries 4 models with 45s cap. When 3.8 hits 503 and 2.5 hits 404, it spends **~16s** before returning `TourRun::Fallback`.
2. **Engine 2 (`analyze_screen_spatial`)**: Instead of stopping, line 4227 starts a brand new screen capture and loops through 3 models with a 20s timeout (**~10s**).
3. **Engine 3 (`plain-text VLM fallback`)**: Line 4292 starts a third screen capture and query with a 12s timeout (**~4s**).
- **Cumulative dead air: $16\text{s} + 10\text{s} + 4\text{s} = \mathbf{\sim 30\text{ SECONDS}}$!**

#### C. The Censorship Rule in System Prompt (`src-tauri/src/screen_tour.rs#L120`)
```rust
// screen_tour.rs line 120:
"4. People: never identify anyone from their face. Name a person only if the title, channel, \
caption or on-screen text says so; otherwise call them \"the presenter\" / \"the person\" and say \
what the title or channel suggests (worded as an inference)."
```
- A unit test on line 1202 (`prompt_is_answer_first_ignores_chrome_and_never_identifies_faces`) explicitly locked in this censorship behavior!
- Gemini has facial recognition knowledge for Elon Musk, MrBeast, Joe Rogan, Narendra Modi, Taylor Swift, etc., but our prompt explicitly ordered Gemini: *"never identify anyone from their face"*, forcing it to describe clothes instead.

#### D. Canned Spoken Line Discards Identity (`src-tauri/src/orchestrator.rs#L4274`)
Even when Gemini returns a recognized scene in `payload.overview`, the orchestrator ignores it and speaks:
`"I've highlighted 4 elements on your screen, sir. Detailed breakdown is in the sidebar."`

#### E. Heavy Ingestion Resolution & Cold TLS Handshakes
- Capture is 1536px in `screen_tour` (~400 KB base64 upload).
- Every call instantiates a fresh `reqwest::Client::builder().build()`, incurring a 150ms–400ms cold TLS handshake tax every turn.

---

## 2. Implementation Blueprint for Muse Spark

### Phase 1: Clean Model Ladder & Promote Verified Fast Models
In `src-tauri/src/vision.rs`:
1. Update `GEMINI_VISION_MODEL`:
   ```rust
   pub const GEMINI_VISION_MODEL: &str = "gemini-3.5-flash-lite";
   pub const GEMINI_VISION_FALLBACKS: &[&str] = &["gemini-3.5-flash", "gemini-flash-lite-latest"];
   ```
2. In `tour_model_ladder`:
   Remove dead models `gemini-3.8-flash` (503), `gemini-2.5-flash` (404), `gemini-2.0-flash` (404).
   Set ladder order:
   ```rust
   for m in [
       GEMINI_VISION_MODEL,       // gemini-3.5-flash-lite (1.27s)
       "gemini-3.5-flash",        // high entity reasoning (2.37s)
       "gemini-flash-lite-latest",
   ] {
       push(m);
   }
   ```
3. Update unit tests in `vision::tour_ladder_tests` to reflect the clean model ladder.

### Phase 2: Kill the 3-Engine Retry Cascade & Clamp Timeouts
In `src-tauri/src/screen_tour.rs` & `src-tauri/src/orchestrator.rs`:
1. Clamp timeouts:
   - In `screen_tour.rs`: Change HTTP client timeout from 45s to **4.0 seconds** (`Duration::from_secs(4)`). Change `TOUR_TOTAL_TIMEOUT_SECS` from 75s to **6s**.
   - In `vision.rs`: Change spatial client timeout from 20s to **4.0 seconds**.
2. Stop the cascade in `orchestrator.rs#L4204`:
   If `run_screen_tour` returns `TourRun::Fallback`, **DO NOT cascade into `analyze_screen_spatial` and `plain-text VLM`**.
   Instead, immediately speak the local WinRT OCR answer (which was already computed locally in **25ms** via `try_ocr_answer`).
   ```rust
   TourRun::Fallback { acked } => {
       if let Some(res) = try_ocr_answer(&app, &prompt, &request_id).await {
           clear_active_request(&request_id);
           return Ok(res);
       }
   }
   ```
   **Worst-case failure silence drops from 30+ seconds to $\le 1.5$ seconds!**

### Phase 3: Enable Public Figure & Celebrity Identification in Prompts
1. In `src-tauri/src/screen_tour.rs#L120`, replace rule 4 with:
   ```rust
   "4. People & Public Figures: Actively identify and NAME any public figures, celebrities, \
   YouTubers, streamers, influencers, politicians, athletes, tech leaders, and notable creators \
   visible on screen (e.g. 'Elon Musk', 'MrBeast', 'Marques Brownlee', 'Joe Rogan', 'Narendra Modi', \
   'Sam Altman'). State their full recognized name in 'label' and 'callout_title', and their role/title \
   in 'summary' and 'details'. Cross-reference visual facial likeness with any on-screen text clues \
   (channel names, video titles, chyrons, interview badges, handles). Only use generic descriptors \
   ('an audience member', 'a passerby') for private, non-famous individuals.\n"
   ```
2. Update unit test `prompt_is_answer_first_ignores_chrome_and_never_identifies_faces` in `screen_tour.rs#L1192`:
   Rename and assert that the prompt instructs naming public figures and celebrities.
3. In `src-tauri/src/vision.rs` (`build_spatial_prompt`), add explicit person identification instruction:
   ```rust
   "- category: one of [object, ui_element, text_block, chart, diagram, code, product, food, person, other]\n\
   - label: specific name (e.g. 'MrBeast', 'Elon Musk', 'Revenue Chart', 'Submit Button')\n\
   If a person is a recognized public figure, celebrity, YouTuber, or politician, name them by their full name.\n"
   ```
4. In `src-tauri/src/screen_context.rs`, ensure on-screen OCR text lines and YouTube channel metadata are formatted into `prompt_block` as `[On-Screen Context Clues from OS & OCR]`.

### Phase 4: Speak the Person's Name in Voice Output
In `src-tauri/src/orchestrator.rs`:
- In `run_screen_analysis` (spatial path, line 4274), speak `payload.overview` or the identified personalities:
  ```rust
  let spoken_line = if !payload.overview.trim().is_empty() {
      format!("{}, sir.", payload.overview.trim().trim_end_matches('.'))
  } else {
      let n = payload.items.len();
      format!("I've highlighted {} {} on your screen, sir. Detailed breakdown is in the sidebar.",
          n, if n == 1 { "element" } else { "elements" })
  };
  speak_line(&app, spoken_line, &request_id);
  ```

### Phase 5: Pooled HTTP Client & Ingestion Resolution
1. In `src-tauri/src/vision.rs`:
   Add a static pooled client:
   ```rust
   static SHARED_VISION_CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
       reqwest::Client::builder()
           .pool_idle_timeout(std::time::Duration::from_secs(90))
           .tcp_keepalive(std::time::Duration::from_secs(60))
           .connect_timeout(std::time::Duration::from_secs(3))
           .timeout(std::time::Duration::from_secs(4))
           .build()
           .unwrap_or_default()
   });
   pub fn shared_vision_client() -> reqwest::Client {
       SHARED_VISION_CLIENT.clone()
   }
   ```
2. Reuse `shared_vision_client()` in `analyze_tour_image`, `analyze_screen_spatial`, and `locate_via_vision`.
3. In `src-tauri/src/orchestrator.rs#L3776`:
   Change capture max width from `1536` to `768` (`crate::vision::VISION_FAST_W`). Cuts base64 upload payload from 400 KB down to 70 KB (82% reduction).

### Phase 6: Frame-Hash Result Cache (Muse Spark P5)
1. In `src-tauri/src/vision.rs`:
   Add a lightweight mutex cache storing `(u64_hash, Instant, SpatialAnalysisPayload)`.
   If a new visual query captures an identical screen (hash matches within 60s), return the cached payload immediately (<30ms, 0 quota burn).

---

## 3. Verification & Acceptance Criteria

1. **Test Verification**:
   - `cargo test --lib -- vision::`: All tests pass.
   - `cargo test --lib -- screen_tour::`: All tests pass.
   - `cargo test --lib -- orchestrator::`: All tests pass.
   - `cargo check --features custom-protocol,admin-brain`: Clean (0 warnings, 0 errors).
   - `npm test -- --run`: 200/200 pass.
   - `npx tsc --noEmit`: Clean (0 errors).
2. **Behavioral Acceptance**:
   - Saying *"analyse the screen"* on a YouTube video of a creator (e.g., Marques Brownlee, MrBeast, Joe Rogan) accurately names them in the spoken overview and callout title.
   - Spoken turnaround time is **$\le 2.0\text{s}$** (was 30s+).
   - Worst-case failure turnaround time is **$\le 1.5\text{s}$** (falls back to local WinRT OCR, never dead-air silence).
   - Local RAM baseline remains at **~177 MB** (0 MB local model RAM).
