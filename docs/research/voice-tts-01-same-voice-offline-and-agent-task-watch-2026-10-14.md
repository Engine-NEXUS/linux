# Research: Same Voice Offline + Screen-Watch ("tell me when done") + Keep-Awake (2026-10-14)

**Status**: Research only — no code touched (user directive).
**User asks**: (1) cloud voice == local voice, auto-downloaded on voice change;
(2) screen stays on while NEXUS works; (3) "remind me when it is done" while an
agent (Claude / Antigravity / opencode / Devin) runs — without blowing API limits.

## Codebase facts (verified read-only today)

- No `SetThreadExecutionState` / execution-state API anywhere — keep-awake does not exist.
- No "remind me when done" intent — `WatchScreenEmail` (deadline watch) and the
  Sentinel + `proactive_policy` (Low/Medium/High/Critical) + `memcore/offer` exist
  and are the right substrate; no caller watches *agent-task completion*.
- Voice stack: Edge cloud (e.g. Ava) primary → Kokoro twin fallback; twins are
  different speakers by construction (`voice_catalog.rs`); Kokoro files git-ignored,
  downloaded on first online run.

## Part 1 — Same voice offline: three honest routes

Edge voices are server-side weights. There is **no download-the-voice endpoint**;
"same voice offline" always means *reproduce the timbre locally*. Reference-clip
enrollment happens once while online (user picks a voice → NEXUS synthesizes a
6–30 s phoneme-rich reference script in that voice → stores clip + small pack).

