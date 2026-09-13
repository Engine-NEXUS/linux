# NEXUS Documentation Index

> Complete technical documentation for the NEXUS floating desktop assistant.
> A cross-platform, Siri-like, thin-client assistant that talks to a fat server (n8n + Ollama).
> All documentation reflects the current state of the codebase as of 2026-08-23.

---

## Start Here

If you're new to the project, read in this order:

1. **[architecture/01-system-overview.md](./architecture/01-system-overview.md)** — the mental model (thin client + fat server, text-only protocol, 3 trigger paths, 3 response paths).
2. **[architecture/02-data-flow-graphs.md](./architecture/02-data-flow-graphs.md)** — sequence diagrams for every major flow.
3. **[architecture/03-component-map.md](./architecture/03-component-map.md)** — which file does what.
4. **[credentials/01-credential-architecture.md](./credentials/01-credential-architecture.md)** — where secrets live and how they flow.
5. **[features/01-wake-word.md](./features/01-wake-word.md)** — how the always-listening ear works.
6. **[changes/CHANGELOG.md](./changes/CHANGELOG.md)** — what changed and why, per commit.

---

## Table of Contents

### Architecture (How the system is built)

| # | Document | Description |
|---|----------|-------------|
| 01 | [system-overview.md](./architecture/01-system-overview.md) | High-level architecture map: thin client + fat server, 5 golden rules, 3 trigger paths, 3 response paths, runtime process topology |
| 02 | [data-flow-graphs.md](./architecture/02-data-flow-graphs.md) | ASCII sequence diagrams for: general request, Tier 3 fixed command, Tier 3 parameterized command, boot greeting, sleep/wake greeting, meeting suppression, OAuth flow, API key flow, sidecar auto-spawn, cancel/barge-in |
| 03 | [component-map.md](./architecture/03-component-map.md) | Every source file mapped to its purpose, key exports, and what it talks to (Rust, frontend, Python sidecar, config, models, notebooks) |
| 04 | [tech-stack.md](./architecture/04-tech-stack.md) | Every crate, library, and tool chosen for NEXUS, with the reason it was picked over alternatives + feature flags + port allocation |
| 05 | [state-machine.md](./architecture/05-state-machine.md) | Frontend Zustand state machine: states, transitions, side effects per transition, barge-in, Tier 3 bypass, boot greeting bypass, meeting override |
| 06 | [liquid-glass-screenshot-blur.md](./architecture/06-liquid-glass-screenshot-blur.md) | Liquid glass blur for sidebar and architect windows |
| 07 | [central-orchestrator.md](./architecture/07-central-orchestrator.md) | Central orchestrator: single owner of request lifecycle, routing, loading state, cancellation, request IDs |
| 08 | [oauth-github-flow.md](./architecture/08-oauth-github-flow.md) | GitHub OAuth 2.0 PKCE flow: browser redirect, token exchange, deep-link callback, polling detection |
| 09 | [request-flow-evolution.md](./architecture/09-request-flow-evolution.md) | Evolution of request handling from scattered to centralized (Phase 1 → Phase 4) |
| 10 | [github-subcommand-system.md](./architecture/10-github-subcommand-system.md) | GitHub sub-command system: 28 typed commands via octocrab, conflict detection, centralized confirmation, token management |

### Features (What each feature does and how)

