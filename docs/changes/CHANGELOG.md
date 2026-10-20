# NEXUS — Changelog

> All commits in reverse chronological order, organized by feature area.
> Each entry links to a detailed writeup of what changed and why.

## Universal Agent Watcher & Cross-Platform Music Control (2026-10-15)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-10-15 | feat(agent-watch/media): universal agent completion monitoring (Claude, Codex, Antigravity, Cursor, Trae, Windsurf, generic CLI/MCP), active foreground target resolution via GetForegroundWindow, OS keep-awake leasing, proactive voice alerting, and cross-platform native Apple Music & Spotify playback control | [131-universal-agent-watcher-and-cross-platform-music-control.md](131-universal-agent-watcher-and-cross-platform-music-control.md), [126-universal-agent-watcher-and-cross-platform-music-control-plan.md](../features/126-universal-agent-watcher-and-cross-platform-music-control-plan.md), [07-universal-agent-watcher-and-media-engine-research.md](../research/07-universal-agent-watcher-and-media-engine-research.md) |

## Ghost Waves, Browser Search & Typing, TTS Latency & Intent Isolation (2026-09-29)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-29 | feat(waves/search/tts): implement ghost waves wake-up activation & strict resting stillness, local browser search & address bar focus (Ctrl+L), line-by-line dictation mode, sub-5ms app-open cached TTS, and strict GitHub intent isolation | [55-ghost-waves-browser-search-typing-and-intent-isolation-implementation.md](55-ghost-waves-browser-search-typing-and-intent-isolation-implementation.md) |
| — | 2026-09-29 | research(arch): deep-dive root cause analysis and solutions architecture for ghost waves persistence, strict motion rules, local browser search/typing (Ctrl+L/dictation), app-open TTS latency elimination, and GitHub intent isolation | [54-deep-dive-research-root-causes-and-implementation-architecture.md](54-deep-dive-research-root-causes-and-implementation-architecture.md), [voice-and-ghost-mode-root-cause-analysis-and-flow-hardening-2026-09-29.md](../research/voice-and-ghost-mode-root-cause-analysis-and-flow-hardening-2026-09-29.md), [73-ghost-waves-browser-search-typing-and-intent-isolation.md](../features/73-ghost-waves-browser-search-typing-and-intent-isolation.md) |

## STT Hallucination Mitigation, Strict Wave Motion Rule & Intent Parser Tightening (2026-09-29)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-29 | fix(stt/avatar/intent): RMS energy floor gate (<0.005) & 200ms length gate, Latin/silence hallucination intercept, pause waves on idle rest (0.15 flat baseline), eliminate truncated "open" architect routing, demote offline WhatsApp bridge probe logs | [53-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md](53-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md), [72-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md](../features/72-stt-hallucination-mitigation-wave-motion-and-architect-routing-fix.md) |

## System Voice Lock, Ghost Mode Waves & Universal Win32 Barge-In (2026-09-28)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-28 | fix(tts/ghost/win32): deep research, root cause resolution, and implementation for Ava Neutral voice lock, Lottie waves persistence, sub-ms Win32 browser control, and instant spoken barge-in interruption | [54-deep-dive-research-root-causes-and-implementation-architecture.md](54-deep-dive-research-root-causes-and-implementation-architecture.md), [voice-and-browser-control-deep-dive-2026-09-28.md](../research/voice-and-browser-control-deep-dive-2026-09-28.md), [73-system-voice-lock-ghost-orb-waves-and-win32-barge-in.md](../features/73-system-voice-lock-ghost-orb-waves-and-win32-barge-in.md) |

## Takeover Detection Deleted — Esc Is the Cancel Button (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | fix(ghost): takeover detector deleted entirely (`decide_takeover`/observe judgment/`abort_task` removed) after third live misfire (00:15:37 yield on keyboard-only open); mouse use ALWAYS free; Esc = cancel; entry narration updated | [ghost plan §3](../features/64-ghost-mode-plan.md), [timeline #11](../research/ghost-mode/03-irregularities-and-fixes-timeline.md) |

## Vision Speed Mode — Parallel Race + Fast Capture (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | feat(vision): race mode (`visionRace: speed` — Groq ∥ Gemini via tokio::join!, first valid wins, both quota units counted, 429-marked) + 768px fast capture in race mode; sequential default unchanged; Settings select | [vision-grounding-plan](../research/ghost-mode/vision-grounding-plan-2026-09-27.md) |

## Vision Grounding v2 — Grid Overlay, Dual Providers, Quotas (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | feat(vision): axis-grid overlay (Axis-Grid Scaffold), Groq→Gemini fallback with Pacific-day quota counters, provider order setting, spoken limit notices, first-run key nudge, quota UI; keys stay in OS keychain | [vision-grounding-plan](../research/ghost-mode/vision-grounding-plan-2026-09-27.md) |

## Cross-Phase Audit — Fixes Found by Re-Testing (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | audit: A1 cooldown ordering (reject before store); A2 get_settings keychain injection (prevented wipe-on-save); A4+B1 PII/history/memory sanitization + shadowed-parts memory-drop fix; B1 remember-hook moved post-intercepts (room dictation safe); B2 piper-voice streaming guard; C1 post-vision stop re-check; C2 verified-path diary log; 4 new tests | [phase-d-closeout](../research/phase-d-closeout-2026-09-27.md) |

## Phase D Ecosystem — Custom Intents & Safe Self-Improvement (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | feat(specs): declarative `intents.yaml` user intents (open+say, fallback-slot, 11 tests); feat(improve): edge-case miner (miss/failure clusters → suggestions, report command, 6 tests); D1/D4 deferred with reasoning | [phase-d-closeout](../research/phase-d-closeout-2026-09-27.md) |

## Phase C Strategic Features — Vision, Memory Plus, Protocol, Voice & Triggers (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | feat(ghost): vision grounding fallback (Groq VLM 0-1000 coords, UIA-first, 8 tests); feat(pulse): proactive diary (wake/compound/ghost/webhook hooks, boot rollup, diary_summary, 4 tests); feat(protocol): versioned Tauri↔Worker contract v1 (health advertise, payload stamp, mismatch warn, spec doc, 3+3 tests); feat(voice): multi-voice Piper (drop-in models, per-voice reload, picker lists customs, 5 tests); feat(webhook): localhost triggers (127.0.0.1:39220, bearer keychain token, diary-logged, 6 tests) | [improvement-plan.md](../research/improvement-plan.md), [protocol-v1](../phone/protocol-v1.md) |

