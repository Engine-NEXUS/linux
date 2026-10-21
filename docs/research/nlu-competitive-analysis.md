# NLU/Intent System — Academic & Competitive Analysis

## NEXUS NLU Architecture (3-Tier Cascade)

```
Transcript -> Tier 1: Deterministic regex parser (Rust, <1ms)
  -> Tier 2: BERT-Mini ONNX (Python sidecar, ~50-100ms, lazy-started)
  -> Tier 3: Qwen Brain (admin-only, ~500ms, lazy-started)
  -> Fallback: Cloudflare Worker (general Q&A)
```

**Tier 1 — Deterministic Parser** (intent_parser.rs, 6171 lines):
- 25+ parse functions covering all intent types
- App registry fuzzy matching (phonetic + Levenshtein)
- Phonetic alias map ("goes to mode" -> "ghost mode")
- Entity extraction (repo names, PR numbers, owners)

**Tier 2 — BERT-Mini ONNX** (server/nlu/):
- Model: google/bert_uncased_L-2_H-128_A-2 (4.4M params)
- 55 intents, 51 slot labels
- Joint intent + slot classification
- OTA update capable (family devices pull from Worker R2)

**Tier 3 — Qwen Brain** (admin-only):
- Model: Qwen2.5-0.5B-Instruct GGUF (480 MB)
- Compile-time admin-brain feature + runtime admin.json gate
- Continuous learning loop (brain_monitor.rs)
- 50 approved examples triggers auto-retrain

---

## Competitive NLU Comparison

| System | Approach | Intents | NLU Method | Training |
|--------|----------|---------|------------|----------|
| **NEXUS** | 3-tier cascade | 55 | Regex + BERT-Mini + Qwen | Manual + brain auto-train |
| **Leon** | 3-mode routing | Unlimited | LLM-driven + skills | Manual + self-model |
| **Home Assistant** | Sentence templates | Unlimited | Fuzzy matcher + LLM | Template-based |
| **Mycroft** | Padatious + Adapt | Hundreds | RNN intent + keyword | Padatious (RNN) |
| **Rhasspy** | FST (fsticuffs) | Hundreds | Finite state transducer | Profile-based training |
| **Almond** | Genie semantic parser | Hundreds | Formal language (ThingTalk) | Synthesis-based |
| **Open Assistant** | End-to-end LLM | Unlimited | RLHF-trained model | SFT + reward + PPO |
| **LangChain** | LLM function calling | Unlimited | LLM-driven | None (LLM is NLU) |

---

## NEXUS Innovations (Academic Context)

### 1. Cryptographic Test Locks (data_foundation.py)
**What**: Frozen 481-row evaluation lock with SHA-256 hashes. Prevents data leakage between train/validation/test splits.
**Academic precedent**: Similar to held-out test sets in ML papers, but NEXUS adds cryptographic guarantees. No academic paper uses SHA-256 for dataset lock verification.
**Gap**: Most academic papers use simple random splits. NEXUS's approach is more rigorous but harder to reproduce externally.

### 2. Phrase-Family Separation
**What**: Train/validation/calibration/test splits separated by phrase family. No phrase overlaps across splits.
**Academic precedent**: Standard practice in speech recognition (Kaldi, espnet). NEXUS formalizes it with explicit file quarantine and lock files.
**Verification**: python server/nlu/prepare_evaluation_splits.py validates no cross-contamination.

### 3. Phonetic Alias Normalization
**What**: Deterministic soundalike phrase normalizer catches Whisper acoustic mishearings ("goes to mode" -> "ghost mode", "open what's up" -> "open whatsapp").
**Academic precedent**: Similar to phonetic confusions in ASR error analysis (arXiv papers on speech recognition errors). Not applied at NLU level in other systems.
**Unique**: NEXUS applies this at the intent parser level, before NLU classification. Other systems handle this at the ASR level or not at all.

### 4. OTA NLU Model Distribution
**What**: Admin publishes to R2 + KV manifest. Family devices pull improved models on startup with SHA-256 verification.
**Academic precedent**: Federated learning (McMahan et al., 2017) uses similar model distribution, but NEXUS uses centralized R2 distribution (not federated averaging).
**Unique**: No other open-source AI assistant has OTA NLU model updates for family devices.

---

## What NEXUS is Missing (NLU)

1. **No multi-language NLU**: English only. Leon supports multiple languages. Home Assistant supports 100+ languages. Academic papers (e.g., XLS-R, mBERT) show multilingual transfer learning works.

2. **No formal semantic parsing**: Almond uses ThingTalk formal language for compound commands. NEXUS uses regex + BERT, which can fail on complex multi-intent queries.

3. **No LLM-driven NLU fallback**: Leon uses LLM as a smart routing mode. NEXUS falls back to Worker (cloud LLM) but doesn't use it for NLU disambiguation.

4. **No active learning loop**: BabyAGI's self-building agents improve from user interactions. NEXUS's brain monitor retrains every 50 examples but doesn't actively seek edge cases.

---

## References
- arXiv:2304.03416 — Successive Refinement (wake word, related to data quality)
- Federated Learning (McMahan et al., 2017) — model distribution
- Kaldi/espnet — speech recognition split methodology
- Almond (stanford-oval/genie-toolkit) — formal semantic parsing
- Leon (leon-ai/leon) — layered memory and NLU
- docs/features/55-nlu-data-perfection-voice-scaling.md