| # | Document | Description |
|---|----------|-------------|
| 01 | [wake-word.md](./features/01-wake-word.md) | openWakeWord KWS engine: 3-stage ONNX pipeline, sound-alikes, speaker verification, meeting suppression |
| 02 | [tier3-commands.md](./features/02-tier3-commands.md) | Acoustic command classifiers that skip STT for ~200ms latency. 39 commands (30 fixed + 9 parameterized). Type 1 vs Type 2 flow |
| 03 | [meeting-privacy-mode.md](./features/03-meeting-privacy-mode.md) | 4-layer suppression: manual pause, WASAPI detection, process scan, TTS muting. What gets suppressed, hysteresis, frontend integration |
| 04 | [boot-greeting.md](./features/04-boot-greeting.md) | "Hello sir, how can I assist you today?" on fresh boot (uptime < 15 min) or sleep/wake. Suppression conditions, non-blocking design |
| 05 | [sidecar-manager.md](./features/05-sidecar-manager.md) | Auto-spawn Python FastAPI sidecar in background. pythonw.exe, port 49152, log redirection, sidecar reuse, package-qualified invocation |
| 06 | [mic-permissions.md](./features/06-mic-permissions.md) | WebView2 permission handler: auto-approve mic/camera for NEXUS-owned origins only. No more permission dialog on restart |
| 07 | [app-registry.md](./features/07-app-registry.md) | Pre-indexed app launcher (Raycast/Alfred style). Disk cache + in-memory HashMap + fuzzy match. ~1ms per command |
| 08 | [voice-enrollment.md](./features/08-voice-enrollment.md) | Speaker verification: 5 enrollment clips, sherpa-onnx embeddings, cosine similarity, wake variants, sound-alikes |
| 09 | [audio-pipeline.md](./features/09-audio-pipeline.md) | Complete local audio chain: ScriptProcessorNode capture → Silero VAD → faster-whisper STT (127.0.0.1) → Web Speech API TTS |
| 10 | [window-overlay.md](./features/10-window-overlay.md) | Transparent, frameless, always-on-top orb. Region-aware click-through. Bottom-center positioning. Slide animation. macOS accessory app |
| 11 | [system-tray.md](./features/11-system-tray.md) | Tray menu: show, pause/resume, settings, quit. Autostart. Single instance. Deep-link forwarding |
| 12 | [settings-window.md](./features/12-settings-window.md) | Dedicated tabbed settings window (600x720): General, Audio, Wake Word, Privacy, Backend. White theme. Settings persisted to JSON via Rust IPC |
| 13 | [response-sidebar.md](./features/13-response-sidebar.md) | Right-edge transparent window (280x500) that shows server responses only. Slides in from right when n8n/Ollama responds. Not for local commands |
| 14 | [nsis-installer.md](./features/14-nsis-installer.md) | Custom white-themed NSIS installer with NEXUS branding, 220x500 sidebar image, no desktop shortcut (Start Menu only). 40.1 MB LZMA compressed |
| 15 | [setup-wizard.md](./features/15-setup-wizard.md) | 4-step onboarding wizard (520x680): Welcome → Server → Voice → Accounts. Multi-option Google + GitHub cards with brand icons. API keys section |
| 45 | [central-orchestrator.md](./features/45-central-orchestrator.md) | Central orchestrator: single owner of request lifecycle, routing, loading, cancellation |
| 46 | [github-oauth-connect.md](./features/46-github-oauth-connect.md) | GitHub OAuth connect button: browser redirect, one-click authorize, auto-detection |
| 47 | [loading-indicator-ownership.md](./features/47-loading-indicator-ownership.md) | Loading indicator centralized in orchestrator (Rust owns show/hide) |
| 48 | [github-subcommand-system.md](./features/48-github-subcommand-system.md) | GitHub sub-command system: 28 typed commands, conflict detection with copy-paste, centralized confirmation, natural language parsing |
| 51 | [51-nlu-training-and-voice-collection.md](./features/51-nlu-training-and-voice-collection.md) | Complete guide for `nexus collect` (voice sample collector) and `nexus train` (BERT-Mini retraining pipeline) — 52 intents, 45 slot types, data flow, troubleshooting |

### Credentials (How API keys, OAuth, and device tokens work)

