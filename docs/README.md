# NEXUS Documentation Master Index

> **NEXUS Floating Desktop Assistant** — High-performance, local-first voice assistant and multimodal automation agent engineered in Rust (Tauri), TypeScript (React), and Python (BERT-Mini / openWakeWord / FastSTT).

---

## 🧭 Master Navigation & Domain Structure

```
docs/
├── architecture/          # Core system design, sequence diagrams, state machines, and orchestrator
├── features/              # Feature specifications, UI choreographies, and UX flows (Features 01–57)
├── research/              # Deep-dive research across hardware, STT, NLU, live dictation, and MCP
│   ├── micspecification/  # Laptop mic arrays, acoustic profiling, fan resonance, and DSP gates
│   ├── wakeword/          # Acoustic training, augmentation, and false alarm suppression
│   ├── nlu-intent/        # Phrasing catalogs, BERT-Mini audits, and Whisper STT prompt conditioning
│   ├── live-mode/         # Real-time audio streaming, WebSocket protocol, and live dictation
│   ├── mcp-connection/    # 9-part research series on Model Context Protocol bridges & OAuth 2.1
│   └── system-architecture/ # Enterprise patterns, UI motion diagnostics, and sidecar event flows
├── 9router-research/      # Free LLM manager, 9Router cloud delegation, and latency optimization
├── mcp/                   # Model Context Protocol servers (WhatsApp, Swiggy, Amazon, Google, etc.)
├── wake-word/             # openWakeWord 3-stage KWS pipeline, Conv-Attention v3, and Tier-3 models
├── credentials/           # OAuth 2.1 PKCE, Fernet encryption at rest, and credential security
├── changes/               # Detailed commit changelogs and architectural migration writeups (01–38)
├── testing/               # Repeatable testing playbooks, cryptographic evaluation locks, and release gates
├── meeting-protection/    # 4-layer WASAPI mic session probing and call privacy suppression
└── reviews/               # Codebase reviews, gap audits, and PR retrospectives
```

---

## 🚀 Quick Start Guide

If you are exploring the codebase or contributing a new feature, follow this reading sequence:

1. **[architecture/01-system-overview.md](architecture/01-system-overview.md)** — Core mental model: Thin client (Tauri/Rust) + Fat Cloud Server/Worker + Local ONNX fast paths.
2. **[architecture/02-data-flow-graphs.md](architecture/02-data-flow-graphs.md)** — Sequence diagrams for voice input, local regex routing, NLU intent classification, and MCP execution.
3. **[architecture/07-central-orchestrator.md](architecture/07-central-orchestrator.md)** — Central request orchestrator: single owner of lifecycle, request routing, LLM cascades, and barge-in cancellation.
4. **[research/README.md](research/README.md)** — Index of all acoustic, DSP, STT, and NLU research investigations.
5. **[changes/CHANGELOG.md](changes/CHANGELOG.md)** — Complete chronological history of commits and feature upgrades.
6. **[testing/README.md](testing/README.md)** — Testing procedures, cryptographic dataset locks, and release verification gates.

---

## 📑 Domain Catalogs

### 1. 🏗️ [Architecture](architecture/README.md)
*Master architecture specifications, component maps, and state machines.*
- **[01. System Overview](architecture/01-system-overview.md)** — Process topology, 5 golden rules, and channel separation.
- **[02. Data Flow Graphs](architecture/02-data-flow-graphs.md)** — Visual ASCII sequence diagrams for all voice and OS flows.
- **[03. Component Map](architecture/03-component-map.md)** — File-to-subsystem mapping across Rust, TypeScript, and Python.
- **[04. Tech Stack](architecture/04-tech-stack.md)** — Rationale for every crate and library, port assignments, and feature flags.
- **[05. State Machine](architecture/05-state-machine.md)** — Frontend Zustand state machine transitions and side-effects.
- **[06. Liquid Glass Backdrop](architecture/06-liquid-glass-screenshot-blur.md)** — Hardware-accelerated screenshot blur for overlays.
- **[07. Central Orchestrator](architecture/07-central-orchestrator.md)** — Unified request manager and cancellation engine.
- **[08. GitHub OAuth Flow](architecture/08-oauth-github-flow.md)** — PKCE browser redirect and token exchange flow.
- **[09. Request Flow Evolution](architecture/09-request-flow-evolution.md)** — Migration from scattered IPC to centralized orchestration.
- **[10. GitHub Subcommand System](architecture/10-github-subcommand-system.md)** — 28 typed commands via octocrab with confirmation gates.
- **[11. Master System Flows](architecture/11-complete-system-flows.md)** — 16 end-to-end visual sequence diagrams.

