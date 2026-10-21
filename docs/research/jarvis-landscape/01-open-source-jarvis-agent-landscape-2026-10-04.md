# Open-Source Jarvis / Voice-Agent Landscape — Reuse Map for NEXUS (2026-10-04)

**Goal:** stop rebuilding pillars from scratch. For every feature/pillar/animation NEXUS has (or lacks), find the best
production-grade open-source implementation we can legally copy or adapt, and say *what* to take.

## 0. Method, confidence, and honest gaps

- **Sources:** each project's GitHub README page was fetched on 2026-10-04 (stars/license/features below come from that fetch),
  plus web searches for comparisons. Star counts drift daily — re-check before quoting.
- **Instagram / social media was NOT directly scraped** (login-walled, no API access from this tool). The "viral Jarvis" repos
  that surface from social-style searches (bertrandmbanwi/Jarvis, ethanplusai/JARVIS, akshayaggarwal99/jarvis-ai-assistant)
  are small hobby projects (e.g. 37 stars) — useful for *ideas*, **not production-grade code**. If you have specific reels/handles,
  send me the repo names and I'll add them.
- **Legend:** ✅ has it (stated in fetched source) · ◐ partial/limited · ❌ not present in fetched source · `?` not verified (do not assume).
  "ULTRON" column = NEXUS today, taken from `AGENTS.md`, not re-audited in code this session.
- **Not verified, check before copying:** exact license text of individual sub-models (flagged in §5), and anything marked `?`.

## 1. Projects found — overview & verdict

Verdict key: **TAKE** = copy/adapt code or model · **PATTERN** = copy the design, reimplement in Rust/TS · **REF** = read for ideas only · **SKIP**.

