# NEXUS CLI — Master Command Reference

The `nexus` CLI is the unified cross-platform developer toolchain and operational interface for NEXUS. It coordinates compilation, local development, real-time microphone probing, neural wake-word training, BERT-Mini NLU dataset operations, MCP server health audits, and Cloudflare Worker deployments.

---

## Command Quick Reference

| Command | Category | Description | Primary Use Case |
| :--- | :--- | :--- | :--- |
| [`nexus install`](./01-install-and-setup.md#1-nexus-install) | Lifecycle | One-time setup: installs dependencies, compiles release binary, and links global `nexus` command | First-time machine setup |
| [`nexus setup`](./01-install-and-setup.md#2-nexus-setup) | Lifecycle | Installs prerequisites and builds local binary without modifying global system PATH | CI / headless builds |
| [`nexus clean`](./01-install-and-setup.md#3-nexus-clean) | Lifecycle | Wipes all compilation artifacts (`target/`, `dist/`, caches) | Resetting build state |
| [`nexus dev`](./02-dev-build-run.md#1-nexus-dev) | Development | Starts Vite dev server + Tauri runtime with hot-module reloading | Active UI & backend feature coding |
| [`nexus build`](./02-dev-build-run.md#2-nexus-build) | Development | Builds optimized Vite frontend and compiles Rust release executable | Pre-release binary generation |
| [`nexus start`](./02-dev-build-run.md#3-nexus-start--nexus-run) | Development | Launches the compiled production release binary (`nexus run` alias) | Testing production release runtime |
| [`nexus check`](./03-diagnostics-and-mcp.md#1-nexus-check) | Diagnostics | Audits system tools, Node, Rust, Python, Clang, frontend, and Worker status | Preflight developer health check |
| [`nexus mcp check`](./03-diagnostics-and-mcp.md#2-nexus-mcp-check) | Diagnostics | Audits all registered Model Context Protocol servers end-to-end (read-only) | Validating WhatsApp, Swiggy, Amazon bridges |
| [`nexus train`](./04-nlu-train-audit-collect.md#1-nexus-train) | NLU Engine | End-to-end BERT-Mini retrain: clean → build dataset → train PyTorch → ONNX export | Updating intent & slot neural model |
| [`nexus audit`](./04-nlu-train-audit-collect.md#2-nexus-audit) | NLU Engine | Audits dataset provenance, family leakage, slot boundaries, and OOS accuracy | Verifying model before production release |
| [`nexus collect`](./04-nlu-train-audit-collect.md#3-nexus-collect) | NLU Engine | Targeted spoken voice collection: interactive category menus or flags (`-c`, `-i`) | Expanding training data with real voice |
| [`nexus stats`](./04-nlu-train-audit-collect.md#4-nexus-stats) | NLU Engine | Displays 55-intent category coverage, voice health, and training recommendations | Identifying weak intent categories |
| [`nexus wake probe`](./05-wake-word-suite.md#1-nexus-wake-probe) | Wake Word | 1.5s ambient room FFT scan: auto-tunes HPF cutoff, pre-gain, and silence gate | Calibrating microphone acoustics |
| [`nexus wake test`](./05-wake-word-suite.md#2-nexus-wake-test) | Wake Word | Real-time live microphone listening test with visual meter and confidence scores | Verifying spoken "NEXUS" detection |
| [`nexus wake test --batch`](./05-wake-word-suite.md#3-nexus-wake-test---batch) | Wake Word | Batch benchmark across all 3,000 positive, negative, and noise audio files | Verifying overall model recall and FA rate |
| [`nexus wake test --devices`](./05-wake-word-suite.md#4-nexus-wake-test---devices) | Wake Word | Simulates 5 hardware profiles (Studio, Laptop, Bluetooth, Far-Field, Office) | Testing cross-hardware invariance |
| [`nexus wake train`](./05-wake-word-suite.md#5-nexus-wake-train) | Wake Word | Trains 128-dim ONNX wake classifier with multi-device acoustic augmentations | Updating wake neural weights |
| [`nexus wake ingest`](./05-wake-word-suite.md#6-nexus-wake-ingest) | Wake Word | Ingests 600 open-source noise profiles screened via `faster-whisper` anti-poisoning | Expanding negative background noise library |
| [`nexus wake record`](./05-wake-word-suite.md#7-nexus-wake-record) | Wake Word | Fast voice recorder for positive wake takes, soundalikes, and ambient noise | Collecting new wake audio datasets |
| [`nexus wake stats`](./05-wake-word-suite.md#8-nexus-wake-stats) | Wake Word | Displays audio counts and storage sizes across positive, negative, and background | Monitoring wake dataset volume |
| [`nexus data nlu validate`](./06-data-foundation-and-worker.md#1-nexus-data-nlu) | Data Locks | Validates dataset cryptographic SHA-256 hashes and split locks | Ensuring anti-poisoning data integrity |
| [`nexus data wake validate`](./06-data-foundation-and-worker.md#2-nexus-data-wake) | Data Locks | Validates wake audio manifest and model ONNX fingerprints | Checking wake model integrity |
| [`nexus worker`](./06-data-foundation-and-worker.md#3-nexus-worker) | Cloud Backend | Deploys Cloudflare Worker router, D1 databases, and KV model bindings | Deploying self-hosted cloud backend |

---

## Detailed Command Documentation Guides

1. [**01. Installation & Environment Setup**](./01-install-and-setup.md) — `nexus install`, `nexus setup`, `nexus clean`
2. [**02. Development, Compilation & Launch**](./02-dev-build-run.md) — `nexus dev`, `nexus build`, `nexus start`, `nexus run`
3. [**03. System Diagnostics & MCP Bridge Verification**](./03-diagnostics-and-mcp.md) — `nexus check`, `nexus mcp check`
4. [**04. BERT-Mini NLU Engine, Training & Voice Collection**](./04-nlu-train-audit-collect.md) — `nexus train`, `nexus audit`, `nexus collect`, `nexus stats`
5. [**05. Apex Wake Word Suite & Hardware Invariance**](./05-wake-word-suite.md) — `nexus wake probe`, `test`, `test --batch`, `test --devices`, `train`, `ingest`, `record`, `stats`
6. [**06. Cryptographic Data Foundations & Cloudflare Worker**](./06-data-foundation-and-worker.md) — `nexus data nlu`, `nexus data wake`, `nexus worker`
