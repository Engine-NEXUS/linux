# NEXUS — Changelog

> All commits in reverse chronological order, organized by feature area.
> Each entry links to a detailed writeup of what changed and why.

---

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
| `89d9296` | 2026-08-19 | feat: wake-word variants + sound-alikes for pronunciation tolerance | [15-oww-kns.md](./15-oww-kns.md) |
| `656ec72` | 2026-08-19 | feat: voice wake word "NEXUS" via VAD + ASR + speaker verification | [15-oww-kns.md](./15-oww-kns.md) |

## Colab Training

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `8fb1832` | 2026-08-19 | fix: Colab compliance — disk cleanup, Drive checkpointing, idle timeout prevention | [13-colab-training.md](./13-colab-training.md) |
| `7ab3859` | 2026-08-19 | fix: Colab notebook ACAV/FMA download failures with retries and fallback | [13-colab-training.md](./13-colab-training.md) |

## TTS

| Commit | Date | Summary | Details |
|--------|------|---------|---------|
| `fb4c88c` | 2026-08-19 | fix: remove comma pause in "Didn't catch that sir" TTS | [16-tts-fixes.md](./16-tss-fixes.md) |

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
