# NEXUS — Project Notes

## Diagnostic Fixes — 2026-09-12

### NLU Server ONNX Export (build integration)

The BERT-Mini retraining produced `best_model.pt` but the ONNX export step
was missing. The NLU server requires `nexus_nlu.onnx` to start — without
it, `sys.exit(1)` and every unparseable command blocked for 30s.

**Fix:** `nexus.mjs` now calls `syncNluModel()` before every build. This
copies the ONNX model, `labels.json`, and tokenizer from
`server/nlu/model/` to `src-tauri/resources/server/nlu/model/` so the
Tauri installer bundles the latest trained model automatically.

**`tauri.conf.json` resources** now includes `labels.json`:
```json
"resources/server/nlu/model/labels.json"
```

**To retrain and bundle:**
```bash
cd server/nlu
python train.py           # trains → best_model.pt
python export_onnx.py    # exports → nexus_nlu.onnx (max_length=64)
# Then: nexus build      # syncs model to resources + bundles in installer
```

### NLU Training Commands (`nexus train` + `nexus collect` + `nexus audit`)

**`nexus train`** performs cleaning, generation, merge, canonical conflict/slot/leakage repair, BERT-Mini training, stable ONNX export, resource sync, post-training audit, and temporary-file cleanup.

```bash
nexus train                    # full pipeline (with audit + cleanup)
nexus train --clean-only       # just clean the dataset
nexus train --skip-train       # data generation/repair without training
nexus train --keep-temp        # keep generated data and checkpoint (~17 MB)
```

**`nexus audit`** validates both data and model quality:

```bash
nexus audit                    # structural checks + ONNX evaluation
nexus audit --dataset-only     # structural checks only
```

It writes `server/nlu/audit_report.json` and `docs/research/nlu-model-data-audit-latest.md`.

**`nexus collect`** prompts for 23 intent families, waits for Enter, records two seconds, transcribes with Groq/Moonshine, and requires save/retry/edit/skip/end review before adding a sample.

```bash
nexus collect
nexus collect --intent type_text
nexus collect --count 20
nexus collect --text-only
nexus collect --list
```

Output: `server/admin/data/collected_samples.jsonl` (gitignored, admin-only). `nexus train` merges approved samples and then deletes the temporary collection file unless `--keep-temp` is supplied.

Requirements: working microphone and `sounddevice`/`numpy`/`scipy` (auto-installed). Groq is auto-loaded from NEXUS settings; local STT is the fallback.

### NLU Server Startup Cooldown

Was: 30s wait per failed startup, no cooldown → "loading non stop".
Now: 15s timeout + 60s cooldown after failure. If the NLU server can't
start (missing model, missing deps), it won't block subsequent commands.

### Brain Server Non-Blocking Spawn

Was: `ensure_brain_running()` blocked for 12s while Qwen loaded.
Now: spawns process + background thread. First command falls back to
NLU/deterministic while brain loads in background.

### Wake Word Grace Period (10s after restart)

Was: 5s grace period after stream restart. Intel SST driver produces
transient audio bursts 5-10s after restart that false-trigger the model.
Now: 10s grace period. See `wakeword_oww.rs::detect_chunk()`.

### NLU Model Dimension Fix

The ONNX export used `max_length=32` but the server tokenizes with
`max_length=64` (MAX_LEN). Fixed `export_onnx.py` to use `max_length=64`.

### Live-Mode Intent Mapping

`nlu_client.rs` now maps all 11 live-mode intents to `NluResult`:
`type_text`, `press_key`, `press_hotkey`, `confirm_send`, `cancel_action`,
`browser_new_tab`, `browser_navigate`, `browser_search`,
`whatsapp_open`, `whatsapp_search`, `focus_app`.

### Known Issue: NLU Model Accuracy

The current model has low accuracy (5-9% confidence) because the
training dataset (`dataset.json`) contains malformed intent labels
from the previous merge (e.g., `OpenArchitect` instead of
`open_architect`, debug objects as intent labels). The deterministic
parser handles most commands correctly — the NLU is a fallback only.
**To fix:** clean the dataset labels, retrain, re-export ONNX, rebuild.

## Live Mode — Phase 1 Implementation (2026-09-11)

**Live mode is the always-listening, STT-only, full-laptop control
capability.** It adds keyboard simulation, WhatsApp full flow, browser
navigation, window focus, and a state machine for sequential commands.

### New Dependencies

```toml
enigo = "0.5"     # Cross-platform keyboard/mouse simulation
arboard = "3.4"   # Clipboard for paste-text pattern (avoids autocomplete corruption)
```

### New Module: `src-tauri/src/live/`

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, LiveResult, LiveIntent, 14 Tauri commands |
| `state.rs` | State machine (Idle → AppOpen → ChatActive → TextTyped) |
| `safety.rs` | Whitelist, denylist, confirmation gates |
| `commands/keyboard.rs` | Type text, press keys, hotkey combos, clipboard paste |
| `commands/whatsapp.rs` | Open → search contact → type → send (with confirmation) |
| `commands/browser.rs` | New tab, navigate, search, open site by name |
| `commands/window.rs` | Window focus with AttachThreadInput trick (Windows) |