| Route | How | Offline quality | Disk / RAM | CPU speed (laptop) | License | Verdict |
|-------|-----|----------------|------------|--------------------|---------|---------|
| R1 zero-shot synth (XTTS-v2) | Clone from 6 s clip, synthesize directly (Casanova et al. 2024) | SIM ~0.6–0.85, good | ~2 GB weights + torch ~2 GB disk; RAM ~2–4 GB | Slow on CPU (~seconds per sentence, est.) | **CPML — needs Coqui permission for commercial use** | Rejected: heavy, slow, license-hostile, kills idle-RAM story |
| **R2 convert, don't synthesize (OpenVoice V2)** | Keep Kokoro (fast, shipped) as base speaker → tone-color converter maps timbre to the enrolled cloud voice (Qin et al., MIT/MyShell 2024, MIT license) | Tone matches; prosody stays Kokoro's (keeps our emotion control) | Converter ~100–200 MB; RAM ≈ Kokoro + small | Feed-forward, no autoregression (12×RT on A10G per paper; CPU per-sentenceemmel-scale, to measure) | **MIT, free commercial** | **Recommended** |
| R3 cache-and-replay | Record every cloud utterance; offline replays exact audio for repeated lines (extends today's CACHED_PHRASES) | Exact — but only for previously heard sentences | KBs | Instant | Safest (own usage cache) | Complement, not a solution |

**Legal red line (say plainly):** cloning Microsoft's voice talent (Ava, Ryan, …)
from Edge output has no public license grant; Azure's own custom-voice rules require
talent consent. Do NOT ship Microsoft-talent reference clips. The enrollment must be
the **user's own voice** (reads 30 s script once) or an explicitly royalty-free
reference; cloning a cloud persona for *personal offline continuity* sits in the
same gray zone as format-shifting — disclose it in the UI, keep clips local-only,
never redistribute.

**Proposed design (for the plan):** voice change while online → enroll (synthesize
reference script in chosen voice OR record user) → download converter pack into the
existing voice slot (`active_voice.bin` pattern) → offline path = Kokoro twin →
converter; missing pack = plain Kokoro + one spoken notice. Audition step stays
(doc 09 §3): user confirms the twin before it becomes default.

## Part 2 — Screen stays on + "tell me when done"

**Keep-awake (trivial, $0, no API):** `SetThreadExecutionState(ES_CONTINUOUS |
ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)` while TTS speaks / ghost session live /
task-watch armed; clear on idle. ~20 lines in `power.rs`. This is the entire
"screen always on while NEXUS works" feature — no cloud involved.

**Task watch ("nexus, tell me when Claude is done"):** new `WatchTask` intent
("tell me when X is done", "watch this", "remind me when it's done") → watcher
records the target's OS signature (window title, process, output file) → polls
**local signals only** ($0): title-text change, process CPU decay, file mtime,
UIA events → on suspected completion, ONE vision/LLM confirm → alert through the
existing `proactive_policy` (Medium: idle ≥30 s; High: breakpoint) + spoken line.
Keep-awake armed for the watch duration so the laptop can't sleep through the finish.

**API-limit math (why naive polling is banned):** vision every 30 s = 2,880
calls/day > Gemini free 500 RPD → dead in hours. Event-driven local watch uses
~1–3 cloud calls *per task*. Table:

| Approach | Cloud calls / 8-h agent run | vs free tiers | Verdict |
|----------|-----------------------------|---------------|---------|
| Screenshot → Gemini every 30 s | ~960 vision calls | **Blows 500 RPD ~halfway** | Banned |
| Title/CPU/file poll locally, vision-confirm on change | 1–3 | Trivial (Groq 14.4k/d, Gemini 1.5k/d) | **Design** |
| STT hot-mic while waiting | 0 extra (already streaming) | — | Free |
| UIA event-driven (no poll) | 0 until event | — | Best, needs validation per app |

## Part 3 — Prior art worth stealing from

| Project / paper | What it is | What NEXUS takes |
|-----------------|------------|------------------|
| UFO (Microsoft, Zhang et al. 2024) | Windows UI agent with hierarchical control | Completion-signal patterns (control-state diffs as done-detectors) |
| OSWorld / WindowsAgentArena (Xie / Bonatti et al. 2024) | OS-agent benchmarks | Task-success detectors we can invert into watchers |
| ScreenSpot / OmniParser / Ferret-UI (2024) | Screen grounding models/parsers | Grounding already owned (vision.rs); not needed for watching |
| OpenVoice V1/V2 (Qin et al., MIT/MyShell 2024) | Decoupled tone-color converter, MIT, commercial-free | **R2 converter choice** |
| XTTS-v2 (Casanova et al. 2024) | 6-s zero-shot clone, CPML | Rejected (see table) — cite as evaluated |
| VALL-E / VALLE-X, VoiceBox, HierSpeech++ | SOTA cloning/flow TTS | Too heavy / research-only for a laptop; watch only |
| kNN-VC / FreeVC | Retrieval/conversion VC | Fallback converter if OpenVoice disappoints |
| Azure Custom Neural Voice | Consent-gated cloning service | The legal shape to copy: explicit enrollment + local-only clips |

## Sources (fetched 2026-10-14; paper citations without URLs were NOT re-fetched — titles/authors only, no guessed links)

**Voice cloning / conversion (fetched):**
- S1 XTTS-v2 model card (CPML license, 6-second clip cloning, 17 languages, <200 ms
  streaming claim): https://huggingface.co/coqui/XTTS-v2
- S2 coqui-ai-TTS maintained fork (v0.27.x, Windows wheels, cached cloned voices,
  OpenVoice + kNN-VC models included): https://github.com/idiap/coqui-ai-TTS
- S3 XTTS model docs (clone/fine-tune/streaming recipes): https://docs.coqui.ai/en/latest/models/xtts.html
- S4 OpenVoice paper — Qin, Zhao, Yu, Sun (MIT / MyShell, 2023): decoupled
  base-speaker + tone-color converter, feed-forward, 12× real-time on A10G:
  https://arxiv.org/html/2312.01479v2
- S5 OpenVoice V2: native EN/ES/FR/ZH/JA/KO + MIT license, free commercial use
  (from April 2024): https://docs.myshell.ai/technology/openvoice
- S6 OpenVoiceV2 checkpoints + setup: https://huggingface.co/myshell-ai/OpenVoiceV2
- S7 OpenVoice research page: https://research.myshell.ai/open-voice

**Microsoft voice legality (fetched):**
- S8 Custom-voice training/consent rules (talent permission + acknowledgement
  recording required; training data never reused across customers):
  https://learn.microsoft.com/en-us/azure/ai-foundry/responsible-ai/speech-service/text-to-speech/data-privacy-security
- S9 TTS transparency note + code of conduct (acceptable-use constraints):
  https://learn.microsoft.com/en-us/legal/cognitive-services/speech-service/text-to-speech/transparency-note
- S10 Edge Read-Aloud commercial use via edge-tts — NO public grant found (ask
  Microsoft Legal before commercialising):
  https://learn.microsoft.com/en-us/answers/questions/5925556/commercial-use-of-edge-read-aloud-voices-via-edge
- S11 Same, Sept 2026 thread — Azure Speech is the supported commercial path:
  https://learn.microsoft.com/en-us/answers/questions/6007729/commercial-use-of-microsoft-edge-text-to-speech-vi

**Agent / OS-control prior art (foundational, titles only):**
- S12 UFO (Microsoft) — Windows UI agent, hierarchical control.
- S13 OSWorld / WindowsAgentArena — OS-agent benchmarks and task-success detectors.
- S14 ScreenSpot / OmniParser / Ferret-UI — screen grounding (already owned in-repo).
- S15 VALL-E / VALLE-X, VoiceBox, HierSpeech++ — SOTA cloning, too heavy for laptop use.
- S16 kNN-VC / FreeVC — retrieval-based voice conversion, fallback converter option.

**OS facts (Win32, no fetch needed):**
- S17 `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED |
  ES_DISPLAY_REQUIRED)` — the keep-awake call; codebase grep confirms it is absent.

## Recommendation

1. Keep-awake in `power.rs` first (hours of work, zero risk, user-visible tonight).
2. `WatchTask` on local signals + proactive alert (no new windows/processes; the
   API-limit question answers itself: ~zero cloud cost by construction).
3. Voice twin via R2 (OpenVoice-class converter) with own-voice enrollment;
   ship R3 cache extension alongside (cheap exactness for stock phrases).
4. Measure before promising: converter CPU RTF on this laptop, SIM score vs the
   cloud persona, audition-gated default.
