# 127 — "I heard 3 different voices": one voice everywhere, cloud and local identical (ANALYSIS + PLAN, nothing built)

Date: 2026-10-16. User directive (binding): *"make sure the user doesn't feel any difference in local or cloud TTS voice — I need a carbon copy, exactly the same voice."*

> **SUPERSEDED (2026-10-16, same day):** the user rejected a Kokoro lookalike. The exact-voice plan is in `128-exact-microsoft-voice-locally-and-settings-hub-architecture-audit-and-plan.md`. Sections 1 (what caused the three voices) and the fixed-prosody / no-engine-switch fixes (V1-V3, V7) remain valid.

---

## 1. What produced "three voices" (evidence, not guesses)

| # | Source of difference | Evidence | Status |
|---|---|---|---|
| 1 | **Prosody changes per line** inside the SAME cloud voice | Boot log 15:54-15:55: hello = Neutral (cached), "Working on it sir." = `emotion=Some(Cheerful)` (**+10 % rate, +30 Hz pitch**), the boot question = `emotion=Some(Urgent)` (**+20 % rate, +20 Hz pitch**), the cloud reply = Neutral. `ttsEmotion: "auto"` in settings.json. Four different-sounding deliveries of one voice in 25 s. | **Confirmed** |
| 1b | The "Urgent" was a **bug**: `pick_emotion` uses substring matching; the mail said "Team **Un**stop" -> contains "stop" -> Urgent. A question about reopening apps was read in an alarmed voice. | `tts_edge.rs::pick_emotion` (`lower.contains("stop")`) | **Confirmed** |
| 2 | **Different engine inside one session** | Earlier live log (17:36-17:37): `Windows voice used (local voice too slow)`, `engine=local`, "streaming reply: engine=local" next to Edge lines. My own safety net (doc 125 Option B: Windows voice when Edge/Kokoro is late) deliberately switches to the Windows voice (Zira/Aria). **That design contradicts this directive.** | **Confirmed (my change)** |
| 3 | **Voice identity drift between sessions / cache** | `tts_cache/` holds **60 clips of en-GB-RyanNeural (JARVIS) and 64 of en-US-AvaNeural**; settings.json now says `selectedVoice: "nexus"`, `ttsVoice/edgeTtsVoice: en-US-AvaNeural`, but `offlineVoiceModel: "bm_george"` (the JARVIS twin) while the installed `voices/manifest.json` has `kokoro_voice: af_heart` (the NEXUS twin). Three records, three answers: cloud=Ava, settings offline=George, installed offline=Heart. Another session switched JARVIS <-> NEXUS and the pieces did not move together. | files above | **Confirmed** |
| 4 | **Local twin is a different model** | Edge "Ava/Ryan Neural" is Microsoft's proprietary model; Kokoro `af_heart`/`bm_george` is a different model. Even perfectly configured, the offline voice is a *lookalike*, never a copy. | Fact |

**So: a carbon copy is impossible while the cloud voice and the offline voice are two different models.** Items 1-3 are fixable bugs; item 4 is structural.

## 2. The only way to get an exact copy

Same model + same voice vector on both paths. Options, honestly ranked:

| Option | Exact copy? | Offline? | Latency | Cost / risk |
|---|---|---|---|---|
| **A. Kokoro everywhere** (one engine). Online: Kokoro running on a **GPU you control or a hosted Kokoro API**; offline: the SAME Kokoro + the SAME `.bin` voice on the laptop CPU. | **Yes** (same weights; only floating-point noise differs) | Yes, fully | Cloud-GPU: faster than Edge. CPU offline: ~real-time (measured RTF 0.7-1.7 earlier, so a 2 s sentence takes 1.4-3.4 s) | Needs a host (your 24/7 server in docs/features/121 has a 12 GB GPU; Kokoro-82M needs < 1 GB). Hosted APIs exist but their price/terms are **unverified**. |
| **B. Edge only, no offline voice** for new text. Offline: pre-rendered Edge clips (large phrase bank) + caption + chime; dynamic replies are text-only until the network returns. | **Yes, always the same voice** | Degraded (silent for new sentences) | Edge as today (~1.1-1.3 s) | Zero new infrastructure. Offline is honest-but-quiet. |
| C. Clone Edge's voice into a local model (fine-tune on Edge output). | Close, not exact | Yes | Local | Microsoft's terms on using Edge TTS output as training data are doubtful; large effort; **not recommended**. |
| D. Keep today's pair and EQ/level-match them. | No | Yes | — | Reduces but never removes the difference. Rejected by the directive. |

