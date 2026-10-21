# Cross-Check & Source Classification — Case 1 / 2 / 3 (2026-10-05)

Second pass over [doc 01](01-open-source-jarvis-agent-landscape-2026-10-04.md). This time repos were **shallow-cloned and read**
(LICENSE files + exact file paths), not just summarised from README pages. Source list: [doc 06](06-sources-and-citations-2026-10-05.md).

* **Case 1** — open-source repo with a compatible licence → clone/copy the dedicated files.
* **Case 2** — not copyable (closed, source-available, non-commercial, or no code) → understand it via papers/docs/journals, then plan our own implementation.
* **Case 3** — no repo, no paper, no docs → parked list for later.

**Language rule:** NEXUS = Rust (`src-tauri`) + TypeScript/React. "Copy as-is" is realistic only for Rust/TS code. Python code is
either a **sidecar** (costs RAM — you already optimised idle RAM to ~104 MB) or a **design port** (re-write in Rust).
Column *Reuse mode* says which.

## 0. Corrections to doc 01 found by this cross-check

| # | Was | Now | Evidence |
|---|---|---|---|
| 1 | `isair/jarvis` = "REF, licence unconfirmed" | **Non-commercial licence** — cannot copy; **Case 2** (learn only) | `LICENSE`: "non-commercial purposes… Commercial use requires a separate commercial license"; derivatives must carry the same terms |
| 2 | Kokoro TTS = M–L effort, blocked by GPL espeak-ng | **Case 1, Rust-native path exists**: `pguso/kokoro` (Apache-2.0, Rust) + `misaki-rs` (MIT, English G2P, **espeak is an optional cargo feature** → can be disabled). Caveats: tiny repos (8 and 10 stars); dictionary *data* licence must be checked against upstream Misaki; out-of-vocabulary words get no fallback with espeak off | WebFetch of both repos; `docs.rs/misaki-rs` |
| 3 | VAD = "take Silero via Handy" (idea) | **Exact files located** in Handy (MIT, Rust): `src-tauri/src/audio_toolkit/vad/{mod,silero,smoothed,earshot}.rs` + `resources/models/silero_vad_v4.onnx`; uses `vad-rs` crate (MIT) | cloned repo |
| 4 | Local STT = "already used" (Python Moonshine sidecar) | New option: Handy depends on **`transcribe-rs`** (MIT, ONNX; Moonshine/Parakeet/SenseVoice) → could **remove the Python STT sidecar**. Unverified: whether it supports the exact `moonshine medium streaming` model NEXUS uses | Handy `Cargo.toml`; crate licence via search |
| 5 | Sandbox = "pattern from open-interpreter" | open-interpreter's repo **is a Codex fork** (`codex-rs/`, Apache-2.0) containing **Rust** crates `windows-sandbox-rs` (115 files), `execpolicy`, `process-hardening`, `shell-command`. They depend on sibling workspace crates (`codex-protocol`, `codex-utils-*`, `codex-otel`) → extraction cost is real | cloned repo, `Cargo.toml` |
| 6 | Smart Turn = "take model" | Pipecat bundles the **reference mel code** (`_whisper_features.py`) and the model; our Rust port already matches HF to 3e-5. Pipecat also has `turns/{user_start,user_stop,user_mute}` **strategy modules** (vad / min-words / transcription / wake-phrase start; eager / deferred stop; mute strategies) — a ready design for barge-in | cloned repo |
| 7 | MCP server mode = "pattern" | **`rmcp`** — the official Rust MCP SDK (MIT, ~4k stars, client+server, stdio + streamable HTTP, `#[tool]` macros) → a crate, nothing to copy | WebFetch |
| 8 | ElevenLabs UI = MIT | Clone has **no root `LICENSE` file**; the MIT claim comes from the GitHub page only → re-verify in `package.json` before copying anything | cloned repo |

## 1. CASE 1 — open source, copy the dedicated files

Licence column = what the repo's LICENSE says (verified by reading the file unless marked †: † = from web page/search only).

