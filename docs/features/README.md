# NEXUS Assistant — Feature Implementation Index

This folder contains detailed documentation for every feature implemented,
bug fixed, and architecture decision made across the `prem224k` and `prem22k`
branches, forming the basis of PR #7 (merge of both branches into `main`).

**Date:** 2026-08-29
**Branches merged:** `prem224k` + `prem22k` → `main`
**PR:** #7

---

## Feature Index

### Voice & PR Analysis (prem224k — this session)

| # | Feature | File | Description |
|---|---|---|---|
| 01 | GitHub PR Analysis via Voice | [01-github-pr-analysis.md](01-github-pr-analysis.md) | Say "analyse PR 5 in servx" → GLM-4.7-Flash code review in sidebar |
| 02 | STT Mishearing Fixes | [02-stt-mishearing-fixes.md](02-stt-mishearing-fixes.md) | Post-processing corrections + dynamic hotwords for tiny.en |
| 03 | Sidebar Streaming Text Animation | [03-sidebar-streaming-animation.md](03-sidebar-streaming-animation.md) | ChatGPT/Gemini-style word fade-in, left-to-right, top-to-bottom |
| 04 | On-It-Sir Flow | [04-on-it-sir-flow.md](04-on-it-sir-flow.md) | Immediate ack for long queries → orb hides → "Here is the analysis" |
| 05 | Worker Fuzzy Repo Matching | [05-worker-fuzzy-repo-matching.md](05-worker-fuzzy-repo-matching.md) | Levenshtein distance matching for misheard repo names |

### Wake-Word & Hotkey (prem224k)

| # | Feature | File | Description |
|---|---|---|---|
| 06 | Wake-Word Reliability | [06-wake-word-reliability.md](06-wake-word-reliability.md) | Single-frame high-confidence trigger (0.5+ bypass) |
| 07 | State-Dependent Hotkey | [07-state-dependent-hotkey.md](07-state-dependent-hotkey.md) | Sidebar-aware: close sidebar OR wake, not both |
| 08 | CSP for Silero VAD | [08-csp-silero-vad.md](08-csp-silero-vad.md) | CDN script-src + worker-src blob for VAD WASM |

### Voice Engines & Cross-Platform (prem22k)

| # | Feature | File | Description |
|---|---|---|---|
| 09 | Multi-Voice TTS Engine | [09-multi-voice-tts.md](09-multi-voice-tts.md) | Gemini Flash, Fish Audio Ethan, ElevenLabs Jarvis/Nova/Echo/Onyx |
| 10 | Non-Activating Overlay | [10-non-activating-overlay.md](10-non-activating-overlay.md) | Orb doesn't steal keyboard focus from IDEs/terminals |
| 11 | Linux D-Bus MPRIS | [11-linux-mpris.md](11-linux-mpris.md) | Native media control via zbus (PlayPause, Next, Previous) |
| 12 | VAD Post-TTS Mute Gate | [12-vad-post-tts-mute.md](12-vad-post-tts-mute.md) | 300ms mic mute after TTS to prevent echo self-triggering |

### CI/CD & Setup (prem22k)

| # | Feature | File | Description |
|---|---|---|---|
| 13 | GitHub Actions CI/CD | [13-github-actions-cicd.md](13-github-actions-cicd.md) | Auto-build Windows NSIS .exe installer on push |
| 14 | Setup Wizard Redesign | [14-setup-wizard-redesign.md](14-setup-wizard-redesign.md) | Voice persona selection, API keys, preferences |

### Architecture & Merge

| # | Feature | File | Description |
|---|---|---|---|
| 15 | Architecture Decisions | [15-architecture-decisions.md](15-architecture-decisions.md) | Serverless model, sidebar delivery, done event timing |
| 16 | Conflict Resolution | [16-conflict-resolution.md](16-conflict-resolution.md) | How 3 overlapping files were merged from both branches |

### AK Repo Port (2026-08-29 — this session)

