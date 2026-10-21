# 136 — Voice, connectivity, questions and RAM: master compendium (research, decisions, implementation plan, problems faced)

Production distribution and low-memory release criteria are in [plan 137](137-free-open-source-low-memory-production-plan-2026-10-17.md). This plan refines the memory and free-use assumptions below; it does not mark any planned runtime work as complete.

Owner's provider-role correction: Groq handles online speech-to-text in NEXUS. Groq also offers a cloud TTS service, but that service is not selected for the production same-voice path. Gemini handles requested vision and analysis/research, including GitHub material retrieved through separate GitHub access. See the provider table and GitHub migration in plan 137.

Later owner correction (2026-10-17): Groq does offer TTS, but the overriding requirement is **one voice online and offline**. Plan 137 now makes a single locally runnable TTS voice the production default for every spoken line. The Edge-first voice policy below is historical and superseded for that profile; Groq remains the online STT route and Gemini the requested vision/analysis route.

Date: 2026-10-17. Purpose: one document the owner can review later. It consolidates everything researched, measured, decided and planned in the 2026-10-15 -> 2026-10-17 work on **voice (Edge/Kokoro), connectivity fallback, spoken notices, question/answer reliability, settings architecture and RAM**. Status legend: **DONE** (built and tested earlier), **PLAN** (decided, not built), **OPEN** (needs a decision or a measurement), **M** measured on this PC, **R** recorded earlier by another session, **E** estimate.

Related documents (full file names, because some numbers are used twice by different sessions):
`docs/features/123-boot-to-first-word-latency-analysis-and-plan.md`, `124-questions-keep-mic-open-and-follow-up-memory-audit-and-plan.md`, `125-response-delay-and-whatsapp-learning-analysis-and-plan.md`, `126-questions-answer-capture-and-question-memory-live-log-analysis-and-plan.md` (NOT the music/agent doc with the same number), `127-one-voice-everywhere-carbon-copy-tts-analysis-and-plan.md` (superseded), `128-exact-microsoft-voice-locally-and-settings-hub-architecture-audit-and-plan.md`, `129-free-open-source-one-voice-everywhere-research-and-plan.md` (superseded in part), `130-ram-usage-by-state-measured-and-estimated.md`, `131-voice-quality-variety-and-ram-reduction-plan-measured.md`, `132-ram-budget-600-700-mb-for-all-tasks-plan.md`, `133-coordination-with-ghost-no-vision-plan-impact-and-file-ownership.md`, `134-edge-first-local-standby-connectivity-and-latency-lanes-plan.md`, `135-smart-fallback-timing-and-spoken-connection-notices-plan.md`, and `docs/changes/130-voice-never-silent-stack-and-whatsapp-number-route.md`.

---

## 1. The final product contract (what the owner decided)
1. **Conversation must be fast with no perceptible delay; tasks may think and take time** (an immediate acknowledgement + one completion line, never dead air).
2. **Edge TTS (online) is the first priority.** The local voice is used **only when there is no internet**.
3. **Smart standby**: the local voice, local speech recognition and the local command model are kept warm when the link looks unstable; unloaded after 30 minutes of proven stability.
4. **Switching back to online only when the connection is proven** (OS says connected + 3 probes >= 10 s apart + a one-word canary through Edge < 1.5 s + >= 60 s), and only between utterances; the local engine stays loaded for a 10-minute relapse window.
5. **Spoken connection notices are reactive only**: NEXUS says "give me a minute, I can't reach the server" (or "slow connection") **only when the user tries to reach it** while offline/weak. "I'm back, sir" (local ready) and "I'm back online, sir" are spoken **only if the user had called during that outage**. No unprompted "I'm offline".
6. Notices are varied (never repeat any of the last 4), **not shorter on repeats, just different**; from the **4th call** in one offline episode the category STILL_OFFLINE is used ("Still the same, sir. I'll let you know when I'm back.").
7. **Free, open-source friendly**: users bring their own Groq/Gemini keys (LLM + STT); no paid TTS account, no Docker, no card required.
8. Keep every feature; the memory target (< 700 MB, ideally 600-650) is a goal, not a hard-coded cap; instant fallback has priority over the all-features-on ceiling.
9. Do not touch GPT-sol's Ghost "no-vision" work; share files only through narrow anchored hooks.

