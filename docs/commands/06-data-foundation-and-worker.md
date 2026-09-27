# NEXUS CLI — Data Foundation & Cloudflare Worker Commands

This document details the cryptographic dataset validation, model manifest fingerprinting, and Cloudflare Worker edge deployment commands: `nexus data nlu`, `nexus data wake`, and `nexus worker`.

---

## 1. `nexus data nlu`

### Description
Manages and verifies cryptographic dataset locks, test set immutability, and external benchmark integrity for the NLU engine. Enforces anti-poisoning guardrails by validating SHA-256 hashes against `split_lock.json`, `evaluation_lock.json`, and `external_evaluation_lock.json`.

### Syntax
```powershell
# Validate all NLU dataset locks and hashes:
nexus data nlu validate

# Freeze the held-out evaluation set:
nexus data nlu freeze-evaluation

# Import and validate CLINC150 out-of-scope evaluation samples:
nexus data nlu import-clinc
```

### Real-World Use Cases
1. **Pre-Training Sanity Check**: Ensures that no training data modifications have accidentally corrupted or leaked into the locked evaluation sets.
2. **CI Pipeline Validation**: Executed during automated CI to guarantee 100% data integrity before running retrains.

---

## 2. `nexus data wake`

### Description
Audits and fingerprints openWakeWord ONNX graphs and audio manifest files. Computes exact SHA-256 digests for `melspectrogram.onnx`, `embedding_model.onnx`, and `nexus.onnx`, storing them in `src-tauri/resources/oww/model_manifest.json`.

### Syntax
```powershell
# Validate model hashes and audio manifest integrity:
nexus data wake validate

# Re-compute and write SHA-256 digests after retraining:
nexus data wake fingerprint-models
# or:
nexus data wake fingerprint
```

### Real-World Use Cases
1. **Model Promotion**: Automatically updates `model_manifest.json` after exporting a new `nexus.onnx` checkpoint.
2. **Runtime Verification**: Rust engine loads `model_manifest.json` at startup to ensure neural models have not been corrupted on disk.

---

## 3. `nexus worker`

### Description
Deploys the NEXUS Cloudflare Worker backend. Configures D1 database migrations, sets up KV model distribution namespaces (`CACHE`, `MODELS`), and deploys serverless routing endpoints to Cloudflare's global edge network.

### Syntax
```powershell
nexus worker
```

### Needs & Prerequisites
- **Wrangler CLI**: `npm install -g wrangler` or local `server/worker/node_modules`
- **Cloudflare Account**: Authenticated via `npx wrangler login` or `CLOUDFLARE_API_TOKEN`

### Edge Endpoints Deployed
- `GET /health`: Edge ping and latency test.
- `POST /api/orchestrate`: Multi-agent 9Router and LLM execution.
- `GET /models/nlu/latest`: Over-the-air (OTA) model version manifest.
- `GET /models/nlu/download?name=<file>`: Streams verified model blobs from R2 bucket.
- `POST /oauth/callback`: Spec-OAuth deep link handler for Swiggy, WhatsApp, and Google.

### Real-World Use Cases
1. **Self-Hosting Backend**: Deploying your own dedicated Cloudflare Worker backend instance.
2. **OTA Model Distribution**: Publishing updated NLU neural weights so family devices pull new capabilities automatically over the air without rebuilding the desktop app.