| # | Project | Stars | License | Lang | What it really is | Fit for NEXUS (Tauri/Rust + React, Windows, voice-first) | Verdict |
|---|---|---|---|---|---|---|---|
| 1 | [NousResearch/hermes-agent](https://github.com/NousResearch/hermes-agent) | 251k | MIT | Python/TS | Self-improving general agent: auto-created skills, FTS5 memory, cron, subagents, messaging gateway, 7 sandbox backends | Not voice-first, but best-in-class **memory + skills + scheduler** design; Windows native installer exists | **PATTERN** (memory, skills, cron) |
| 2 | [openclaw/openclaw](https://github.com/openclaw/openclaw) | 391k | MIT | TS/JS/Rust | Personal agent gateway: 20+ chat channels, native apps on 5 OSes, Canvas, voice, ClawHub skills, MCP, cron | Biggest ecosystem; **serious security record** (see §4); Node 24+ runtime | **PATTERN** (channel pairing, skills format) + **SKIP** the code |
| 3 | [open-jarvis/OpenJarvis](https://github.com/open-jarvis/OpenJarvis) | 10.5k | Apache-2.0 | Python | Local-first agent framework; 8 built-in agents; imports Hermes (~150) + OpenClaw (13.7k) skills; **energy/latency/cost eval** | Good eval philosophy + skills importer; voice = TTS output only | **PATTERN** (eval harness, skill import) |
| 4 | [bertrandmbanwi/Jarvis](https://github.com/bertrandmbanwi/Jarvis) | 37 | MIT | Python/Next.js | "Viral-style" Jarvis: OWW wake + Moonshine + Kokoro + 2,400-particle Three.js orb + 106 tools | **Almost identical stack to NEXUS but smaller & macOS-only**; NEXUS orb is already ahead (5,200 particles) | **REF** |
| 5 | [isair/jarvis](https://github.com/isair/jarvis) | 1.9k | not confirmed | Python | 100% local voice assistant, diary + knowledge-graph memory, MCP, PII redaction, animated face, desktop apps | Closest "third person in the room" UX idea; license must be checked | **REF** |
| 6 | [pipecat-ai/pipecat](https://github.com/pipecat-ai/pipecat) | 16.2k | BSD-2 | Python | Voice-agent pipeline framework (STT/LLM/TTS, Silero VAD, interruptions, transports) | Server/WebRTC-oriented; wrong shape for an on-device Rust app | **PATTERN** (interruption design) |
| 7 | [livekit/agents](https://github.com/livekit/agents) | 14.5k | Apache-2.0 (turn model: LiveKit Model License) | Python | WebRTC voice-agent framework, semantic turn detector, test framework, telephony, MCP | Needs LiveKit server; turn model is **not** Apache | **REF** (test harness idea) |
| 8 | [pipecat-ai/smart-turn](https://github.com/pipecat-ai/smart-turn) | 1.6k | BSD-2 | Python/ONNX | Audio-based end-of-turn model, v3.2: **8 MB int8**, ~10 ms CPU, **23 languages incl. Hindi** | Drops straight into a Rust ONNX (tract/ort) pipeline | **TAKE** |
| 9 | [moonshine-ai/moonshine](https://github.com/moonshine-ai/moonshine) | 11.2k | MIT | multi | On-device streaming STT; Python/WASM/iOS/Android/Win/Linux | Already our local STT fallback | **ALREADY USED** |
| 10 | [dscripka/openWakeWord](https://github.com/dscripka/openWakeWord) | 2.8k | Apache-2.0 code; **pre-trained models CC BY-NC-SA** | Python | Wake word framework + synthetic-data training | Already used; our `nexus.onnx` is self-trained | **ALREADY USED** (license check §5) |
| 11 | [hexgrad/kokoro](https://github.com/hexgrad/kokoro) | 9.2k | Apache-2.0 | Python | 82M-param TTS; en/es/fr/hi/it/ja/pt/zh | Offline, permissive; was our earlier lazy engine | **TAKE** (as offline default) |
| 12 | [OHF-Voice/piper1-gpl](https://github.com/OHF-Voice/piper1-gpl) | 5.8k | **GPL-3.0** | C++/Python | Maintained Piper fork | **GPL contaminates a closed-source app** | **SKIP** unless we stay on the older MIT build |
| 13 | [cjpais/Handy](https://github.com/cjpais/Handy) | 32.8k | MIT | **Tauri/Rust** | Offline STT app: Silero VAD, hotkeys (hold/toggle), Whisper/Parakeet, paste | **Same stack as NEXUS** — best Rust reference for VAD + hotkey + paste | **TAKE** (patterns/code) |
| 14 | [elevenlabs/ui](https://github.com/elevenlabs/ui) | 2.4k | MIT | React/TS | Orb, waveform, live-waveform, bar-visualizer components (shadcn) | Our orb is more advanced; useful for agent-state API + audio-reactive wiring | **REF** |
| 15 | [browser-use/browser-use](https://github.com/browser-use/browser-use) | 117k | MIT | Python | LLM drives a real browser (DOM-level), local or cloud | Replaces our keyboard-only `Ctrl+L` browser flow with real page control | **TAKE** (optional sidecar) |
| 16 | [microsoft/UFO](https://github.com/microsoft/UFO) | 9.9k | MIT | Python | Windows desktop agent: **UI Automation + vision**, speculative multi-action (−51% LLM calls), RAG on traces | Same problem as our Ghost Mode (UIA-first, vision fallback) | **PATTERN** (+ benchmark ourselves against it) |
| 17 | [microsoft/OmniParser](https://github.com/microsoft/OmniParser) | 25.5k | code CC-BY-4.0; icon_detect v3 MIT (older = AGPL); icon_caption MIT | Python | Screenshot → structured UI elements | Better grounding than our Groq/Gemini axis-grid | **TAKE** (v3 weights only) |
| 18 | [bytedance/UI-TARS-desktop](https://github.com/bytedance/UI-TARS-desktop) | 39.2k | Apache-2.0 | TS | GUI-agent desktop app + Agent TARS CLI, MCP, local/remote operators | Heavy VLM dependency; useful operator abstraction | **REF** |
| 19 | [OpenInterpreter/open-interpreter](https://github.com/OpenInterpreter/open-interpreter) | 68.5k | Apache-2.0 | **Rust** (Codex-based) | New Rust rewrite: sandbox + approvals, ACP, computer use, AGENTS.md | Rust — sandbox/approval design may port directly; no voice | **PATTERN** (sandbox/approval) |
| 20 | [mem0ai/mem0](https://github.com/mem0ai/mem0) | 66.6k | Apache-2.0 | Python/TS | Memory layer; claims LoCoMo 92.5, ~1 s latency, ~7k tokens | Python dependency; our memory is file-based and works | **REF** (retrieval design) |
| 21 | [letta-ai/letta](https://github.com/letta-ai/letta) | 25k | Apache-2.0 | Python/TS | Stateful agents platform | Too heavy; overlaps our memory | **SKIP** |
| 22 | [OHF-Voice/wyoming](https://github.com/OHF-Voice/wyoming) | 399 | MIT | Python | Peer-to-peer voice-service protocol (wake/STT/TTS satellites) | Useful only if we ever expose NEXUS as a Home Assistant satellite | **SKIP** for now |
| 23 | [mediar-ai/screenpipe](https://github.com/mediar-ai/screenpipe) | 21.8k | **Commercial / source-available (non-commercial personal use)** | Rust/TS | Screen+audio memory, OCR, pipes, MCP | **Cannot copy code** into a product | **SKIP** (idea: UIA tree + OCR fallback) |
| 24 | [leon-ai/leon](https://github.com/leon-ai/leon) | 17.6k | MIT | JS/Python | Assistant mid-rewrite (2.0 preview) | Unstable, docs stale | **SKIP** |
| 25 | [kyutai-labs/unmute](https://github.com/kyutai-labs/unmute) | n/a | not stated in README | Python | Full duplex-ish STT+TTS stack, ~450 ms | GPU-class models, not laptop-friendly | **SKIP** |
| 26 | [TEN-framework/ten-vad](https://github.com/TEN-framework/ten-vad) · [snakers4/silero-vad](https://github.com/snakers4/silero-vad) | 2.1k · 8.7k | (Silero MIT; TEN: check) | C/Python | Low-latency VAD | Silero already proven in Handy (Rust) | **TAKE** Silero |
| 27 | [KoljaB/RealtimeSTT](https://github.com/KoljaB/RealtimeSTT) | 9.8k | not confirmed | Python | WebRTC+Silero VAD + faster-whisper streaming | We already have equivalent in Rust | **SKIP** |

## 2. Feature / pillar matrix — NEXUS vs. the field

Rows are every pillar NEXUS has or should have. Read across to see who is best.

| Pillar | **NEXUS today** (AGENTS.md) | Hermes | OpenClaw | OpenJarvis | bertrand/Jarvis | isair/jarvis | Pipecat | LiveKit | **Best OSS source to take from** |
|---|---|---|---|---|---|---|---|---|---|
| Wake word | ✅ custom OWW "nexus", 97.5% recall / 1.67% FA, speaker-verify optional | ❌ | `?` | ❌ | ✅ OWW "hey jarvis" | ✅ "Jarvis" | ❌ | ❌ | **Ours is ahead** — nothing to take |
| VAD | ✅ (WebRTC fail-open + RMS gates) | ❌ | `?` | ❌ | `?` | `?` | ✅ Silero | ✅ | Silero via **Handy** (Rust) |
| Turn detection / end-of-speech | ◐ hand-built endpointing (hesitant-speaker test) | ❌ | `?` | ❌ | `?` | `?` | ✅ Smart Turn | ✅ semantic (non-Apache) | **Smart Turn v3.2** (BSD-2, Hindi incl.) |
| Barge-in / interruption | ◐ v4 requires "nexus" in verify transcript; instant-stop tier *planned* | ❌ | `?` | ❌ | `?` | `?` | ✅ | ✅ | **Pipecat** interruption pattern |
| STT local | ✅ Moonshine (medium/small/tiny) | ◐ voice memos | `?` | ❌ | ✅ Moonshine + faster-whisper | ✅ Whisper | ✅ via plugins | ✅ via plugins | Already best-of-breed |
| STT cloud | ✅ Groq Whisper v3 turbo | ❌ | `?` | ❌ | `?` | ❌ | ✅ 15+ | ✅ | — |
| Streaming/live caption STT | ✅ `/stream` WS caption | ❌ | `?` | ❌ | ◐ live text overlay | `?` | ✅ | ✅ | — |
| TTS offline | ◐ Piper fallback (⚠ check license) | `?` | `?` | ✅ (output) | ✅ Kokoro | ✅ Piper/Chatterbox | ✅ Kokoro/XTTS | ✅ | **Kokoro** (Apache-2.0) |
| TTS cloud | ✅ edge-tts (⚠ unofficial endpoint) | `?` | `?` | `?` | ❌ | ❌ | ✅ 25+ | ✅ | Add a **contractual** provider as option (Cartesia/ElevenLabs/Azure) |
| Sentence-chunked streaming TTS + emotion prosody | ✅ | `?` | `?` | ❌ | ◐ sub-second Kokoro | `?` | ✅ | ✅ | — |
| **Orb animation** (idle/listen/think/speak) | ✅ custom WebGL, 5,200 particles, TTS-envelope beat sync, entrance burst, text morph | ❌ | ◐ Canvas | ❌ | ✅ 2,400-particle Three.js | ◐ animated face | ❌ | ❌ | **Ours is ahead**; only look at ElevenLabs UI for state→prop API |
| Waves / waveform viz | ✅ ghost-mode bars | ❌ | `?` | ❌ | ◐ | ❌ | ❌ | ❌ | ElevenLabs UI `live-waveform` (MIT) if wanted |
| Response caption word-by-word | ✅ (edge-tts boundaries) | ❌ | `?` | ❌ | ◐ | `?` | ❌ | ❌ | — |
| Persistent memory | ◐ file-based core/episodic + "remember that" | ✅ FTS5 + agent-curated + user model | ✅ local state | ✅ traces | ✅ SQLite semantic | ✅ diary + knowledge graph | ❌ | ❌ | **Hermes FTS5 pattern** (SQLite, port to `rusqlite`) ; mem0 for retrieval ideas |
| Self-improving skills | ❌ (static `intents.yaml`) | ✅ auto-create/edit skills | ✅ ClawHub | ✅ imports both | ✅ Agent Skills | `?` | ❌ | ❌ | **agentskills.io `SKILL.md` format** (Hermes/OpenClaw/OpenJarvis all speak it) |
| MCP | ◐ client only (Swiggy/WhatsApp/Amazon) | ✅ | ✅ | `?` | ✅ both directions | ✅ | `?` | ✅ native | Add **MCP server** mode (expose NEXUS tools) |
| Scheduler / cron | ❌ (webhook triggers only) | ✅ | ✅ | `?` | `?` | `?` | ❌ | ❌ | **Hermes cron** pattern |
| Messaging channels | ◐ WhatsApp via MCP | ✅ 5 | ✅ 20+ | `?` | ❌ | ❌ | ◐ WhatsApp transport | ❌ | OpenClaw **DM-pairing approval** pattern only |
| Browser control | ◐ keyboard only (Ctrl+L, new tab, type) | `?` | ✅ | `?` | ✅ Playwright + extension | ✅ Chrome | ❌ | ❌ | **browser-use** (MIT) sidecar |
| Windows GUI control | ✅ Ghost Mode: UIA-first + vision fallback, Esc cancel, UIPI detect | `?` | ◐ nodes | ❌ | ❌ (macOS) | ❌ | ❌ | ❌ | **UFO** (UIA+vision) + **OmniParser v3** for grounding |
| Sandbox / execution isolation | ◐ whitelist/denylist; no process sandbox | ✅ 7 backends | ◐ optional | `?` | ◐ approvals | ◐ redaction | ❌ | ❌ | **open-interpreter (Rust, Codex-based)** sandbox/approval design |
| Confirmation gates / safety | ✅ confirm-gated send, destructive warnings, refusal battery | `?` | ◐ pairing | `?` | ✅ approvals + Keychain | ✅ PII redaction | ❌ | ❌ | Ours is strong; keep |
| PII filtering / secrets | ✅ regex PII + OS keychain | `?` | ◐ | `?` | ✅ Keychain | ✅ | ❌ | ❌ | — |
| Multi-agent / subagents | ◐ compound plans (sequential) | ✅ parallel subagents | `?` | ✅ 8 agents | ✅ planner/exec/QA | ❌ | ❌ | ❌ | Hermes subagent pattern (low priority) |
| Local-LLM routing | ◐ 9Router cloud cascade; admin-only Qwen | ✅ 300+ models | ✅ | ✅ local-first | ✅ 3-tier fast/brain/deep | ✅ Ollama | ✅ | ✅ | OpenJarvis "local first, cloud when needed" policy |
| Eval / observability | ◐ e2e fixture 49/50, SLO collector, soak harness | `?` | `?` | ✅ energy/FLOPs/latency/$ | ❌ | ❌ | ◐ | ✅ pytest judge | **LiveKit test style** + **OpenJarvis cost metrics** |
| Desktop app / installer | ✅ Tauri (MSI/exe, NSIS broken locally) | ✅ PS installer | ✅ 5 OS apps | ✅ exe/dmg/deb | ◐ macOS overlay | ✅ 3 OS | ❌ | ❌ | — |
| Auto-update / model OTA | ✅ NLU model OTA via Worker | `?` | ◐ version check | `?` | ❌ | `?` | ❌ | ❌ | — |
| Home-automation protocol | ❌ | ❌ | ❌ | ❌ | ❌ | ◐ MCP | ❌ | ❌ | Wyoming (optional, later) |

## 3. Prioritised "don't rebuild, take this" list

Ranked by (value to NEXUS) ÷ (integration effort), with license status.

| Pri | Pillar | Take | From | License | Effort | Why |
|---|---|---|---|---|---|---|
| **P0** | End-of-turn detection | Smart Turn v3.2 ONNX (8 MB, ~10 ms CPU) | pipecat-ai/smart-turn | BSD-2 ✅ | S (ONNX already in stack via tract) | Replaces hand-tuned endpointing → fewer cut-offs/early stops; supports Hindi |
| **P0** | Offline TTS default | Kokoro | hexgrad/kokoro | Apache-2.0 ✅ | **M–L** (corrected: needs a non-espeak G2P, see doc 02) | Removes dependence on unofficial edge-tts endpoint — but its phonemizer re-introduces GPL espeak-ng unless replaced |
| **P0** | Licence hygiene | `THIRD_PARTY_NOTICES` + audit of Piper build, OWW base models, edge-tts | — | — | S | **DONE (audit): [doc 02](02-license-audit-2026-10-04.md)** — found espeak-ng (GPL-3) statically linked via piper-rs |
| **P1** | Skills format | Adopt `SKILL.md` / agentskills.io | Hermes / OpenClaw / OpenJarvis | MIT/Apache ✅ | M | One format unlocks hundreds of community skills — **but only via vetted allowlist** (§4) |
| **P1** | Memory search | SQLite **FTS5** session search + periodic "memory nudge" | Hermes pattern | MIT ✅ | M (rusqlite) | Our memory is flat files; FTS5 gives recall without a Python sidecar |
| **P1** | Barge-in | Interruption state machine | Pipecat pattern | BSD-2 ✅ | M | Unblocks the "bare *stop* never interrupts TTS" gap already in your notes |
| **P1** | Windows grounding | OmniParser **icon_detect v3 (MIT)** + UFO's UIA-first/speculative multi-action | microsoft | MIT / MIT ✅ | L | Direct upgrade for Ghost Mode vision step; v1/v1.5 weights are AGPL — avoid |
| **P2** | Real browser control | browser-use as optional sidecar | browser-use | MIT ✅ | M–L | Replaces blind keyboard browsing with DOM-level actions |
| **P2** | Scheduler | cron-style "unattended automations" | Hermes | MIT ✅ | M | Missing pillar |
| **P2** | MCP server mode | Expose NEXUS tools over MCP | Jarvis(bertrand)/Hermes pattern | MIT ✅ | M | Two-way MCP is table stakes in this field |
| **P2** | Sandbox/approvals | Process sandbox + approval policy | open-interpreter (Rust) | Apache-2.0 ✅ | L | Only relevant once third-party skills run code |
| **P3** | VAD/hotkey/paste reference | Handy's Silero + hold/toggle/tap hotkeys | cjpais/Handy | MIT ✅ | S | Cross-check our Rust implementation |
| **P3** | Eval metrics | Energy/latency/cost per task | OpenJarvis | Apache-2.0 ✅ | M | Feeds your SLO board |
| **P3** | Orb state API | Agent-state → orb props | elevenlabs/ui | MIT ✅ | S | Reference only — NEXUS orb stays |

**Do NOT replace:** the wake-word model/pipeline, the WebGL orb, STT stack, or Ghost Mode safety rails — in every comparison
above NEXUS is equal or ahead there, and the cost of swapping outweighs any gain.

## 4. Risks of "clone and copy"

1. **OpenClaw supply chain:** reports describe 341 malicious ClawHub skills (AMOS stealer campaign, Jan 2026), multiple CVEs
   (command injection, SSRF, path traversal) and 135k+ exposed instances. Take *ideas* (DM pairing, skills format),
   **not** its code or an unvetted skill marketplace. Any skill importer in NEXUS needs allowlist + signature/hash pin + no code-exec by default.
   Source: [barrack.ai](https://blog.barrack.ai/openclaw-security-vulnerabilities-2026/), [bitdoze](https://www.bitdoze.com/openclaw-security-guide/) (third-party reports; not independently verified).
2. **Star counts ≠ production quality.** The Instagram-style Jarvis repos have 37–2k stars; the 100k+ repos are general agents, not voice assistants.
3. **Language/runtime tax.** Most winners are Python/Node. Anything we adopt as a sidecar adds RAM (you spent a whole phase cutting idle RAM to ~104 MB).
   Prefer ONNX models (Smart Turn, Kokoro, OmniParser) and pattern ports to Rust over new Python processes.
4. **Copying code ≠ copying licence obligations.** MIT/Apache/BSD require keeping notices; GPL (Piper fork) and non-commercial (Screenpipe, OWW pre-trained models) do not allow closed redistribution.

## 5. Licence flags to resolve before shipping

| Item | Issue | Action |
|---|---|---|
| Piper TTS | `piper1-gpl` is **GPL-3.0**; older `rhasspy/piper` was MIT | Confirm which build/voices NEXUS bundles; if GPL, replace with Kokoro or isolate as a separate process |
| openWakeWord pre-trained models | CC BY-NC-SA (non-commercial) | NEXUS uses self-trained `nexus.onnx`, but confirm the **melspectrogram + embedding** base models' licences |
| edge-tts | Unofficial client of Microsoft's consumer TTS endpoint (from background knowledge, not fetched today) | Treat as dev/personal only; add a contractual TTS provider for distribution |
| LiveKit turn-detector | LiveKit Model License, not Apache | Use Smart Turn (BSD-2) instead |
| OmniParser | Code CC-BY-4.0; v3 detector MIT; **older detectors AGPL** | Use v3 weights only |
| Screenpipe | Non-commercial source-available | Do not copy code |
| isair/jarvis | Licence not confirmed from README | Read the LICENSE file before any reuse |

## 6. What I could not verify (so you can cross-check)

- Rows marked `?` in §2 — e.g. whether OpenClaw/Hermes ship a wake word or orb-style UI; fetched pages didn't say.
- Exact latency/accuracy numbers (Mem0's LoCoMo figures, Smart Turn's 10 ms) are the projects' own claims.
- Current NEXUS column is from `AGENTS.md` notes, not a fresh code audit.
- No Instagram/TikTok/X content was read (see §0).

## 7. Suggested next step

Implement **P0** first (Smart Turn + Kokoro default + licence audit) — each is small, uses ONNX already in the stack, and removes a
real production risk. Then decide on P1 (`SKILL.md` + FTS5 memory) as one "skills & memory" phase.


## 8. Star ratings (out of 5)

Rubric: ★★★★★ best-in-class production · ★★★★ strong · ★★★ works, gaps · ★★ basic/partial · ★ minimal · – absent · ? not verified (excluded from averages).
Ratings are my judgement from the READMEs fetched on 2026-10-04 and `AGENTS.md` (NEXUS self-reported, not independently benchmarked). Pipecat/LiveKit are frameworks, not assistants, so their averages are not comparable.

| Pillar | NEXUS | Hermes | OpenClaw | OpenJarvis | bertrand/Jarvis | isair/jarvis | Pipecat | LiveKit | Best in field |
|---|---|---|---|---|---|---|---|---|---|
| Wake word | ★★★★☆ | – | ? | – | ★★★☆☆ | ★★★☆☆ | – | – | NEXUS (ahead of the rest) |
| VAD / turn detection / barge-in | ★★☆☆☆ | – | ? | – | ? | ? | ★★★★★ | ★★★★★ | Pipecat + Smart Turn |
| Speech-to-text | ★★★★☆ | ★★☆☆☆ | ? | – | ★★★☆☆ | ★★★☆☆ | ★★★★☆ | ★★★★☆ | NEXUS ≈ Pipecat |
| Text-to-speech | ★★★☆☆ | ? | ? | ★★☆☆☆ | ★★★☆☆ | ★★★☆☆ | ★★★★★ | ★★★★★ | Pipecat / LiveKit providers; Kokoro offline |
| Orb / animation / visual identity | ★★★★★ | ★☆☆☆☆ | ★★★☆☆ | ★★☆☆☆ | ★★★☆☆ | ★★☆☆☆ | – | – | NEXUS |
| Memory | ★★★☆☆ | ★★★★★ | ★★★☆☆ | ★★★★☆ | ★★★☆☆ | ★★★★☆ | – | – | Hermes (FTS5 + user model) |
| Skills / extensibility | ★★☆☆☆ | ★★★★★ | ★★★★★ | ★★★★☆ | ★★★☆☆ | ★★★☆☆ | ★☆☆☆☆ | ★★☆☆☆ | Hermes / OpenClaw (SKILL.md) |
| MCP | ★★★☆☆ | ★★★★☆ | ★★★★☆ | ? | ★★★★☆ | ★★★★☆ | ? | ★★★★☆ | bertrand (both directions) |
| Windows desktop (GUI) control | ★★★★☆ | ? | ★★☆☆☆ | – | ★★☆☆☆ | ★☆☆☆☆ | – | – | Microsoft UFO (UIA+vision) |
| Browser control | ★★☆☆☆ | ? | ★★★★☆ | ? | ★★★★☆ | ★★★☆☆ | – | – | browser-use |
| Scheduler / proactive | ★★☆☆☆ | ★★★★★ | ★★★★☆ | ? | ? | ? | – | – | Hermes cron |
| Messaging channels | ★★☆☆☆ | ★★★★☆ | ★★★★★ | ? | – | – | ★☆☆☆☆ | – | OpenClaw (20+) |
| Safety / confirmation gates | ★★★★☆ | ★★★★☆ | ★★☆☆☆ | ? | ★★★★☆ | ★★★★☆ | ? | ? | NEXUS ≈ Hermes ≈ bertrand ≈ isair |
| Sandbox / isolation | ★☆☆☆☆ | ★★★★★ | ★★★☆☆ | ? | ★★☆☆☆ | ? | ? | ? | Hermes (7 backends) |
| Testing / observability | ★★★☆☆ | ? | ? | ★★★★☆ | ? | ? | ★★★☆☆ | ★★★★★ | LiveKit |
| Packaging / installer | ★★★☆☆ | ★★★★☆ | ★★★★★ | ★★★★☆ | ★★☆☆☆ | ★★★★☆ | ? | ? | OpenClaw (5 OS apps) |
| Local-first / offline | ★★★☆☆ | ★★★☆☆ | ★★★☆☆ | ★★★★★ | ★★★★☆ | ★★★★★ | ★★★☆☆ | ★★★☆☆ | OpenJarvis / isair |
| Maturity / community | ★★☆☆☆ | ★★★★★ | ★★★★★ | ★★★☆☆ | ★☆☆☆☆ | ★★☆☆☆ | ★★★★☆ | ★★★★★ | OpenClaw / Hermes / LiveKit |
| **Average (scored rows only)** | **2.9** (18 rows) | **3.4** (14 rows) | **3.7** (13 rows) | **2.3** (12 rows) | **2.7** (15 rows) | **2.9** (14 rows) | **1.9** (14 rows) | **2.2** (15 rows) | |