| # | Document | Description |
|---|----------|-------------|
| 01 | [credential-architecture.md](./credentials/01-credential-architecture.md) | Master doc: 3 credential types (OAuth tokens, API keys, device tokens), where secrets live, credential flow at request time, security properties |
| 02 | [oauth-flow.md](./credentials/02-oauth-flow.md) | OAuth2 PKCE flow step-by-step for Google + GitHub. Token exchange, refresh, disconnect. Scopes requested |
| 03 | [api-keys.md](./credentials/03-api-keys.md) | API key management: add/remove/list endpoints. Fernet encryption at rest. How keys are used at request time. Google API keys vs OAuth |
| 04 | [google-integrations.md](./credentials/04-google-integrations.md) | Which Google APIs NEXUS uses, how each is authenticated (OAuth vs API key), scopes, quotas, setup instructions |
| 05 | [github-integration.md](./credentials/05-github-integration.md) | GitHub OAuth flow, scopes (repo read:org workflow), token characteristics, what NEXUS can do with GitHub |
| 06 | [device-registration.md](./credentials/06-device-registration.md) | Device registration and validation. Database schema. Local config. Future hardening plans |
| 07 | [security-best-practices.md](./credentials/07-security-best-practices.md) | Threat model, secret hygiene rules, production deployment checklist, incident response (if secrets exposed), text-only protocol as security property |
| 08 | [setup-page-guide.md](./credentials/08-setup-page-guide.md) | UI walkthrough of the setup page: server config, Google/GitHub OAuth, API keys, voice enrollment, save & continue |

### Changes (What changed and why, per commit)

| # | Document | Description |
|---|----------|-------------|
| — | [CHANGELOG.md](./changes/CHANGELOG.md) | All commits in reverse chronological order, organized by feature area |
| 01 | [browser-suppression.md](./changes/01-browser-suppression.md) | Disabled Windows restartable apps + removed Edge auto-launch (no browser on boot) |
| 02 | [non-blocking-sidecar.md](./changes/02-non-blocking-sidecar.md) | Moved sidecar spawn to background thread (5s → 0.2s to orb visible) |
| 03 | [boot-greeting.md](./changes/03-boot-greeting.md) | "Hello sir" greeting on fresh boot (uptime < 15 min) |
| 04 | [sleep-wake-detection.md](./changes/04-sleep-wake-detection.md) | Wall-clock time-jump detection for sleep/wake greeting |
| 05 | [mic-permission-handler.md](./changes/05-mic-permission-handler.md) | WebView2 permission handler (no more mic prompt on restart) |
| 06 | [sidecar-port-change.md](./changes/06-sidecar-port-change.md) | Port 8443 → 49152 (IANA dynamic range, no dev conflicts) |
| 07 | [silent-sidecar.md](./changes/07-silent-sidecar.md) | pythonw.exe instead of python.exe (no terminal window) |
| 08 | [connection-restart-fix.md](./changes/08-connection-restart-fix.md) | 3 root causes of "connection not found" on restart fixed |
| 09 | [frontend-embedding.md](./changes/09-frontend-embedding.md) | Frontend not embedded in .exe (ERR_CONNECTION_REFUSED) fixed |
| 10 | [auto-spawn-sidecar.md](./changes/10-auto-spawn-sidecar.md) | Sidecar auto-spawns on NEXUS startup |
| 11 | [tier3-commands.md](./changes/11-tier3-commands.md) | Acoustic command classifiers (skip STT, ~200ms latency) |
| 12 | [expanded-commands.md](./changes/12-expanded-commands.md) | 39 commands (30 fixed + 9 parameterized) |
| 13 | [colab-training.md](./changes/13-colab-training.md) | Colab notebook fixes (melspectrogram path, disk cleanup, Drive checkpointing, download retries) |
| 14 | [meeting-privacy-mode.md](./changes/14-meeting-privacy-mode.md) | Meeting detection + wake/TTS suppression |
| 15 | [oww-kws.md](./changes/15-oww-kws.md) | Migrated from VAD+ASR (~30% recall) to openWakeWord KWS (~100% recall) |
| 16 | [tts-fixes.md](./changes/16-tts-fixes.md) | Removed comma pause in "Didn't catch that sir" TTS |
| 17 | [white-theme-ui-overhaul.md](./changes/17-white-theme-ui-overhaul.md) | White theme design tokens, settings window, setup wizard. Orb changes later reverted |
| 18 | [orb-revert.md](./changes/18-orb-revert.md) | Reverted orb window to original 200x200 after user feedback. Settings + setup kept |
| 19 | [nsis-installer.md](./changes/19-nsis-installer.md) | Custom white-themed NSIS installer with branded images (220x500 sidebar, 180x68 header) |
| 20 | [setup-wizard-redesign.md](./changes/20-setup-wizard-redesign.md) | 4-step setup wizard with multi-option Google + GitHub account cards |
| 21 | [response-sidebar.md](./changes/21-response-sidebar.md) | Right-side response sidebar (280x500) that shows only for server responses |
| 22 | [installer-desktop-shortcut-removal.md](./changes/22-installer-desktop-shortcut-removal.md) | Removed desktop shortcut option from NSIS installer (Start Menu only) |
| 23 | [meeting-detection-self-trigger-fix.md](./changes/23-meeting-detection-self-trigger-fix.md) | Fixed NEXUS detecting its own WebView2 as a meeting (wake/TTS deadlock) |
| 24 | [local-first-intent-routing.md](./changes/24-local-first-intent-routing.md) | Local commands now execute before contacting sidecar (no more n8n dependency for basic commands) |
| 25 | [stt-server-auto-start.md](./changes/25-stt-server-auto-start.md) | STT server now auto-starts with NEXUS (was the root cause of all command failures) |
| 26 | [stt-performance-optimization.md](./changes/26-stt-performance-optimization.md) | STT: base→tiny.en, beam_size 5→1, eager loading — 54x faster, 22% less RAM |
| 27 | [native-app-priority-resolution-cache.md](./changes/27-native-app-priority-resolution-cache.md) | Opens native apps/PWAs/Store apps instead of browser tabs. Resolution cache + daily scan + cross-platform PWA discovery |
| 28 | [hot-mic-preinit-vad.md](./changes/28-hot-mic-preinit-vad.md) | Eliminates 2s wake-to-listen delay: hot mic + pre-init VAD + parallel init |
| 29 | [central-orchestrator.md](./changes/29-central-orchestrator.md) | Central orchestrator implementation: single owner of request lifecycle, routing, loading, cancellation |
| 30 | [github-oauth-fix.md](./changes/30-github-oauth-fix.md) | Fixed GitHub Connect button: shell plugin config, capabilities scope, fallback, macOS deep-link |
| 31 | [github-subcommand-system.md](./changes/31-github-subcommand-system.md) | GitHub sub-command system (Phase 2A): 28 typed commands via octocrab, conflict detection, centralized confirmation, 114 new tests |
| 33 | [33-nlu-training-voice-collection-cleanup.md](./changes/33-nlu-training-voice-collection-cleanup.md) | NLU training pipeline + voice collection (`nexus collect`, `nexus train`) + auto-cleanup of temp files + expanded synthetic data (1097 examples) |

