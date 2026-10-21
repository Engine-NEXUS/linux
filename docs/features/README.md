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
| 28 | System Volume Control Research | [28-system-volume-control-research.md](28-system-volume-control-research.md) | Deep research on programmatic volume control across Windows (Core Audio COM), macOS (CoreAudio), and Linux (PipeWire/PulseAudio/ALSA) — permissions, APIs, Rust integration |
| 29 | TTS Auto-Volume Plan | [29-tts-auto-volume-plan.md](29-tts-auto-volume-plan.md) | Plan: before NEXUS speaks, set system volume to configured level (default 75%), restore after TTS completes — Windows COM, macOS CoreAudio, Linux shell |

### NLU Training & Voice Collection (2026-09-12)

| # | Feature | File | Description |
|---|---|---|---|
| 51 | NLU Training & Voice Collection | [51-nlu-training-and-voice-collection.md](51-nlu-training-and-voice-collection.md) | Complete guide for `nexus collect` (voice sample collector) and `nexus train` (full BERT-Mini retraining pipeline) — 52 intents, 45 slot types, data flow, troubleshooting, admin brain integration |

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
| R-NLU-01 | External NLU Data Sources | [../research/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md](../research/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md) | Ranked comparison of public datasets, opt-in user telemetry, custom speech vendors, scraping/legal constraints, privacy architecture, ingestion quality gates, and phased NEXUS data plan |
| R-NLU-02 | BERT-Mini Deep Audit | [../research/nlu-model-and-dataset-deep-audit-2026-09-14.md](../research/nlu-model-and-dataset-deep-audit-2026-09-14.md) | Structural and ONNX audit, before/after metrics, repaired slot training, remaining OOS/live-intent gaps, and prioritized retraining plan |
| R-NLU-03 | Code vs Research Accuracy Gap | [../research/nlu-wakeword-code-vs-research-accuracy-gap-2026-09-14.md](../research/nlu-wakeword-code-vs-research-accuracy-gap-2026-09-14.md) | NLU and wake implementation compared with primary calibration, OOS, joint-SLU, and keyword-spotting research; ranked low-RAM improvements |
| R-NLU-LIVE | Latest NLU Audit | [../research/nlu-model-data-audit-latest.md](../research/nlu-model-data-audit-latest.md) | Generated by `nexus audit` with current dataset and model metrics |
| T-NLU-01 | NLU Future Testing Playbook | [../testing/nlu-future-testing-and-model-promotion-playbook.md](../testing/nlu-future-testing-and-model-promotion-playbook.md) | Permanent test procedure: baseline metrics, intent/slot/OOS/safety/voice suites, external-data gates, candidate comparison, model promotion, rollback, and report templates |
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

### Architecture Mapper Polish (2026-09-02)

| # | Feature | File | Description |
|---|---|---|---|
| 33 | Architecture Mapper Voice → Loading Flow | [33-architecture-mapper-voice-loading-flow.md](33-architecture-mapper-voice-loading-flow.md) | "Open architecture mapper" → immediate "On it sir" → orb hides → loading.json at top-right → background analysis → window opens when ready |
| 34 | Architecture Mapper Layout Redesign | [34-architecture-mapper-layout-redesign.md](34-architecture-mapper-layout-redesign.md) | 3 view tabs (Files/Hotspots/Cycles) in header, full-width map, 2-row bottom section (file inspector + analytics) |
| 35 | Custom-Protocol Build Requirement | [35-custom-protocol-build-requirement.md](35-custom-protocol-build-requirement.md) | Frontend changes require Rust binary rebuild (`cargo build --release --features custom-protocol`) — frontend is embedded in the binary |

### Settings Sidebar Research (2026-09-06)

| # | Feature | File | Description |
|---|---|---|---|
| 49 | Settings Sidebar Research | [49-settings-sidebar-research.md](49-settings-sidebar-research.md) | Research for unified settings sidebar: Google/GitHub OAuth, Gemini/Groq API keys, TTS volume slider, orb position/size sliders |
| 50 | Orb Position + Size Persistence Plan | [50-orb-position-persistence-plan.md](50-orb-position-persistence-plan.md) | Detailed plan for orb position/size sliders with 100% persistence assurance — saves to settings.json, overwrites on new save |

### Interactive Plan Approval & Voice Confirmation (2026-09-21)

