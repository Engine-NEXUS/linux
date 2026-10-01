# NEXUS CLI — Diagnostics & MCP Verification Commands

This document details the system health check and Model Context Protocol (MCP) verification commands: `nexus check` and `nexus mcp check`.

---

## 1. `nexus check`

### Description
Performs an exhaustive preflight diagnostic audit across all required toolchains, environment variables, sidecars, build artifacts, and cloud connections.

### Syntax
```powershell
nexus check
```

### Audited Components
1. **Node.js Environment**: Node version, npm version, `frontend/node_modules` integrity.
2. **Rust Toolchain**: `rustc` version, `cargo` version, target toolchains (`x86_64-pc-windows-msvc`).
3. **Clang / LLVM (`LIBCLANG_PATH`)**: Validates that LLVM binaries exist for C++ interop and `bindgen`.
4. **Python & Audio Packages**: Python 3.10+, `onnxruntime`, `scipy`, `numpy`, `sounddevice`, `faster-whisper`.
5. **Model Integrity**: Checks presence and SHA-256 fingerprints of `nexus.onnx`, `melspectrogram.onnx`, `embedding_model.onnx`, and `nexus_nlu.onnx`.
6. **Cloudflare Worker Connectivity**: Pings the configured Worker API (`NEXUS_SERVER_URL` or default edge route).

### Real-World Use Cases
1. **Troubleshooting Startup Failures**: Diagnoses why an assistant build is failing or why STT audio is silent.
2. **Pre-Commit Health Checks**: Quickly ensures your local environment satisfies all compiler and runtime contracts.

### Example Output
```text
=================================================================
  NEXUS SYSTEM & TOOLCHAIN PREFLIGHT DIAGNOSTICS
=================================================================
[✓] Node.js Runtime       : v20.11.0 (OK)
[✓] Rust Toolchain        : rustc 1.80.0 (x86_64-pc-windows-msvc) (OK)
[✓] LLVM / Clang          : LIBCLANG_PATH=C:\Program Files\LLVM\bin (OK)
[✓] Python Environment    : Python 3.12.3 (sounddevice, onnxruntime, scipy, whisper) (OK)
[✓] Wake Word Models      : openWakeWord ONNX graph verified (860,071 bytes) (OK)
[✓] NLU Engine Models     : BERT-Mini ONNX graph verified (OK)
[✓] Cloudflare Edge       : https://nexus-worker.chitkullakshya.workers.dev (HTTP 200) (OK)
=================================================================
  ALL DIAGNOSTIC CHECKS PASSED — READY FOR OPERATION
=================================================================
```

---

## 2. `nexus mcp check`

### Description
Audits and probes every registered Model Context Protocol (MCP) server end-to-end in a **strict read-only** mode (no messages sent, no orders placed, no state modified).

### Syntax
```powershell
nexus mcp check
```

### Probed Integrations
- **WhatsApp Bridge (`whatsapp`)**: Tests transport liveness and probes pairing status (`paired` vs `auth_required` with QR code availability).
- **Swiggy Food (`swiggy-food`)**: Validates spec-OAuth 2.1 token validity in the local Auth Vault and checks restaurant query tool reachability.
- **Amazon Commerce (`amazon`)**: Probes search catalog tool reachability.
- **GitHub (`github`)**: Validates GitHub PAT / OAuth scopes and verifies repo read/write tool availability.
- **Spotify (`spotify`)**: Probes Spotify Web API token and playback state tool.
- **Render & Vercel (`render`, `vercel`)**: Probes cloud deployment tools and API tokens.

### Real-World Use Cases
1. **Proactive Auth Refresh**: Detects when a 20-day WhatsApp session or OAuth token has expired before an actual user command fails.
2. **Debugging Tool Registration**: Verifies that new MCP schemas and tools are correctly exposed to the orchestrator.

### Example Output
```text
=================================================================
  NEXUS MCP SERVER HEALTH AUDIT (READ-ONLY)
=================================================================
  [•] WhatsApp Bridge      : CONNECTED (Session paired with Mommy / Self)
  [•] Swiggy Food          : READY (OAuth token valid, expires in 18 days)
  [•] Amazon Shopping      : READY (Search tools responsive)
  [•] GitHub Tools         : READY (Authorized for Engine-NEXUS repositories)
  [•] Spotify Player       : READY (Active device: 'Living Room Echo')
  [•] Vercel Deployer      : READY (2 active projects linked)
=================================================================
  6 / 6 MCP SERVERS HEALTHY & CONNECTED
=================================================================
```