### 2. ⚡ [Features](features/README.md)
*Detailed documentation of all implemented features (Features 01–57).*
- **[01–15. Core Desktop Assistant](features/README.md)** — Wake word, Tier-3 commands, meeting mode, boot greeting, NSIS installer, setup wizard.
- **[45–50. Subcommands & Settings](features/README.md)** — GitHub subcommands, loading indicators, and settings sidebar.
- **[51–53. NLU Training & Stability](features/README.md)** — `nexus collect`, `nexus train`, LLM cascades, and mic stability.
- **[54. Interactive Voice Approval](features/54-interactive-voice-approval-and-confirmation-sidebar.md)** — 5s voice confirmation window with instant early reaction.
- **[55. NLU Data Foundation & STT Conditioning](features/55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md)** — Whisper prompt biasing and speaker-invariant OTA model updates.
- **[56. Targeted Intent Training & MCP Promotion](features/56-targeted-intent-training-and-mcp-data-promotion.md)** — Interactive category collection (`nexus collect -c -i`) and zero-quarantine promotion.
- **[57. Apex Wake Word Evolution & Hardware Adaptation](features/57-apex-wake-word-evolution-and-hardware-adaptation.md)** — Mic spectral prober, chassis resonance auto-tuning, and dynamic AGC.

### 3. 🔬 [Research](research/README.md)
*Comprehensive research knowledge base organized into 6 thematic subdirectories.*
- **[micspecification/](research/micspecification/apex-wake-word-and-laptop-mic-hardware-adaptation.md)** — Laptop microphone hardware prober, chassis resonance peaks, dynamic AGC, and impulsive noise filters.
- **[wakeword/](research/wakeword/wake-word-training-production-plan-2026-09-14.md)** — openWakeWord acoustic training, negative augmentation, and loss weighting.
- **[nlu-intent/](research/nlu-intent/command-phrasing-catalog-2026-09-11.md)** — 55-intent phrasing catalogs, dataset audits, and STT vocabulary bias.
- **[live-mode/](research/live-mode/live-mode-feasibility-2026-09-11.md)** — Low-latency continuous voice streaming and live dictation.
- **[mcp-connection/](research/mcp-connection/README.md)** — 9-part research series on MCP bridges, OAuth 2.1, and QR session rotation.
- **[system-architecture/](research/system-architecture/05-enterprise-patterns-research.md)** — Enterprise patterns, UI animation root causes, and diagnostics.

### 4. 🌐 [9Router Research](9router-research/README.md)
*Free LLM aggregation, multi-user brain tiers, and Cloudflare Worker AI routing.*
- **[01–04. Router Brain & Free Models](9router-research/README.md)** — Free model manager, local Qwen thinking brain, and 500MB cloud delegation.
- **[05–08. MCP Ecosystem & Routing](9router-research/README.md)** — Free MCP servers, Indian consumer apps, and latency-optimized routing.
- **[09–13. Multi-User Tiers & Roadmap](9router-research/README.md)** — Admin vs Family tiers, continuous training, and n8n command center.