### Testing Documentation

| # | Document | Description |
|---|----------|-------------|
| T-INDEX | [Testing Documentation Index](./testing/README.md) | Current repeatable testing procedures and links to historical results |
| T-NLU-01 | [NLU Future Testing and Model Promotion Playbook](./testing/nlu-future-testing-and-model-promotion-playbook.md) | Permanent 1,000-line procedure for dataset integrity, intents, slots, leakage, OOS/safety, real voice, external data, training, promotion gates, rollback, and test reporting |

### NLU Data Research

| # | Document | Description |
|---|----------|-------------|
| R-NLU-01 | [External NLU Data Sources and Acquisition Plan](./research/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md) | Extreme comparison of public NLU/speech datasets, opt-in live-user data, custom collection vendors, scraping constraints, privacy, rankings, ingestion design, evaluation gates, and phased implementation plan |
| R-NLU-02 | [BERT-Mini Model and Dataset Deep Audit](./research/nlu-model-and-dataset-deep-audit-2026-09-14.md) | Complete structural/model audit, root causes, before/after metrics, repaired training pipeline, remaining intent/OOS gaps, external-dataset mapping, and prioritized retraining plan |
| R-NLU-LIVE | [Latest Generated NLU Audit](./research/nlu-model-data-audit-latest.md) | Automatically regenerated by `nexus audit`; current dataset and ONNX quality metrics |

