# 137 — Free, open-source, low-memory NEXUS: production plan

Date: 2026-10-17. Status: plan and release criteria; no runtime behavior changed.

Owner's voice correction: the speaker must sound the same online and offline. Groq does offer a cloud TTS API, but it is not the default speaking route because its cloud voices would differ from the local fallback. Groq remains the online speech-to-text provider; Gemini handles requested vision and analysis/research, including GitHub material. NEXUS should synthesize all replies with one selected offline-capable voice engine, regardless of connectivity. This replaces the Edge-first speaking policy in plan 136 for the production same-voice profile.

## 1. The owner's actual requirement

NEXUS must be free to obtain and run, with no NEXUS subscription, mandatory payment card, hosted NEXUS inference bill, Docker requirement, or hidden paid dependency. Users supply their own Gemini and Groq API keys for the cloud features they choose. The default setup must stay within provider free allowances when those allowances are available. A user with a paid provider account must control that account's charges; NEXUS must never silently select a paid-only model or enable paid usage.

It must also be comfortable on ordinary 4 GB and 8 GB laptops while the owner runs a browser, editor, and other applications. A small idle number is insufficient if the first spoken turn makes the process tree approach 1 GB. The memory budget covers NEXUS plus all WebView2 children and sidecars, including offline voice, offline speech recognition, hand mode, Ghost, Hub, and agent watcher when active. Working set, private bytes/commit, peaks, and the time at each level must all be reported.

Every feature remains available. Heavy features may load on demand or unload when idle; this can add an explicitly measured first-use delay. Since speech uses one local voice in both states, losing the network no longer triggers a voice-engine change. Offline speech recognition and cloud analysis still change availability. “Instant first speech” and “everything unloaded for low RAM” are competing requirements until a smaller local voice is measured. Do not promise both from an estimate.

## 2. What the present evidence says

The project measured about 269 MB commit and 236 MB working set for the idle process tree on one 16 GB Windows laptop. The current Kokoro path adds about 368 MB working set when active. The local NLU added 42 MB in a test process. Offline Python STT has only an older footprint estimate of roughly 130 MB loaded or 340 MB active. Hand mode, Hub windows, Ghost, and watcher overlap are not yet measured together. Sources: [RAM measurement 130](130-ram-usage-by-state-measured-and-estimated.md), [RAM plan 132](132-ram-budget-600-700-mb-for-all-tasks-plan.md), and [compendium 136](136-voice-connectivity-and-ram-master-compendium.md).

Thus the currently measured idle plus voice is already roughly 600 MB resident before local STT or optional windows. The old “always ready” projection around 690 MB is an estimate and does not prove a production ceiling. The 4.3-second short-line and RTF 2.40 figures in compendium 136 do not arithmetically agree for a stated 2.4-second audio line; check the raw release measurement before using either as a gate.

The code currently has a 4-second first Edge wait, a Windows-voice emergency fallback, a lazy Python offline STT server, and a hard-coded “2,000 req/day free tier” error message in the Groq STT path. The Edge and Windows paths must leave the default audible chain for a consistent local voice. These are release review items, not proof of a stable free service. The repository search found no root LICENSE file as of this plan. A public “open source” release requires an explicit project license and dependency/model asset inventory.

## 3. Free-use contract and provider behavior

### Provider responsibilities

| Need | Default route | Account or consent |
|---|---|---|
| Hear the user online | Groq transcription | User's Groq key; local STT only as an optional offline pack. |
| Speak to the user | One selected local TTS engine and voice in both online and offline states | No Edge/Windows/Groq voice substitution during normal operation. The selected voice is installed locally and used for every novel utterance. |
| Understand an image or screen on explicit request | Gemini multimodal model | User's Gemini key and explicit screen/image permission. Simple Ghost keyboard/UIA/cursor actions stay local without vision-model calls. |
| Analyze or research a repository, issue, or pull request | Local GitHub retrieval and context selection, followed by Gemini analysis | Public GitHub material can be read without a private-repo token where supported; private material needs separately authorized GitHub access and clear consent before selected code is sent to Gemini. |
| Read a normal answer or research request | Gemini where a cloud model is needed | User's Gemini key, a compatible free-tier model, and bounded context. Deterministic local commands do not need a model. |

