# NEXUS Assistant — Branch `prem22k` Feature & Architecture Summary

This document details all technical implementations, architectural extractions from open-source AI assistants, speech engine integrations, bug fixes, and CI/CD enhancements made on branch **`prem22k`**.

---

## 1. 🚀 Executive Summary

Branch `prem22k` upgrades **NEXUS** into a fully serverless, highly responsive, zero-latency desktop assistant. Key highlights include:

* **Open-Source Architectural Upgrades:** Extracted windowing, audio gating, D-Bus media controls, and single-instance locks from leading open-source projects (**QwenPaw**, **Seeva**, **Newelle**, **Natively**, **PyGPT**).
* **Next-Gen Speech & AI Engines:** Integrated **Google Gemini 3.5 Transcribe** (STT), **Gemini 3.1 Flash TTS Preview**, and **Fish Audio s2.1-pro** (Ethan voice model).
* **Pop!_OS / Wayland Compatibility:** Resolved Linux WebKitGTK DOM surface suspension, fixed GNOME shortcut hijacking, and added multi-channel IPC event broadcasting.
* **Automated Windows `.exe` CI/CD:** Configured NSIS installer templates and GitHub Actions workflow for automatic Windows executable installer builds.

---

## 2. 🏛️ Open-Source Architectural Features Extracted

### 1. Non-Activating Floating Overlay Window (*from Seeva & QwenPaw*)
* **Concept:** Floating assistant windows (orb & sidebars) use non-activating window flags (`set_focusable(false)`).
* **Implementation:** Modified `src-tauri/src/window_manager.rs`, `wakeword_oww.rs`, `hotkey.rs`, and `commands.rs`.
* **Impact:** Prevents NEXUS from stealing active keyboard focus or causing full-screen window flicker when waking up over IDEs, terminals, or video players.

### 2. Linux Native D-Bus & MPRIS Media Control (*from Newelle*)
* **Concept:** Direct communication over Linux D-Bus (`org.mpris.MediaPlayer2`) using `zbus` 5.x.
* **Implementation:** Created `src-tauri/src/mpris.rs` with `send_mpris_command` (`PlayPause`, `Play`, `Pause`, `Next`, `Previous`, `Stop`) and `send_native_notification`.
* **Impact:** Provides zero-latency media playback control and desktop toast notifications without launching external sub-shells or `pkill` processes.

### 3. Dual-Phase Audio VAD 300ms Post-TTS Mute Gate (*from Natively & PyGPT*)
* **Concept:** Drops microphone PCM buffer for **300ms** immediately after TTS finishes speaking.
* **Implementation:** Added `last_tts_active` timer in `wakeword_oww.rs` chunk processing loop and a 300ms delay in frontend `main.tsx` `startListening()`.
* **Impact:** Allows speaker hardware decay and room acoustic reflections to clear, completely eliminating TTS echo self-triggering loops.

### 4. Single-Instance Daemon & Lock Handling (*from QwenPaw*)
* **Concept:** Enforces a single running daemon process via `tauri-plugin-single-instance`.
* **Implementation:** Enhanced single-instance callback in `src-tauri/src/lib.rs` to route CLI arguments (`--setup`, `--settings`, `nexus://` deep links) to existing running windows.
* **Impact:** Re-launching `./install.sh` or CLI commands brings the existing setup/settings window to focus instead of corrupting audio device locks or spawning duplicate background tasks.

---

## 3. 🎙️ Next-Gen Speech & AI Engines

