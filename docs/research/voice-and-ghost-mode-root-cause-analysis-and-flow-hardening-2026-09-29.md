# Deep Research: Ghost Mode Waves, Browser Search & Typing, TTS Latency, and Intent Isolation (2026-09-29)

## 1. Executive Summary

During hands-on voice session testing in NEXUS on Windows, five specific flow-breaking issues and operational anomalies were identified:
1. **Ghost Mode & Wake Up Waves Disappearance**: When waking up NEXUS via wake word or entering Ghost Mode, the wave animation does not display or appears visually extinguished.
2. **Strict Wave Motion Invariant**: Waves must strictly oscillate **only** when STT is capturing user vocal energy or when TTS is actively synthesizing/playing speech; at all other times (idle/rest), waves must stay calm and static without autonomous movement.
3. **Browser Typing & Search Inoperability**: After opening Brave or opening a new tab, saying "search" or specifying queries fails to focus the address bar, type the query, or type dictated text line-by-line.
4. **TTS App-Open Lag (1-2s Delay)**: When opening applications (e.g. Brave), a 1.5s to 2.0s delay occurs before NEXUS speaks "Brave open, sir".
5. **False GitHub "No repository found sir" Usurpation**: Unrecognized commands, pauses, or non-developer speech incorrectly trigger GitHub repository lookups and speak `"No repository found sir. Open a repo in your browser or GitHub Desktop."`.

This research document details the complete root-cause architecture, line-by-line code paths, failure modes, future edge cases, and architectural solutions across the frontend and backend.

---

## 2. Root Cause Analysis of the 5 Core Issues

### 2.1 Issue 1 & 2: Ghost Mode & Wake Word Waves Animation

