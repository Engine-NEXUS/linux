# 129 — Free for every user, open source, ONE voice everywhere (RESEARCH + PLAN, nothing built)

Date: 2026-10-16. New product constraint from the owner: *NEXUS is a production-level open-source project; it must be 100 % free for users (they bring their own Groq / Gemini keys); the voice must be identical in "cloud" and "local".*
This supersedes the product recommendation of `128` Part A (Azure container) and `127`. Part B of 128 (settings architecture) still stands and is a prerequisite here.

---

## 1. What the constraint rules out (verified)

| Idea | Why it fails for a free, distributable product |
|---|---|
| Microsoft Ava/Ryan neural container | Every user would need an Azure account, a Speech resource, Docker, 12 GB RAM and an always-on billing link (stops after ~10 failed 10-15-min meter calls); disconnected licence needs approval + yearly fee. Cannot be redistributed. |
| Edge online voices (what NEXUS uses now) | Unofficial endpoint. A Microsoft Q&A moderator said using it commercially without an Azure subscription "could be a violation of our terms"; community reports of the method being blocked (Dec 2025) and 403/429 storms (we hit both). Text of every reply (personal content) goes to Microsoft. No local copy exists, so it can never be identical to an offline voice. |
| Windows Narrator voices via the SAPI adapter | Not licensed for third-party apps (the adapter's own README says so), breaks on updates, Ryan only. |
| Google Cloud TTS | Needs a billing account; different voice; no local twin. |
| Groq Orpheus TTS | Paid ($22 per 1 M characters); the only limits visible for the model are tiny (10 RPM, 100 requests/day, 3.6 K tokens/day on the Developer table); cloud-only, so no identical local voice. |
| Gemini API TTS | Free tier exists but preview-model limits are low (a third-party README lists ~50 requests/day, **unofficial**); cloud-only, no local twin; model names changing (2.5 -> 3.1 preview). |
| Cloudflare Workers AI (MeloTTS) via the project's Worker | 10 000 neurons/day **shared by the whole account**: fine for one user, useless as the shared backend of an open-source app (each user would need their own Cloudflare account). |

Conclusion: **any cloud TTS is either paid, account-gated, rate-limited, or a different voice from any offline voice.** The only way to guarantee "identical, free, offline, redistributable" is **the same open-weights model running on the user's own machine** (a "cloud" that runs the same weights, if ever wanted, is optional and identical by construction).

## 2. The engine choice (research)

| Engine | Licence (verified from the repo/model page) | Size / speed | Fit |
|---|---|---|---|
| **Kokoro-82M** | **Apache-2.0** weights; trained only on permissive / public-domain / synthetic audio (clean provenance); v1.0: 54 voices, 8 languages | 82 M params, ~88-326 MB files; real-time-ish on CPU (**measured on THIS laptop earlier: RTF 0.7-1.7**) | Already integrated (`tts_kokoro.rs`, `tts_swap.rs`, pinned HF revision, SHA-256 pinned, no espeak/GPL: `misaki-rs`). **Best fit.** |
| Kitten TTS (mini 80 M, micro 40 M, nano 15 M / 25 MB int8) | **Code Apache-2.0**; the new 1.7 B "KittenTTS 2" is under a separate **Stellon Labs Community License** (read before use) | tiny, CPU-friendly ONNX; quality below Kokoro | Possible "lite" pack for very slow PCs; **different voice**, so only as an explicit user choice. |
| Piper | Repo now **GPL-3.0 and embeds espeak-ng** (the old MIT repo is archived); voice licences vary | very fast, most robotic | Avoid: copyleft would infect the app (we removed GPL espeak deliberately). |
| MeloTTS | MIT | CPU real-time | Lower quality than Kokoro; no advantage. |
| Orpheus-3B, Chatterbox, XTTS, F5, Dia ... | mixed (some non-commercial) | GPU-class | Too heavy for "runs on any user's laptop". |

**Decision: Kokoro-82M is the one engine.** It is the only candidate that is permissively licensed, already shipping in the code base, runs on ordinary CPUs, and sounds clearly better than the lightweight alternatives.

## 3. "But I want that Ava/Ryan voice" — what is honestly possible

* Microsoft's voices cannot be copied into a free, redistributable package. Cloning them into Kokoro would use Microsoft audio as training material; third-party analyses say Microsoft agreements restrict using AI output to train other AI (I could not find the Azure Speech clause itself - **unverified**, ask a lawyer before shipping anything derived from it).
* **KVoiceWalk** (Apache-2.0; verified) is the legitimate route to a *custom* voice: it searches Kokoro's voice-style tensor with a random walk so the output imitates a **target recording** (20-30 s, mono 24 kHz, single speaker, plus its transcript). The author's example went from 71 % (closest stock voice) to ~92-93 % speaker similarity (Resemblyzer) in 10 000 steps; results vary, some runs stall; cost: ~4 h on an RTX 3060 (≈26 h on CPU). A community report says pure-Kokoro cloning plateaus around the 80s and may need a second stage (RVC) for exact identity - **we should not promise an exact copy**.
* So we can create a **NEXUS Signature Voice**: our own `.bin` voice tensor (0.5 MB), Apache-2.0 licensed by the project, byte-identical on every install, with a model card that documents the target recording's origin and consent. The target must be rights-clear: (a) a recording by the owner or a consenting contributor, or (b) a public-domain LibriVox reader. For a JARVIS-like British male, (b) is easy. The owner may still make a **private, never-distributed** voice from any audio for their own PC.
* Free GPU for the search exists: the project already used a free Kaggle T4 for wake-word training (AGENTS.md, 2026-08-30), so a 4-hour run costs nothing.

Expected outcome: a voice that feels like "a" consistent NEXUS, close in character to what you choose as the target, **not** a bit-exact Ava. Everything is identical online/offline/on every user's PC because there is only one engine and one voice file.

## 4. Where the users' Groq / Gemini keys fit
Keys keep powering **LLM + STT** (Groq Whisper, router cascade, Gemini vision) exactly as today. They are **not** used for TTS by default (see table 1: limits, price, no identical twin). Optional, off by default: a user who wants may point NEXUS at **their own Kokoro server** (the open "Kokoro-FastAPI" style endpoint running the same weights and the same voice file) for speed; the voice stays identical because the weights and voice tensor are the same, and the voice-fingerprint test (T7) proves it.

## 5. Plan

**T0 — Choose the voice by ear (0.5 day).** Generate 12 sample lines with the stock Kokoro voices (`bm_george`, `bm_lewis`, `bf_emma`, `am_michael`, `af_heart`, ...) plus 2-3 KVoiceWalk candidates; blind-rate them. Gate: you pick THE voice (or approve building a signature voice).

**T1 — Make local fast enough (1-2 days).** Benchmark matrix on this laptop and on a low-end reference (4-core, 8 GB): fp32 / fp16 / int8 model files, ORT threads 2/4/6, DirectML or OpenVINO on the Intel iGPU, warm model, first-chunk size. Budget: first audio <= 1.0 s for a <= 6-word line, RTF <= 0.6, RAM <= 400 MB. Stock-line clip bank (rendered locally once, persisted; **auto-growing** with every line spoken) removes latency for the ~70 % repeated lines. Gate: numbers on paper before any other work.

**T2 — One identity in settings (0.5-1 day; = doc 128 S0-S2).** `voicePack` is the only stored voice setting (`{id, engine: kokoro, file, sha256, licence, prosody}`); the cloud/local "twin" concept disappears. Hot-fix the stale-snapshot overwrite first.

**T3 — Remove every other audible voice (0.5 day).** Windows voice and auto-emotion out of the audible path; fixed prosody per voice pack; `pick_emotion` whole-word fix; **Edge becomes an explicit opt-in "Online voice (unofficial, different from your offline voice)", default OFF in the open-source release** (the owner's personal build can keep it on). If the engine is late: wait, never switch voice; if impossible: caption + chime.

**T4 — Signature voice pipeline (2-3 days).** Script + docs for KVoiceWalk on Kaggle (free T4), 3-5 seeds, scoring = Resemblyzer similarity + ASR round-trip WER + a MOS predictor + your ABX; publish `voices/nexus-signature.bin` with a model card (target provenance, consent, licence Apache-2.0). Gate: your approval by ear.

**T5 — Distribution & licences (0.5 day).** First-run download from the pinned Hugging Face revision (already built), `NOTICE` file (Kokoro Apache-2.0, misaki, onnxruntime MIT), CI check that no GPL crate or espeak enters `Cargo.lock`, and that Edge code is unreachable unless the opt-in is set.

**T6 — Optional own-server accelerator (1 day, later).** Settings field "my Kokoro server URL"; same weights + voice file; per-release fingerprint test.

**T7 — Prove identity every release (0.5 day).** `scripts/voice_fingerprint.py` renders 20 fixed sentences, compares Resemblyzer cosine and F0 against golden clips: local CPU vs any configured remote must be >= 0.97 / within 3 Hz, otherwise CI fails.

## 6. Decisions needed from you
1. Accept that the free, open-source voice is **Kokoro-based (our own signature voice)**, not Microsoft's Ava/Ryan? (This is the only fully free + identical + redistributable design I could find.)
2. Target for the signature voice: **owner recording**, **public-domain LibriVox British male**, or just a **stock Kokoro voice** (`bm_george`) to start?
3. OK to make **Edge opt-in and OFF by default** in the open-source release (keeping it ON in your personal build)?
4. Do you want a **private** Ava/Ryan-flavoured voice for your own PC only (never published)? It carries the unverified Microsoft-terms risk above.
5. Go-ahead for **T0-T3** first?

## 7. Not verified
Kokoro speed on a low-end PC and with int8/DirectML (T1 measures it); the audible quality of a KVoiceWalk voice (published similarity 0.92-0.93 on one run; community reports plateaus in the 80s); the exact clause in Microsoft's terms about training on TTS output; Groq free-plan TTS limits (the docs page did not show the Free tab); Gemini TTS official limits; KittenTTS 2's community licence text. Nothing in this document is built.

## Sources
- Kokoro-82M model card (Apache-2.0, 82 M, training data statement): https://huggingface.co/hexgrad/Kokoro-82M
- KVoiceWalk (Apache-2.0, method, 71 % -> ~93 %): https://github.com/RobViren/kvoicewalk
- Kitten TTS (sizes, Apache-2.0 code, separate community licence for v2): https://github.com/KittenML/KittenTTS
- Piper now GPL-3.0 with embedded espeak-ng: https://github.com/OHF-Voice/piper1-gpl
- Groq Orpheus (model card, $22 / M characters; limits table): https://console.groq.com/docs/model/canopylabs/orpheus-v1-english , https://console.groq.com/docs/rate-limits
- Groq deprecated PlayAI TTS for Orpheus: https://releases.sh/release/rel_slohPX8XO2tKT7GyKNH-b-playai-tts-deprecated-orpheus-now-platform-wide
- Gemini TTS pricing / free tier: https://ai.google.dev/gemini-api/docs/pricing
- Edge TTS legal discussion (Microsoft Q&A): https://learn.microsoft.com/en-us/answers/questions/2088770/are-opensource-edge-tts-free-for-commercial-use , https://learn.microsoft.com/en-us/answers/questions/2392491/unofficial-edge-tts-api
- Cloudflare Workers AI pricing (10 000 neurons/day, MeloTTS): https://developers.cloudflare.com/workers-ai/platform/pricing/
- Open-source TTS roundups (secondary): https://localclaw.io/blog/local-tts-guide-2026 , https://builderai.tools/blog/open-weight-text-to-speech-models-2026
- Azure container facts (rejected for distribution): https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-container-howto