## Phase B High-Impact Features — Memory, Voice & Execution (2026-09-27)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-27 | feat(memory): 3-tier persistent memory (core/episodic/context injection, remember hook, 8 tests); feat(tts): sentence-chunked streaming (first-audio ≈1 sentence) + emotion prosody (auto/manual, 12 tests); feat(commands): parallel WorkerBackend batches (JoinSet, 5 tests) + crash-safe checkpoints (per-step write, boot sweep, 4 tests) | [improvement-plan.md](../research/improvement-plan.md) |

## Phase A Quick Wins — Security, Privacy & Observability (2026-09-26)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-26 | feat(security): API keys moved to OS keychain, PII filtering middleware, speaker verification wired, health dashboard, settings export/import | [improvement-plan.md](../research/improvement-plan.md) |

## Takeover Re-Scope: Mouse Use Never Ends the Session (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | fix(ghost): takeover requires commanded target (mouse use never yields), task-abort vs session-end split, `ghost:session` event, `ExitGhostControl` intent, idle stop inform | [47-takeover-rescope.md](./47-takeover-rescope.md), [ghost plan §3](../features/64-ghost-mode-plan.md) |

## Ghost Voice Messaging + Trace Taps (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): voice desktop messaging in-session (visible typing, confirm-gated send), TRIGGER banner removed, debug_trace pipeline taps, Send-safe drain refactor | [46-ghost-voice-messaging-trace.md](./46-ghost-voice-messaging-trace.md) |

## NLU Phonetic Alias Map & Persistent Missed-Intent Logging (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(nlu): deterministic soundalike phrase normalizer for Ghost Mode and apps + expanded stop phrases + persistent `%APPDATA%/com.nexus.assistant/missed_intents.jsonl` audit logging | [51-nlu-phonetic-alias-map-and-missed-intent-logging.md](./51-nlu-phonetic-alias-map-and-missed-intent-logging.md) |

## STT Hallucination Fix — Prefix-Pad Poisoning & Vocabulary (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | fix(stt): removed VERIFY_RING prefix-pad poisoning Groq audio capture buffer + expanded NEXUS_VOCABULARY with ghost mode and app commands + fixed double trigger console banner | [50-stt-hallucination-verify-ring-prefix-pad-and-ghost-mode-vocabulary.md](./50-stt-hallucination-verify-ring-prefix-pad-and-ghost-mode-vocabulary.md) |

## Live Audio Telemetry, Trigger Debouncing & Diagnostics Parity (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(console): real-time 12-slice speaker audio waveform and status meter in `nexus start` + atomic 1.5s neural trigger debouncing + restored ASCII cloud connection diagnostics table | [49-live-audio-telemetry-and-connection-diagnostics-parity.md](./49-live-audio-telemetry-and-connection-diagnostics-parity.md) |

## Instant Neural Wake Alignment & Clean Console UX (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(wakeword): eliminated 1.5s latency gap between `nexus wake test` and `nexus start` + bypassed redundant Stage-2 STT verification + fail-open speech onset VAD + quad-mic active stereo downmixer + clean trigger banner console UX in `run.ps1` | [48-instant-neural-wake-alignment-and-clean-console-ux.md](./48-instant-neural-wake-alignment-and-clean-console-ux.md) |

## "Hey Jarvis" vs. "NEXUS" Comparative Parity & Ingestion Repair (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(wakeword): empirical benchmark vs openWakeWord Jarvis + resolved 22.05kHz data ingestion bug + universal on-the-fly resampling + Binary Focal Loss retraining across 31,048 samples (0.00% FA multi-syllabic parity) | [47-hey-jarvis-vs-nexus-acoustic-parity-and-resampling-fix.md](./47-hey-jarvis-vs-nexus-acoustic-parity-and-resampling-fix.md) |
| — | 2026-09-25 | feat(wakeword): dual-target positive sample balancing (178 files) + 1.5s cold-boot grace + VAD speech onset decapitation fix + active quad-mic stereo downmixer | [46-dual-target-wake-word-balancing-and-rust-engine-alignment.md](./46-dual-target-wake-word-balancing-and-rust-engine-alignment.md) |

## Ghost Voice Entry Fixes (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | fix(ghost): initial ring emit on entry (orb never learned silent sessions) + ghost hot-mic loop with echo/meeting guards + silent-miss anti-nag + heartbeat client marker | [44-ghost-voice-entry-waves-hotmic-fix.md](./44-ghost-voice-entry-waves-hotmic-fix.md) |

## Ghost Waves Transition (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): smile-to-waveform transition on session start — pinch + live 3-bar waves in Lottie palette, reverses on exit | [43-ghost-waves-transition.md](./43-ghost-waves-transition.md), [ghost plan](../features/64-ghost-mode-plan.md) |

## Ghost Mode Voice Entry Split (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | fix(ghost): "ghost mode" enters cursor control (new `EnterGhostControl`), bare-mode phrases stripped from Ghostwriter dictation entry | [42-ghost-mode-entry-split.md](./42-ghost-mode-entry-split.md), [ghost plan](../features/64-ghost-mode-plan.md) |

## Ghost Phase 4 Hardening (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): 5-point calibration probe + exclusive-fullscreen auto-pause + refusal battery (pure scorer, password-exclusion pinning) | [41-ghost-hardening.md](./41-ghost-hardening.md), [ghost plan](../features/64-ghost-mode-plan.md) |