## 2. Timeline of problems, evidence and what was done
| # | When | Symptom (evidence) | Root cause | Result |
|---|---|---|---|---|
| 1 | 10-15 | 4-5 s before "Ok sir" (user log) | cloud voice crate was stale (HTTP 403), every line paid the Edge timeout; cache staleness test asked "is bing.com reachable" | **DONE** crate 0.1.6 + explicit MP3 output format, circuit breaker (`tts_health.rs`), cache staleness by real success, live probe OK |
| 2 | 10-15 | "okay so what is the weather" would have confirmed a pending send/merge | loose yes/no regex with no end anchor | **DONE** strict whole-sentence answer classifier (frontend `confirmReply.ts`, Rust `offer.rs` ANSWER_VOCAB), 2 min TTL |
| 3 | 10-15 | WhatsApp chat opening took ~57 s and typed nothing useful | UI Automation cannot read WhatsApp's chat list; polling loop counted polls, not time | **DONE** 9 s total budget, blind-UIA detection + session breaker, keyboard route with OCR header verification, alias learning, phone-number deep link + `.vcf` contact import |
| 4 | 10-15 | Boot greeting up to 160 s late or never | fixed 45 s sleep, idle-band presence rule, health check before the hello | **DONE** stage-ready gating, hello on every logon (3 time-of-day lines), details in parallel, autostart task off the setup path + priority 4, stage created first, resume-from-sleep hello |
| 5 | 10-15 | "shut down my laptop" asked a question, never closed apps | old flow | **DONE** "Good night, sir" flow: graceful `WM_CLOSE` close, stops and names an app that asks to save, 3 s cancel window |
| 6 | 10-15 17:35 log | "send on / set down my laptop" went to the cloud; 429 storm at start-up; half-spoken answers | Whisper mishearing of "shut down"; 29 stock lines requested back-to-back; Edge failed mid-reply and the rest was dropped | **DONE** mishearing table (computer-only), paced pre-render + stop at first 429, mid-reply fallback (**to be removed** by the new policy, see 7) |
| 7 | 10-16 | "three different voices" | (a) auto-emotion prosody per line (Cheerful +30 Hz, Urgent +20 Hz) - and "Team **Un**stop" contained "stop" -> Urgent; (b) my own Windows-voice safety net; (c) Ryan/Ava/af_heart/bm_george records drifting in settings | **PLAN** fixed prosody, whole-word emotion match (or none), remove the Windows voice from the audible chain |
| 8 | 10-16 15:55 log | "do it" -> no response | Whisper heard "Dread."; the first unreadable reply destroyed the pending question; the boot question was a 16 s monologue; nothing remembered the question | **PLAN** doc 126: answer-aware STT, keep/re-ask once, question memory by risk TTL, short question first |
| 9 | 10-16 | NEXUS persona chosen, JARVIS value came back | settings are one shared file; each Save writes the whole stale form; Command Hub mirrors only 2 of 5 voice fields (23:02:43 IPC write, 23:02:44 Save overwrote `offlineVoiceModel`) | **PLAN** doc 128 Part B: patch-based settings store, one voice fact, live events |
| 10 | 10-16 | "Is there a free exact copy of the Edge voice?" | see research 3.1-3.4 | decision: Edge stays primary; no exact free local copy exists |
| 11 | 10-16/17 | RAM: how much per state? | measurements in section 4 | **PLAN** budget + governor (doc 132) |
| 12 | 10-17 | "ONNX arena off saves 88 MB" | release timing: it makes synthesis ~34 % slower | **corrected**: not a default |
| 13 | 10-17 | Kokoro as live voice | 3-word line = 4.3 s on this laptop on battery (RTF 2.4) vs Edge ~1.1-1.3 s | **decision**: Edge first, Kokoro only as the offline/standby voice |
| 14 | 10-17 | GPT-sol's Ghost no-vision plan landed in the same tree | file overlap, shared question/settings systems | **DONE** doc 133 (ownership rules, requests to hand over) |

