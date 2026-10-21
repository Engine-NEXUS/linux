# Sources & Citations (2026-10-05)

Verification level: **R** = repo cloned and files/LICENSE read · **W** = web page fetched · **S** = search-result snippet only · **P** = paper/doc page.

## Repositories

| Repo | Licence (as read) | Level | Used for |
|---|---|---|---|
| https://github.com/cjpais/Handy | MIT | R | Rust Silero VAD, paste, shortcuts, `transcribe-rs`/`vad-rs` usage |
| https://github.com/pipecat-ai/pipecat | BSD-2-Clause | R | Smart Turn reference code, `turns/*` strategies, evals |
| https://github.com/pipecat-ai/smart-turn | BSD-2-Clause | W | End-of-turn model (v3.2) |
| https://github.com/OpenInterpreter/open-interpreter | Apache-2.0 (+NOTICE) | R | Codex fork: `windows-sandbox-rs`, `execpolicy`, `process-hardening` |
| https://github.com/browser-use/browser-use | MIT | R | DOM/browser agent |
| https://github.com/microsoft/UFO | MIT | R | UIA + OmniParser grounding |
| https://github.com/microsoft/OmniParser | code CC-BY-4.0; v3 detector MIT; older detectors AGPL | W | Screen parsing |
| https://github.com/NousResearch/hermes-agent | MIT | R (partial — blobless clone) | Memory/skills/cron design |
| https://github.com/open-jarvis/OpenJarvis | Apache-2.0 | R | Eval/skills import |
| https://github.com/isair/jarvis | **Non-commercial custom licence** | R | Learn-only (Case 2) |
| https://github.com/openclaw/openclaw | MIT | W | Channel gateway, security record |
| https://github.com/elevenlabs/ui | MIT per GitHub page; no root LICENSE in clone | R/W | Orb component reference |
| https://github.com/pguso/kokoro | Apache-2.0 | W | Rust Kokoro inference |
| https://github.com/MicheleYin/misaki-rs | MIT (dictionary data: see upstream Misaki) | W | Rust English G2P, espeak optional |
| https://github.com/hexgrad/kokoro | Apache-2.0 | W | Model |
| https://github.com/modelcontextprotocol/rust-sdk (`rmcp`) | MIT | W | MCP client/server crate |
| https://github.com/moonshine-ai/moonshine | MIT | W | STT models |
| https://github.com/dscripka/openWakeWord | code Apache-2.0; pre-trained models CC BY-NC-SA | W | Wake word |
| https://github.com/OHF-Voice/piper1-gpl | GPL-3.0 | W | Licence risk |
| https://github.com/mediar-ai/screenpipe | Commercial/source-available | W | Case 2 |
| https://github.com/mem0ai/mem0 | Apache-2.0 | W | Memory retrieval reference |
| https://github.com/livekit/agents | Apache-2.0 (turn model: LiveKit Model License) | W | Test framework idea |
| https://github.com/ErikEkstedt/VoiceActivityProjection | not verified | S | Case 2 |
| https://github.com/kyutai-labs/unmute | not stated | S | Case 2 |

## Papers, docs, specs

| Source | URL | Level | Used for |
|---|---|---|---|
| OpenAI Realtime — Voice activity detection (semantic_vad, eagerness) | https://developers.openai.com/api/docs/guides/realtime-vad | S/P | C2-1 |
| Apple ML Research — Voice Trigger System for Siri | https://machinelearning.apple.com/research/voice-trigger | S/P | C2-2 |
| Apple ML Research — Personalized "Hey Siri" | https://machinelearning.apple.com/research/personalized-hey-siri | S/P | C2-2 |
| Apple — Lattice-Based False Trigger Mitigation Using GNNs | (via Apple ML Research listing) | S | C2-2 |
| Agent Skills specification | https://agentskills.io/specification | S | C1-7 |
| Eliciting Spoken Interruptions to Inform Proactive Speech Agent Design (CUI 2021) | https://arxiv.org/pdf/2106.02077 | S/P | C2-5 |
| Moshi: a speech-text foundation model for real-time dialogue | https://arxiv.org/abs/2410.00037 | S/P | C2-6 |
| Real-time and Continuous Turn-taking Prediction Using Voice Activity Projection | https://arxiv.org/pdf/2401.04868 | S/P | C2-7 |
| Yeah, Un, Oh: Continuous and Real-time Backchannel Prediction | https://arxiv.org/html/2410.15929v1 | S/P | C2-7 |
| OpenClaw voice wake / talk mode docs | https://docs.openclaw.ai/nodes/voicewake.md | S | Competitor design |
| OpenClaw security reports | https://blog.barrack.ai/openclaw-security-vulnerabilities-2026/ , https://www.bitdoze.com/openclaw-security-guide/ | S (third-party) | R1 |

## Local evidence (this repo)

* `docs/research/jarvis-landscape/02-license-audit-2026-10-04.md` — espeak-ng GPL finding
* `docs/research/jarvis-landscape/03-smart-turn-prototype-2026-10-04.md` — Smart Turn prototype results
* `AGENTS.md` — NEXUS baseline claims (not re-audited)

## Scratch clones (not committed)

Shallow clones live in the session scratchpad (`…/scratchpad/repos/`); delete after copying. Nothing from them is in the repo yet.