## Ghost Phase 3 Overlap Engine (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): drill-depth flag + follow-up queue + stop-word intercept in `process_transcript` (voice stop reaches drills; rest queues silently, drains in order) | [40-ghost-overlap-engine.md](./40-ghost-overlap-engine.md), [ghost plan](../features/64-ghost-mode-plan.md) |
| — | 2026-09-25 | fix(ghost): no glide-home after stop/takeover (was driving into the user's hands) | [40-ghost-overlap-engine.md](./40-ghost-overlap-engine.md) |

## Ghost Phase 2 Mouse Control (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): eased mouse module (glide/click/double/scroll/drag/restore) + UIA-first resolver + `live_ghost_click` with focus-verify | [39-ghost-mouse-control.md](./39-ghost-mouse-control.md), [ghost plan](../features/64-ghost-mode-plan.md) |

## Ghost Phase 1 Keyboard Drill (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): Win-search launcher + WhatsApp drill runner with per-step stop/session guards (send stays confirm-gated) | [38-ghost-keyboard-drill.md](./38-ghost-keyboard-drill.md), [ghost plan](../features/64-ghost-mode-plan.md) |
| — | 2026-09-25 | feat(ghost): stop flag + voice-stop via `live_cancel`, session announce rails | [38-ghost-keyboard-drill.md](./38-ghost-keyboard-drill.md) |

## Ghost Mode Ring + Takeover Leash (2026-09-25)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-25 | feat(ghost): session machine + takeover detector + dynamic Esc panic + ring UI on stage | [37-ghost-ring-and-takeover.md](./37-ghost-ring-and-takeover.md), [ghost plan](../features/64-ghost-mode-plan.md) |

## Multi-Source Noise Hardening & Hardware Invariance (2026-09-23)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-23 | feat(noise): ingested 600 multi-source background profiles (keyboard, fan, HVAC, domestic, telecom) screened with `faster-whisper` anti-poisoning | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md) |
| — | 2026-09-23 | feat(train): trained device-invariant ONNX classifier on 11,760 positive augmented windows + 35,715 negative windows; verified 96.6% recall & 99.7% noise rejection | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md), [Feature doc](../features/58-multi-source-noise-hardening-and-hardware-invariance.md) |
| — | 2026-09-23 | feat(eval): created multi-device hardware invariance benchmark (`test_device_invariance.py`), passing all 5 hardware microphone profiles | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md) |

## Apex Wake Word Evolution — 560 Clean Samples Retrain (2026-09-23)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-23 | feat(wake): audited 562 positive takes with `faster-whisper` + RMS gates; quarantined 2 bad takes; retrained ONNX classifier on 560 pristine recordings + 20,754 negative/noise windows | [40-apex-wake-word-retrain-560-positive-samples.md](./40-apex-wake-word-retrain-560-positive-samples.md) |
| — | 2026-09-23 | feat(eval): achieved **98.2% peak validation recall** and **0.4% false alarm rate**; verified 43/43 wake tests and 522/522 total Rust tests passing | [40-apex-wake-word-retrain-560-positive-samples.md](./40-apex-wake-word-retrain-560-positive-samples.md) |

## CI Windows Streamlining & Rust Compilation Fixes (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | fix(wake): fixed missing `STREAM_DEAD_ZERO_SECS` / `STREAM_SUSPECT_ZERO_SECS` in mock-wake build; updated `enigo 0.5` key matching in `screen.rs` | [39-ci-windows-streamline-and-rust-compilation-fixes.md](./39-ci-windows-streamline-and-rust-compilation-fixes.md) |
| — | 2026-09-22 | ci: streamlined GitHub Actions CI to Windows, frontend, and Python validation; eliminated Linux/macOS false failures; fixed PowerShell quote parsing in installer build | [39-ci-windows-streamline-and-rust-compilation-fixes.md](./39-ci-windows-streamline-and-rust-compilation-fixes.md) |

## Apex Wake Word Evolution, Data Poisoning Quarantine & Adaptive Microphone DSP (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(wake): acoustic poisoning audit (`audit_positive_samples.py`), quarantined 135 bad audio files, leaving 456 pristine positive recordings | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md), [Hardware spec](../research/micspecification/apex-wake-word-and-laptop-mic-hardware-adaptation.md) |
| — | 2026-09-22 | feat(dsp): spectral ambient mic prober (`nexus wake probe` / `acoustic_profile.rs`), auto-tunes 128.3 Hz HPF, hardware pre-gain (2.5x), and impulsive noise gate (8.0x) | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md) |
| — | 2026-09-22 | feat(train): 1,442 multilingual negatives + 400 background sound clips (`generate_background_sounds.py`) + `BCEWithLogitsLoss(pos_weight=8.0)` + `reset_after_trigger()` | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md) |

## Targeted Intent Training, Category Drill-Down & MCP Data Promotion (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(collect): targeted voice collection (`nexus collect -c <cat> -i <intent>`), full 55-intent phrase catalog, and entity slot extraction | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |
| — | 2026-09-22 | feat(nlu): zero-quarantine MCP promotion into `dataset.json` (294 rows across `order_food`, `send_whatsapp_message`, `search_product`), re-locked splits | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |
| — | 2026-09-22 | feat(stats): aligned 55-intent schema with `nlu_stats.py`, displaying full active training rows and mastery status | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |

## NLU Data Foundation, STT Conditioning & Voice Scaling (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(nlu): decoupled acoustic STT from text NLU; fixed multilingual Whisper hallucinations with prompt biasing and `temperature=0.0` | [36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md](./36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md), [STT bias](../research/nlu-intent/stt-vocabulary-bias-2026-09-22.md) |
| — | 2026-09-22 | feat(data): repaired external benchmark locks in `data_foundation.py`, automated 70/30 data audit, and documented speaker-invariant OTA updates | [36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md](./36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md) |

## STT Domain Vocabulary Bias (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(stt): `NEXUS_VOCABULARY` decoder-bias prompt on both Groq call sites + budget/coverage guard test | [36-stt-vocabulary-bias.md](./36-stt-vocabulary-bias.md), [STT vocabulary research](../research/nlu-intent/stt-vocabulary-bias-2026-09-22.md) |

## MCP Connect System — Best-of-Combine + Round-2 Audit (2026-09-20)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-20 | feat(mcp): 4-state connect machine + sidebar Connect card on first failure + auto-retry monitor (WhatsApp QR via `pairing_status`, rotation probe, parallel probes) | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | feat(oauth): Swiggy spec-OAuth on Worker + vault silent refresh + "Login with Swiggy" (Workers/connections UI) | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | fix(mcp): round-2 live-source audit — card self-update vs 20-30s QR rotation, refresh-token rotation persistence, RFC 8707 `resource`, audit-log hygiene test, parallel probes | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | feat(mcp): circuit breaker, `mcp_audit.jsonl` trail, output cap + sanitizer, `nexus mcp check`, actionable bridge guidance | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md), [00-overview](../mcp/00-overview-and-status.md) |