## 3. Research findings (with sources)
### 3.1 Can the exact Microsoft voice (Ava / Ryan) run locally?
* **Yes, as Microsoft's container, not as a file.** The registry lists `neural-text-to-speech:3.13.1-amd64-en-us-avaneural` and `...-en-gb-ryanneural` (34 Ryan versions). Docs: SSML behaviour is consistent between cloud and container.
* **Requirements**: Docker (Linux containers), AVX2, min 6 cores/12 GB (recommended 8/16), an Azure Speech resource (free **F0** allowed, 0.5 M characters/month), **always connected for metering**: reports every 10-15 min, stops serving after 10 failed attempts (~1.7-2.5 h offline grace).
* **Disconnected licence**: request form + approval ("strategic customer or partner") + yearly commitment plan -> unlikely for an individual.
* **Verdict**: rejected as a product component (every user would need an Azure account + Docker).
Sources: https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-container-ntts , https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-container-howto , https://learn.microsoft.com/en-us/azure/ai-services/containers/disconnected-containers , https://mcr.microsoft.com/v2/azure-cognitive-services/speechservices/neural-text-to-speech/tags/list
### 3.2 Windows Narrator "natural" voices
On-device **Ryan (UK male)** exists, **Ava does not**. Windows does not expose them to apps; the MIT `NaturalVoiceSAPIAdapter` hacks them in with keys extracted from system files and its own page warns it can break after any update and that newer Win11 voices reportedly stopped working. Rejected. Sources: https://support.microsoft.com/en-us/topic/appendix-a-supported-languages-and-voices-4486e345-7730-53da-fcfe-55cc64300f01 , https://github.com/gexgd0419/NaturalVoiceSAPIAdapter
### 3.3 Edge TTS (unofficial endpoint) legality and reliability
A Microsoft Q&A moderator said using it commercially without an Azure subscription may violate their terms (no legal advice); developers report the method being blocked (Dec 2025); we saw HTTP 403 for weeks (stale crate) and 429 on 2026-10-15. **Consequence**: fine as the owner's primary voice, but for an open-source release it should be clearly labelled unofficial and have a graceful offline path. Sources: https://learn.microsoft.com/en-us/answers/questions/2088770/are-opensource-edge-tts-free-for-commercial-use , https://learn.microsoft.com/en-us/answers/questions/2392491/unofficial-edge-tts-api
### 3.4 Other cloud TTS (all rejected as the default)
Google Cloud TTS (free: 4 M chars Standard, ~1 M Neural2/Studio; billing account probably required; different voice; no local twin). Groq Orpheus ($22/M chars; model-table limits 10 RPM / 100 RPD / 3.6 K tokens per day; PlayAI deprecated Jan 2026). Gemini TTS (free preview with low limits; model names changing). Cloudflare Workers AI MeloTTS (10 000 neurons/day shared by the whole account). Sources: https://cloud.google.com/text-to-speech/pricing , https://console.groq.com/docs/model/canopylabs/orpheus-v1-english , https://ai.google.dev/gemini-api/docs/pricing , https://developers.cloudflare.com/workers-ai/platform/pricing/
### 3.5 The open-source local voice (Kokoro-82M)
Apache-2.0 weights, 82 M parameters, trained on permissive/public-domain/synthetic audio, 54 voices in 9 languages. Voice grades (data quality): **af_heart A, af_bella A-, af_nicole B-, bf_emma B-**, most male voices C+/C/D; British males: **bm_george C, bm_fable C**, bm_lewis D+, bm_daniel D. KVoiceWalk (Apache-2.0) can search a custom voice tensor toward a target recording (one example 71 % -> ~93 % Resemblyzer similarity in 10 000 steps; ~4 h on a 3060, ~26 h on CPU; results vary). Piper is now GPL-3.0 with embedded espeak-ng (avoid). Kitten TTS small models are Apache-2.0 code; the 1.7 B v2 has a separate community licence. Sources: https://huggingface.co/hexgrad/Kokoro-82M , https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md , https://github.com/RobViren/kvoicewalk , https://github.com/KittenML/KittenTTS , https://github.com/OHF-Voice/piper1-gpl
### 3.6 ONNX Runtime memory options
`ort 2.0.0-rc.12` exposes CPU arena on/off, memory-pattern, intra threads, prepacking, `commit_from_memory_directly`/ORT-format in-place bytes; the CPU provider has **no** "grow by exact amount" strategy (only CUDA/CANN do). Source (secondary): https://www.mintlify.com/microsoft/onnxruntime/performance/memory-optimization
### 3.7 Offline speech recognition candidates
sherpa-onnx ships Moonshine-tiny (27 M params) and Whisper-tiny.en (39 M) in Q8; no public RAM figure found -> must be measured. Source: https://github.com/k2-fsa/sherpa-onnx