### New Tauri Commands (14)

`live_type_text`, `live_press_key`, `live_press_hotkey`,
`live_whatsapp_open`, `live_whatsapp_search`, `live_whatsapp_send`,
`live_whatsapp_type_message`, `live_browser_new_tab`,
`live_browser_navigate`, `live_browser_search`, `live_open_site`,
`live_focus_app`, `live_cancel`, `live_get_state`

### New Intent Parser Patterns

- `type <text>` → type_text
- `press <key>` → press_key
- `press <key1> <key2>` → press_hotkey
- `send` / `send it` → confirm_send
- `stop` → cancel_action
- `new tab` / `open new tab` → browser_new_tab

### Safety Layer

- **Whitelist:** Only allowed tools can execute (type_text, press_key,
  open_app, whatsapp_send, etc.)
- **Denylist:** Banking apps, password managers, crypto wallets are blocked
  (1password, bitwarden, bank, paypal, coinbase, metamask, etc.)
- **Confirmation gates:** `whatsapp_send` and `confirm_send` always require
  user confirmation before execution

### State Machine

The state machine tracks context across sequential voice commands:
```
Idle → AppOpen { app } → ChatActive { app, contact } → TextTyped { app, contact, text }
```
Auto-resets to Idle after 30s of silence.

### Clipboard Paste Pattern (from OpenDex)

For text > 50 chars, uses clipboard paste (Ctrl+V) instead of
character-by-character typing. This avoids autocomplete corruption in
WhatsApp/search boxes. Previous clipboard contents are restored after
pasting.

### Window Focus: AttachThreadInput Trick (from ghost-hands)

`SetForegroundWindow` silently fails from background processes on Windows.
The workaround: attach our input queue to the foreground thread's using
`AttachThreadInput`, call `SetForegroundWindow`, then detach.

### NLU Model Update

**New intents:** 47 → 58 (added 11 live-mode intents)
**New training examples:** 1,960 → 2,438 (+478 new examples)
**New slot types:** `B-text`, `I-text`, `B-key`, `I-key`, `B-keys`,
`I-keys`, `B-target`, `I-target`

To retrain:
```bash
cd server/nlu
python generate_live_data.py     # generate new examples
python merge_live_data.py         # merge into dataset.json
python train.py                   # retrain BERT-Mini
```

### Test Results

- 323 Rust tests pass (29 new live-mode tests)
- `cargo check` clean, 0 warnings
- 18 live-mode unit tests (state, safety, keyboard, browser)
- 11 live-mode intent parser tests

## Multi-Worker Optimization — Cloud-First Architecture (2026-09-01)

**Single Worker, internally modularized.** No separate Workers — one deploy,
no cross-Worker latency. The monolithic `index.ts` is split into modules:

- `src/quota.ts` — per-user daily usage tracking + cost control (D1 `usage_log`)
- `src/cache.ts` — edge caching (KV namespace, D1 fallback)
- `src/models.ts` — model constants + fallback chains + truncation
- `src/research.ts` — ad-free search (Wikipedia REST + Wikidata, no API key)
- `src/clean.ts` — result cleaning, dedup, prompt-injection guard

**New D1 tables:** `usage_log` (per-user daily quotas), `cache_entries`
(D1 cache fallback when KV not bound).

**New KV namespace:** `CACHE` (edge cache for search results, PR analysis,
repo metadata). Create with `npx wrangler kv namespace create CACHE` and
paste the ID in `wrangler.toml`.

**Quota limits (per user/day):** 500 requests, 3000 neurons, 10 deep
analyses, 100 searches. Global neuron budget: warn at 8000, hard reject
deep at 9500.

**Search routing:** `isSearchQuestion()` detects factual questions
("what is X", "who is Y") and routes them to Wikipedia/Wikidata retrieval
with citations, instead of blind LLM answering.

**Offline local commands (new):**
- `close <app>` — taskkill on Windows, pkill on Unix
- `open chat with <name>` / `message <name>` / `chat with <name>` —
  WhatsApp deep links with local contacts file lookup at
  `%APPDATA%/com.nexus.assistant/contacts.json`
- Both work offline, zero internet, zero RAM overhead

**NLU pre-warm removed.** NLU server now only starts on first
unparseable command (lazy). Saves 50-100 MB at idle.

**STT idle monitor wired but disabled.** `lazy_stt::start_idle_monitor()`
is called from `lib.rs` but `STT_KEEP_ALIVE=true` means STT is never killed.
The idle cost is only ~128 MB (model loaded, not transcribing) and killing
it adds 10-15s delay on the next command (cold model load). The monitor
thread runs for future use but is a no-op. Peak STT RAM during active
transcription is ~340 MB.

**Lazy Kokoro TTS (2026-09-01).** Kokoro is no longer loaded at boot.
`speak_text` calls `ensure_engine_loaded()` on first use — loads in ~1.7s
(one-time), then stays loaded. Saves ~350 MB at idle. See `tts.rs`.

**WebView2 low-memory mode (2026-09-01).** The orb window sets
`MemoryUsageTargetLevel::Low` via `ICoreWebView2_23::SetMemoryUsageTargetLevel`
at creation time. WebView2 drops cached data and swaps to disk. Saves ~40 MB.
See `mic_permissions.rs::set_low_memory_mode()`.