## Motion Fixes + Analyse-PR Dashboard + Minimal Sidebar + Phase 11 (2026-09-18)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-18 | fix(orb): missing `done` handshake on long replies — guarded `finishSpokenResult` + 60s failsafe + inline loading show | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(pr-list): Analyse closes list + loading + analysis dashboard in sidebar (Worker `analysis` field was console-logged-dropped) | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | style(sidebar): flat minimal pass — gradients, inset stacks, specular rims, dead backdrop-filters removed | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(parser): list_prs accepts "pull requests" noun + "and all" tails (regex was the hole, not data); Phase 11 +45 rows → candidate test 0.9004 | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(collect): category menu (`github/mcp/apps/messages/live/random`) + fixed duplicate `list_prs` key silently dropping 21 phrases | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-20 | feat(parser): `canonical_repo_name()` sound-alias map (cervix/srvx→servx, zinc→zync) across deterministic + NLU repo paths | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |

## Data Foundation and Wake-Model Gates (2026-09-14)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-14 | feat(data): add provenance registry, frozen NLU evaluation locks, wake-audio grouped-split validation, deployed model fingerprints, and `nexus data` | [Data foundation gates](../testing/data-foundation-and-wake-model-gates.md) |

## BERT-Mini Dataset and Model Deep Audit (2026-09-14)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-14 | fix(nlu): repair slot loss, tokenizer-offset alignment, conflicts, leakage, corrupt ASR rows, and complete test coverage | [BERT-Mini audit](../research/nlu-model-and-dataset-deep-audit-2026-09-14.md) |
| — | 2026-09-14 | feat(nlu): add `nexus audit`, machine-readable report, canonical dataset repair, post-training quality audit, and 0.85 confidence gate | [Latest audit](../research/nlu-model-data-audit-latest.md) |
| — | 2026-09-14 | docs(testing): add permanent NLU future-testing, model-promotion, and rollback playbook | [Testing playbook](../testing/nlu-future-testing-and-model-promotion-playbook.md) |

## NLU Training Pipeline + Voice Collection + Auto-Cleanup (2026-09-12)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-12 | feat(nlu): `nexus collect` — interactive voice sample collector with Groq auto-load | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | feat(nlu): `nexus train` — 7-step pipeline with auto-cleanup of temp files | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | feat(nlu): expand synthetic data generator (478 → 1097 examples + 58 negative examples) | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | docs(nlu): complete training guide (1100+ lines) + change log | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md), [51-nlu-training-and-voice-collection.md](../features/51-nlu-training-and-voice-collection.md) |

## Admin Brain + PR-List + STT Capture + TTS Fallback + NLU Expansion (2026-09-05 → 2026-09-06)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `d780ff4` | 2026-09-06 | fix(brain+stt): try brain before NLU + upgrade STT to base.en + admin config path | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `04fd346` | 2026-09-06 | fix(brain+parser): enable admin-brain in builds + fuzzy list fallback | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `fcea326` | 2026-09-06 | fix(stt): capture audio from cpal stream directly — bypass getUserMedia | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9999856` | 2026-09-06 | feat(pr-analysis): structured 4-section output — impact, bugs, stats, merge conflicts | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `4fc8bce` | 2026-09-06 | fix(pr-list): match sidebar height to 1000px | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `2ff4814` | 2026-09-05 | feat(pr-list): voice-driven PR list sidebar with Merge + Analyse buttons | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `103faf4` | 2026-09-05 | fix(mic): silence-recovery skips restart during baton pass + STT corrections | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `0122ee7` | 2026-09-05 | fix(brain): wire up complete training pipeline — 6 broken links fixed | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `f502910` | 2026-09-05 | perf(parser): cache all regexes + add 8 missing command patterns (177× faster) | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `050ef82` | 2026-09-05 | fix(tts): use cloud Edge TTS as primary, Piper only when network is down | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `894f1f5` | 2026-09-05 | feat(brain): negative training + verbal 'wrong' feedback + execution failure reporting | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `efc6416` | 2026-09-05 | feat(brain): admin isolation + compile-time gate + lazy_brain + negative training | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `ba58d4a` | 2026-09-05 | feat(brain): wire brain monitor into transcript pipeline + auto-retrain script | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9454e3d` | 2026-09-05 | feat(brain): Qwen 0.5B brain server + pronunciation learning + auto-train monitor | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9b3f983` | 2026-09-05 | feat(nlu): expand intent labels 7→46 + seed 2185 balanced examples | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |

## Central Orchestrator + GitHub OAuth + Sub-Command System (2026-09-04)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `c22f6a3` | 2026-09-04 | feat(github): add typed GitHub sub-command system (Phase 2A) | [31-github-subcommand-system.md](./31-github-subcommand-system.md) |
| `a5005bd` | 2026-09-04 | fix(github): wire conflict panel + confirmation flow | [31-github-subcommand-system.md](./31-github-subcommand-system.md) |
| `d2a802f` | 2026-09-04 | docs: add architecture, feature, and changelog docs for orchestrator + OAuth | [29-central-orchestrator.md](./29-central-orchestrator.md), [30-github-oauth-fix.md](./30-github-oauth-fix.md) |
| `ac0373b` | 2026-09-04 | feat: add central orchestrator module in Rust | [29-central-orchestrator.md](./29-central-orchestrator.md) |