## 4. Measurements (all on this laptop: i7-1355U, 15.7 GB; M unless marked)
### 4.1 Idle process tree (release build, running for hours)
| Process | Commit MB | Working set MB |
|---|---|---|
| nexus.exe | 69.6 | 94.1 (private WS 16.5 after Windows trimmed it; peak WS 165.6) |
| WebView2 tree (browser 39/51, renderer 52/46, GPU 83/12, 2 utility 20/25, crashpad 3/4) | 197.3 | 138.4 |
| **Total** | **268.9** | **235.6** |
### 4.2 Kokoro (int8 `model_quantized.onnx`, 88.1 MB file)
| Stage | WS MB (default settings) |
|---|---|
| harness baseline | ~15 |
| + pronunciation dictionary (misaki-rs parses ~14 MB JSON into hash maps) | 139-153 (**+~125**) |
| + ONNX session loaded | 275-288 (**+135**) |
| after first synthesis | 339-350 (**+64**) |
| after 5 syntheses | 368-379 |
| after `unload()` | 59-75 (**~300 MB returned**) |
| NLU (tract BERT-mini): load + first parse | +42 MB, 662 ms first parse, ~37 ms afterwards, only ~23 MB returned on drop |
### 4.3 Speed (RELEASE test build, laptop on battery 14 %, Balanced, 1.7 GHz, ~34 % background load = pessimistic)
| ONNX setting | RTF (synthesis time / audio time) | 3-word line (2.4 s audio) | WS after 5 syntheses |
|---|---|---|---|
| default | **2.40** | **4.3 s** | 379 MB |
| arena + memory-pattern off | 3.21 (**+34 % slower**) | 7.6 s | 277 MB |
| 4 threads, arena off | 3.35 | 7.3 s | 276 MB |
| 2 threads, arena off | 3.74 | 8.3 s | 275 MB |
Earlier recorded figure (probably on AC): RTF 0.7-1.7 (R). Plugged-in re-measurement is still **OPEN**.
### 4.4 Latency of one cloud reply (from the owner's 15:55 log)
End of speech 19.68 -> transcript 20.00 (STT 0.32 s) -> parse 20.29 (0.29 s, first-turn NLU load) -> cloud answer 21.02 (0.61 s) -> Edge synthesis done 22.31 (1.28 s) -> first audio ~22.32. **~2.6 s total.** The ack "Working on it sir." was synthesized for 0.84 s and then discarded because the answer arrived first.
### 4.5 Test status of code changes made (before this documentation round)
Rust lib 1558 passed (12 ignored), e2e 2, offline 10, user commands 7; vitest 311; `tsc` and `npm run build` clean; dead-code ratchet 156 with none new; turn-end and hygiene gates OK; IPC gate fails only on another session's `agent_watch_*`. `turn_detect::model_loads_and_runs` is a known timing flake under load.

