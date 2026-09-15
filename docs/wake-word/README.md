# NEXUS Wake Word Engine Documentation

This folder documents the design, architecture, model evolution, and testing of the NEXUS **always-listening wake word detection engine** powered by openWakeWord running locally in Rust via `tract-onnx`.

---

## 📚 Section Breakdown

### 🏗️ 1. Architecture & Pipeline Foundations
- **[01-wake-word-research.md](01-wake-word-research.md)** — Initial feasibility study, false alarm analysis, and openWakeWord benchmark comparison.
- **[02-wake-word-architecture-decision.md](02-wake-word-architecture-decision.md)** — Architectural decision record selecting openWakeWord over VAD+ASR and Porcupine.
- **[03-vad-asr-old-approach.md](03-vad-asr-old-approach.md)** — Legacy VAD+ASR architecture documentation and post-mortem on high latency / missed wakes.
- **[04-oww-kws-new-approach.md](04-oww-kws-new-approach.md)** — Key advantages and latency characteristics of the streaming openWakeWord classifier.
- **[05-oww-3-stage-pipeline.md](05-oww-3-stage-pipeline.md)** — 3-stage ONNX pipeline: `melspectrogram.onnx` → `embedding_model.onnx` → `nexus.onnx` classifier.
- **[09-audio-pipeline.md](09-audio-pipeline.md)** — Complete 16 kHz mono audio capture ring buffer, 80ms chunk processing, and silence gates.
- **[10-rust-integration.md](10-rust-integration.md)** — Pure Rust implementation using `tract-onnx`, zero Python runtime dependencies.

### 🧠 2. Model Training & Negative Augmentation
- **[06-model-training.md](06-model-training.md)** — Classifier training methodology, loss functions, learning rates, and ONNX export.
- **[07-speaker-verification.md](07-speaker-verification.md)** — Speaker embedding cosine similarity and voice enrollment verification.
- **[08-wake-variants-soundalikes.md](08-wake-variants-soundalikes.md)** — Soundalike phrase catalog (*"next"*, *"texas"*, *"necklace"*, *"nexus prime"*) for hard negative mining.
- **[13-colab-training-notebook.md](13-colab-training-notebook.md)** — Google Colab T4 GPU automated training workflow and Drive checkpointing.
- **[14-model-validation-results.md](14-model-validation-results.md)** — Empirical benchmark results and recall verification across real speech.

### ⚡ 3. Tier-3 Command Classifiers (Acoustic Fast-Path)
- **[15-tier3-command-classifiers.md](15-tier3-command-classifiers.md)** — Direct acoustic command classification skipping STT for ~200ms ultra-low latency execution.
- **[16-tier3-decision-comparison.md](16-tier3-decision-comparison.md)** — Trade-off matrix: Acoustic Classifier vs Local STT vs Cloud NLU.
- **[17-tier3-resource-analysis.md](17-tier3-resource-analysis.md)** — CPU and memory overhead profile when running multiple ONNX models in parallel.
- **[18-tier3-training-approach.md](18-tier3-training-approach.md)** — Multi-class intent classifier architecture and data synthesis for fixed commands.
- **[19-tier3-testing-strategy.md](19-tier3-testing-strategy.md)** — Test plan for acoustic command classifiers under noisy environments.
- **[20-expanded-command-system.md](20-expanded-command-system.md)** — Catalog of 39 acoustic commands (30 fixed + 9 parameterized).

### 🚀 4. v3 Apex Evolution & Master Blueprints
- **[21-training-camp-v3.md](21-training-camp-v3.md)** — Comprehensive plan for v3 Conv-Attention acoustic training and dataset synthesis.
- **[21-v3-training-plan.md](21-v3-training-plan.md)** — Step-by-step curriculum for multi-speaker real-voice recording and augmentation.
- **[22-hundred-combo-review.md](22-hundred-combo-review.md)** — 100-combination combinatorial evaluation of acoustic hyperparameters.
- **[22-v3-master-document.md](22-v3-master-document.md)** — Master engineering document for Alexa/Jarvis parity: $\ge 95\%$ recall at $\le 1$ false alarm/hour.

### 🧪 5. Testing & Performance Metrics
- **[11-testing-strategy.md](11-testing-strategy.md)** — Comprehensive test harness, synthetic noise injection, and FAR/FRR measurement.
- **[12-performance-expectations.md](12-performance-expectations.md)** — Latency budget, memory footprint, CPU utilization, and accuracy benchmarks.