#### Architectural Diagnosis
There is an architectural divergence between two visual representations inside [`Avatar.tsx`](file:///C:/PROJECTS/ULTRON/frontend/src/avatar/Avatar.tsx):
- **Visual Representation A**: The Lottie facial orb (`wakeup.json`), which renders three loading circles and a smiling face.
- **Visual Representation B**: The Lottie waveform (`waves.json`) and procedural bars (`ghostWaveBars`), which render audio-reactive waveforms.

In `Avatar.tsx`, the wave visualizer is gated exclusively by `ghostPhase`:
```tsx
{(ghostPhase === "waves" || ghostPhase === "leaving") && (
  <div className="ghost-waves" ...
```
Where `ghostPhase` transitions to `"waves"` **only** when `ghostActive === true`.

#### Failure Mechanism
1. **Normal Wakeup Exclusion**: During normal wake word detection ("NEXUS" or "Hey NEXUS"), `ghostActive` is `false`. `ghostPhase` remains `"smile"`. The waves container is never rendered, showing only the facial orb. The user expects the dynamic waves visualizer upon wake up.
2. **Invisible Resting Baseline in Ghost Mode**:
   When Ghost Mode is active and speech stops:
   - `waveSource(st.state, st.ttsActive, st.micLevel)` evaluates to `"rest"`.
   - The scale target is clamped to `0.15`.
   - Opacity is dimmed: `wavesContainerRef.current.style.opacity = "0.35"`.
   - Lottie animation is paused: `w.pause()`.
   - The 70px–120px bars scaled by `0.15` measure only 10.5px to 18px tall. Against a transparent window and dark desktop backgrounds, **they appear completely extinguished / missing**.
3. **Acoustic Pre-gate Deadband vs. STT Capture**:
   In `wakeword_oww.rs`, `LEVEL_TX` only emits `audio:level` if RMS exceeds the noise floor with a level deadband of `0.02`. In `Avatar.tsx`, `WAVE_REST_FLOOR = 0.04`. If user speech is quiet or between words, `waveSource` flips to `"rest"`, freezing the waves mid-utterance.

---

### 2.2 Issue 3: Inability to Search or Type in Browser Line-by-Line

#### Architectural Diagnosis
1. **Search Intent Cloud Misdirection**:
   In [`src-tauri/src/orchestrator.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/orchestrator.rs) line 451:
   ```rust
   ParsedIntent::AnalyseRepo { .. }
   | ParsedIntent::AnalysePr { .. }
   | ParsedIntent::Search { .. }
   | ParsedIntent::Unknown { .. } => Subsystem::WorkerBackend,
   ```
   `ParsedIntent::Search` is routed to the Cloudflare Worker web search API instead of driving the local browser.
2. **Unconnected Local Browser Automation**:
   [`src-tauri/src/live/commands/browser.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/live/commands/browser.rs) has a fully implemented `browser::search(query)` function:
   ```rust
   pub fn search(query: &str) -> Result<(), String> {
       keyboard::press_hotkey(&["ctrl", "l"])?;
       thread::sleep(Duration::from_millis(200));
       keyboard::type_text(query)?;
       thread::sleep(Duration::from_millis(200));
       keyboard::press_key("enter")?;
       Ok(())
   }
   ```
   However, `orchestrator.rs` **never calls this function**. It only exposes it via an IPC command `live_browser_search` which has zero callers from the voice pipeline.
3. **Bare "Search" Command Rejection**:
   In [`src-tauri/src/intent_parser.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/intent_parser.rs):
   `parse_search_command` expects a query prefix (`"search "` with a trailing space). Saying bare `"search"` returns `None`, falling into `ParsedIntent::Unknown`.
4. **Missing Ghost Dictation / Typing State**:
   In Ghost Mode, [`orchestrator.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/orchestrator.rs) only handles `OpenApp`, `SendWhatsAppMessage`, `WhatsappChat`, `BrowserCloseTab`, and `BrowserNewTab`. There is no state or intent for:
   - Focusing address bar (`Ctrl+L`).
   - Typing into the focused window (`type <text>`).
   - Line-by-line continuous dictation ("type whatever I am saying").
   When the user said `"Dribble."`, `"Sermy."`, `"Ssort."`, the engine did not know the user intended to type into the newly opened browser tab.

---

### 2.3 Issue 4: 1-2 Second Delay in TTS on App Open ("Brave opened sir")

#### Architectural Diagnosis
1. **Dynamic String Cache Miss**:
   In [`src-tauri/src/tts.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/tts.rs), exact-match audio caching (< 5ms) only exists for static boot phrases (`"Ok sir."`, `"On it sir."`). Dynamic confirmation strings like `format!("{target} open, sir.")` (e.g. `"Brave open, sir."`) always miss the cache.
2. **Sequential Blocking Cloud WebSocket Synthesis**:
   On a cache miss, `tts.rs` calls Microsoft Edge-TTS cloud over WebSocket (`tts_edge.rs`). Opening TLS, negotiating WebSocket handshake, sending SSML, generating neural audio, and streaming back PCM takes **1,200ms to 1,800ms**.
3. **Serial Execution Chain**:
   ```
   resolve_and_open_app (100-300ms)
     → focus_app_by_title (50-100ms)
       → IPC emit Result to frontend (10-20ms)
         → frontend checks isMeetingActive & getSavedSettings (20-40ms)
           → IPC call speak_text to Rust (10-20ms)
             → Edge-TTS WebSocket cloud network synthesis (1200-1800ms)
               → Rodio playback
   ```
   Total elapsed delay before user hears speech: **1.5s to 2.2s**.

---

### 2.4 Issue 5: False Fallback to GitHub "No repository found sir"

#### Architectural Diagnosis
1. **Frontend `recorder.ts` Line 354 Mic Truncation Expansion**:
   ```ts
   // Fix truncated "open-" / "open " from Intel SST mic silence.
   if (/^open-?$/i.test(t.trim())) {
     t = "open architecture mapper";
     logFixes.push("open-→open architecture mapper (mic truncation recovery)");
   }
   ```
   Whenever STT captured `"open"` or `"open-"` (such as a pause after saying "open", or mic clipping), `recorder.ts` **rewrote the transcript to `"open architecture mapper"`**.
   Then line 734:
   ```ts
   if (intent.action === "open_architect") {
     await invoke("open_architect_with_auto_detect");
   ```
   `open_architect_with_auto_detect` checked for a GitHub repo in the active window. Because no repository was focused, lines 751-752 spoke:
   `"No repository found sir. Open a repo in your browser or GitHub Desktop."`!
2. **Frontend `recorder.ts` Line 187 `isLongRunningQuery` False Positives**:
   `hasAnalyse` included generic verbs (`"create"`, `"build"`, `"show"`, `"check"`, `"what is"`, `"explore"`).
   `hasRepo` included generic nouns (`"project"`, `"architecture"`, `"code"`, `"codebase"`, `"repo"`).
   Phrases like `"create a new project"`, `"check the code"`, `"show my project"` matched `isArchitectQuery = true`.
3. **Worker Regex Over-reach (`server/worker/src/index.ts`)**:
   In `index.ts` lines 223 & 245, any transcript containing words like `"code"`, `"change"`, `"issue"`, `"branch"`, `"commit"` routed to `github_analyse` or `github`. When the user did not specify a repo, `resolveRepo` attempted to match against user repos and failed:
   `"I couldn't find a repository matching ... in your GitHub account."`

---

## 3. Comparison & Verification Matrix

| Issue | Observed Failure | Root Cause in Code | Proposed Architectural Solution | Target Metric |
| :--- | :--- | :--- | :--- | :--- |
| **1. Ghost & Wake Waves** | Waves missing on wake word or invisible in Ghost Mode | `ghostPhase` only enables on Ghost Mode; idle rest scale `0.15` + `0.35` opacity makes bars invisible | Unify wave visualizer for wake word and Ghost Mode; visible static resting baseline (no oscillation) | Visible immediately on wake & ghost |
| **2. Strict Wave Motion** | Waves moving during silence or failing to sync with speech | Level deadband `0.02` drops quiet voice; TTS activity flapping | Oscillation strictly drives on `micLevel >= 0.01` during STT and `ttsActive` during TTS; static at rest | 100% still on silence, 100% reactive on speech |
| **3. Browser Search & Typing** | Saying "search" or dictating in browser does nothing | `ParsedIntent::Search` routed to Worker; `browser::search` not hooked; bare "search" unhandled | Route `search <query>` to `browser::search` (Ctrl+L $\to$ type $\to$ Enter); bare "search" focuses URL bar; add line-by-line typing | < 100ms local typing execution |
| **4. App Open TTS Lag** | 1.5s–2s delay before hearing "Brave open, sir" | Dynamic string misses cache; blocking Edge-TTS cloud WebSocket latency | Local fast-ack or pre-cached synthesis (`"Ok sir."` / local Piper fallback) | < 50ms time-to-speech |
| **5. False GitHub Repo Error** | Unrecognized commands say "No repository found sir" | `recorder.ts` line 354 rewrites `"open"` to `"open architecture mapper"`; loose regexes in `recorder.ts` & Worker | Remove line 354 rewrite; isolate GitHub intents strictly to developer keywords (*"analyse"*, *"repo"*, *"pr"*, *"github"*) | Zero false GitHub triggers |

---

## 4. Comprehensive Future Failure Possibilities & Breakpoint Analysis

Beyond the five immediate issues, deep investigation revealed several systemic vulnerabilities that can break user flows:

### 4.1 Meeting Mode & TTS Mute Trap
- **Mechanism**: When TTS plays, `meeting.set_tts_playing(true)` suppresses wake word detection and STT capture to prevent echo feedback.
- **Breakpoint**: In `run.ps1` log:
  `[00:18:48] RUST wake: detection suppressed by meeting/TTS-mute state (20 chunks dropped so far)`
  If a TTS call completes in Rodio but frontend fails to emit `tts-ended`, or if `meeting.set_tts_playing(false)` is delayed by a 500ms sleep, the microphone remains permanently suppressed. The user speaks immediately after TTS and their first 1-2 seconds of speech are discarded.
- **Prevention**: Guard `meeting.set_tts_playing` with a strict hardware playback watchdog timer (max 10s timeout) that auto-releases mic suppression.

### 4.2 Cpal vs. WebRTC Baton Race Condition
- **Mechanism**: `wakeword_oww.rs` uses direct `cpal` streaming. When the frontend attempts `getUserMedia()` or baton passing, both systems contend for the Windows Intel SST audio device.
- **Breakpoint**:
  `[00:18:10] BATON wake: cpal stream resumed (mic baton pass — frontend released mic)`
  If the frontend opens the mic while cpal is capturing, the Intel SST audio driver crashes or delivers zero-byte silent buffers.
- **Prevention**: Eliminate the frontend baton pass entirely. Keep audio capture 100% inside Rust `cpal`, streaming only levels and transcripts to the frontend.

### 4.3 Ghost Watchdog Spurious Wake Loop
- **Mechanism**: When a Ghost session is idle for > 15s, the watchdog re-emits a listen event up to 3 times:
  `[00:19:14] RUST ghost: watchdog poke — session live but idle, re-emitting listen`
- **Breakpoint**: In a room with background noise (fan, keyboard typing, TV), Groq Whisper transcribes background noise into hallucinated tokens (*"Mersh"*, *"Sermy"*, *"Ssort"*).
- **Prevention**: Ensure the acoustic energy gate in `stt.rs` (`rms < 0.005` or `samples < 3200`) drops quiet background captures before sending them to Whisper, and increment silent-miss counter to park the watchdog.

### 4.4 Missing NLU Retrain Path
- **Mechanism**: In `src-tauri/src/brain_monitor.rs` line 347:
  `p.parent().map(|d| d.join("resources").join("server").join("nlu").join("merge_and_train.py"))`
- **Breakpoint**:
  `[00:19:41] RUST [brain_monitor] retrain failed: python: can't open file '.../merge_and_train.py': No such file or directory`
  In release mode, `nexus.exe` runs from `src-tauri/target/release/`, where no `resources/server/nlu/` directory exists. The script is located in `server/nlu/merge_and_train.py`. Every failed retrain spawns a dead Python process and spams error logs.
- **Prevention**: Check `server/nlu/merge_and_train.py` relative to current working directory before falling back to binary parent paths.

---

## 5. Architectural Implementation Plan

### Step 1: Intent Isolation & GitHub Keyword Hardening
1. In [`frontend/src/audio/recorder.ts`](file:///C:/PROJECTS/ULTRON/frontend/src/audio/recorder.ts):
   - Delete line 354: `if (/^open-?$/i.test(t.trim())) { t = "open architecture mapper"; }`.
   - Constrain `isArchitectQuery` to require both an architect keyword (`"architect"`, `"architecture mapper"`) and an explicit mapping target.
   - Constrain `isLongRunningQuery` so general conversational verbs (`"create"`, `"show"`, `"check"`) without `"repo"`, `"pr"`, or `"branch"` never enter GitHub analysis.
2. In [`server/worker/src/index.ts`](file:///C:/PROJECTS/ULTRON/server/worker/src/index.ts):
   - Restrict `github_analyse` and `github` intents to require explicit terms (`"github"`, `"pull request"`, `"pr"`, `"repository"`).

### Step 2: Browser Search, Address Bar Focus & Line-by-Line Typing
1. In [`src-tauri/src/intent_parser.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/intent_parser.rs):
   - Support bare `"search"` (returns `ParsedIntent::BrowserSearchFocus`).
   - Support `"search <query>"` (returns `ParsedIntent::BrowserSearch { query }`).
   - Support `"type <text>"` (returns `ParsedIntent::TypeText { text }`).
   - Support `"start typing"` / `"type whatever i say"` (returns `ParsedIntent::StartDictation`).
   - Support `"stop typing"` (returns `ParsedIntent::StopDictation`).
2. In [`src-tauri/src/orchestrator.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/orchestrator.rs):
   - Intercept `BrowserSearch` and dispatch to `crate::live::commands::browser::search(&query)` (sends `Ctrl+L` $\to$ types query $\to$ `Enter`).
   - Intercept `BrowserSearchFocus`: sends `Ctrl+L` to focus address bar and speaks `"Ready to search, sir."`.
   - Intercept `TypeText`: calls `crate::live::commands::keyboard::type_text(&text)`.
   - Intercept Dictation mode: continuously streams STT transcripts into `keyboard::type_text`.

### Step 3: Fast App-Open Audio Latency Optimization
1. In [`src-tauri/src/orchestrator.rs`](file:///C:/PROJECTS/ULTRON/src-tauri/src/orchestrator.rs):
   - In `run_ghost_open`, use the pre-cached `"Ok sir."` phrase (< 5ms playback) immediately upon launching the app, rather than waiting for cloud synthesis of dynamic strings.
   - Alternatively, synthesize dynamic confirmations via local Piper TTS (< 40ms) when Edge-TTS latency would delay response.

### Step 4: Unified Wave Visualizer & Motion Invariant
1. In [`frontend/src/avatar/Avatar.tsx`](file:///C:/PROJECTS/ULTRON/frontend/src/avatar/Avatar.tsx):
   - Make the wave animation the standard visualizer during active wake and Ghost Mode sessions.
   - When `src === "rest"`: Keep the bars at a visible resting height (e.g. 40px–50px) with opacity `1.0`, but **zero motion** (no sinusoidal wave, Lottie paused).
   - When `src === "mic"` or `src === "tts"`: Animate dynamically in real-time with live speech audio levels.