**Idle RAM (2026-09-01): ~104 MB** (before first transcription) or
**~232 MB** (after first transcription, STT model loaded).

| Component | Before first transcription | After first transcription |
|---|---|---|
| nexus.exe (Rust + wake word, NO Kokoro) | 47.8 MB | 47.8 MB |
| WebView2 (orb, low-mem mode) | 35.8 MB | 35.8 MB |
| STT Python (model not yet loaded) | 20.6 MB | 128.6 MB |
| **TOTAL idle** | **104.2 MB** | **232.2 MB** |

After first TTS speak, Kokoro loads and stays loaded: **+350 MB → ~582 MB**.
This is the active state, not idle.

**Worker test suite:** `npm test` in `server/worker/` runs 23 vitest
tests covering quota, cache keys, search question detection, and dedup.

**Rust test suite:** `cargo test --test offline_commands` runs 10 tests
for close_app and whatsapp_chat parsing.

## STT Architecture — Groq Primary + Moonshine Fallback (2026-09-06)

**STT uses Groq Whisper Large v3 Turbo (cloud) as primary, with Moonshine
Small Streaming (local) as fallback when network is unavailable.**

### Primary: Groq Cloud STT
- **Model:** `whisper-large-v3-turbo` (809M params, cloud, ~247ms latency)
- **Free tier:** 20 RPM, 2,000 RPD, 28,800 audio sec/day, 25MB file limit
- **Code:** `src-tauri/src/stt_groq.rs` — sends WAV as multipart to
  `https://api.groq.com/openai/v1/audio/transcriptions`
- **Config:** `localSttOnly: false` in settings.json + valid `groqApiKey`
- **Fallback trigger:** network error, 429 rate limit, 401 auth error,
  or any Groq API error → falls back to local Moonshine

### Fallback: Moonshine Local STT (replaces faster-whisper)
- **Model:** Moonshine Small Streaming (123M params, 7.84% WER, ~165ms CPU)
- **Engine:** `moonshine-voice` Python package (ONNX Runtime, no PyTorch)
- **Code:** `server/stt_server.py` — FastAPI server on `127.0.0.1:39217`
- **Port:** 39217 (`POST /transcribe`, `GET /health`)
- **Audio format:** multipart/form-data with WAV (16kHz, mono, 16-bit PCM)
- **Latency:** ~165ms per transcription after model load; first call ~2s
  (model loading). Model loads in ~1.8s at startup.
- **RAM:** ~150-300MB (model + runtime)
- **Idle timeout:** server killed after 5 min of no requests (saves RAM)
- **Installer:** server files bundled in `resources/server/` via Tauri
  resources config. Production path: `exe_dir/resources/server/stt_server.py`.
- **Requirements:** `pip install moonshine-voice fastapi uvicorn python-multipart`
- **Model download:** automatic on first run via `get_model_for_language("en")`
- **Config:** `MOONSHINE_MODEL` env var (default: `small_streaming`)
  Options: `tiny_streaming` (34M, 12% WER), `small_streaming` (123M, 7.84%),
  `medium_streaming` (245M, 6.65%)

### Why Moonshine over faster-whisper
- Moonshine Small (123M) has 7.84% WER vs Whisper tiny.en's ~18%
- Moonshine is 10-100x faster than Whisper for real-time speech
- Moonshine uses ONNX Runtime (same as wake word engine)
- Moonshine is designed for voice command recognition (streaming, low latency)
- Moonshine MIT license, no restrictions

### Routing logic (`src-tauri/src/stt.rs`)
```
if localSttOnly:
    → local Moonshine (privacy mode, audio never leaves device)
elif groq_key available:
    → try Groq cloud first
    → on error/timeout/429: fall back to local Moonshine
else:
    → local Moonshine directly
```

### Historical: faster-whisper era (pre-2026-09-06)
The previous architecture used faster-whisper `tiny.en` (39M params, ~18% WER)
via a Python sidecar. It was replaced by Moonshine Small Streaming which has
2.3x better accuracy (7.84% vs 18% WER) at similar RAM and faster latency.
- **Hallucination filter:** applied in `stt.rs` — catches
  "thank you for watching", < 2 alphabetic chars, etc.

## NLU Server — Lazy Python Sidecar (2026-08-31)

The **NLU server** (`server/nlu_server.py`) is the only remaining Python
dependency. It provides ML-based intent classification (BERT-Mini ONNX)
as a fallback when the deterministic parser (`intent_parser.rs`) can't
handle a command.

- **Port:** `39218` (separate from the old STT port 39217)
- **Lazy manager:** `src-tauri/src/lazy_nlu.rs` — spawns on first
  unparseable command, kills after 60s idle.
- **Model:** `server/nlu/model/nexus_nlu.onnx` + `.data` + `tokenizer/`
  — committed to git (~18 MB) so fresh clones work without downloading.
- **Requirements:** `server/nlu/requirements.txt` (numpy, onnxruntime,
  fastapi, uvicorn, pydantic, transformers).