## P0 Security — Device Authentication & OAuth CSRF Fix (2026-10-02)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-10-02 | **security(worker):** three OAuth token endpoints returned live tokens keyed on a client-supplied `?user_id=` with no authentication. Added `server/worker/src/auth.ts` (SHA-256 hashing, CSPRNG tokens, constant-time compare, device auth, single-use TTL'd OAuth state) and a deny-by-default `PUBLIC_ROUTES` gate. Account identity now comes only from the verified device | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |
| — | 2026-10-02 | fix(worker): `state` was the literal string `provider:userId`, so it **leaked the user id through the OAuth redirect**. Now 32 random bytes, hashed at rest, single-use, 10-min TTL. `/oauth/exchange` — previously the deep-link leg with **no CSRF check at all** — now redeems state and verifies provider | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |
| — | 2026-10-02 | fix(worker): six more handlers trusted client-supplied `user_id` (`oauth/status`, `oauth/exchange`, `oauth/disconnect`, all three `apikeys` routes). Removed `Access-Control-Allow-Origin: *`; reflects origin with `Vary: Origin` | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |
| — | 2026-10-02 | feat(worker): `schema.sql` + new `migrations/0001_device_auth.sql` — `device_token_hash`, expiry/revocation enforced in the SQL predicate, and an `oauth_states` table | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |
| — | 2026-10-02 | feat(rust): `device_auth.rs` mints a 256-bit token from two UUIDv4s (CSPRNG) and stores it in the **OS keyring**; `worker_proxy.rs` adds an allowlisted `worker_request` proxy so the WebView never holds a credential. Deleted the renderer's direct `fetchGithubToken()` and 7 direct Worker calls | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |
| — | 2026-10-02 | test: 76/76 Worker (49 existing + 27 new auth), 572/572 Rust (563 + 9), frontend 28/28, `wrangler --dry-run` clean | [46-device-auth-and-oauth-csrf-fix.md](./46-device-auth-and-oauth-csrf-fix.md) |

## P1 Voice Latency — Adaptive Endpoint, Silero v6, PP-OCRv6 (2026-10-02)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-10-02 | perf(vad): adaptive endpointing replaces the fixed 3000ms redemption with a pre-pausal cut-off — fires early only when the tail is unambiguously dead (probability collapsed **and** near-silent), otherwise falls through to the old path untouched. 3000ms → ≤400ms | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |
| — | 2026-10-02 | fix(vad): two bugs found while wiring — `resumeVad()` never reset state, so one adaptive cut-off would latch and **silently drop every later turn** in the retry loop; and `micVad.pause()` re-enters `onSpeechEnd` synchronously under `submitUserSpeechOnPause: true`, so the `endpointFinalized` guard must stay above the pause call | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |
| — | 2026-10-02 | feat(vad): Silero v5 → v6 via `@ricky0123/vad-web` 0.0.31. Deleted the now-unreferenced v5 model from `public/` (git retains it); model name collapsed to one constant; `npm ci` re-verified in a clean dir | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |
| — | 2026-10-02 | perf(ocr): `rapidocr-onnxruntime` 1.4.4 (PP-OCRv4) → `rapidocr` 3.9.2 (PP-OCRv6 **tiny**). **1312ms vs 4138ms — 3.1x faster**, and the runtime weight download is gone (models ship in the wheel). Upstream's `small` default would have been **27% slower** than v4 | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |
| — | 2026-10-02 | fix(ocr): 3.x returns a `RapidOCROutput` **dataclass**, not the legacy `(result, elapse)` tuple — `hasattr(out,"__iter__")` is `False`, so the old unpack was a hard `TypeError`. Rewrote result handling; the `or []` idiom then raised `ValueError` because `boxes` is a numpy array. Verified end-to-end through the shipped `_get_engine()` | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |
| — | 2026-10-02 | test: endpoint decision extracted to pure `audio/endpoint.ts` + **17 unit tests**, mutation-verified (loosening `probFloor` fails 1 test, breaking ambiguous-band handling fails 6, dropping the resume rule fails 3). Thresholds documented as **uncalibrated** — the repo ships no speech fixtures | [47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md](./47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md) |

## Computer-Control Architecture Research (2026-10-02)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-10-02 | docs(research): ten-angle audit of the 2026 competitive + platform landscape, and the prioritised work order (P0 security → P1 voice → P2 visual) | [62-competitive-and-platform-audit-2026-10.md](../features/62-competitive-and-platform-audit-2026-10.md) |
| — | 2026-10-02 | docs(research): **20** — why screenshot-plus-vision is the wrong abstraction for an assistant on a machine you own, and the semantic-first redesign | [research/20](../features/research/20-linux-computer-control-architecture-rethink.md) |
| — | 2026-10-02 | docs(research): **21** — every perception × actuation channel enumerated and scored on Wayland support / consent / privilege / latency / verifiability, with desktop coverage and whole-system design comparisons. Enumerating found **two errors in doc 20** (CDP input synthesis under-sold; compositor/shell tier missed) | [research/21](../features/research/21-linux-computer-control-full-comparison.md) |
| — | 2026-10-02 | docs(research): **22** — evidence base. `file:line` audit of the current stack: no mouse on Linux, X11-only keyboard, `focus_window` returns `true` while doing nothing, the inert privacy gate, and the dead `live_*` voice path | [research/22](../features/research/22-linux-perception-actuation-codebase-audit.md) |

## Multi-Source Noise Hardening & Hardware Invariance (2026-09-23)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-23 | feat(noise): ingested 600 multi-source background profiles (keyboard, fan, HVAC, domestic, telecom) screened with `faster-whisper` anti-poisoning | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md) |
| — | 2026-09-23 | feat(train): trained device-invariant ONNX classifier on 11,760 positive augmented windows + 35,715 negative windows; verified 96.6% recall & 99.7% noise rejection | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md), [Feature doc](../features/58-multi-source-noise-hardening-and-hardware-invariance.md) |
| — | 2026-09-23 | feat(eval): created multi-device hardware invariance benchmark (`test_device_invariance.py`), passing all 5 hardware microphone profiles | [41-multi-source-noise-hardening-and-device-invariance.md](./41-multi-source-noise-hardening-and-device-invariance.md) |

## Apex Wake Word Evolution — 560 Clean Samples Retrain (2026-09-23)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-23 | feat(wake): audited 562 positive takes with `faster-whisper` + RMS gates; quarantined 2 bad takes; retrained ONNX classifier on 560 pristine recordings + 20,754 negative/noise windows | [40-apex-wake-word-retrain-560-positive-samples.md](./40-apex-wake-word-retrain-560-positive-samples.md) |
| — | 2026-09-23 | feat(eval): achieved **98.2% peak validation recall** and **0.4% false alarm rate**; verified 43/43 wake tests and 522/522 total Rust tests passing | [40-apex-wake-word-retrain-560-positive-samples.md](./40-apex-wake-word-retrain-560-positive-samples.md) |