| # | Feature | File | Description |
|---|---|---|---|
| 17 | Mic Baton Pass | [17-ak-port-mic-baton-pass.md](17-ak-port-mic-baton-pass.md) | Pause/resume cpal stream around getUserMedia to fix Intel SST mic lock |
| 18 | Cancel Hotkey + Double Wake Fix | [18-ak-port-cancel-hotkey-double-wake-fix.md](18-ak-port-cancel-hotkey-double-wake-fix.md) | Ctrl+Space cancel hotkey + fix triple event emission causing "on it sir" twice |
| 19 | Audio Volume + Multi-Turn VAD | [19-ak-port-audio-volume-multi-turn-vad.md](19-ak-port-audio-volume-multi-turn-vad.md) | RMS volume tracking for avatar reactivity + "didn't catch that" retry (max 3) |
| 20 | STT Fix + Wake Reliability | [20-stt-fix-wakeword-reliability.md](20-stt-fix-wakeword-reliability.md) | STT server missing __main__ block + wake word model assessment |
| 21 | Liquid Glass Sidebar | [21-liquid-glass-sidebar.md](21-liquid-glass-sidebar.md) | Screenshot-capture blur (GDI BitBlt + Rust blur) for non-activating windows + pending content pattern for dynamic windows |
| 22 | Worker AI Latency Optimization | [22-worker-ai-latency-optimization-plan.md](22-worker-ai-latency-optimization-plan.md) | Plan to reduce Worker AI response from 40s to 3-4s via prompt truncation, model tiering, GitHub API caching, and SSE streaming |
| 23 | TTS Voice Research | [23-tts-voice-research-elevenlabs-vs-fish.md](23-tts-voice-research-elevenlabs-vs-fish.md) | ElevenLabs vs Fish Audio free tier analysis — Fish Audio s2.1-pro-free recommended (free API, ~100ms TTFA, voice cloning) |
| 24 | TTS Deep Research (All Providers) | [24-tts-deep-research-all-providers.md](24-tts-deep-research-all-providers.md) | 10 TTS providers compared — Kokoro (self-host, Apache 2.0, unlimited), Fish Audio, Google Cloud, Polly, Piper, Azure, HuggingFace, Coqui, ElevenLabs, TTSMaker |
| 25 | Rich Repo Analysis Dashboard | [25-rich-repo-analysis-dashboard.md](25-rich-repo-analysis-dashboard.md) | GLM-4.7-flash (free) + GitHub languages API + pie charts (languages/frameworks) + databases + features + top bar heading |
| 26 | Wake-Word Training Guide | [26-wake-word-training-guide.md](26-wake-word-training-guide.md) | How to train a custom openWakeWord ONNX model for "nexus" with TTS samples + negative samples |
| 27 | Loading Indicator Overlay | [27-loading-indicator-overlay.md](27-loading-indicator-overlay.md) | Transparent click-through Lottie animation at top-right corner, shown during Worker processing after "On it sir" |
| 28 | Linux Wayland Compatibility | [28-linux-wayland-compatibility.md](28-linux-wayland-compatibility.md) | Layer-shell, PipeWire, ashpd shortcuts, .desktop parsing, manylinux |
| 29 | Overlay Architecture Analysis | [29-overlay-architecture-analysis.md](29-overlay-architecture-analysis.md) | Multi-window vs single-overlay vs native vs hybrid Raycast model |
| 30 | Computer-Use Vision-Action Loop | [30-computer-use-vision-action-loop.md](30-computer-use-vision-action-loop.md) | Astra-style screenshot→action loop, grounding, harness, gates (research only) |
| 31 | JARVIS-like Assistants Research | [31-jarvis-like-assistants-research.md](31-jarvis-like-assistants-research.md) | Open-source clones, wearables, computer-control patterns, steal list |

### Research System (2026-09-01)

| # | Feature | File | Description |
|---|---|---|---|
| R01 | Research Sources | [research/01-research-sources.md](research/01-research-sources.md) | 9 ad-free research sources (Wikipedia, Wikidata, DDG, knowledgelib, SearchX, Tavily, Google CSE, Serper, Wolfram, Semantic Scholar) |
| R02 | LLM Cascade | [research/02-llm-cascade.md](research/02-llm-cascade.md) | Gemini Flash Lite (1,500/day) → Groq Qwen 3.8 (14,400/day) → Cloudflare llama-3.2-3b (~200/day) |
| R03 | API Keys & Secrets | [research/03-api-keys-and-secrets.md](research/03-api-keys-and-secrets.md) | Every API key, where to get it, free tier details, Cloudflare secret setup |
| R04 | Cascade Architecture | [research/04-cascade-architecture.md](research/04-cascade-architecture.md) | Full retrieval + synthesis flow, decision trees, prompt construction |
| R05 | Capacity Analysis | [research/05-capacity-analysis-10-users.md](research/05-capacity-analysis-10-users.md) | Whether each free tier survives 10 active users — 153x LLM headroom |
| R06 | Latency Benchmarks | [research/06-latency-benchmarks.md](research/06-latency-benchmarks.md) | Measured end-to-end latency for every command type, from live tests |
| R07 | Deployment Guide | [research/07-deployment-guide.md](research/07-deployment-guide.md) | Step-by-step deploy: secrets, D1 schema, KV namespace, wrangler config |
| R08 | Testing Results | [research/08-testing-results.md](research/08-testing-results.md) | 13 live test queries with timings, provider routing, and output quality |
| R09 | Intent Routing Fixes | [research/09-intent-routing-fixes.md](research/09-intent-routing-fixes.md) | Bug fixes: "research" keyword, isSearchQuestion, isAcademicQuery, reasoning leakage |
| R10 | Future Improvements | [research/10-future-improvements.md](research/10-future-improvements.md) | Pending keys, streaming, parallel racing, cache versioning, scaling to 100+ users |