| ID | NEXUS pillar / gap | Source repo | Files / crates to take | Licence | Reuse mode | Effort | Risk / note |
|---|---|---|---|---|---|---|---|
| C1-1 | **Neural VAD** (replaces RMS gate) | `cjpais/Handy` | `src-tauri/src/audio_toolkit/vad/{mod,silero,smoothed}.rs`, `resources/models/silero_vad_v4.onnx`; crate `vad-rs`† | MIT ✅ | **Copy Rust as-is** | S–M | `vad-rs` is a git dependency in Handy; check it builds with our `ort` pin |
| C1-2 | **End-of-turn detection** | `pipecat-ai/smart-turn` + `pipecat` | model + `_whisper_features.py` (reference only) | BSD-2 ✅ | **Done**: `turn_detect.rs` (prototype, unwired) | S (wire-in) | Benefit unproven until real-speech eval (doc 03) |
| C1-3 | **Barge-in / turn strategies** | `pipecat-ai/pipecat` | design of `turns/user_start/{vad,min_words,transcription,wake_phrase}`, `user_stop/{eager,deferred}`, `user_mute/*` | BSD-2 ✅ | **Design port** (Python → Rust enum/trait) | M | Fixes the known gap "bare *stop* never interrupts TTS" |
| C1-4 | **Local STT in Rust** | `cjpais/transcribe-rs` (used by Handy) | crate `transcribe-rs` (feature `onnx`) | MIT† | Crate dependency | M | Spike first: Moonshine medium-streaming support & accuracy vs current sidecar |
| C1-5 | **Offline TTS, GPL-free** | `pguso/kokoro` + `MicheleYin/misaki-rs` + `onnx-community/Kokoro-82M-v1.0-ONNX` | `KokoroTts` module, `misaki-rs` with `default-features=false`, `model_quantized.onnx` (~92 MB int8) | Apache-2.0 / MIT / Apache-2.0† | **Copy Rust** | M | Dictionary-data licence unverified; OOV words; 92–325 MB model download |
| C1-6 | **MCP server mode** | `modelcontextprotocol/rust-sdk` | crate `rmcp` | MIT† | Crate dependency | M | Expose NEXUS tools to Claude/Cursor etc. |
| C1-7 | **Skills format** | `agentskills/agentskills` spec; loaders in Hermes / OpenJarvis | `SKILL.md` (YAML front-matter + progressive disclosure); Hermes `agent/skill_utils.py`, `agent/skill_commands.py`; **TypeScript** `apps/desktop/src/app/capabilities/skills/frontmatter.ts` (+ test) is directly copyable for the settings UI | MIT (Hermes ✅) / Apache-2.0 (OpenJarvis ✅) / spec open | **Design port** to Rust (`serde_yaml`); **copy TS** parser | M | Import only from an allow-list (see risk R1) |
| C1-8 | **Memory with FTS5 search** | `NousResearch/hermes-agent` | `hermes_state_fts.py` (external-content FTS5 table over `messages` + insert/delete/update triggers, fail-open trigger detach on corruption), `hermes_state.py` (WAL, one writer), `agent/memory_manager.py` | MIT ✅ | **Design port** to `rusqlite` (bundled, FTS5) | M | Read from the clone: schema pattern confirmed; skip its CJK tokenizer (English-only today) |
| C1-9 | **Scheduler / cron** | `NousResearch/hermes-agent` | `cron/jobs.py`, `cron/scheduler.py`, `cron/scheduler_preflight.py`, `cron/scheduler_failure_copy.py`, `agent/monitoring/cron_health.py` | MIT ✅ | **Design port**; crate `tokio-cron-scheduler`† | M | Needs persistence + missed-run policy |
| C1-10 | **Windows sandbox / exec policy** | `OpenInterpreter/open-interpreter` (Codex fork) | `codex-rs/execpolicy` (Starlark prefix rules), `process-hardening`, later `windows-sandbox-rs` | Apache-2.0 ✅ (LICENSE + NOTICE) | **Copy Rust**, but crates depend on workspace siblings | L | Start with `execpolicy` only; `windows-sandbox-rs` = separate spike |
| C1-11 | **Real browser control** | `browser-use/browser-use` | `browser_use/dom/service.py`, `browser/session.py`, `agent/*` | MIT ✅ | **Python sidecar** (opt-in) or CDP port | L | Adds a Python process; keep lazy-start + 60 s idle kill like NLU |
| C1-12 | **UI grounding for Ghost Mode** | `microsoft/UFO`, `microsoft/OmniParser` | `ufo/automator/ui_control/grounding/omniparser.py`, `inspector.py`; OmniParser **v3** detector (MIT) + caption (MIT) | MIT ✅ / models MIT† (older detectors AGPL — avoid) | Python sidecar or ONNX in Rust | L | Benchmark against current Groq/Gemini axis-grid first |
| C1-13 | **Eval / observability** | `open-jarvis/OpenJarvis`, `livekit/agents` (tests), `pipecat/evals` | eval scenario format (`pipecat/evals/release/scenarios/scripted/*.yaml`), cost/latency metrics | Apache-2.0 ✅ / BSD-2 ✅ | Design port | M | Feeds your SLO board |
| C1-14 | **Messaging channels** | `openclaw/openclaw` (TS) | channel adapters, DM-pairing | MIT† | Selective copy of TS adapters | L | **Security** (R1) — deferred |
| C1-15 | **Orb audio-reactive API** (reference) | `elevenlabs/ui` | orb props / live-waveform | MIT† | Reference only | – | NEXUS orb is already ahead; no root LICENSE file in clone |