## CI Windows Streamlining & Rust Compilation Fixes (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | fix(wake): fixed missing `STREAM_DEAD_ZERO_SECS` / `STREAM_SUSPECT_ZERO_SECS` in mock-wake build; updated `enigo 0.5` key matching in `screen.rs` | [39-ci-windows-streamline-and-rust-compilation-fixes.md](./39-ci-windows-streamline-and-rust-compilation-fixes.md) |
| — | 2026-09-22 | ci: streamlined GitHub Actions CI to Windows, frontend, and Python validation; eliminated Linux/macOS false failures; fixed PowerShell quote parsing in installer build | [39-ci-windows-streamline-and-rust-compilation-fixes.md](./39-ci-windows-streamline-and-rust-compilation-fixes.md) |

## Apex Wake Word Evolution, Data Poisoning Quarantine & Adaptive Microphone DSP (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(wake): acoustic poisoning audit (`audit_positive_samples.py`), quarantined 135 bad audio files, leaving 456 pristine positive recordings | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md), [Hardware spec](../research/micspecification/apex-wake-word-and-laptop-mic-hardware-adaptation.md) |
| — | 2026-09-22 | feat(dsp): spectral ambient mic prober (`nexus wake probe` / `acoustic_profile.rs`), auto-tunes 128.3 Hz HPF, hardware pre-gain (2.5x), and impulsive noise gate (8.0x) | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md) |
| — | 2026-09-22 | feat(train): 1,442 multilingual negatives + 400 background sound clips (`generate_background_sounds.py`) + `BCEWithLogitsLoss(pos_weight=8.0)` + `reset_after_trigger()` | [38-apex-wake-word-evolution-and-hardware-adaptation.md](./38-apex-wake-word-evolution-and-hardware-adaptation.md) |

## Targeted Intent Training, Category Drill-Down & MCP Data Promotion (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(collect): targeted voice collection (`nexus collect -c <cat> -i <intent>`), full 55-intent phrase catalog, and entity slot extraction | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |
| — | 2026-09-22 | feat(nlu): zero-quarantine MCP promotion into `dataset.json` (294 rows across `order_food`, `send_whatsapp_message`, `search_product`), re-locked splits | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |
| — | 2026-09-22 | feat(stats): aligned 55-intent schema with `nlu_stats.py`, displaying full active training rows and mastery status | [37-targeted-intent-training-and-mcp-data-promotion.md](./37-targeted-intent-training-and-mcp-data-promotion.md) |

## NLU Data Foundation, STT Conditioning & Voice Scaling (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(nlu): decoupled acoustic STT from text NLU; fixed multilingual Whisper hallucinations with prompt biasing and `temperature=0.0` | [36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md](./36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md), [STT bias](../research/nlu-intent/stt-vocabulary-bias-2026-09-22.md) |
| — | 2026-09-22 | feat(data): repaired external benchmark locks in `data_foundation.py`, automated 70/30 data audit, and documented speaker-invariant OTA updates | [36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md](./36-nlu-data-foundation-stt-conditioning-and-voice-scaling.md) |

## STT Domain Vocabulary Bias (2026-09-22)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-22 | feat(stt): `NEXUS_VOCABULARY` decoder-bias prompt on both Groq call sites + budget/coverage guard test | [36-stt-vocabulary-bias.md](./36-stt-vocabulary-bias.md), [STT vocabulary research](../research/nlu-intent/stt-vocabulary-bias-2026-09-22.md) |

## MCP Connect System — Best-of-Combine + Round-2 Audit (2026-09-20)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-20 | feat(mcp): 4-state connect machine + sidebar Connect card on first failure + auto-retry monitor (WhatsApp QR via `pairing_status`, rotation probe, parallel probes) | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | feat(oauth): Swiggy spec-OAuth on Worker + vault silent refresh + "Login with Swiggy" (Workers/connections UI) | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | fix(mcp): round-2 live-source audit — card self-update vs 20-30s QR rotation, refresh-token rotation persistence, RFC 8707 `resource`, audit-log hygiene test, parallel probes | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md) |
| — | 2026-09-20 | feat(mcp): circuit breaker, `mcp_audit.jsonl` trail, output cap + sanitizer, `nexus mcp check`, actionable bridge guidance | [35-mcp-connect-system](./35-mcp-connect-system-best-of-combine.md), [00-overview](../mcp/00-overview-and-status.md) |

## Motion Fixes + Analyse-PR Dashboard + Minimal Sidebar + Phase 11 (2026-09-18)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-18 | fix(orb): missing `done` handshake on long replies — guarded `finishSpokenResult` + 60s failsafe + inline loading show | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(pr-list): Analyse closes list + loading + analysis dashboard in sidebar (Worker `analysis` field was console-logged-dropped) | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | style(sidebar): flat minimal pass — gradients, inset stacks, specular rims, dead backdrop-filters removed | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(parser): list_prs accepts "pull requests" noun + "and all" tails (regex was the hole, not data); Phase 11 +45 rows → candidate test 0.9004 | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-18 | feat(collect): category menu (`github/mcp/apps/messages/live/random`) + fixed duplicate `list_prs` key silently dropping 21 phrases | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |
| — | 2026-09-20 | feat(parser): `canonical_repo_name()` sound-alias map (cervix/srvx→servx, zinc→zync) across deterministic + NLU repo paths | [34-motion-analyse-flow-minimal-sidebar-phase11.md](./34-motion-analyse-flow-minimal-sidebar-phase11.md) |

## Data Foundation and Wake-Model Gates (2026-09-14)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-14 | feat(data): add provenance registry, frozen NLU evaluation locks, wake-audio grouped-split validation, deployed model fingerprints, and `nexus data` | [Data foundation gates](../testing/data-foundation-and-wake-model-gates.md) |

## BERT-Mini Dataset and Model Deep Audit (2026-09-14)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-14 | fix(nlu): repair slot loss, tokenizer-offset alignment, conflicts, leakage, corrupt ASR rows, and complete test coverage | [BERT-Mini audit](../research/nlu-model-and-dataset-deep-audit-2026-09-14.md) |
| — | 2026-09-14 | feat(nlu): add `nexus audit`, machine-readable report, canonical dataset repair, post-training quality audit, and 0.85 confidence gate | [Latest audit](../research/nlu-model-data-audit-latest.md) |
| — | 2026-09-14 | docs(testing): add permanent NLU future-testing, model-promotion, and rollback playbook | [Testing playbook](../testing/nlu-future-testing-and-model-promotion-playbook.md) |

