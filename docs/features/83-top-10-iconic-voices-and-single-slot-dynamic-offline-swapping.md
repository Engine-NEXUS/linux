# Feature 83 — Top 10 Iconic Voices & Single-Slot Dynamic Offline Swapping

**Date:** 2026-10-01  
**Category:** Audio & Voice Synthesis / Personalization / Offline Robustness  
**Status:** SPECIFICATION COMPLETE (Ready for Implementation Planning)  
**Research Spec:** `docs/research/tts/01-cloud-primary-single-slot-offline-voice-swapping-architecture-2026-10-01.md`  

---

> **REVISION 2026-10-05 — offline engine is now Kokoro-82M, not Piper.** Piper's espeak-ng is GPL-3 (licence audit, doc 02). The single slot is now **one shared model (`kokoro_model.onnx`, ~88 MB, downloaded once) + exactly one voice file (`active_voice.bin`, 0.5 MB)**; the "≤ 65 MB / one `.onnx`" invariant in §4.2/§6 no longer applies (≈ 89 MB). Changing voice downloads only the 0.5 MB voice file; the latest request wins; downloads are pinned to an immutable revision with SHA-256 verification. Online remains first priority; the local voice is used only while the internet is down and NEXUS flips back automatically when it returns. Details: `docs/changes/81-kokoro-local-tts-replaces-piper.md` and `docs/research/jarvis-landscape/09-…`. The Piper-specific text below is historical.

---

## 1. Feature Summary

Feature 83 modernizes the speech synthesis system of NEXUS by introducing a curated lineup of **10 iconic AI voice personalities** (JARVIS, FRIDAY, NEXUS Classic, Siri, Alexa, Google, Cortana, Samantha, Alfred, Emergency Offline) in the Command Hub. 

The architecture guarantees:
1. **Studio-Grade Cloud Primary:** Broadcast-quality neural speech synthesis streaming via Edge-TTS ($0 cost, 0 MB idle RAM).
2. **True Persona-Matched Offline Fallback:** When the user selects a voice, a background worker asynchronously fetches a matching local model (Piper VITS), replacing the previous offline voice.
3. **Strict Single-Slot Disk Invariant:** Exactly **one local voice file (~45–63 MB)** exists on the user's hard drive at any time. Switching voices deletes the previous local model and renames the new one atomically, permanently protecting the user from disk bloat.
4. **Persistent Profile Memory:** The user's selection is stored in `settings.json` and mirrored in long-term memory (`memory.rs`), surviving app updates and reboots.
5. **Acoustic Fast-ACK Consistency:** Switching voices automatically triggers a sub-400ms background re-synthesis of instant acknowledgment phrases (*"On it sir"*, *"Opening Command Hub, sir"*), ensuring 100% voice uniformity across all turns.

---

## 2. The 10 Iconic Personalities Catalog

```
┌────────────────────────────────────────────────────────────────────────┐
│                        TOP 10 ICONIC VOICE MATRIX                      │
├────┬─────────────┬───────────┬──────────────┬──────────────┬───────────┤
│ #  │ Key         │ Name      │ Persona      │ Accent       │ Cloud ID  │
├────┼─────────────┼───────────┼──────────────┼──────────────┼───────────┤
│ 1  │ jarvis      │ JARVIS    │ Iron Man AI  │ 🇬🇧 British M  │ Ryan      │
│ 2  │ friday      │ FRIDAY    │ Tactical AI  │ 🇮🇪 Irish F    │ Emily     │
│ 3  │ nexus       │ NEXUS     │ Expressive   │ 🇺🇸 US Female │ Ava       │
│ 4  │ siri        │ SIRI      │ Modern iOS   │ 🇺🇸 US Female │ Jenny     │
│ 5  │ alexa       │ ALEXA     │ Echo Lead    │ 🇺🇸 US Female │ Aria      │
│ 6  │ google      │ GOOGLE    │ Tech Lead    │ 🇺🇸 US Male   │ Brian     │
│ 7  │ cortana     │ CORTANA   │ Halo AI      │ 🇺🇸 US Female │ Michelle  │
│ 8  │ samantha    │ SAMANTHA  │ "Her" OS     │ 🇺🇸 US Female │ Sara      │
│ 9  │ alfred      │ ALFRED    │ Master Butler│ 🇬🇧 British M  │ Oliver    │
│ 10 │ offline_safe│ EMERGENCY │ Offline Safe │ 🇺🇸 US Female │ Amy       │
└────┴─────────────┴───────────┴──────────────┴──────────────┴───────────┘
```

---

## 3. User Interface Specifications (Command Hub: Audio Tab)

### 3.1 Voice Card Grid Layout
Inside `SettingsSidebarApp.tsx` $\to$ `AudioTab`, the previous long list of generic names is replaced with a **curated 2×5 responsive grid** of rich cards:

Each voice card displays:
- **Header:**
  - Avatar badge (e.g. 🤖 for JARVIS, 🛡️ for FRIDAY, 🌟 for NEXUS, 🎙️ for Google).
  - Voice Name (e.g. `JARVIS`, `FRIDAY`).
  - Region & Gender tag (e.g. `🇬🇧 UK · Male`, `🇮🇪 Ireland · Female`).
- **Description:** One-sentence tone summary (e.g. *"Calm, intelligent, British baritone with witty refinement"*).
- **Controls:**
  - **Preview Button (`▶ Play Demo`):** Plays the signature preview dialogue instantly. When playing, the icon toggles to `⏸ Pause` / audio wave bars.
  - **Selection State:**
    - Inactive card: Click card to equip.
    - Active card: Glowing cyan border, checkmark icon (`✓ Active`), and sync status pill.
- **Offline Sync Status Pill (on active card):**
  - `✓ Cloud + Offline Ready` (green) — Local twin is verified and cached on disk.
  - `⬇ Syncing Offline Voice (45%)` (animated blue pulse) — Background worker is streaming the offline model.
  - `⚡ Cloud Only (Offline Pending)` (amber) — Offline twin queued.

---

## 4. Technical Architecture & File Contracts

### 4.1 Settings Model (`NexusSettings`)
Extend `NexusSettings` in `src-tauri/src/commands.rs`:
```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusSettings {
    // ... existing fields ...
    
    /// User-selected iconic voice key (e.g. "jarvis", "friday", "nexus").
    /// Default: "nexus"
    #[serde(default = "default_selected_voice")]
    pub selected_voice: String,

    /// Cloud Edge-TTS voice identifier (e.g. "en-GB-RyanNeural").
    #[serde(default = "default_edge_tts_voice")]
    pub edge_tts_voice: String,

    /// Local offline voice model identifier (e.g. "en_GB-alan-medium").
    #[serde(default = "default_offline_voice")]
    pub offline_voice_model: String,
}
```

### 4.2 Local Storage Directory Layout
```
%APPDATA%/com.nexus.assistant/voices/
   ├── active_offline.onnx        (~45-63 MB — ONLY ONE MODEL EVER STORED)
   ├── active_offline.onnx.json   (~5 KB — phoneme dictionary & sample rate)
   ├── voice_download.tmp         (Ephemeral download buffer — auto-cleaned)
   └── voice_manifest.json        (Contains current model metadata)
```

### 4.3 Atomic File Swapping Mechanism
The download worker executes the following transaction:
1. Stream remote model from voice repository CDN to `%APPDATA%/voices/voice_download.tmp`.
2. Compute SHA-256 hash and verify against the catalog checksum.
3. If valid:
   - Check if `%APPDATA%/voices/active_offline.onnx` exists.
   - Delete previous `active_offline.onnx`.
   - Rename `voice_download.tmp` $\to$ `active_offline.onnx`.
   - Write updated `voice_manifest.json`.
   - Reload local Piper engine handle (`TtsState.piper_engine`).
4. If invalid or aborted:
   - Delete `voice_download.tmp`.
   - Keep existing `active_offline.onnx` intact.

---

## 5. State Machine & Fallback Logic

```
                    [ App Boot / Network Check ]
                                 │
                   ┌─────────────┴─────────────┐
                   │                           │
          [ Internet Online ]         [ Internet Offline ]
                   │                           │
                   ▼                           ▼
        Stream Edge-TTS Neural       Load %APPDATA%/voices/
             (0 MB RAM)                active_offline.onnx
                   │                           │
                   │ (Network Drops)           ▼ (If missing/corrupt)
                   └───────────────────► Fallback to bundled
                                         en_US-amy-medium.onnx
```

---

## 6. Verification & Acceptance Criteria

1. **Top 10 Selection:** The Command Hub displays exactly the 10 specified iconic voices.
2. **Audio Preview:** Clicking `▶` on any voice card plays its authentic 2-second signature sample without modifying saved user preferences.
3. **Instant Cloud Switch:** Clicking to equip a voice switches all subsequent spoken responses to the new voice in <200ms.
4. **Single-Slot Disk Constraint:** The `%APPDATA%/com.nexus.assistant/voices/` directory never contains more than 1 `.onnx` model file (disk usage strictly capped at ~65 MB).
5. **Atomic Safety:** Disconnecting network during an active offline voice download leaves the previous offline voice operational and leaves zero orphan `.tmp` files.
6. **Fast-ACK Parity:** Instant phrases (*"On it sir"*, *"Opening Command Hub, sir"*) re-synthesize in the background and match the newly equipped voice.
7. **Offline Continuity:** Disconnecting Wi-Fi results in the assistant speaking in the matching local voice rather than muting or failing.