## 2. CASE 2 — not copyable → understand it, then build our own

| ID | Topic | Why not Case 1 | What to read | What we take away | Plan hook |
|---|---|---|---|---|---|
| C2-1 | **Semantic turn "eagerness"** | OpenAI Realtime `semantic_vad` is a closed API | OpenAI Realtime VAD guide | Behaviour spec: classifier score → *wait longer when low, answer immediately when high*; user-facing `eagerness` low/medium/high | Phase 1: expose `turnEagerness` setting mapped onto Smart Turn probability thresholds + hard limits |
| C2-2 | **False-trigger mitigation** | Apple's Siri system is closed | Apple ML Research "Voice Trigger System for Siri"; "Personalized Hey Siri"; "Lattice-Based False Trigger Mitigation Using GNNs" | Two-stage: trigger checker → speaker ID → *directed-speech* check on the **full utterance** | Phase 6 (optional): second-pass check using the STT transcript (is this addressed to NEXUS?) to cut the 1.67 % FA further |
| C2-3 | **`isair/jarvis` design** | **Non-commercial licence** (LICENSE read) | its README/docs only | "Third person in the room" context handling, PII redaction before model, knowledge-graph memory | Ideas for memory phase; **do not copy code** |
| C2-4 | **Screenpipe ambient memory** | Source-available, personal non-commercial | its docs/architecture page | Event-driven capture; accessibility-tree + OCR fallback; "pipes" as markdown agents | Future: UIA-tree capture (we already have UIA) — own implementation |
| C2-5 | **Proactive speech timing** | Research only; no implementation found | Eliciting Spoken Interruptions to Inform Proactive Speech Agent Design (CUI 2021, arXiv 2106.02077) | Interrupt at task *breakpoints*; urgency shortens delay; optional "access ritual" | Future: rules for the mail/commute sentinels (when to speak unprompted) |
| C2-6 | **Full-duplex dialogue (Moshi)** | Paper licence CC BY-NC-SA; model/code licence not verified here; GPU-class | arXiv 2410.00037 | Concept only: 160–200 ms full duplex | **Deferred** — not laptop-friendly |
| C2-7 | **Voice Activity Projection / backchannels** | Repo exists (`ErikEkstedt/VoiceActivityProjection`) but **licence not verified** and research-grade | arXiv 2401.04868, 2410.15929 | Predicting turn shifts & backchannel slots | Defer; re-check licence before any reuse |
| C2-8 | **Kyutai Unmute** | No licence stated in README; GPU | kyutai.org posts | ~450 ms stack reference | Skip |

## 3. CASE 3 — no repo, no paper, no docs found

Honest result: **none of the 18 pillars in the matrix landed here** — every pillar had at least a repo or a paper.
The only genuine Case-3 items are things I **could not reach**:

| ID | Item | Why it is Case 3 | What I need from you |
|---|---|---|---|
| C3-1 | Features shown in **Instagram/TikTok reels** about "Jarvis" agents | Login-walled; no repo names to follow | Send the handles/reel links or the repo names — I'll classify each |
| C3-2 | `?` cells in the star table (e.g. does OpenClaw/Hermes ship an orb/wake word, browser tools) | README pages didn't say; not worth guessing | Re-check only if you plan to copy from those projects |
| C3-3 | TEN VAD licence; `vad-rs` / `transcribe-rs` licences (web-search only) | Not read from source | Confirm in Phase 1/2 before merging |

## 4. Risks that apply to Case 1

* **R1 — skill marketplaces.** OpenClaw's ClawHub had hundreds of malicious skills (third-party reports); any SKILL.md importer ships **allow-list + hash pin + no auto-exec**.
* **R2 — licence propagation.** MIT/Apache/BSD → keep headers and add `THIRD_PARTY_NOTICES.md` (Apache-2.0 also requires carrying NOTICE). NEXUS itself still has **no LICENSE** (doc 02 finding #2).
* **R3 — GPL espeak-ng** remains linked until Phase 3 lands (doc 02 finding #1).
* **R4 — tiny upstreams.** `pguso/kokoro` (8★) and `misaki-rs` (10★) are single-maintainer; vendor the code (don't depend on `git =` HEAD) and pin hashes.
* **R5 — sidecar RAM.** Every Python sidecar must be lazy-started and idle-killed (existing `lazy_nlu.rs` pattern).