## NLU Training Pipeline + Voice Collection + Auto-Cleanup (2026-09-12)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| — | 2026-09-12 | feat(nlu): `nexus collect` — interactive voice sample collector with Groq auto-load | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | feat(nlu): `nexus train` — 7-step pipeline with auto-cleanup of temp files | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | feat(nlu): expand synthetic data generator (478 → 1097 examples + 58 negative examples) | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md) |
| — | 2026-09-12 | docs(nlu): complete training guide (1100+ lines) + change log | [33-nlu-training-voice-collection-cleanup.md](./33-nlu-training-voice-collection-cleanup.md), [51-nlu-training-and-voice-collection.md](../features/51-nlu-training-and-voice-collection.md) |

## Admin Brain + PR-List + STT Capture + TTS Fallback + NLU Expansion (2026-09-05 → 2026-09-06)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `d780ff4` | 2026-09-06 | fix(brain+stt): try brain before NLU + upgrade STT to base.en + admin config path | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `04fd346` | 2026-09-06 | fix(brain+parser): enable admin-brain in builds + fuzzy list fallback | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `fcea326` | 2026-09-06 | fix(stt): capture audio from cpal stream directly — bypass getUserMedia | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9999856` | 2026-09-06 | feat(pr-analysis): structured 4-section output — impact, bugs, stats, merge conflicts | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `4fc8bce` | 2026-09-06 | fix(pr-list): match sidebar height to 1000px | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `2ff4814` | 2026-09-05 | feat(pr-list): voice-driven PR list sidebar with Merge + Analyse buttons | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `103faf4` | 2026-09-05 | fix(mic): silence-recovery skips restart during baton pass + STT corrections | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `0122ee7` | 2026-09-05 | fix(brain): wire up complete training pipeline — 6 broken links fixed | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `f502910` | 2026-09-05 | perf(parser): cache all regexes + add 8 missing command patterns (177× faster) | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `050ef82` | 2026-09-05 | fix(tts): use cloud Edge TTS as primary, Piper only when network is down | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `894f1f5` | 2026-09-05 | feat(brain): negative training + verbal 'wrong' feedback + execution failure reporting | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `efc6416` | 2026-09-05 | feat(brain): admin isolation + compile-time gate + lazy_brain + negative training | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `ba58d4a` | 2026-09-05 | feat(brain): wire brain monitor into transcript pipeline + auto-retrain script | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9454e3d` | 2026-09-05 | feat(brain): Qwen 0.5B brain server + pronunciation learning + auto-train monitor | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |
| `9b3f983` | 2026-09-05 | feat(nlu): expand intent labels 7→46 + seed 2185 balanced examples | [32-brain-prlist-stt-tts-overhaul.md](./32-brain-prlist-stt-tts-overhaul.md) |

## Central Orchestrator + GitHub OAuth + Sub-Command System (2026-09-04)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `c22f6a3` | 2026-09-04 | feat(github): add typed GitHub sub-command system (Phase 2A) | [31-github-subcommand-system.md](./31-github-subcommand-system.md) |
| `a5005bd` | 2026-09-04 | fix(github): wire conflict panel + confirmation flow | [31-github-subcommand-system.md](./31-github-subcommand-system.md) |
| `d2a802f` | 2026-09-04 | docs: add architecture, feature, and changelog docs for orchestrator + OAuth | [29-central-orchestrator.md](./29-central-orchestrator.md), [30-github-oauth-fix.md](./30-github-oauth-fix.md) |
| `ac0373b` | 2026-09-04 | feat: add central orchestrator module in Rust | [29-central-orchestrator.md](./29-central-orchestrator.md) |

## Voice Pipeline Performance + Native App Resolution (2026-08-23)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `92040f8` | 2026-08-23 | feat: hot mic + pre-init VAD + parallel init (A+B+C) — eliminates 2s wake-to-listen delay | [28-hot-mic-preinit-vad.md](./28-hot-mic-preinit-vad.md) |
| `02d162c` | 2026-08-23 | feat: native app priority + resolution cache + daily scan — opens PWAs/Store apps instead of browser tabs | [27-native-app-priority-resolution-cache.md](./27-native-app-priority-resolution-cache.md) |
| `80aabed` | 2026-08-23 | perf: switch STT to tiny.en + greedy decoding — 54x faster, 22% less RAM | [26-stt-performance-optimization.md](./26-stt-performance-optimization.md) |
| `58af31e` | 2026-08-23 | fix: auto-start STT server — root cause of all command failures | [25-stt-server-auto-start.md](./25-stt-server-auto-start.md) |
| `e0d0c80` | 2026-08-23 | fix: local commands hijacked by sidecar — local-first intent routing | [24-local-first-intent-routing.md](./24-local-first-intent-routing.md) |
| `d1e9d20` | 2026-08-23 | fix: meeting detection self-trigger — NEXUS detects own WebView2 as meeting | [23-meeting-detection-self-trigger-fix.md](./23-meeting-detection-self-trigger-fix.md) |

## UI Overhaul + Installer + Response Sidebar (PR #16)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `03a34ad` | 2026-08-22 | feat: right-side response sidebar — shows only for server responses | [21-response-sidebar.md](./21-response-sidebar.md), [22-installer-desktop-shortcut-removal.md](./22-installer-desktop-shortcut-removal.md) |
| `6663e57` | 2026-08-20 | feat: white-themed NSIS installer + setup wizard (orb untouched) | [19-nsis-installer.md](./19-nsis-installer.md), [20-setup-wizard-redesign.md](./20-setup-wizard-redesign.md) |
| `4e1086c` | 2026-08-20 | revert: restore original orb window — keep settings window + setup wizard | [18-orb-revert.md](./18-orb-revert.md) |
| `5ee9275` | 2026-08-20 | feat: white theme UI overhaul — orb card, settings window, setup wizard | [17-white-theme-ui-overhaul.md](./17-white-theme-ui-overhaul.md) |