- **Fallback:** if the NLU server is unavailable, `nlu_client.rs`
  returns `None` and the deterministic parser handles the command.

### Historical STT Pipeline Fixes (2026-08-30, faster-whisper era)

These bugs were fixed in the faster-whisper Python sidecar. They are
**still relevant** (the sidecar was restored after the Moonshine experiment)
but kept for historical context:

1. **`lazy_stt.rs` path bug:** `stt_script_path()` was missing one
   `.parent()` level. Fixed by adding the correct path.
2. **`ensure_stt_running()` not called on hotkey:** Fixed by adding
   calls to `hotkey.rs` and `stt.rs`.
3. **`is_stt_responsive()` used tokio runtime:** Fixed by using a raw
   TCP connection instead.
4. **STT idle timeout too aggressive:** 60s → 5 minutes.

### Whisper hallucination filter (`stt.rs`)

The hallucination filter is still active in `stt.rs`. It catches common
hallucinations on noisy/silent audio:
- "thank you for watching", "you", "bye", "okay", etc.
- Text with < 2 alphabetic characters
Filtered text is replaced with empty string, triggering the frontend's
"didn't catch that" retry logic (up to 3 retries).

## NEXUS CLI — Unified Cross-Platform Command (`nexus.mjs`)

The unified `nexus` command works on Windows, macOS, and Linux:

```
nexus install     Install prerequisites + build + global 'nexus' command
nexus setup       Install prerequisites + build (no global command)
nexus build       Build frontend + Rust release binary
nexus dev         Tauri dev mode (hot reload via Vite)
nexus start       Launch the built app (unified console on Windows)
nexus run         Alias for 'start'
nexus check       Diagnostics (tools, frontend, Rust, NLU, Worker)
nexus clean       Remove build artifacts
nexus worker      Deploy the Cloudflare Worker (optional)
nexus help        Show help
```

- **Windows:** `nexus.cmd` shim → `node nexus.mjs`
- **Unix:** `nexus` shell script → `node nexus.mjs`
- **Global install:** `nexus install` creates a global command in
  `%USERPROFILE%\.local\bin` (Windows) or `/usr/local/bin` (Unix).
- **`nexus start` on Windows** uses `scripts/run.ps1` for the unified
  color-coded console (Rust logs, audio, frontend CDP in one stream).
- The old `scripts/nexus.bat` and `scripts/nexus.cmd` have been removed.

## Connection Diagnostics (`src-tauri/src/diagnostics.rs`)

Checks 5 services and logs a formatted table on startup:

| Service | Check method | Expected |
|---------|-------------|----------|
| STT | HTTP GET to port 39217/health | OK if running, LAZY if not yet started |
| TTS | In-process Kokoro/Fish Audio readiness (hardcoded ready) | Always OK |
| Cloudflare Worker | HTTPS GET to /health | OK if reachable |
| GitHub | HTTPS GET to Worker /oauth/status | OK if OAuth connected |
| Google | HTTPS GET to Worker /oauth/status | OK if OAuth connected |

Also available as:
- Tauri command: `nexus_diagnostics` (returns JSON to frontend)
- CLI: `nexus check` (build/tool diagnostics via `nexus.mjs`)
- Startup: auto-logged 5s after boot

## Wake Word Model Validation + Mic Silence Recovery (2026-08-30)

### Model is PERFECT — the problem is the Intel SST mic driver

Tested the v2 `nexus.onnx` model with the exact Rust pipeline
(mel → normalize → slice[4:80] → embedding → classifier):

| Input | Model probability | Verdict |
|-------|------------------|---------|
| TTS "nexus" | 0.994 | ✅ |
| TTS "hey nexus" | 0.999 | ✅ |
| TTS "nexus wake up" | 0.999 | ✅ |
| TTS "ok nexus" | 0.999 | ✅ |
| 20 negative samples | 0.0001-0.0002 | ✅ perfect rejection |
| **Trigger rate** | **5/5 positives** | ✅ 100% recall on TTS |
| **False positive rate** | **0/20 negatives** | ✅ 0% false triggers |

The model is NOT the problem. The problem is the **Intel Smart Sound
Technology driver** — it stops delivering audio after 2-25 minutes of
use (RMS drops to exactly 0.000000 and stays there).

### Silence Recovery Thread (`wakeword_oww.rs`)

Added a background thread that monitors the audio callback counter and
automatically restarts the cpal stream when the mic goes silent.

**Settings (tuned for Intel SST bursty audio):**
- Poll interval: **5s** (was 30s)
- Silence threshold: **165 callbacks (~5s)** (was 1000/30s)
- Restart method: **`try_device_silent`** (no 5s probe — saves 5s per cycle)
- Nuclear option: every **12 restarts (~60s of silence)**, restarts the
  Windows Audio service (`net stop/start Audiosrv`) to try to unstick the
  Intel SST driver
- Total restart cycle: **~5s** (was 35s with probe)

**Why 5s?** The Intel SST driver delivers audio in brief 5-15s bursts after
each stream restart, then goes silent. A 5s poll gives us the maximum
number of chances to catch a working window.

**Confirmation RMS threshold lowered from 0.01 to 0.002:**
The 500ms confirmation window was rejecting valid wakes because the mic
fades to silence during the confirmation period. At 0.01, a wake with
RMS=0.0048 was rejected. At 0.002, it would be confirmed.