---

## Quick Summary

### prem224k (2 commits + this session's work)
- GitHub PR analysis via voice commands
- STT post-processing for misheard words
- Sidebar streaming text animation
- "On it sir" → "Here is the analysis" flow
- Fuzzy repo name matching (Levenshtein)
- Wake-word single-frame high-confidence trigger
- State-dependent hotkey
- CSP for Silero VAD CDN

### prem22k (20 commits)
- Multi-voice TTS engine (6 voices, 3 providers)
- Non-activating floating overlay window
- Linux D-Bus MPRIS media control
- Dual-phase VAD post-TTS mute gate
- GitHub Actions CI/CD pipeline
- Setup wizard redesign
- Pop!_OS/WebKitGTK fixes
- Multi-hotkey binding for Linux
- Single-instance daemon lock
- NSIS installer auto-launch setup

### Merge conflicts (3 files, all resolved)
- `hotkey.rs` — Combined multi-hotkey + state-dependent logic
- `wakeword_oww.rs` — Took prem224k's precise high-confidence approach
- `tauri.conf.json` — Combined visible:true + CSP changes

### AK repo port (4 features, all implemented and tested)
- Mic baton pass (pause/resume cpal stream for Intel SST compatibility)
- Cancel hotkey (Ctrl+Space) + double "on it sir" fix (triple event emission)
- Audio volume RMS tracking + multi-turn VAD resume + "didn't catch that" retry
- STT server missing `__main__` block fix + wake word reliability assessment

---

## Architecture Evolution Documents (2026-09-02 to 2026-09-03)

These documents trace the architectural shifts in NEXUS — what changed,
why it changed, what the previous approach was, and what we learned.
Created for future reference so contributors understand the rationale
behind current architectural decisions.

| # | Feature | File | Description |
|---|---------|------|-------------|
| 36 | TTS Engine Evolution | [36-tts-engine-evolution.md](36-tts-engine-evolution.md) | Kokoro (350 MB) → Piper (80 MB) — 270 MB RAM reduction, 20x faster load |
| 37 | STT Engine Evolution | [37-stt-engine-evolution.md](37-stt-engine-evolution.md) | Moonshine (failed, garbage transcripts) → faster-whisper (restored) + self-learning |
| 38 | Loading Indicator Evolution | [38-loading-indicator-evolution.md](38-loading-indicator-evolution.md) | Separate Tauri window (Wayland bugs, 40 MB) → in-orb rendering (0 MB, no bugs) |
| 39 | Voice Ack Pipeline Evolution | [39-voice-ack-pipeline-evolution.md](39-voice-ack-pipeline-evolution.md) | Always-ack (250ms) → validate-first + cached TTS (5ms, 67x faster) |
| 40 | Self-Learning STT | [40-self-learning-stt.md](40-self-learning-stt.md) | Static corrections (manual) → self-learning from user repetition (automatic) |
| 41 | CI/CD Evolution | [41-ci-cd-evolution.md](41-ci-cd-evolution.md) | No CI → cross-platform installer pipeline (Windows .exe/.msi + macOS .app) |
| 42 | RAM Optimization Journey | [42-ram-optimization-journey.md](42-ram-optimization-journey.md) | 1,644 MB → 104 MB idle (94% reduction) — lazy loading, window destruction, Piper |
| 43 | Architectural Timeline | [43-architectural-timeline.md](43-architectural-timeline.md) | Master overview of all architectural shifts and key decisions |

### Key Architectural Decisions (Summarized)

1. **Python STT sidecar** (not in-process Rust) — accuracy > elegance
2. **Piper TTS** (not Kokoro) — 270 MB RAM savings, 20x faster load
3. **In-orb loading** (not separate window) — Wayland fix, 40 MB savings
4. **Self-learning STT** (not static corrections) — scales per user
5. **Validate before ack** (not always ack) — don't acknowledge garbage
6. **Cached TTS** (not synthesize each time) — 5ms vs 250ms for "On it sir"
7. **GitHub Actions macOS** (not physical Mac) — free, real Apple hardware
8. **Unsigned .app** (not signed .dmg) — no Apple Developer cert yet
9. **Single Worker** (not multiple) — no cross-Worker latency
10. **No window-vibrancy on sidebar** — DWM renders solid on non-activating windows