## Boot Reliability + Greeting (PR #15)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `4d3c032` | 2026-08-19 | fix: suppress all terminal windows on Windows (CREATE_NO_WINDOW) | — |
| `89ed188` | 2026-08-19 | fix: autostart via Windows Scheduled Task — zero-delay launch on restart | — |
| `96e4962` | 2026-08-19 | feat: first-of-day greeting — "Welcome sir" on first wake, persisted across restarts | [03-boot-greeting.md](./03-boot-greeting.md) |
| `431ec11` | 2026-08-19 | fix: wake engine blocks tokio runtime for 5 min on cold boot (3 root causes) | — |

## Recent Changes (Boot Reliability + Greeting)

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `f4e6ac6` | 2026-08-19 | feat: boot/wake greeting + non-blocking sidecar + no browser on boot | [01-browser-suppression.md](./01-browser-suppression.md), [02-non-blocking-sidecar.md](./02-non-blocking-sidecar.md), [03-boot-greeting.md](./03-boot-greeting.md), [04-sleep-wake-detection.md](./04-sleep-wake-detection.md) |
| `3cfa5ef` | 2026-08-19 | fix: mic prompt every restart + terminal window on every boot | [05-mic-permission-handler.md](./05-mic-permission-handler.md), [07-silent-sidecar.md](./07-silent-sidecar.md) |
| `41474b9` | 2026-08-19 | fix: eliminate "connection not found" on restart — 3 root causes fixed | [08-connection-restart-fix.md](./08-connection-restart-fix.md) |
| `fc46cc7` | 2026-08-19 | fix: frontend not embedded in .exe (root cause of ERR_CONNECTION_REFUSED) | [09-frontend-embedding.md](./09-frontend-embedding.md) |
| `4c987d5` | 2026-08-19 | fix: silent sidecar (no terminal) + port 49152 (dev-friendly) | [06-sidecar-port-change.md](./06-sidecar-port-change.md), [07-silent-sidecar.md](./07-silent-sidecar.md) |
| `61c9c53` | 2026-08-19 | fix: auto-spawn sidecar + build production app (no more localhost:5173 error) | [10-auto-spawn-sidecar.md](./10-auto-spawn-sidecar.md) |

## Command System

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `b0d0cd5` | 2026-08-19 | fix: copy melspectrogram.onnx to OWW resources dir + add command_intents.json | [13-colab-training.md](./13-colab-training.md) |
| `b81261e` | 2026-08-19 | feat: expanded command system — 30 fixed + 9 parameterized commands | [12-expanded-commands.md](./12-expanded-commands.md) |
| `f3ff4bd` | 2026-08-19 | feat: Tier 3 direct command classification (skip ASR for known commands) | [11-tier3-commands.md](./11-tier3-commands.md) |
| `76c82d4` | 2026-08-19 | feat: Silero VAD + pre-indexed app registry for instant launch | [11-tier3-commands.md](./11-tier3-commands.md) |

## Meeting / Privacy Mode

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `b793ebe` | 2026-08-19 | feat: meeting/privacy mode — auto-detect mic usage, suppress wake & TTS | [14-meeting-privacy-mode.md](./14-meeting-privacy-mode.md) |

## Wake Word Engine

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `395369b` | 2026-08-19 | feat: replace VAD+ASR with openWakeWord KWS for wake word detection | [15-oww-kws.md](./15-oww-kws.md) |
| `89d9296` | 2026-08-19 | feat: wake-word variants + sound-alikes for pronunciation tolerance | [15-oww-kns.md](./15-oww-kws.md) |
| `656ec72` | 2026-08-19 | feat: voice wake word "NEXUS" via VAD + ASR + speaker verification | [15-oww-kns.md](./15-oww-kws.md) |

## Colab Training

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `8fb1832` | 2026-08-19 | fix: Colab compliance — disk cleanup, Drive checkpointing, idle timeout prevention | [13-colab-training.md](./13-colab-training.md) |
| `7ab3859` | 2026-08-19 | fix: Colab notebook ACAV/FMA download failures with retries and fallback | [13-colab-training.md](./13-colab-training.md) |

## TTS

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `fb4c88c` | 2026-08-19 | fix: remove comma pause in "Didn't catch that sir" TTS | [16-tts-fixes.md](./16-tts-fixes.md) |

## Earlier Merges

| Commit | Date | Summary |
|--------|------|---------|
| `a668346` | 2026-08-19 | Merge PR #14: fix/tauri-config-and-stt-server |
| `0a5b82b` | 2026-08-19 | fix: tauri config plugin sections + STT server BytesIO wrapper |
| `860280a` | 2026-08-19 | Merge PR #13: feat/e2e-integration-cleanup |

---

## Feature Area Summary

### Voice Pipeline Performance (3 commits)
Eliminated the 2-second wake-to-listen delay with hot mic + pre-init VAD + parallel init. STT latency reduced from 15s to 276ms with tiny.en + greedy decoding. STT server now auto-starts with NEXUS.

### Native App Resolution (1 commit)
NEXUS now opens installed native apps, Store apps, and browser PWAs instead of browser tabs. Added resolution cache for instant repeat commands, daily scan for app changes, and cross-platform PWA discovery.

### Local-First Intent Routing (1 commit)
Local commands (open, search, play) now execute locally before contacting the remote backend. Eliminates dependency on n8n for basic commands.

### Meeting Detection Fix (1 commit)
Fixed NEXUS detecting its own WebView2 process as a meeting, causing wake/TTS suppression deadlock.

### Boot Reliability (6 commits)
Fixed the entire cold-boot experience: no browser reopening, no terminal window, no mic prompt, fast startup, greeting on boot.

### Command System (4 commits)
Added 39 acoustic command classifiers (30 fixed + 9 parameterized) that skip STT for ~200ms latency. Fixed Colab training notebook.

### Meeting / Privacy Mode (1 commit)
Auto-detect mic usage by other apps, suppress wake + TTS during calls.

### Wake Word Engine (3 commits)
Migrated from VAD+ASR (~30% recall) to openWakeWord KWS (~100% recall). Added pronunciation tolerance.

### Colab Training (2 commits)
Fixed download failures and Colab compliance (disk cleanup, Drive checkpointing).

### TTS (1 commit)
Fixed comma-induced pause in error messages.