### Intel SST driver fix (requires admin)

When the mic goes permanently silent, the fix is to restart the driver:

```powershell
# Run as Admin:
pnputil /restart-device "INTELAUDIO\CTLR_DEV_51CA&LINKTYPE_02&DEVTYPE_00&VEN_8086&DEV_AE20&SUBSYS_8BE0103C&REV_10EC\5&111f6c68&0&0000"
```

Or: Device Manager > Sound, video and game controllers > Intel Smart
Sound Technology for Digital Microphones > right-click > Disable > Enable.

Or: Restart the Windows Audio service:
```powershell
Restart-Service -Name "Audiosrv" -Force
```

If none of these work, a full OS restart is required. The Intel SST
driver has a known bug where it stops delivering audio after some time.
Updating to the latest driver from the laptop manufacturer (HP) may help.

### Test scripts (in project root, gitignored)

- `test_wake_model.py` — tests the model with TTS + negative samples
- `test_live_mic.py` — records 5s from the mic and tests the model
- `test_all_devices.py` — tests all audio input devices
- `test_mic_freq.py` — records and shows frequency content
- `gen_tts.py` — generates TTS "NEXUS" samples via Windows SAPI

## RAM Optimization — Lazy Windows + In-Process STT (2026-08-30)

**Idle RAM: 384 MB** (down from 1,644 MB — 77% reduction).

### What was wrong
- `tauri.conf.json` created 5 windows at startup (main, setup, settings,
  sidebar, architect). Each WebView2 window spawns ~7 processes (~250 MB).
  4 of the 5 windows were `visible: false` but still consumed full RAM.
- The old STT server (faster-whisper tiny.en) ran constantly, using ~340 MB
  even when no one was speaking.

### Fix 1: Lazy window creation (`src-tauri/src/dyn_windows.rs`)
- Only `main` (orb) is in `tauri.conf.json` — created at startup.
- `setup`, `settings`, `sidebar`, `architect` are created on-demand by
  `dyn_windows::get_or_create_window()` when first needed.
- `hide_sidebar` / `close_setup_window` / `close_settings_window` now
  **destroy** the window (not `hide()`) — kills the WebView2 process tree
  and frees ~250 MB per window.
- Platform effects (DWM corners, macOS vibrancy) applied at creation time
  inside `get_or_create_window()`.

### Fix 2: Lazy faster-whisper STT server
- STT uses faster-whisper tiny.en via a lazy-started Python sidecar on
  port 39217. See "STT Architecture" section above.
- `lazy_stt.rs` starts the server on first wake/hotkey, kills after 5min idle.
- STT RAM is ~0 MB at idle (server not running), ~340 MB when active.

### Measured RAM (idle, after fix)
| Component          | Before   | After    |
|--------------------|----------|----------|
| NEXUS.exe (Rust)   | 47.9 MB  | 40.8 MB  |
| WebView2 (1 window)| 870 MB   | 344 MB   |
| STT server         | 339 MB   | 0 MB     |
| **TOTAL**          | **1,644 MB** | **385 MB** |

### Files changed
- `src-tauri/tauri.conf.json` — removed 4 windows, kept only `main`
- `src-tauri/src/dyn_windows.rs` — NEW: dynamic window creation/destruction
- `src-tauri/src/stt.rs` — HTTP proxy to faster-whisper sidecar
- `src-tauri/src/lib.rs` — registered new modules, removed startup sidebar vibrancy
- `src-tauri/src/commands.rs` — all show/hide functions use dyn_windows
- `src-tauri/src/architect.rs` — uses dyn_windows for architect window
- `src-tauri/src/hotkey.rs` — sidebar close uses destroy_window
- `src-tauri/src/tray.rs` — settings menu uses dyn_windows
- `src-tauri/src/wakeword_oww.rs` — calls ensure_stt_running() on wake
- `src-tauri/src/stt.rs` — calls mark_stt_request() on each transcription
- `scripts/run.ps1` — no longer starts STT server at boot

## Sidebar — Do NOT use window-vibrancy on non-activating windows (2026-08-30)

**`src-tauri/src/lib.rs` / `src-tauri/src/commands.rs`: the sidebar window
deliberately calls NO `window_vibrancy` function** (no `apply_blur`,
`apply_acrylic`, `apply_mica`). This was a hard-won finding — do not
re-add these calls without reading this section first.

**Why**: the sidebar is a non-activating window (`focus: false`,
`alwaysOnTop: true`, `skipTaskbar: true`) so it never steals keyboard
focus from whatever app the user is working in. Windows' DWM *material*
APIs (Acrylic/Mica via `DWMWA_SYSTEMBACKDROP_TYPE`, or the legacy
`SetWindowCompositionAttribute` accent path used by `apply_blur`) render
a flat, solid **fallback color** for any window that isn't the OS-active
window — this is documented Windows behavior (Mica/Acrylic docs list
"window deactivates" as a fallback-to-solid-color condition), not
something `window-vibrancy` or Tauri can override. Confirmed via
`microsoft/microsoft-ui-xaml#10570` (`DesktopAcrylicBackdrop` loses blur
on `WS_EX_NOACTIVATE` windows) and `tauri-apps/window-vibrancy#183`
(Acrylic/Mica broken on Windows 11 24H2/25H2 in general).