**Recommendation: A, in two stages.** Stage 1 (no server): Kokoro is the ONLY audible engine, locally, with speed work (below) and a big pre-rendered stock-line bank, so there is no engine switch at all. Stage 2 (when the server exists): add the GPU-hosted Kokoro as the fast path using the same voice file; the local one stays as the offline twin. Edge becomes an optional "premium voice" the user must choose knowing it has no identical offline twin (UI says so). Option B is the zero-risk fallback if Stage 1's measured speed is not acceptable.

## 3. Plan

**V0 — Measure before choosing (0.5 day).** On this laptop: Kokoro first-audio latency and RTF for 1-, 3- and 8-word sentences with the current fp32 model, the q8/fp16 variants (`docs` say "not run"), 4 vs 8 ORT threads, and DirectML on the Intel iGPU if it loads. Gate: first audio <= 1.2 s for a typical 6-word sentence, or Stage 1 is rejected in favour of Option B / hosted Kokoro.

**V1 — Single voice identity (0.5 day, do first, independent of A/B).** One record `VoiceIdentity {persona_key, cloud_id, kokoro_voice, prosody}` derived ONLY from `voice_catalog` by `persona_key`; settings.json stores just the key. Startup self-check logs `[VOICE] persona=nexus cloud=en-US-AvaNeural local=af_heart installed=af_heart OK` and **repairs or says out loud** any mismatch (like today's bm_george vs af_heart). Cache files are keyed engine + voice + prosody; a persona change purges the others.

**V2 — Fixed delivery (0.25 day).** `ttsEmotion` default becomes **off**: every line uses the persona's own fixed rate/pitch. Auto-emotion stays only as an explicit opt-in. Independently fix `pick_emotion` to match whole words ("Unstop" must not be "stop"). Test: the same text through the boot sequence yields identical prosody parameters.

**V3 — Remove every audible engine switch (0.5 day).**
* Delete the Windows-voice path from the audible chain (`tts_system.rs` stays only as an emergency "something is wrong" beep-with-caption if explicitly enabled; default off).
* No mid-utterance fallback (undo my 2026-10-16 change in `speak_streaming`): if the engine fails mid-reply, retry the same engine once, then finish the reply as caption-only with a short chime. A silent tail is better than a different person.
* Per-utterance rule becomes per-**session** rule: the engine is chosen once at start-up/network change, announced in the log, and never alternates sentence to sentence.
* Stock-line cache holds only the active engine's renders; Edge-rendered clips never mix with Kokoro clips.

**V4 — Stage 1: Kokoro everywhere (2-3 days, after V0 passes).** Speed work from V0, a bigger stock-line bank (the 29 lines + the 150 most frequent replies/acks, rendered once and persisted), sentence-level streaming with the first sentence short, and the Settings UI wording "One voice, online and offline".

**V5 — Stage 2: shared GPU Kokoro (1-2 days, needs the server decision).** A small HTTP endpoint (Kokoro-FastAPI style) on the user's server; same `kokoro_voice` id and voice vector; request signed by the existing device token; the laptop falls back to local Kokoro with **bit-compatible settings** (same speed, same phonemizer `misaki`, same sample rate 24 kHz) when unreachable.

**V6 — Prove it is the same voice (1 day).** Objective + human gate:
* Objective: render 30 fixed sentences on both paths; compute median F0, F0 range, spectral centroid / MFCC distance and a speaker-embedding cosine similarity (a small Python script, `scripts/voice_twin_score.py`); target cosine >= 0.97 and |median F0 diff| <= 3 Hz (same-model renders should be near-identical).
* Human: a blind ABX of 20 sentence pairs; the user must not beat chance (<= 12/20).

**V7 — Observability.** Every utterance logs `[VOICE] engine=kokoro-gpu|kokoro-local persona=... voice=... prosody=fixed words=.. first_audio=..ms` so any future "different voice" report names its cause in one line.

## 4. Decisions needed from the user
1. Is **Kokoro as the one voice** acceptable (it will sound like `af_heart` / `bm_george`, not exactly Microsoft's Ava/Ryan), in exchange for a true cloud/local match? If you want to KEEP the Microsoft voice as the only voice, we choose Option B (no offline speech for new sentences).
2. Which persona is THE voice: **NEXUS (af_heart)** or **JARVIS (bm_george)**? (Both are in the catalog; the system currently disagrees with itself.)
3. Do you have / want the 24/7 GPU server for Stage 2 (docs/features/121)? If not, Stage 1 alone must pass V0.
4. If the engine fails mid-sentence, is "finish as caption + chime" acceptable instead of switching voice?

## 5. Not verified
Which three voices you heard is inferred: item 1 is proven by the log, item 2 by the earlier log, item 3 by the files; I cannot hear the audio. Kokoro latency on this laptop with quantised models / DirectML is **not measured** (V0). Hosted-Kokoro pricing and terms were not checked. The "same weights => same voice" claim assumes identical voice vector, speed, and phonemizer on both paths, which V6 verifies. Nothing here is built.