| # | Feature | File | Description |
|---|---|---|---|
| 54 | Interactive Voice Approval & Confirmation Sidebar | [54-interactive-voice-approval-and-confirmation-sidebar.md](54-interactive-voice-approval-and-confirmation-sidebar.md) | Automatic 5s voice approval listening window with early-exit instant execution (<2s), persistent Response Sidebar confirmation card, and dual voice/mouse controls |
| 55 | NLU Data Foundation, STT Conditioning & Voice Scaling | [55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md](55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md) | STT Whisper prompt conditioning (`temperature=0.0`), decoupled acoustic vs NLU text pipeline, 70/30 data principle, and OTA model distribution |
| 56 | Targeted Intent Training & MCP Data Promotion | [56-targeted-intent-training-and-mcp-data-promotion.md](56-targeted-intent-training-and-mcp-data-promotion.md) | Category-specific interactive voice collection (`nexus collect -c -i`), full 55-intent coverage, zero-quarantine promotion, and aligned intent mastery |
| 57 | Apex Wake Word Evolution & Hardware Adaptation | [57-apex-wake-word-evolution-and-hardware-adaptation.md](57-apex-wake-word-evolution-and-hardware-adaptation.md) | Spectral microphone prober, chassis resonance auto-tuning (128.3 Hz HPF), dynamic AGC pre-gain, phantom trigger elimination, and multilingual negative augmentation |
| 62 | Full-Codebase Audit & Top-5 Critical Fixes | [62-full-codebase-audit-and-top-5-critical-fixes.md](62-full-codebase-audit-and-top-5-critical-fixes.md) | 5-agent audit (6 critical / 5 high / ~20 medium); fixed serde wire tags, NLU OTA pipeline, Moonshine production STT, root package.json, ghostwriter abort, worker quota double-count |
| 63 | Single-Stage Shell & Blackout Policy | [63-single-stage-shell-and-blackout-policy.md](63-single-stage-shell-and-blackout-policy.md) | Transparent overlay window, pixel-identical geometry mapping, dynamic hitbox toggling, and watchdog blackout auto-recovery |
| 64 | Ghost Mode Windows Control Plan | [64-ghost-mode-plan.md](64-ghost-mode-plan.md) | AI drives the real Windows cursor while user speaks, stage ring visualization, keyboard-first Win-search flows, takeover rails |
| 65 | Dual-Target Wake Word Balancing & Engine Alignment | [65-dual-target-wake-word-balancing-and-rust-engine-alignment.md](65-dual-target-wake-word-balancing-and-rust-engine-alignment.md) | Balanced positive sample training, 1.5s cold-boot grace, VAD onset chopping fix, and SHA-256 manifest synchronization |
| 66 | Hey Jarvis vs. NEXUS Benchmark & Multi-Syllabic Parity | [66-hey-jarvis-vs-nexus-comparative-acoustic-benchmark-and-multi-syllabic-parity.md](66-hey-jarvis-vs-nexus-comparative-acoustic-benchmark-and-multi-syllabic-parity.md) | Empirical comparison vs openWakeWord Jarvis, 22.05 kHz ingestion bug resolution, universal resampling, and 0.00% FA multi-syllabic parity |
| 67 | Instant Neural Wake Alignment & Clean Console UX | [67-instant-neural-wake-alignment-and-clean-console-ux.md](67-instant-neural-wake-alignment-and-clean-console-ux.md) | Bypassed redundant Stage-2 STT verification, eliminated 500ms post-hit delay, quad-mic active stereo downmixer, fail-open onset VAD, and minimal trigger banner console |
| 68 | Live Audio Telemetry, Trigger Debouncing & Diagnostics Parity | [68-live-audio-telemetry-and-connection-diagnostics-parity.md](68-live-audio-telemetry-and-connection-diagnostics-parity.md) | Real-time 12-slice speaker waveform and status meter in `nexus start`, atomic 1.5s neural trigger debouncing, and startup cloud connection diagnostics box |
| 69 | NLU Phonetic Alias Map & Persistent Missed-Intent Logging | [69-nlu-phonetic-alias-map-and-missed-intent-logging.md](69-nlu-phonetic-alias-map-and-missed-intent-logging.md) | Deterministic soundalike phrase normalizer for Ghost Mode and apps, stop phrases expansion, and persistent `%APPDATA%/com.nexus.assistant/missed_intents.jsonl` audit logging |
| 70 | **Ghost Mode Complete Program** | [70-ghost-mode-complete.md](70-ghost-mode-complete.md) | One-stop guide: phases 0→4 + voice integration (entry split, waves orb, hot-mic, in-session app/message routing with confirm-gated send), takeover leash, hardening, honest remainders |
| A1 | Ghost Architecture Reference | [ghost-mode-architecture-reference.md](ghost-mode-architecture-reference.md) | Symbol map (ghost.rs state, runners, grounding), wiring call-graph, and the Wry-typed Send invariant (why runtime erasure lives in one place) |
| 71 | Personalized & Neural-Augmented Wake-Word Training | [71-personalized-and-neural-augmented-wakeword-training.md](71-personalized-and-neural-augmented-wakeword-training.md) | 432 positive sample retraining (94 human mic + 88 SAPI + 250 Microsoft Neural) with Binary Focal Loss across 41,183 windows, 98.6% recall at 0.68, 0.00% FA on 186 fast multi-syllabic negatives |
| 72 | STT Hallucination Mitigation, Strict Wave Motion Rule & Architect Routing Fix | [72-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md](72-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md) | Acoustic pre-gate (<0.005 RMS), expanded Whisper hallucination filter, strict resting wave baseline (0.15 flat, Lottie paused), eliminated truncated "open" architect false triggers, demoted WhatsApp bridge probe logs |
| 73 | Ghost Waves, Browser Search & Typing, TTS Latency, and Intent Isolation | [73-ghost-waves-browser-search-typing-and-intent-isolation.md](73-ghost-waves-browser-search-typing-and-intent-isolation.md) | Deep root-cause resolution for wake/ghost wave persistence, local browser search & typing (Ctrl+L / dictation), app-open TTS latency elimination (<5ms cached), and complete GitHub intent isolation |
| 74 | Main Center + Per-Action Sub-Centers (plan) | [74-main-center-with-per-action-sub-centers.md](74-main-center-with-per-action-sub-centers.md) | Decision-tree router (one sub-center per action family), Alexa-mapped validity gate with spoken repeat-backs, single UI/STT/TTS directors; P0–P5 phased plan |
| 75 | Ghost-Session UX, Animation Timing & STT Gating (plan) | [75-ghost-session-ux-animation-timing-and-stt-gating.md](75-ghost-session-ux-animation-timing-and-stt-gating.md) | Orb persistence in ghost, speaking visibility, Esc/Ctrl+Space exits, n× repetition + Latin-gibberish STT gates, loading timing; P-A–P-E phased plan |
| 75 | GoogleCenter Hierarchical Domain Architecture & Proactive Sentinel | [75-google-center-hierarchical-domain-architecture-and-proactive-sentinel.md](75-google-center-hierarchical-domain-architecture-and-proactive-sentinel.md) | Domain sub-services (mail, calendar, maps, photos) unit-tested in isolation, unified GoogleCenter hub with OAuth2 PKCE, proactive background deadline sentinel, and Main Center UI/TTS integration |
| 76 | Vision Mode Screen Email Scanning & Proactive Watch Memory | [76-vision-mode-screen-email-scanning-and-proactive-watch-memory.md](76-vision-mode-screen-email-scanning-and-proactive-watch-memory.md) | VLM screen semantic extraction for open emails, dedicated active watch memory store (watches.json), and background Sentinel proactive deadline alerts |
| 77 | Multi-Email Google Accounts, Direct Native OAuth & Settings Refactor | [77-multi-email-google-accounts-and-direct-oauth-settings.md](77-multi-email-google-accounts-and-direct-oauth-settings.md) | Direct native desktop OAuth (RFC 8252) with loopback listener, multi-account storage in Windows Credential Manager, Settings accounts refactor, and Brave URL extraction fix |
| 78 | Sentinel Orb-Landing Event & Tracking Layer | [78-sentinel-orb-landing-event-and-tracking-layer.md](78-sentinel-orb-landing-event-and-tracking-layer.md) | Structured `orchestrator:sentinel-alert` event per detected change (spec wire shape) + lifecycle tracking store; landing animation deferred to a later phase |
| 79 | Cloned Agent-Subsystem Integration | [79-cloned-agent-subsystem-integration.md](79-cloned-agent-subsystem-integration.md) | Clone → extract → delete (mcp-agent, Memento, agent-workflow-mcp; LedgerMind dropped as Linux-only); provenance SHAs + corrected source map |
| 80 | Production Orb via Lottie Creator-MCP & Five-State Matrix | [80-production-orb-creator-mcp-pipeline.md](80-production-orb-creator-mcp-pipeline.md) | Creator-MCP editing pipeline (persistent relay, batch runner, export audits), wakeup markers + asset rotation, 3-bar waves prune, final state matrix (idle freeze / thinking loading / listening+speaking zoom), duplicate `pulse-listen` last-wins fix |
| 96 | Ghost Mode Window Focus Polling, Contextual Search & Human Cursor Motion | [96-ghost-mode-focus-search-and-human-motion-architecture.md](96-ghost-mode-focus-search-and-human-motion-architecture.md) | UFO-style 150ms focus retry loop with Win32 Alt-key lock bypass, context-aware WhatsApp search drill routing, sub-20ms local UIA Edit control grounding, and Fitts's Law 60 FPS humanized cursor gliding |
| 98 | Screen Analysis Celebrity Recognition & Zero-Delay Pipeline | [98-screen-analysis-celebrity-recognition-and-zero-delay-spec.md](98-screen-analysis-celebrity-recognition-and-zero-delay-spec.md) | Eliminates 30s dead air down to ~1.2s via healthy Gemini model pruning (gemini-3.5-flash-lite / gemini-3.5-flash), kills 3-engine cascading retry, unblocks face recognition for public figures, and provides pooled HTTP/2 client |
| 126 | Universal Agent Watcher & Cross-Platform Music Control | [126-universal-agent-watcher-and-cross-platform-music-control-plan.md](126-universal-agent-watcher-and-cross-platform-music-control-plan.md) | Universal agent completion monitoring (Claude, Codex, Antigravity, Cursor, Trae, Windsurf, generic CLI/MCP), implicit foreground window targeting, OS keep-awake leasing, proactive voice alerting, and cross-platform native Apple Music & Spotify control |
| 126 | Questions, Answer Capture & Question Memory | [126-questions-answer-capture-and-question-memory-live-log-analysis-and-plan.md](126-questions-answer-capture-and-question-memory-live-log-analysis-and-plan.md) | Live-log analysis of unanswered questions and the plan for answer capture + question memory |
| 127 | One Voice Everywhere / Carbon-Copy TTS (superseded) | [127-one-voice-everywhere-carbon-copy-tts-analysis-and-plan.md](127-one-voice-everywhere-carbon-copy-tts-analysis-and-plan.md) | Superseded by 128/129; bug findings still stand |
| 128 | Exact Microsoft Voice Locally + Settings Hub Architecture | [128-exact-microsoft-voice-locally-and-settings-hub-architecture-audit-and-plan.md](128-exact-microsoft-voice-locally-and-settings-hub-architecture-audit-and-plan.md) | Where Ava/Ryan can legally be run locally; how the settings hub drifted |
| 129 | Free Open-Source One Voice Everywhere | [129-free-open-source-one-voice-everywhere-research-and-plan.md](129-free-open-source-one-voice-everywhere-research-and-plan.md) | Free/open-source constraint analysis (Kokoro as the one engine) |
| 130 | RAM Usage by State (measured + estimated) | [130-ram-usage-by-state-measured-and-estimated.md](130-ram-usage-by-state-measured-and-estimated.md) | Idle/active RAM tables |
| 131 | Voice Quality, Variety & RAM Reduction (measured) | [131-voice-quality-variety-and-ram-reduction-plan-measured.md](131-voice-quality-variety-and-ram-reduction-plan-measured.md) | Kokoro/NLU memory breakdown, arena-off speed cost |
| 132 | RAM Budget 600-700 MB | [132-ram-budget-600-700-mb-for-all-tasks-plan.md](132-ram-budget-600-700-mb-for-all-tasks-plan.md) | Memory governor design and eviction order |
| 133 | Coordination With Ghost (no-vision plan) | [133-coordination-with-ghost-no-vision-plan-impact-and-file-ownership.md](133-coordination-with-ghost-no-vision-plan-impact-and-file-ownership.md) | File ownership rules between sessions |
| 134 | Edge First, Local Standby, Latency Lanes | [134-edge-first-local-standby-connectivity-and-latency-lanes-plan.md](134-edge-first-local-standby-connectivity-and-latency-lanes-plan.md) | Connectivity state machine, bank, standby, two latency lanes |
| 135 | Smart Fallback Timing & Spoken Connection Notices | [135-smart-fallback-timing-and-spoken-connection-notices-plan.md](135-smart-fallback-timing-and-spoken-connection-notices-plan.md) | Switch timing, reactive notices, varied phrases |
| 136 | Voice / Connectivity / RAM Master Compendium | [136-voice-connectivity-and-ram-master-compendium.md](136-voice-connectivity-and-ram-master-compendium.md) | One-file review of 126-135 + the owner decisions |
| 137 | Free Open-Source Low-Memory Production Plan (GPT) | [137-free-open-source-low-memory-production-plan-2026-10-17.md](137-free-open-source-low-memory-production-plan-2026-10-17.md) | Free-provider contract, license, memory gates; single-local-voice default rejected |
| 138 | Shutdown Never Explained; Misheard Power Commands; How-To in Sidebar | [138-shutdown-never-explain-misheard-power-commands-and-howto-in-sidebar-plan.md](138-shutdown-never-explain-misheard-power-commands-and-howto-in-sidebar-plan.md) | Loose power match, learned phrases, steps card, sidebar delivery |
| 140 | Agent-Grade Computer Control: Research + NEXUS Action Kernel | [140-agent-grade-computer-control-research-and-nexus-action-kernel-plan.md](140-agent-grade-computer-control-research-and-nexus-action-kernel-plan.md) | How Claude Code/Codex/UFO2 control a PC safely (typed tools, policy, sandbox, undo); risk classes, quarantine delete, provenance; phased plan |