Worse: calling these material APIs **overrides** Tauri's own
`transparent: true` mechanism (`tao` registers the window with DWM via
`DwmEnableBlurBehindWindow` + an empty blur region at window creation —
that's what actually makes a Tauri window see-through, no material
needed). When the material then fails to render (because the window is
never active), DWM falls back to **solid opaque** instead of the
window's original see-through state. This produced a fully opaque
black/grey panel that looked worse than doing nothing.

**The fix**: removed the vibrancy calls entirely. The sidebar was
already genuinely transparent via `transparent: true` in
`tauri.conf.json` — the same mechanism the main orb window uses
successfully. Result: sharp (not blurred) but real, focus-independent
transparency. `src-tauri/src/dwm_corners.rs` still calls
`DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND)`
directly (a plain window-shape attribute, not a material — unaffected
by the active/inactive issue) so the OS window's corners match the CSS
card's `border-radius`, avoiding a "double panel" mismatch (WebView2
has no `CornerRadius` support, so without this the DWM-painted window
rectangle and the rounded CSS card show as two different shapes).

Also removed: a CSS chromatic-aberration effect (red/cyan inset
`box-shadow` on `.sidebar-card::after`) that was meant to simulate
glass prism fringing. Without real optical refraction (no SVG
`feDisplacementMap` — `backdrop-filter: url()` is also a no-op on a
transparent WebView2, see below), it just read as a colored-border
rendering bug. Replaced with a neutral lit-bezel `box-shadow` stack
(top specular + bottom shadow) for a physical-glass feel without color.

Separately confirmed: CSS `backdrop-filter` (blur or `url()` SVG
refraction) is a **no-op in a transparent WebView2** — see
`MicrosoftEdge/WebView2Feedback#4945`. It can't composite against
nothing. Don't rely on it for this window; any blur must come from a
native OS mechanism, and per above, none is currently available for a
non-activating window without a full WinRT `DesktopAcrylicController` +
`SystemBackdropConfiguration.IsInputActive = true` interop (what
PowerToys uses for its non-activating flyouts) — out of scope unless
revisited.

### Screenshot-capture blur (the actual liquid glass)

Since native blur is unavailable for non-activating windows, the sidebar
uses a **"fake blur"**: right before `win.show()`, Rust captures the desktop
region behind the window via GDI `BitBlt`, blurs it with
`image::imageops::fast_blur(sigma=32)`, encodes it as a PNG data URI, and
hands it to the frontend as a CSS `background-image` on `.sidebar-card`
via the `--sidebar-backdrop-image` CSS variable. This gives a genuine
frosted-glass look without depending on window activation state.

**Critical timing:** the capture MUST happen before `win.show()` so the
sidebar doesn't capture itself. If the window is already visible (re-show),
capture is skipped. See `src-tauri/src/sidebar_backdrop.rs`.

Full implementation guide: `docs/features/21-liquid-glass-sidebar.md`.

### Dynamic window pending-content pattern (race-free event delivery)

When a window is created on-demand (via `dyn_windows.rs`), the WebView2
needs time to load the HTML and mount the React app. If Rust emits Tauri
events immediately after creating the window, **those events are lost**
because no listener exists yet.

**Fix:** store the content in a `static Mutex<Option<...>>` and let the
frontend fetch it on mount via a `get_pending_*` command. This is race-free
regardless of how long the WebView takes to load. If the window already
exists (React loaded), events are also emitted as a fast path.

Currently used for:
- `PENDING_SIDEBAR` in `commands.rs` → `get_pending_sidebar_content`
- `PENDING_ARCHITECT_REPO` in `architect.rs` → `get_pending_architect_repo`

**All show/create commands MUST be `async`** — `WebviewWindowBuilder::build()`
dispatches to the main thread, and a synchronous Tauri command runs on a
blocking thread that can't yield, causing a deadlock.

To reuse this pattern for a new window, see the "How to Reuse" section in
`docs/features/21-liquid-glass-sidebar.md`.

## Architecture Mapper — Phase 1 Latency Optimization (2026-08-30)

Phase 1 now uses **Approach C (hybrid)** for a 3-4s first response:

1. **Parallelized GitHub API calls** (`tokio::join!`): metadata + recursive
   tree are fetched concurrently using the symbolic ref `HEAD` (verified
   against repos with `main` and `master` default branches). Cuts ~600-1000ms
   off the critical path vs the old sequential metadata→tree flow.
2. **Instant Rust heuristic clustering** for first paint (~5ms) — the diagram
   appears in ~1-1.5s with generic layer labels.
