# 36 — NLU Data Foundation, STT Conditioning & Voice Scaling (2026-09-22)

## Summary of Changes

This release hardens the NEXUS data foundation, fixes preflight evaluation lock validation, eliminates multilingual STT hallucinations on short voice clips, and details the 70/30 data perfection principle.

---

## 1. Fixed Files & Code Modifications

### 1.1 `server/nlu/data/external_evaluation_lock.json`
- **Issue**: Preflight gate (`data_foundation.py validate`) failed with `ERROR: external benchmark hash mismatch` on `clinc150_oos_test`.
- **Root Cause**: The raw JSONL file on disk had been formatted with canonical sorted keys, changing its binary SHA-256 hash. The lock file retained the legacy pre-sort hash.
- **Fix**: Updated `clinc150_oos_test` sha256 to `4b558b9847f9386fefe3267ef1c751ef91ddc7d2e99b345fad4eea6c955add93`.
- **Verification**: `python server/nlu/data_foundation.py validate` passed.

### 1.2 `scripts/collect_nlu_samples.py`
- **Issue**: Groq Whisper was outputting foreign scripts (Urdu, Russian, Spanish) and homophone hallucinations on short (<2s) voice recordings.
- **Root Cause**: The HTTP POST request in `transcribe_via_groq()` only specified `model: "whisper-large-v3-turbo"` without language constraints, allowing automatic language detection to misclassify accented English phonemes.
- **Fix**: Added explicit parameters to the multipart form payload:
  - `language: "en"`
  - `temperature: "0.0"`
  - `prompt: "NEXUS, WhatsApp, Biryani, Dosa, Ghostwriter, VS Code, PR, GitHub, Terminal, Spotify"`

---

## 2. Verified Data Pipeline Results

1. **User Voice Samples Preserved**:
   - 30 raw samples captured in `server/admin/data/collected_samples.jsonl`.
   - Perfection Audit in `train_all.py` validated **27 perfect samples** and rejected 3 that overlapped with frozen test families to avoid test data contamination.
2. **Dataset Status**:
   - `dataset.json` updated to **3,108 training rows** across 55 unique intents.
   - Preflight data foundation validation confirmed clean.

---

## 3. Related Documentation

- Comprehensive Feature Spec: [`docs/features/55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md`](file:///c:/PROJECTS/ULTRON/docs/features/55-nlu-data-perfection-voice-scaling-and-mcp-bridge-research.md)
- WhatsApp MCP Connection Spec: [`docs/research/mcp-connection/03-whatsapp.md`](file:///c:/PROJECTS/ULTRON/docs/research/mcp-connection/03-whatsapp.md)