### 1. Google Gemini 3.5 Transcribe (STT)
* **Model:** `gemini-3.5-transcribe`
* **Implementation:** [`frontend/src/audio/stt.ts`](file:///home/premsaik/Desktop/Projects/NEXUS-Agent/frontend/src/audio/stt.ts)
* **Capabilities:** 85+ languages, automatic code-switching, filler word removal, domain vocabulary biasing.
* **Fallback:** Automatically falls back to local `faster-whisper` server (port `39217`) if no Gemini API key is configured.

### 2. Google Gemini 3.1 Flash TTS Preview (TTS)
* **Model:** `gemini-3.1-flash-tts-preview`
* **Implementation:** Added **Gemini Flash (Google AI)** to `CURATED_VOICES` in [`frontend/src/audio/ttsPlayer.ts`](file:///home/premsaik/Desktop/Projects/NEXUS-Agent/frontend/src/audio/ttsPlayer.ts).
* **Capabilities:** Natural, expressive speech synthesis with zero local GPU memory overhead.

### 3. Fish Audio `s2.1-pro` Model (Ethan Voice)
* **Voice ID:** `536d3a5e000945adb7038665781a4aca`
* **Model:** `s2.1-pro`
* **Implementation:** Integrated `playFishAudio(...)` streaming engine with MP3 blob playback.

### 4. Multi-Tier Speech Engine Hierarchy
```
    Speech Input (Mic)
            │
            ▼
 ┌──────────────────────┐   Fallback   ┌──────────────────────┐
 │ Gemini 3.5 Transcribe│ ────────────>│ Local faster-whisper │
 └──────────────────────┘              └──────────────────────┘

    Speech Output (TTS)
            │
            ▼
 ┌──────────────────────┐   Fallback   ┌──────────────────────┐   Fallback   ┌──────────────────────┐
 │  Gemini 3.1 Flash    │ ────────────>│  Fish Audio Ethan    │ ────────────>│ WebSpeech / WebAudio │
 └──────────────────────┘              └──────────────────────┘              └──────────────────────┘
```

---

## 4. 🐧 Pop!_OS / Linux Desktop Environment Fixes

* **WebKitGTK DOM Suspension Fix:** Changed `"visible": false` to `"visible": true` in `tauri.conf.json` for the `main` window. Prevents Linux WebKitGTK from freezing the DOM event loop while hidden.
* **Multi-Channel IPC Broadcast:** Added native Tauri event listeners for `"assistant:wake"` and `"nexus://wake"` alongside `window.__NEXUS_WAKE__`.
* **Multi-Hotkey Binding:** Registered `Ctrl+Shift+Space`, `Ctrl+Alt+Space`, and `Alt+Space` in `src-tauri/src/hotkey.rs` to bypass GNOME Wayland shortcut hijacking.
* **Instant Wake-Word Sensitivity:** Lowered openWakeWord `MIN_POSITIVE_DETECTIONS` from `2.0` to `1.0` in `wakeword_oww.rs` for instant acoustic peak triggers.

---

## 5. 📦 Environment Configuration & CI/CD Pipeline

* **Environment Variables (`.env` & `env.example`):** Added root `.env` and `env.example` files. Configured Rust `get_settings` in `commands.rs` to automatically read `GEMINI_API_KEY` and `FISH_AUDIO_API_KEY` from system environment variables if empty in `settings.json`.
* **Windows NSIS Installer (`nexus-installer.nsi`):** Updated NSIS finish page to execute `$INSTDIR\nexus.exe --setup` automatically upon installation completion.
* **GitHub Actions Workflow (`.github/workflows/build-windows.yml`):** Created automated CI/CD pipeline running on `windows-latest` cloud runners. Compiles `NEXUS_0.1.0_x64-setup.exe` and uploads the installer artifact on every push to `prem22k`.

---

## 6. 📝 Commit History Log (`prem22k`)

| Commit Hash | Message Summary |
|---|---|
| `4742d95` | `fix: use working-directory: ./frontend in GitHub Actions workflow for Windows PowerShell compatibility` |
| `3c0ba57` | `ci: add GitHub Actions workflow to build native Windows NEXUS .exe installer on push to prem22k` |
| `733b9aa` | `feat: update NSIS Windows installer to automatically launch setup wizard (--setup) on completion` |
| `9d540f5` | `feat: integrate Gemini 3.5 Transcribe (STT) and Gemini 3.1 Flash TTS Preview (TTS) engines with fallback` |
| `0f6b00d` | `fix: resolve Pop!_OS WebKitGTK DOM suspension bug by making main window visible: true and broadcasting native IPC wake events` |
| `65f48e0` | `fix: call register() on global shortcut in hotkey.rs and set MIN_POSITIVE_DETECTIONS to 1.0 for instant wake-word trigger` |
| `2fc5886` | `feat: add .env and env.example configuration with environment fallback for Fish Audio API key` |
| `c01e9b9` | `feat: integrate Ethan (Fish Audio s2.1-pro model 536d3a5e000945adb7038665781a4aca) into voice persona suite` |
| `25d667e` | `feat: enhance single-instance daemon lock handling for secondary CLI args (--setup, --settings)` |
| `d36ae95` | `feat: implement dual-phase audio VAD 300ms post-TTS mute gate` |
| `41150ae` | `feat: implement native Linux D-Bus MPRIS media control and desktop notifications via zbus` |
| `335e8ca` | `feat: implement non-activating floating overlay window to prevent stealing keyboard focus` |
| `4d51104` | `fix: WebKitGTK setup window flicker, multi-tier TTS fallback, and Linux taskbar positioning` |
| `f5514bc` | `fix: direct neural audio streaming for voice previews and updated CSP media permissions` |
| `6ee5a48` | `fix: auto-open setup wizard on launch & manage running instances during install` |