3. **Async LLM enrichment** (`enrich_phase1` command): after first paint, the
   client POSTs the heuristic layers + sample file paths to the Worker's
   `phase1_enrich` intent. The LLM (Mistral 24B) rewrites generic labels into
   repo-specific ones (e.g. "Client / Presentation Layer" → "Next.js App
   Router (React 19)") and writes a real summary. Result streams back via
   the `architect:phase1-enriched` event ~2-3s later and merges in-place.
   **Never blocks first paint.** If the Worker/LLM fails, the heuristic
   diagram remains (graceful degradation).

| Component | File | What changed |
|-----------|------|--------------|
| Rust parallel fetch | `src-tauri/src/architect.rs` | `analyze_repo_phase1` uses `tokio::join!` + `HEAD` ref |
| Rust enrichment cmd | `src-tauri/src/architect.rs` | New `enrich_phase1` command + `Phase1Enrichment`/`EnrichedLayer` types |
| Rust session accessor | `src-tauri/src/network.rs` | New `get_session_info()` public helper |
| Worker handler | `server/worker/src/index.ts` | New `handlePhase1Enrich` + `phase1_enrich` intent dispatch |
| Frontend store | `frontend/src/architect/architectStore.ts` | New `enrichPhase1` action + `sample_file_paths` field |
| Frontend app | `frontend/src/architect/ArchitectApp.tsx` | Calls `enrich_phase1` after paint + listens for enriched event |

## Building

**Always build the desktop app with the Tauri CLI:**

```powershell
pwsh ./scripts/build.ps1          # frontend + tauri release build + bundles
```

If you need a plain cargo build (faster iteration, no installer), you **must**
pass the `custom-protocol` feature:

```powershell
npm --prefix frontend run build
cargo build --release --features custom-protocol   # run inside src-tauri/
```

### Why `custom-protocol` is mandatory

Tauri decides whether to load the bundled frontend or the Vite dev server
purely from this feature flag:

```rust
// tauri-macros/src/context.rs
dev: cfg!(not(feature = "custom-protocol")),
```

- Feature **on**  → windows load `http://tauri.localhost/...` (embedded assets)
- Feature **off** → windows load `devUrl` = `http://localhost:5173`

`cargo tauri build` adds the feature automatically; a bare `cargo build
--release` does **not**. A release binary built without it shows
`localhost refused to connect` / `ERR_CONNECTION_REFUSED` in every window,
because no Vite server is running. Clearing the WebView2 profile does not
help — the dev URL is baked into the binary at compile time.

The feature is deliberately **not** in `[features] default`, because
`tauri dev` needs it off for hot reload.

### Verifying which URL the app actually loads

```powershell
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"
Start-Process .\src-tauri\target\release\nexus.exe
Start-Sleep 15
Invoke-RestMethod http://127.0.0.1:9222/json/list | Select-Object title, url
```

Expected: every window on `http://tauri.localhost/...`.
Bad: `http://localhost:5173/...` → rebuild with `--features custom-protocol`.

## Frontend windows

Every window declared in `src-tauri/tauri.conf.json` must have a matching
rollup input in `frontend/vite.config.ts`, otherwise its HTML file is absent
from `dist/` and the window fails to load in release builds (dev mode hides
this — the Vite server serves any HTML file on demand).

| tauri.conf.json window | HTML            | vite input |
| ---------------------- | --------------- | ---------- |
| `main`                 | `index.html`    | `main`     |
| `setup`                | `setup.html`    | `setup`    |
| `settings`             | `settings.html` | `settings` |
| `sidebar`              | `sidebar.html`  | `sidebar`  |

## Local ports

| Service           | Port    | Notes                                        |
| ----------------- | ------- | -------------------------------------------- |
| STT (faster-whisper) | 39217 | Lazy-started Python sidecar (POST /transcribe) |
| NLU server        | `39218` | Lazy Python sidecar (BERT-Mini ONNX). Override: `NLU_PORT` |
| Sidecar (legacy)  | `41098` | Legacy FastAPI sidecar, not used at runtime  |
| Vite dev server   | `5173`  | Dev only                                     |

## Architecture (serverless — 2026-08-27)

NEXUS is now **fully serverless**. No sidecar, no n8n, no Ollama, no server.

```
NEXUS laptop → HTTP POST → Cloudflare Worker → APIs → text response
                              ↑
                        D1 database (OAuth tokens)
                        Workers AI (intent + summarization)
```

- **Worker** (`server/worker/`): Cloudflare Worker on the edge. Handles
  intent classification, API calls (GitHub/Google), summarization, OAuth
  exchange, token storage, and user registration. <5ms cold start.
- **D1**: Cloudflare's free SQLite. Stores OAuth tokens, API keys, and
  device registrations. 5GB free.
- **Workers AI**: Free tier (10K neurons/day) for intent classification
  (Qwen 0.5B) and summarization (Qwen 14B).
- **Client** (`src-tauri/src/network.rs`): HTTP POST to the Worker. No
  WebSocket. Emits state/ack/result/done events to the frontend.
- **NEXUS_SERVER_URL**: Baked into the installer at build time. Points to
  the Worker URL (e.g. `https://nexus-worker.xxx.workers.dev`).

The old sidecar (`server/sidecar/`) is kept in the repo for reference but
no longer spawned at startup. `sidecar_manager.rs` has been removed.

### Building the installer with the Worker URL

```powershell
$env:NEXUS_SERVER_URL = "https://nexus-worker.your-subdomain.workers.dev"
pwsh ./scripts/build.ps1
```

## Runtime paths (Windows)

- App data (config, logs): `%APPDATA%\com.nexus.assistant\`
- WebView2 profile: `%LOCALAPPDATA%\com.nexus.assistant\EBWebView`
  (note: **Local**, not Roaming — `app_data_dir()` returns Roaming and is the
  wrong path for WebView2)

## Wake word (openWakeWord)

- Default feature: `wakeword-oww` (tract-onnx inference in Rust)
- Models: `src-tauri/resources/oww/{melspectrogram.onnx, embedding_model.onnx, nexus.onnx}`
- Threshold: 0.35, chunk size: 1280 samples (80ms at 16kHz)
- **Detection logic (2026-08-29):** Max-based detection + secondary confirmation.
  The old averaging approach diluted single good frames (0.4+) with surrounding
  0.0s, giving avg=0.03 which never triggered. Now uses max probability in the
  12-frame buffer, so a single 0.36+ frame triggers. After a raw detection,
  collects 500ms of audio and checks RMS ≥ 0.01 to confirm real speech (filters
  noise spikes). Refractory period: 3s.
- **Model v2 (2026-08-30, Kaggle):** Retrained on Kaggle T4 GPU with:
  - 5000 positive samples (5 phrase variants: "nexus", "hey nexus", "nexus wake up", "ok nexus", "nexus please")
  - 30+ soundalike negatives (vs 8 in v1)
  - 80000 training steps (vs 50000), layer_size=64 (vs 32)
  - 2x augmentation rounds, target FP/hr=0.1 (vs 0.2)
  - Model size: 415KB (vs 205KB v1)
  - Kernel: `chitkullakshya/train-nexus-wakeword-v2`
  - v1 backup: `src-tauri/resources/oww/nexus_v1.onnx.backup`
- **Silence gate + AGC (2026-08-28):** `detect_chunk` computes RMS of each
  80ms chunk and skips the classifier entirely if RMS < 0.0005 (~-66dBFS).
  The `nexus.onnx` model emits 0.6-0.9 probabilities on pure digital silence
  (out-of-distribution input), which caused spontaneous false wakes. The
  gate prevents the model from ever seeing silence. Min positive detections
  = 2. Regression test: `test_silence_never_triggers_wake`.
  - **AGC (Automatic Gain Control):** If RMS passes the gate but is below
    TARGET_RMS (0.03), the chunk is amplified up to 50x before feeding the
    classifier. This makes quiet/whispered "NEXUS" produce the same model
    input as loud "NEXUS", so the model (trained on normal-volume TTS)
    recognizes low-volume speech without retraining.
  - Gate: 0.0005, threshold: 0.45. Pure silence (RMS=0) is blocked.
  - Model: trained on Kaggle (v22), accuracy 78.6%, recall 58.2%, FP/hr 1.33.
- **Mic device enumeration (2026-08-27):** `start_audio_capture` enumerates
  ALL input devices, probes each for 5 seconds, and picks the first one
  that produces non-silent audio (RMS > 0.0001). If all devices are silent
  (Intel SST bug), falls back to the best device anyway. This fixes the
  "wake word doesn't work, only hotkey" issue caused by cpal getting
  silence from the Intel Smart Sound Technology driver.
- **FIXED (2026-08-24):** The wake word now works — probability 0.991 for real
  "NEXUS" speech. The root cause was a 32768x input scaling mismatch: cpal
  produces f32 audio in [-1.0, 1.0] but the openWakeWord melspectrogram model
  expects int16-scale float32 values in [-32768, 32767]. Fix: multiply audio
  by 32768.0 in `wakeword_oww.rs` before feeding to the melspectrogram model.
- **Mic conflict (FIXED):** The frontend's `warmMic()` (getUserMedia via WebView2)
  conflicts with the Rust cpal wake-word stream on Intel Smart Sound Technology
  drivers. `warmMic()` is disabled at startup; the mic is acquired on first
  wake instead. This is why cpal was getting silence (RMS=0.0000).
- The global hotkey still works independently of the wake-word model.
- **Command models (2026-08-25):** Training 4 category-level acoustic models
  (`command_open`, `command_close`, `command_search`, `command_play`) via
  `train_nexus_commands.ipynb` on Google Colab. Models detect command TYPE,
  then STT extracts the parameter (which app, what query). See
  `src-tauri/resources/oww/commands/command_intents.json`.

## Known limitations (2026-08-26 audit)

- **Speaker verification is NOT wired.** Enrollment works (setup wizard ->
  embedding -> JSON on disk), but `wakeword_oww::WakeEngine::process`
  accepts every wake regardless of speaker. The verification API in
  `voice_profile.rs` is `#[allow(dead_code)]` until an audio ring buffer
  is added to retain the wake utterance for embedding extraction.
- **All windows skip the taskbar.** `main`, `setup`, `settings`, and
  `sidebar` all have `skipTaskbar: true` in `tauri.conf.json`. NEXUS is
  accessible only via the floating orb, the system tray, the global
  hotkey, and the wake word.
- **STT server auto-launcher writes `server/start_stt.cmd`** with an
  absolute path to the local Python interpreter. This file is gitignored
  (machine-specific, leaks username).
