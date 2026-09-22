# NEXUS Research Knowledge Base

Welcome to the **NEXUS Research Knowledge Base**. This directory organizes all technical research, benchmark studies, hardware investigations, and design analyses conducted across the NEXUS system lifecycle.

---

## 📁 Directory Structure & Categories

```
docs/research/
├── micspecification/        # Microphone hardware, DSP, dynamic AGC, and spectral profiling
├── wakeword/                # Wake word acoustic training, augmentation, and models
├── nlu-intent/              # Intent classification, slot extraction, catalogs, and STT bias
├── live-mode/               # Ultra-low latency voice dictation and live audio streaming
├── mcp-connection/          # Model Context Protocol bridges (WhatsApp, Swiggy, Amazon, etc.)
└── system-architecture/     # Core system patterns, diagnostic reviews, and UI animations
```

---

## 📑 Research Catalog by Domain

### 🎙️ 1. Microphone Hardware & DSP (`micspecification/`)
Deep-dive investigations into laptop internal mic arrays, spectral noise gates, dynamic AGC, and hardware calibration:
- **[Apex Wake Word & Laptop Mic Hardware Adaptation](micspecification/apex-wake-word-and-laptop-mic-hardware-adaptation.md)**:
  Analysis of laptop mic arrays (Intel Smart Sound Technology, Realtek HD Audio), fan resonance peaks (113.3 Hz), automatic high-pass filtering (128.3 Hz), dynamic hardware pre-gain scaling, and impulsive noise rejection (coughs, key clicks).

### ⚡ 2. Wake Word Research (`wakeword/`)
Production plans and benchmarks for real-time edge wake word detection:
- **[Wake Word Training Production Plan](wakeword/wake-word-training-production-plan-2026-09-14.md)**:
  Architecture and deployment specs for openWakeWord embedding classifiers, false alarm suppression, and Google Colab / local PyTorch training workflows.

### 🧠 3. NLU & Intent Engine (`nlu-intent/`)
Taxonomy, benchmark datasets, STT conditioning, and model evaluation:
- **[Command Phrasing Catalog](nlu-intent/command-phrasing-catalog-2026-09-11.md)**:
  Reference phrasing corpus across all 55+ production intent families.
- **[External NLU Data Sources & Acquisition Plan](nlu-intent/external-nlu-data-sources-and-acquisition-plan-2026-09-13.md)**:
  Strategic plan for external dataset acquisition (MASSIVE, CLINC150, SLURP) and zero-poisoning curation.
- **[NLU Model & Dataset Deep Audit](nlu-intent/nlu-model-and-dataset-deep-audit-2026-09-14.md)**:
  Dataset integrity verification, slot consistency checks, and train/test leakage audit.
- **[NLU Model Data Audit Latest](nlu-intent/nlu-model-data-audit-latest.md)**:
  Latest verification benchmarks for BERT-Mini ONNX model accuracy and OOS rejection.
- **[NLU & Wake Word Accuracy Gap Analysis](nlu-intent/nlu-wakeword-code-vs-research-accuracy-gap-2026-09-14.md)**:
  Comparative analysis between research expectations and live code performance.
- **[STT Vocabulary Biasing & Conditioning](nlu-intent/stt-vocabulary-bias-2026-09-22.md)**:
  Whisper STT prompt biasing, temperature stabilization (0.0), and multilingual hallucination suppression.
- **[List PRs Tolerance, Collect Categories & Sidebar Minimalism](nlu-intent/list-prs-tolerance-collect-categories-sidebar-minimalism-2026-09-18.md)**:
  Deterministic regex relaxation, interactive voice collector category tree, and minimal UI overhaul.

### 🚀 4. Live Mode & Audio Streaming (`live-mode/`)
Real-time audio streaming and continuous dictation:
- **[Live Mode Feasibility Study](live-mode/live-mode-feasibility-2026-09-11.md)**:
  Feasibility and latency budgeting for live desktop voice streaming.
- **[Live Mode Source Analysis](live-mode/live-mode-source-analysis-2026-09-11.md)**:
  Deep-dive analysis of open-source streaming STT engines and WebSocket protocols.

### 🔌 5. MCP (Model Context Protocol) Bridges (`mcp-connection/`)
Comprehensive multi-part research series on MCP client connections, OAuth security, and real-time pairing:
- **[01. Industry Patterns & Architecture](mcp-connection/01-industry-patterns.md)**
- **[02. OAuth 2.1 Specification & PKCE Flows](mcp-connection/02-oauth-spec.md)**
- **[03. WhatsApp MCP Bridge & Session Rotation](mcp-connection/03-whatsapp.md)**
- **[04. Swiggy Food Delivery Bridge & Spec-OAuth](mcp-connection/04-swiggy.md)**
- **[05. Amazon & Long-Tail Commerce Bridges](mcp-connection/05-amazon-and-long-tail.md)**
- **[06. Shared Bridge Infrastructure & Fallback](mcp-connection/06-shared-infrastructure.md)**
- **[07. Comparison & Verdict Matrix](mcp-connection/07-comparison-verdict.md)**
- **[08. Gap Analysis & Live Upgrade Plan](mcp-connection/08-gap-analysis-and-upgrades.md)**

### 🏗️ 6. System Architecture & Core Diagnostics (`system-architecture/`)
Cross-cutting architectural evaluations and debugging studies:
- **[05. Enterprise Integration Patterns](system-architecture/05-enterprise-patterns-research.md)**
- **[06. Implementation Deep Research](system-architecture/06-implementation-deep-research.md)**
- **[Diagnostic Review & Performance Audit](system-architecture/diagnostic-review-2026-09-12.md)**
- **[Orb Stuck Animation Root Cause Analysis](system-architecture/orb-stuck-animation-root-cause-2026-09-18.md)**
- **[PR Analyse Button & Sidecar Event Flow](system-architecture/pr-analyse-button-flow-2026-09-18.md)**