Gemini's image-understanding API accepts image input, but the Gemini key does not grant access to a GitHub account. GitHub is the source of repository data and has its own permissions and rate limits. Sources: [Gemini image understanding](https://ai.google.dev/gemini-api/docs/image-understanding), [GitHub repository API](https://docs.github.com/en/rest/repos/repos), and [GitHub App device flow](https://docs.github.com/en/apps/creating-github-apps/writing-code-for-a-github-app/building-a-cli-with-a-github-app).

“Free application” and “unlimited free cloud inference” are different claims. Gemini lists free-tier access for some models, and Groq publishes free-plan limits, but available models, account limits, geography, and quotas can change. Gemini says its published limits are not guaranteed. The product must say “NEXUS itself has no fee; cloud features use your own provider account and are subject to its terms and limits.” [Google pricing](https://ai.google.dev/gemini-api/docs/pricing), [Google rate limits](https://ai.google.dev/gemini-api/docs/rate-limits), [Groq rate limits](https://console.groq.com/docs/rate-limits).

Release requirements:

1. Offer Free-only mode by default. Permit only models and features currently shown as available to that user's free account. Fetch account/model capability where the provider permits it; keep a conservative local allowlist as a fallback and update it with releases. Never infer “free” from a model name alone.
2. Let users enter Gemini and Groq keys separately, test them, and see which feature uses each one. Store keys in the OS credential vault; remove plaintext settings fallbacks after migration. Export, diagnostics, logs, and crash reports must not include keys or private transcripts.
3. Classify provider responses as success, invalid key, quota exhausted, throttled, service unavailable, or network offline. Respect Retry-After and rate-limit headers; do not retry a 429 in a loop. Show a useful, nontechnical status and keep local commands available.
4. Allow paid models or usage only through a separate explicit user choice, with the provider's account/billing status shown. NEXUS itself cannot guarantee zero charges on a paid provider account. No silent failover to a paid route.
5. Run a no-key mode for local desktop commands and text/manual control where possible; explain which conversational and transcription features need a key or an optional local model.

GitHub analysis needs a separate production migration. The current PR analysis uses a hosted Worker and Cloudflare model, the general router excludes PR/repository analysis from its direct providers, and typed GitHub commands fetch a token from that Worker. For a no-required-hosted-service release, move read-only public-repo retrieval into the desktop client; support private repos through a locally stored, least-privilege GitHub credential or GitHub App device flow. Fetch only the selected repository/PR files and diffs, redact secrets, cap context and request count, and ask before sending private code to Gemini. Keep GitHub writes in the existing explicit authorization/confirmation flow. Do not treat a Gemini key as a GitHub credential. Sources in this repository: [GitHub PR analysis](01-github-pr-analysis.md), [GitHub commands](../../src-tauri/src/github_cmd.rs), and [general router](../../src-tauri/src/router.rs).

Groq offers Orpheus cloud TTS, but its online voices would not preserve the chosen local Kokoro/other voice across a network outage. Edge Ryan/Ava also cannot be treated as exact local twins of Kokoro voices. A cache of Edge or Groq phrases covers only previously generated phrases and cannot preserve a novel reply. The production same-voice profile therefore uses a locally runnable TTS voice for all speech, even while Gemini and Groq provide online intelligence and transcription. Kokoro's model card identifies Apache-2.0 weights; a smaller Apache-2.0 Kitten TTS Nano model is another candidate whose model file is about 24 MB, but neither file size nor license proves output quality, speed, or runtime RAM. Benchmark both in the actual NEXUS process before choosing. Audit the runtime, phonemizer, model, voice and attribution chain before distribution. Sources: [Groq TTS](https://console.groq.com/docs/text-to-speech/), [Groq free-plan limits](https://console.groq.com/docs/rate-limits), [Kokoro model card](https://huggingface.co/hexgrad/Kokoro-82M), [Kitten TTS Nano model](https://huggingface.co/KittenML/kitten-tts-nano-0.1). Keeping an Edge-voice legacy option would need a clear notice that it cannot meet the same-voice requirement offline; it is not the default.

## 4. Proposed measurable memory contract

These are release targets, not current results. Measure on both a 4 GB low-end Windows laptop and an 8 GB laptop, in release builds after five minutes of use. Record whole-tree working set and private bytes every second, including peaks and p95. No claim passes using Task Manager's trimmed idle private working set alone.

| State | Proposed whole-tree working-set target | Gate |
|---|---:|---|
| Idle, orb and wake word ready | at most 300 MB | 30 minutes, stable after startup |
| Typical online talk / local command / Ghost, with the chosen local voice active | at most 550 MB sustained | 30 mixed turns; identical voice online/offline; no steady upward trend |
| Offline turn with voice and recognition | at most 650 MB sustained | 20 turns, including first-use and recovery |
| Short overlap of optional heavy features | below 800 MB peak, with measured return to baseline | Hand, Hub, watcher and Ghost combinations; no ignored child process |

If the 4 GB machine cannot meet these numbers without unacceptable responsiveness, ship a clearly labeled Low Memory default for 4 GB systems with lower targets and explain first-use delays. Do not relax the whole-tree measurement or present a low idle figure as full-task usage. The production release must not claim a universal 700 MB cap until all active-state combinations are measured on target hardware.

## 5. Architecture for the default low-memory profile

1. Keep the baseline process tree lean. Inspect WebView2 browser, renderer, GPU, utility, extra windows and sidecars separately. Open Hub and settings views on demand and release their resources after close. Do not add another resident process for ordinary cloud turns.
2. Keep Groq STT and Gemini analysis requests stateless where possible. Reuse HTTP connections, not heavyweight local models. Keep deterministic commands ready. Load the 42 MB local NLU only for a grammar miss, pre-warm on a wake only when memory headroom permits, and unload after idle if measurement shows a real saving.
3. Select one local voice ID for the entire application. Generate boot lines, acknowledgements, questions, task receipts and novel answers from that voice, with fixed prosody. Pre-render common phrases from the same local engine and bound in-memory/disk caches; voice or prosody changes invalidate the cache. Voice previews must identify the installed local voice honestly. Remove “carbon-copy” wording for Edge/Kokoro pairs because a different model does not provide identical audio.
4. Benchmark the present Kokoro path against smaller locally runnable, permissively licensed candidates such as Kitten TTS Nano in release builds on 4 GB and 8 GB PCs. Compare blind-listening quality, pronunciation, persona consistency, first-audio p50/p95, CPU/battery use, model download size and whole-tree resident/commit memory. For Kokoro, first compact the large pronunciation dictionary with exact-output tests, then try memory-mapped/ORT-format loading. Reject optimizations that materially slow first audio; the measured CPU-arena-off option saved memory but made synthesis about 34% slower on the test laptop. Keep the selected engine warm during an active conversation only when headroom permits; unload after idle and cover cold start with clips generated by that same voice.
5. Replace mandatory Python-based offline STT with a packaged, optional in-process model only after its accuracy, language coverage, startup time, license, and full resident footprint are measured. Groq STT is the normal online path. The local model downloads on request or as an explicit offline pack; it is not silently installed or permanently resident. Local voice and local STT may alternate residency when an active session needs both, provided the actual delay is shown in the acceptance test.
6. Use pressure-aware admission: measure available system memory and NEXUS's whole tree before loading a heavy component. Evict idle caches/models first and pause optional background work. Do not kill an active Ghost action, recording, speech, hand tracking, or user-requested watcher to hit an arbitrary number. If memory is insufficient, report the limitation and offer a reversible choice.
7. Smart standby now applies mainly to offline STT and the local command model. The local TTS is the same online and offline, so no network recovery proof is needed to change the speaker. Warm the selected TTS during an active conversation when the measured budget permits; unload it after idle on low-memory machines. Tune the hold using measured first-speech delay, battery use and RAM. Connection recovery still governs Groq/Gemini requests and reactive notices.

The default therefore keeps ordinary online use small, while offline packs and simultaneous heavy features remain functional with measured load/unload delays. A “ready now” profile may keep models warm for users who explicitly value instant fallback over memory.

## 6. Implementation order and release gates

| Phase | Work | Evidence required |
|---|---|---|
| P0: legal and baseline audit | Add root license proposal, inventory code/model/voice/phonemizer licenses, list every external endpoint, measure whole-tree commit/working set on 4 GB and 8 GB devices, and benchmark candidate local voices. Correct inconsistent Kokoro timing. | Published dependency/asset ledger; reproducible memory, latency and voice-quality comparison; one chosen local voice for release. |
| P1: free-only provider contract | Label Groq STT and Gemini analysis/vision separately in onboarding; vault-only storage, model availability check, no paid auto-route, quota and Retry-After handling, clear no-key behavior. Replace hard-coded Groq free-tier wording. Remove Groq-first research routing from the production default. | Fresh free accounts complete supported tasks with no payment method where providers allow; 401/429/offline tests; secret-leak tests; provider-route tests; no cloud TTS in the default speech path. |
| P2: low-memory foundation | Implement the selected single-voice path, compact dictionary where relevant, same-voice bounded audio cache, idle model unload, window/sidecar lifetime audit, and live whole-tree memory meter. Test one optimization at a time; keep CPU arena defaults unless a speed-neutral result is measured. | Online/offline A/B trials use the same model, voice asset and prosody settings, with no detectable provider-switch change in speaker identity; memory and first-audio gates pass in release builds. |
| P3: local resilience | Package optional local STT, test its license and multilingual coverage, integrate voice/STT residency scheduling, connection proof for online requests and reactive notices. | Offline first and repeated turns work with no Python install; speech keeps the same local voice through connection loss; memory and latency meet the declared profile. |
| P4: user-facing speed and reliability | Banked acks, streaming first answer sentence, question retention, settings patch-write fix, stable voice selection, and local GitHub retrieval feeding bounded Gemini analysis with file/line citations. | Real turn traces; no lost question, overwritten setting, silent turn or inconsistent voice; public/private GitHub tests with no required Worker and no private-code transfer before consent. |
| P5: release qualification | Install/uninstall, 4 GB/8 GB soak tests, feature-combination tests, cold-start/offline/429 scenarios, accessibility, crash recovery, privacy review and package-size audit. | All published gates pass on clean machines; unsupported cases are documented. |

No phase is “done” because a unit test passes. The release gate needs real-device p50/p95 latency, peak and sustained memory, supported-task completion, failure recovery, and reproducible install steps. Ship Windows first if that is the only platform measured; describe macOS/Linux support by feature rather than claiming parity from shared code.

## 7. Immediate decisions resolved by this plan

- Default: Free-only, low-memory, one selected local speaking voice in every network state, Groq STT online, Gemini for requested vision and analysis/research, and local deterministic control for simple tasks. Groq TTS exists but is not selected for the same-voice profile.
- Never require an unofficial TTS endpoint, a paid cloud account, Python, or Docker for basic installation and local commands.
- No fixed 1 GB allocation. Heavy models are loaded only when needed, and the whole-tree budget includes every NEXUS child process.
- Connection loss no longer switches the speaking voice. Fast first speech and offline transcription still need a measured balance of model residency, RAM and latency; optional higher-memory settings may keep speech recognition ready.
- The project may state “free to use” only with a clear explanation that third-party APIs have their own terms, quotas, availability and possible charges on a user's paid account.