## 5. Implementation plan (consolidated, ordered)
Ownership: ours = `tts*.rs`, `tts_network.rs`, `lazy_stt.rs`, `voice_catalog.rs`, `nlu_local.rs`, `memcore/offer.rs`, `memcore/briefing.rs`, `conversation.rs`, `proactive_policy.rs`, `power_*.rs`, new `connectivity.rs`, `connection_voice.rs`, `ram_budget.rs`; shared (one-line anchored hooks only) = `orchestrator.rs`, `lib.rs`, `commands.rs`, `center.rs`, `intent_parser.rs`, `stt_groq.rs`, `wakeword_oww.rs`, `frontend/src/net/orchestrator.ts`, `AGENTS.md`; **GPT-sol's files are never edited** (`ghost_center.rs`, `ghost_learning.rs`, `ghost.rs`, `mouse.rs`, `shortcuts/*`, `screen.rs`, `ghostHotMic.ts`).
| Phase | Work | Days | Gate |
|---|---|---|---|
| **P0** | per-turn `[LAT]` stage timing + `[NET]` state log + `[RAM]` sampler; 20-turn baseline; plug-in re-measure of Kokoro; measure hand mode / Hub / ghost / agent-watch / offline STT (needs the owner ~10 min at the PC) | 1 | every row of the RAM table is M, baseline latency table exists |
| **P1** | speed-neutral wins: stock acks ("Working on it sir" etc.), wake-time pre-warm (NLU + Edge connection), settings hot-fix (hub mirrors all voice fields, theme toggle sends one field) | 1-1.5 | ack heard <= 0.3 s; first-turn parse ~0; regression test replays the 23:02:43/44 sequence |
| **P2** | connectivity state machine (OS events + request outcomes + adaptive probes), hedge at `max(2.0 s, 1.5 x p95)`, canary recovery, anti-flap, utterance-boundary switching | 2-3 | 30-min chaos test: no mid-utterance switch, no flapping |
| **P2b** | `connection_voice.rs`: reactive notices only, categories OFFLINE_ON_CALL / LOW_CONNECTION_ON_CALL / LOCAL_READY / BACK_ONLINE / STILL_OFFLINE (from call 4) / NEEDS_INTERNET, >= 12 variants each, no repeat in the last 4, "I'm back" only if the user called during the outage | 1.5 | unit tests: variety, 4th-call rule, silence when the user never called, quiet in meetings |
| **P3** | learned bank (Edge MP3 + word boundaries on disk, LRU 200 MB), paced pre-render (stock lines, notices x persona x user name, Ghost receipt phrases read from GPT-sol's strings, usage-ranked "Opening {app}, sir."), coverage metric | 1.5-2 | banked offline line <= 50 ms in the Edge voice; coverage >= 80 % of local-command acks |
| **P4** | Smart standby for local voice + offline STT (warm on triggers, cold after 30 min stable, dummy warm-up synthesis), call-time notice hook (<= 0.3 s after the wake), remove the Windows voice from the chain | 2-3 | warm within 3 s of a trigger; offline first transcript <= 1.5 s (needs the in-process STT decision) |
| **P5** | RAM work that is speed-neutral: compact pronunciation dictionary (-100..-115 MB, golden-corpus equality test), NLU idle unload with wake pre-warm, orb GPU context release, stock-cache i16, Node heap cap; ORT-format model (if measured) | 3 | each saving measured, speed within +10 % of baseline |
| **P6** | `ram_budget.rs` governor (soft 600 / hard configurable, eviction order NLU -> hub/hand extras -> GPU -> standby last), Hub "Memory" card, profiles Low / Balanced / Generous | 2 | forced all-on test never exceeds the chosen ceiling for 10 min |
| **P7** | questions/answers (doc 126): answer-aware STT prompt, keep/re-ask once instead of dropping, `last_question` memory by risk TTL, question-first boot offer (<= 8 s), `tts_playing` race fix, `[ASK]` logs | 3-4 | scenarios 1-8 of doc 126 pass |
| **P8** | settings store (doc 128 S1-S5): patch-based save, one voice fact, `settings:changed` events | 2 | two-writer test; unknown fields preserved |
| **P9** | streaming the cloud answer into speech by sentence, short first chunk, persistent Edge WebSocket (touches shared `orchestrator.rs`/`router.rs`) | 2 | reply first-audio p50 <= 1.8 s |
| **P10** | fixed prosody (no auto-emotion; whole-word match if kept), voice-fingerprint test, ratchet baselines (`scripts/baselines/ram-budget.json`, ignored memory tests in CI) | 1 | prosody parameters identical across the boot sequence |
Approximate total 20-25 working days; **P0-P4 (about 9-11 days)** deliver the behaviour the owner asked for.

## 6. Challenges and problems faced (and how each was handled)
1. **Two assistants editing the same tree** (this one and GPT-sol): earlier a stale write dropped edits. Handling: baseline `cargo check`, anchored one-line hooks, file ownership table (doc 133), never `git checkout/restore`.
2. **Hand-written patches in shells with `'''`**: heredoc corruption. Handling: write patch scripts with the file tool and verify each assertion.
3. **Edge reliability**: 403 (stale crate), 429 (burst), "no audio was received", timeouts. Handling: crate upgrade + MP3 format, breaker, pacing, adaptive hedge (planned).
4. **Quality vs speed on a small CPU**: Kokoro quality is good (A-grade voices) but slow on battery; British male grade is only C. Handling: Kokoro demoted to offline/standby voice; Edge voice kept in a bank for repeated lines.
5. **Memory measurement pitfalls**: Windows trims idle working sets (private WS 16 MB vs commit 70 MB); `sysinfo::virtual_memory` is virtual size, not commit; unit-test processes add ~15 MB; debug vs release; battery vs AC. Handling: always report WS + commit, label every row M/R/E.
6. **A "free" saving that wasn't**: arena-off cut 88 MB but cost 34 % speed. Handling: measured before adopting; removed from defaults.
7. **Settings drift** from several windows saving whole snapshots (doc 128).
8. **Question handling** built from three separate mechanisms (offers, frontend confirmations, clarifications) with a strict classifier that cannot cope with one mis-transcription (doc 126).
9. **Offline is bigger than TTS**: with no internet there is no cloud LLM and no Groq STT; local STT cold start 10-15 s (and `lazy_stt.rs` now kills it after 60 s). Handling: standby includes STT; the notice plays before any transcript exists.
10. **Conflicting requirements over time**: exact voice copy vs free vs fast vs < 700 MB. Resolved by the owner's policy (Edge first, Smart standby, reactive notices) in section 1.
11. **Spurious `tts-ended`** (frontend `stopTts()` emits an extra ended before started): the Rust listener lacks the generation guard the frontend has, so `tts_playing` goes false while speaking (wake un-muted, shutdown wait wrong). Planned fix in P7.
12. **Duplicate doc numbers** (two 126, two 129) from parallel sessions: use full file names.

## 7. Open items and decisions still pending
* Plug-in (AC) Kokoro speed; offline STT engine choice and RAM; hand mode / Hub / ghost / agent-watch measurements.
* Whether the all-on RAM ceiling should be 700, 850 or "Generous" given Smart standby (idle ~280 MB, ~690 MB warm).
* Wording of notices is drafted in doc 135; the owner chose not to review before building.
* Windows connectivity-event behaviour on the owner's Wi-Fi/ISP, WLAN thresholds and captive-portal detection need live tests.
* Microsoft's terms about training on TTS output (relevant only to a future custom-voice idea).
* The Windows-voice safety net and the mid-reply voice fallback added on 2026-10-15 conflict with the new policy and are scheduled for removal (P4).

## 8. What exists in code today from this work (for orientation)
DONE and tested: `tts_health.rs`, `tts_wav.rs`, `tts_system.rs` (to be removed from the chain), `tts_cache_disk.rs`, Edge crate 0.1.6 + MP3 format, paced stock-line pre-render, strict answer classifiers, `contacts_book.rs` + `wa_open.rs` rework, `power_cmd.rs`/`power_close.rs` shutdown flow, boot hello + resume hello, local-turn recording and "Do what, sir?", risk-based offer TTLs, autostart off the setup path. Dev-only (ignored) tests added for measurement: `tts_kokoro::tests::mem_breakdown`, `nlu_local::tests::mem_nlu_load_parse_unload`. Everything in sections 5 (P0-P10) is **PLAN**.

---
## STATUS UPDATE (2026-10-17)
P0 `[LAT]` per-turn logging and P1 (stock acks, wake-time NLU pre-warm with race-free load, derived voice fields, `settings_patch`, hub theme/voice fixes) are **built and unit-tested**; record: `docs/changes/131-latency-clock-stock-acks-nlu-prewarm-and-voice-settings-consistency.md`. **Not built:** Edge connection pre-warm (a separate connection would not be reused; planned with P9 persistent connection), `[NET]`/`[RAM]` samplers, and phases P2-P10. Not run live: needs `nexus build` + `nexus start`.

---
## OWNER DECISION (2026-10-17, after reviewing GPT plan 137) — binding
* **Response speed is the top priority.** Edge TTS stays the online voice; local Kokoro is used only when there is no internet (only then is slower speech accepted). The **Smart plan (134/135/136) stays.**
* Plan 137's default (one local voice for every turn, engine kept resident for a carbon-copy guarantee) is **rejected**. From 137 we keep only: the free-provider contract (own Groq/Gemini keys, free-only mode, quota/Retry-After handling), a root LICENSE + asset inventory, moving GitHub retrieval off the hosted Worker, and whole-tree RAM gates measured on real devices.
* Exact carbon-copy online/offline is not possible for free (Edge voices cannot be downloaded). Offline parity is best effort: the pre-rendered Edge-voice bank gives the exact voice for banked lines; Kokoro twin otherwise (audition of twins still pending).
* The user's own voice-selection plan is Feature 83: background download, old voice deleted only after the new one verifies (already built). Follow-ups: apply swaps at the next utterance boundary, atomic manifest write + startup file/manifest re-check.
* **Speed-first build order:** persistent Edge connection + short first chunk + streaming answer into speech (P9) and the stock-line bank (P3) move ahead of the RAM governor (P6); P2 connectivity state machine stays next for the offline switch.
* **Never always-loaded (owner, 2026-10-17):** the local voice, offline STT and NLU are NOT kept resident "just in case". They load only on Smart triggers (OS network event, weak signal, failed/slow request, probe loss, first 2 min after start/resume, 10 min after a recovery, learned outage hours) and unload after 30 min of stable connection. The earlier "keep the engine loaded during use" suggestion is withdrawn.