### Wake Word Detection (Detailed Deep Dive)

The wake word system went through a major architectural change. These 20 documents explain the full journey: research, decision-making, old approach, new approach, training, validation, and every component in detail.

#### Research & Decisions

| # | Document | Description |
|---|----------|-------------|
| 01 | [wake-word-research.md](./wake-word/01-wake-word-research.md) | Research into how Alexa, Google, Siri, and open-source projects do wake word detection |
| 02 | [wake-word-architecture-decision.md](./wake-word/02-wake-word-architecture-decision.md) | Why we chose openWakeWord over VAD+ASR, Porcupine, and other options |

#### Old Approach (Deprecated)

| # | Document | Description |
|---|----------|-------------|
| 03 | [vad-asr-old-approach.md](./wake-word/03-vad-asr-old-approach.md) | The original VAD + ASR pipeline and why it failed |

#### New Approach (Current)

| # | Document | Description |
|---|----------|-------------|
| 04 | [oww-kws-new-approach.md](./wake-word/04-oww-kws-new-approach.md) | The new openWakeWord KWS pipeline |
| 05 | [oww-3-stage-pipeline.md](./wake-word/05-oww-3-stage-pipeline.md) | Deep dive: melspectrogram → embedding → classifier |

#### Model Training & Validation

| # | Document | Description |
|---|----------|-------------|
| 06 | [model-training.md](./wake-word/06-model-training.md) | How the custom "nexus" ONNX model was trained |
| 13 | [colab-training-notebook.md](./wake-word/13-colab-training-notebook.md) | Cell-by-cell breakdown of the training notebook |
| 14 | [model-validation-results.md](./wake-word/14-model-validation-results.md) | Runtime validation results — 7/7 detections, 0 false positives |

#### Speaker & Variants

| # | Document | Description |
|---|----------|-------------|
| 07 | [speaker-verification.md](./wake-word/07-speaker-verification.md) | Voice profile system: embeddings, enrollment, verification |
| 08 | [wake-variants-soundalikes.md](./wake-word/08-wake-variants-soundalikes.md) | Wake variants + sound-alikes for pronunciation tolerance |

#### Implementation

| # | Document | Description |
|---|----------|-------------|
| 09 | [audio-pipeline.md](./wake-word/09-audio-pipeline.md) | Audio capture: cpal, downmixing, resampling, chunking |
| 10 | [rust-integration.md](./wake-word/10-rust-integration.md) | Rust integration: tract-onnx, Cargo features, module wiring |

#### Testing & Performance

| # | Document | Description |
|---|----------|-------------|
| 11 | [testing-strategy.md](./wake-word/11-testing-strategy.md) | Test plan: what to verify, how to test, expected results |
| 12 | [performance-expectations.md](./wake-word/12-performance-expectations.md) | Performance: RAM, CPU, latency comparisons |

#### Tier 3: Direct Command Classification

| # | Document | Description |
|---|----------|-------------|
| 15 | [tier3-command-classifiers.md](./wake-word/15-tier3-command-classifiers.md) | Tier 3 architecture: how command classifiers work |
| 16 | [tier3-decision-comparison.md](./wake-word/16-tier3-decision-comparison.md) | All 6 options considered for latency reduction |
| 17 | [tier3-resource-analysis.md](./wake-word/17-tier3-resource-analysis.md) | Measured RAM/CPU/latency breakdown |
| 18 | [tier3-training-approach.md](./wake-word/18-tier3-training-approach.md) | 4 training approaches compared |
| 19 | [tier3-testing-strategy.md](./wake-word/19-tier3-testing-strategy.md) | Test plan for Tier 3 |
| 20 | [expanded-command-system.md](./wake-word/20-expanded-command-system.md) | The 39-command system |

### Meeting Protection

| # | Document | Description |
|---|----------|-------------|
| 01 | [meeting-detection.md](./meeting-protection/01-meeting-detection.md) | Meeting detection architecture and implementation |

