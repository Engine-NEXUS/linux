# Change 50: STT Hallucination Fix — VERIFY_RING Prefix-Pad Poisoning, Ghost Mode Vocabulary & Double Trigger Banner

**Date:** 2026-09-25  
**Domain:** Speech-to-Text, Console UX  

---

## 1. Summary of Changes

### `src-tauri/src/wakeword_oww.rs`
- **Removed `STT_PREFIX_PAD_SAMPLES` constant** (was 2560 samples = 160ms) — unused after prefix-pad removal.
- **`start_stt_capture()`**: Removed the VERIFY_RING prefix-pad injection entirely. The buffer now starts clean on every wake. The VERIFY_RING contains the wake word audio ("NEXUS") + gap audio; injecting it into the command buffer gave Groq the wake word + silence + command as one ambiguous fragment, causing confabulations like "and then the process goes to murder."

### `src-tauri/src/stt_groq.rs`
- **`NEXUS_VOCABULARY`**: Expanded with ghost mode command phrasings ("Activate ghost mode. Open ghost mode. Start ghost mode. Exit ghost mode."), common app opens ("Open WhatsApp. Open Chrome. Open VS Code. Open Spotify. Open Discord."), and additional action verbs ("open close stop cancel navigate settings"). Fixes Groq mishearing "ghost mode" as "goes to mode" / "goes to mold."

### `scripts/run.ps1`
- **Trigger pattern**: Changed from `instant neural trigger|OWW wake detected|wake-word: NEXUS detected` → only `instant neural trigger`. Eliminates double `TRIGGER #01` / `TRIGGER #02` banners caused by the DEBUG `OWW wake detected!` line also matching the pattern.
- **New suppression rules**: Added `OWW wake detected!` and `high-confidence single-frame trigger` to the console suppression list (they are sub-steps of the canonical `instant neural trigger` INFO line).

---

## 2. Root Cause Analysis

### Bug 1 — VERIFY_RING Prefix-Pad (PRIMARY)
`capture_00.wav` profile showed chunks 0–1 at RMS=2317, 751 (i16 scale) — the tail of "NEXUS" inside a 160ms prefix-pad seeded from the verifier ring. Groq received:
```
[NEXUS audio tail] + [1.3s dead silence] + [user command]
```
This fragment is acoustically ambiguous; Groq confabulated entirely unrelated text.

### Bug 2 — Missing Vocabulary Bias
Whisper without "ghost" in its prompt maps `/ɡoʊst moʊd/` to the highest-frequency alternative ("goes to mode"). Adding the phrase to NEXUS_VOCABULARY flips the decoder bet.

### Bug 3 — run.ps1 Pattern Double-Match
Two Rust log lines matched the old OR-pattern:
1. `DEBUG OWW wake detected! (confidence: 85.2%)` → TRIGGER #01
2. `INFO wake-word: instant neural trigger (confidence: 85.2%)` → TRIGGER #02

---

## 3. Verification

- `cargo test --lib wakeword -- --test-threads=1`: **43/43 passed**
- `cargo build --release --features custom-protocol,admin-brain`: **clean, zero warnings**
