# Phase 3 (revised) — Kokoro inside the Feature-83 "cloud-primary + background single-slot swap" design (2026-10-05)

You pointed me to your own design for this: **Feature 83** (`docs/features/83-…single-slot-dynamic-offline-swapping.md`) and its research
(`docs/research/tts/01-cloud-primary-single-slot-offline-voice-swapping-architecture-2026-10-01.md`). Requirement restated:
*the user is on the cloud voice; when they change the voice in settings, the new voice (offline twin) replaces the old one in the background — instantly
usable via cloud, never mute, never corrupt.* My earlier Phase-3 plan (just "add Kokoro to the fallback chain") ignored this and is superseded by this doc.
**STATUS 2026-10-05: IMPLEMENTED (see `docs/changes/81-kokoro-local-tts-replaces-piper.md`). Decisions taken: online-first with automatic cloud return; one shared model + one voice file; Piper removed; dictionary question parked; download approved. Still open: model variant (RAM/accuracy), first-launch-offline bundling, voice audition.**

## 1. What already exists in code (read, not assumed)

| Piece | File | State |
|---|---|---|
| 10-persona catalog, `VoicePersona{cloud_id, local_model (Piper stem), preview_phrase, sha256: None}` | `src-tauri/src/voice_catalog.rs` | ✅ built; **all `sha256` are `None`** |
| `set_voice_preference` (instant cloud switch, Fast-ACK cache re-synthesis, spawns swap), `preview_voice`, `get_voice_status`, `get_voice_transport`, `list_voice_personas` | `tts.rs:392–581` | ✅ built |
| Background swap worker: stream `.tmp` → hash → trial-load → delete old → rename → manifest → engine reload; `voice:status` events (`downloading{progress}`/`ready`/`error`) | `tts_swap.rs` | ✅ built — **downloads a ~60 MB Piper model from `huggingface.co/rhasspy/piper-voices/resolve/main` on every persona change** |
| Hub UI: cards, preview, equip, status pill reacting to `voice:status` | `SettingsSidebarApp.tsx:405–500` | ✅ built |
| Single slot: `%APPDATA%/com.nexus.assistant/voices/{active_offline.onnx(.json), manifest.json}` | `voice_catalog.rs` | ✅ |
| Offline engine = Piper (`piper-rs` → statically linked GPL espeak-ng) | `tts_piper.rs` | ⚠️ GPL (doc 02 finding #1) |

## 2. Problems found in the current implementation (independent of Kokoro)

1. **Rapid switching loses the last choice.** `run_swap` does `if SWAP_ACTIVE.swap(true) { skip }` ("a second equip waits for the next turn" — but nothing re-queues it).
   Switch A → B → C quickly: if A is still downloading, B and C are dropped ⇒ the cloud voice is C but the offline twin ends up A. Needs *latest-wins* semantics.
2. **No integrity pinning.** Every `sha256` is `None`; the only check is a trial load. A truncated-but-loadable or tampered file passes. The URL is `…/resolve/main/…` (a mutable ref).
3. **Piper-voices licences are per voice** (many different source datasets). I have not verified the licence of any of the ten twins (alan, southern_english_female, amy, lessac, kristin, ryan, libritts, hfc_female, northern_english_male). Treat as unverified.
4. **GPL espeak-ng** is linked through `piper-rs` regardless of which twin is loaded.
5. The spec's disk invariant (**≤ 65 MB**, one `.onnx`) cannot hold for Kokoro (below) — the invariant itself must be revised.

## 3. Why Kokoro changes the economics (verified from the HF repo listing)

| | Piper twin (today) | Kokoro |
|---|---|---|
| What defines a voice | a whole ~60 MB VITS model | a **522,240-byte style file** (`voices/<name>.bin`) |
| Shared base | none | **one model for every voice**: `model_quantized.onnx` 92,361,116 B (88.1 MB, int8); `model_q8f16.onnx` 86,033,585 B (82.0 MB); fp16 163 MB; fp32 325 MB |
| Cost of changing voice | download ~60 MB | download **0.5 MB** (after the one-time base) |
| English voices available | per-persona models | 28: US `af_*`/`am_*`, British `bf_*`/`bm_*` (55 voice files total incl. other languages) |
| Licence | per voice, unverified | Apache-2.0 model (hexgrad); voices ship in the same repo ("no specific licence notes" per VOICES.md) |

**Consequence for your requirement:** "new voice replaces old in the background" becomes *near-instant* after the first-ever base download. The slot becomes
**`kokoro_model.onnx` (shared, one-time) + `active_voice.bin` (exactly one, replaced atomically)** ≈ **88.6 MB**, still strictly bounded; you lose the 65 MB cap but gain tiny swaps.
(All 10 persona voices together would be only ~5 MB, but your rule is single-slot, so I keep one.)

### Honest quality finding
Kokoro's own grades (hexgrad `VOICES.md`): `af_heart` **A**, `af_bella` **A−**, `af_nicole` B−, `bf_emma` B−, `af_sarah` C+, `am_michael` C+, `bm_george` **C**, `bm_fable` C, `bm_lewis` **D+**, `bm_daniel` **D**.
The **British male voices — your JARVIS and ALFRED personas — are the weakest** Kokoro voices. There is **no Irish voice** (FRIDAY), so it can only approximate. Since cloud (Edge) is primary this
only matters offline, but the "offline persona match" promise of Feature 83 is weaker for those two personas than for the US-female ones. Audition before locking the mapping.

### Proposed persona → Kokoro voice mapping (to be auditioned, subjective)
| Persona | Cloud (unchanged) | Kokoro voice (grade) | Note |
|---|---|---|---|
| jarvis | en-GB-RyanNeural | `bm_george` (C) | weakest area; alt `bm_lewis` (D+) |
| friday | en-IE-EmilyNeural | `bf_emma` (B−) | no Irish voice exists |
| nexus (default) | en-US-AvaNeural | `af_heart` (A) | |
| siri | en-US-JennyNeural | `af_bella` (A−) | |
| alexa | en-US-AriaNeural | `af_nicole` (B−) | |
| google | en-US-BrianNeural | `am_michael` (C+) | |
| cortana | en-US-MichelleNeural | `af_sarah` (C+) | |
| samantha | en-US-SaraNeural | `af_sky` (C−) / `af_heart` | audition |
| alfred | en-GB-OliverNeural | `bm_fable` (C) | |
| offline_safe | en-US-AvaNeural | `af_heart` (A) | baseline |

## 4. Target design (what the user experiences)

1. **Click a voice card →** cloud voice switches instantly (existing, unchanged); Fast-ACK cache re-synthesised in the background (existing).
2. **Background worker (new Kokoro path):**
   * base present → fetch only `<voice>.bin` (0.5 MB, ~1 s) → verify → atomic replace of `active_voice.bin` → hot-swap in the engine → pill `✓ Cloud + Offline Ready`.
   * base absent (first time ever) → pill `⬇ Syncing Offline Voice (n %)` for the 88 MB base, then the voice file; cloud keeps working throughout.
   * **latest-wins queue**: if the user clicks A → B → C, only C's voice is guaranteed at the end (fixes problem #1).
3. **Offline at any time:** engine order `Edge (cloud) → Kokoro (if base+voice present) → [Piper only if the legacy feature is compiled in] → system fallback`. Never mute.
4. **Atomic & verified:** `.tmp` → SHA-256 against a **pinned manifest** (new, committed: file → sha256 + size + pinned HF *commit* URL, not `main`) → trial load → rename. Old file removed only after the new one verifies.
5. **RAM:** Kokoro session loaded lazily on first offline utterance and unloaded after idle (reuse the existing Piper "unload after 10 min" pattern) to protect the <60 MB idle goal. ONNX session RAM is **not measured yet**.

## 5. Implementation plan (revised Phase 3)

| Step | Work | Source (Case 1) | Acceptance test |
|---|---|---|---|
| 3a | `tts_kokoro.rs`: vocab map, voice-file loader (`[510,1,256]` f32; style row = token-count − 1), phoneme chunking (≤ 510), `ort` session (`input_ids` i64 [1,N] with 0-padding, `style` [1,256], `speed` [1]) → 24 kHz f32 | `pguso/kokoro` `tokenizer.rs`, `synthesizer.rs`, `pipeline.rs` (Apache-2.0) | unit tests on vocab/tokenizer/chunking; **integration test (skipped when model absent)**: "Hello, I'm NEXUS." → non-silent audio, duration plausible, RMS in range |
| 3b | G2P adapter on `misaki-rs` (`default-features = false` ⇒ **no espeak**); custom lexicon for product words (WhatsApp, Ghostwriter, NEXUS, Groq, Gmail…) | `MicheleYin/misaki-rs` (MIT) | `cargo tree` shows **no** `espeak-rs`/`espeak-rs-sys` for this feature; 50-word OOV/pronunciation list auditioned |
| 3c | Catalog: add `kokoro_voice` per persona (keep `local_model` for the optional Piper path); pinned manifest file | – | catalog tests: every persona maps to an existing voice file name; manifest hashes present |
| 3d | Swap worker v2: base+voice, **latest-wins queue**, pinned-hash verify, trial-load via Kokoro, manifest v2, `voice:status` unchanged shape + `phase: base|voice` | – | simulated tests: A→B→C ends on C; network drop mid-download leaves old voice + no orphan `.tmp`; hash mismatch rejected; base-present switch < 3 s |
| 3e | `tts.rs` tier chain: Edge → Kokoro → Piper(optional); captions `estimated: true`; Piper behind cargo feature `piper` (default **off**) | – | with `--no-default-features` build: `cargo tree` has no GPL; full test suite green |
| 3f | Hub wording for base-download phase; settings doc | – | manual UI check + vitest |
| 3g | Measurements → ledger: real-time factor, first-audio latency, idle/loaded RAM, int8 vs q8f16 vs fp16 audition | – | numbers recorded; **no claim without measurement** |

## 6. Risks / open items

* **Dictionary provenance (misaki-rs):** its README says the pronunciation dictionary was *"updated… using eSpeak"* and its `expand_dicts.py` runs `espeak-ng` over word lists. Output derived from GPL data is a **legal grey area**; the original Misaki repo is Apache-2.0 but does not document where its English dictionaries came from. For **personal use** this is fine; for **distribution** it needs your decision (accept the risk / use only the original Misaki data / write a CMUdict-only G2P with letter-to-sound rules).
* **`pguso/kokoro` and `misaki-rs` are tiny single-maintainer repos (8 and 10 stars)** — vendor the code, pin hashes, don't depend on them live.
* **Int8 quality** (`model_quantized`) may audibly degrade vs fp16/fp32 — needs the audition in 3g.
* **No OOV fallback** with espeak off: unknown names/words may be mispronounced; mitigated by the custom lexicon + spell-out fallback.
* **Hosting:** third-party HF URLs; pin to a commit hash and keep SHA-256 in the repo. Consider mirroring to your own bucket/Release if you distribute.
* **Disk cap:** the spec's 65 MB cap becomes ≈ 89 MB (decision below).

## 7. Decisions needed from you

1. **Model variant:** `model_quantized` int8 (88 MB, default), `model_q8f16` (82 MB), or fp16 (156 MB)? — I recommend downloading int8 first and auditioning against fp16.
2. **First-run baseline:** (a) bundle model + `af_heart` in the installer (+~89 MB, always works offline, replaces the bundled 63 MB Amy), or (b) download on first run (small installer, but offline-first-launch has no local voice until the first online session).
3. **Disk invariant:** OK to change "≤ 65 MB, one model" to "one shared base (~88 MB) + exactly one voice file"?
4. **Piper:** keep as an optional legacy cargo feature (default off), or remove entirely?
5. **Dictionary provenance:** OK for personal use now, with the distribution question parked in doc 02's decision list?
6. **Download permission (dev):** `model_quantized.onnx` (92,361,116 B) + voices `af_heart`, `af_bella`, `bm_george`, `bm_fable`, `bf_emma` (≈ 0.5 MB each) from `huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX` into a git-ignored `src-tauri/resources/kokoro/` — still pending your explicit yes.

## Sources
* Your docs: Feature 83 + TTS research 01 (above); code read: `voice_catalog.rs`, `tts_swap.rs`, `tts.rs`, `SettingsSidebarApp.tsx`.
* HF listings (fetched): `onnx-community/Kokoro-82M-v1.0-ONNX` `onnx/` and `voices/` trees; `hexgrad/Kokoro-82M` `VOICES.md` (grades).
* Upstream code read: `pguso/kokoro` (Apache-2.0) `synthesizer.rs`, `voice.rs`, `tokenizer.rs`; `MicheleYin/misaki-rs` (MIT) README/Cargo.toml/`expand_dicts.py`; `hexgrad/misaki` README (Apache-2.0).

## 8. Model-variant decision (measured 2026-10-05)

| Variant | File | Model RAM (+MB) | Peak RAM (+MB) | Median RTF (4 threads) | WER vs reference text | Transcripts identical to fp32 |
|---|---|---|---|---|---|---|
| **int8 `model_quantized` (chosen)** | 92.4 MB | **129** | **330** | 0.98 | 0.042 | **16/16** |
| q8f16 | 86.0 MB | 152 | 366 | 1.38 | 0.042 | 16/16 |
| fp16 | 163.2 MB | 347 | 553 | 0.74 | **0.250** | 10/16 |
| fp32 `model` | 325.5 MB | 351 | 560 | 0.62 | 0.042 | 16/16 (reference) |

Method: 8 sentences × (US `af_heart`, GB `bm_george`) = 16 clips per variant, identical G2P phonemes, one process per variant (clean RAM), 4 ORT threads on the i7-1355U, audio transcribed back with Moonshine small. WER 0.042 for the three good variants is entirely number formatting ("2.30" vs "two thirty") and one "Stopped"→"Stop"; mean duration difference vs fp32 ≤ 0.04 s.

**Decision: keep int8 (`model_quantized.onnx`).** For intelligibility it is indistinguishable from fp32 (16/16 identical transcripts) at **37 % of the model RAM (129 vs 351 MB) and 28 % of the download (92 vs 326 MB)**. fp32 is ~1.6× faster (RTF 0.62 vs 0.98) but costs 2.7× the RAM — wrong trade for a fallback on a RAM-conscious assistant. **fp16 is rejected**: on this CPU it was both bigger and audibly worse (WER 0.25, one clip transcribed as empty). q8f16 is strictly worse than int8 here (slower, more RAM). Caveat: an ASR round-trip measures intelligibility, not naturalness — a listening A/B of int8 vs fp32 is still worthwhile.