### 5. 🔌 [Model Context Protocol (MCP)](mcp/README.md)
*Tool-use bridges, Auth Vault, and desktop dictation.*
- **[00. Overview & Status](mcp/00-overview-and-status.md)** — Server support scorecard and execution breakdown.
- **[01. Auth Vault & One-Login](mcp/01-auth-vault-one-login.md)** — Centralized token vault and silent refresh.
- **[02. Server Catalog](mcp/02-server-catalog.md)** — Full 15-server specification and risk profiles.
- **[03. Scribe Mode & Confirmations](mcp/03-scribe-mode-and-confirmations.md)** — Structured confirmation cards and voice dictation.
- **[07. Ghostwriter & Echo Modes](mcp/07-ghostwriter-echo-modes.md)** — Ultra-fast direct text injection into active windows.

### 6. 🎙️ [Wake Word Engine](wake-word/README.md)
*Local ONNX acoustic modeling and Tier-3 command classifiers.*
- **[01–05. Architecture & Pipeline](wake-word/README.md)** — 3-stage openWakeWord ONNX pipeline in pure Rust.
- **[06–14. Model Training & Validation](wake-word/README.md)** — Colab training workflows, soundalike dataset mining, and benchmark metrics.
- **[15–20. Tier-3 Command Classifiers](wake-word/README.md)** — Direct acoustic classification skipping STT for ~200ms latency.
- **[21–22. v3 Apex Master Blueprints](wake-word/README.md)** — Conv-Attention models, multi-speaker datasets, and hardware adaptation.

### 7. 🔐 [Credentials & Security](credentials/README.md)
*Token exchange, OAuth 2.1 PKCE, and local Fernet encryption.*
- **[01–03. Credential Architecture & API Keys](credentials/README.md)** — Storage locations, AES/Fernet encryption at rest, and secret hygiene.
- **[04–06. Integrations & Device Pairing](credentials/README.md)** — Google, GitHub, and edge device registration.
- **[07–08. Security Best Practices & Setup Guide](credentials/README.md)** — Zero plain-text rule, sandboxing, and setup wizard walkthrough.

### 8. 🧪 [Testing & Verification](testing/README.md)
*Repeatable test suites, cryptographic evaluation locks, and model release gates.*
- **[NLU Testing & Model Promotion Playbook](testing/nlu-future-testing-and-model-promotion-playbook.md)** — Dataset integrity, slot consistency, and OOS promotion gates.
- **[Data Foundation & Wake-Model Gates](testing/data-foundation-and-wake-model-gates.md)** — SHA-256 evaluation locks and model fingerprints.
- **[Phases 1–9 Experiment Records](testing/README.md)** — Phased NLU accuracy recovery and OOS elimination experiments.
- **[Wake Word Phases A–E](testing/README.md)** — Audio preprocessing, SpecAugment, speaker verification, and training decisions.

### 9. 🛡️ [Meeting Protection](meeting-protection/README.md)
- **[01. Meeting Detection & Audio Privacy](meeting-protection/01-meeting-detection.md)** — 4-layer WASAPI session probe, mic scanning, and zero-interruption suppression.

### 10. 🔍 [Reviews & Retrospectives](reviews/README.md)
- **[Architecture Mapper Review](reviews/prem224k-architecture-mapper-review.md)** — Independent audit of the interactive codebase visualizer.

---

## ⚙️ Runtime Process & Port Topology

| Service | Port | Process / Technology | Role |
|---|---|---|---|
| **Tauri UI & Rust Engine** | — | `nexus.exe` (Rust + WebView2) | Wake word detection, hotkeys, local command execution, audio DSP, and floating UI |
| **Local STT Server** | `8000` | `faster-whisper` (Python sidecar) | Streaming speech-to-text fallback with domain prompt biasing |
| **Local NLU Server** | `49152` | `nlu_server.py` (FastAPI + ONNX) | 55-intent classification and BIO slot extraction (BERT-Mini) |
| **Admin Brain (Admin only)** | `39219` | `brain_server.py` (llama.cpp / Qwen) | Local reasoning, compound command planning, and phrasings generation |
| **Cloudflare Worker** | Cloud | `server/worker/` (Cloudflare TypeScript) | OAuth token exchange, D1 credential storage, 9Router LLM cascade, and R2 model OTA |