### Top-Level Docs

| Document | Description |
|----------|-------------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Original principal architecture & implementation specification |
| [DEPLOYMENT.md](./DEPLOYMENT.md) | Server deployment guide |

---

## Quick Reference

### Architecture Comparison

| Aspect | Old (VAD+ASR) | New (openWakeWord KWS) | Tier 3 (Command Classifiers) |
|--------|---------------|------------------------|------------------------------|
| Architecture | VAD gate → ASR → text match | KWS sliding window → probability | OWW classifiers for known commands |
| Recall | ~30% | ~100% (7/7 in validation) | ~95%+ (trained per command) |
| Latency | 500-1000ms | ~80ms (wake) | **~200ms (command → action)** |
| Command latency | 27,000ms (Whisper base) | 27,000ms (still uses Whisper) | **~200ms (skips Whisper entirely)** |
| RAM | ~143 MB | ~30-50 MB | **~5 MB per command** (shared features) |
| False positives | Frequent | 0 observed | Controlled by threshold + negatives |

### Model Files

| File | Size | Role |
|------|------|------|
| `src-tauri/resources/oww/nexus.onnx` | 790 KB | Custom trained wake word classifier |
| `src-tauri/resources/oww/melspectrogram.onnx` | 1.1 MB | Pre-trained mel spectrogram extractor |
| `src-tauri/resources/oww/embedding_model.onnx` | 1.3 MB | Pre-trained embedding extractor |
| `src-tauri/resources/oww/commands/*.onnx` | ~800 KB each | Tier 3 command classifiers |
| `command_intents.json` | — | Intent mapping for command classifiers |
| `train_nexus_oww.ipynb` | — | Wake word training notebook (Colab) |
| `train_nexus_commands.ipynb` | — | Command classifier training notebook (Colab) |

### Ports

| Port | Service |
|------|---------|
| 49152 | Python sidecar (FastAPI) |
| 8000 | Local STT server (faster-whisper) |
| 5678 | n8n (on the server) |
| 11434 | Ollama (on the server) |

### Current Status (2026-08-19)

| Component | Status | Notes |
|-----------|--------|-------|
| Wake word model (nexus.onnx) | TRAINED & VALIDATED | 7/7 detections, 0 false positives |
| 3-stage KWS pipeline | WORKING | mel → embedding → classifier |
| Audio capture (cpal) | WORKING | 48kHz stereo → 16kHz mono |
| Rust integration (tract-onnx) | WORKING | Pure Rust ONNX inference |
| Hotkey wake (Ctrl+Space) | WORKING | Preserved from before |
| Spoken wake ("nexus") | WORKING | 7 detections in ~3 min |
| Speaker verification | PENDING | Ring buffer + verification not yet implemented |
| Tier 3: Command classifiers (Rust) | IMPLEMENTED | Multi-classifier support in wakeword_oww.rs |
| Tier 3: Command event listener (frontend) | IMPLEMENTED | main.tsx listens for command-detected events |
| Tier 3: Training notebook | CREATED | train_nexus_commands.ipynb (run in Colab) |
| Tier 3: Command models | PENDING | Need to run Colab notebook to train 39 models |
| Tier 3: Testing | PENDING | Need trained models first, then run test plan |
| Meeting/privacy mode | IMPLEMENTED | WASAPI + process detection, 4-layer suppression |
| Boot greeting | IMPLEMENTED | Fresh boot + sleep/wake, non-blocking |
| Sidecar auto-spawn | IMPLEMENTED | pythonw.exe, port 49152, non-blocking |
| Mic permission handler | IMPLEMENTED | WebView2 auto-allow for NEXUS origins |
| App registry | IMPLEMENTED | Pre-indexed, ~1ms per command |
| Browser suppression | IMPLEMENTED | RestartApps=0, Edge auto-launch removed |
| Extended testing | PENDING | Multi-speaker, noise, long-running, real reboot |
| Installer | NOT STARTED | Deferred until all testing complete |
