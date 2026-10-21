# Implementation Plan — reuse-first (2026-10-05)

Inputs: [doc 04](04-cross-check-and-case-classification-2026-10-05.md) (what to take, from where) · [doc 01 §8](01-open-source-jarvis-agent-landscape-2026-10-04.md) (star baseline).
Outputs are recorded in [doc 07 — improvement ledger](07-improvement-ledger.md) after each phase (problem → fix → test → before/after).

## Ground rules

1. **Copy before writing.** Each task names the upstream file(s). Copied files keep their licence header; each copy adds a line to `THIRD_PARTY_NOTICES.md` (created in Phase 0).
2. **Every phase is flag-gated, default OFF**, until its acceptance test passes. The existing path stays the fallback — a failed feature must degrade to today's behaviour.
3. **One phase = one branch/commit series**, with `cargo test --lib` (AGENTS.md baseline 874/874; lib now has 878 tests — re-run the full suite in Phase 0 to get the true baseline), `vitest` (baseline **154/154**), `tsc --noEmit` all green before the ledger entry is marked done.
4. **Never run a model inside the cpal audio callback** — worker thread only.
5. The working tree is dirty with another session's orb/TTS-beat work (see `git status`). Each phase starts from a clean commit of that work, or from a worktree, to avoid mixing changes.
6. No new Python process unless the phase says so; sidecars are lazy-start + idle-kill (the `lazy_nlu.rs` pattern).

## Phase 0 — Foundations (partly DONE)

| Task | Status | Detail |
|---|---|---|
| License audit | ✅ done | doc 02 |
| Smart Turn prototype | ✅ done (unwired) | `src-tauri/src/turn_detect.rs`, doc 03 |
| `THIRD_PARTY_NOTICES.md` generator | todo | `cargo about`/`cargo-license` + `license-checker` for `frontend/`, `server/worker/` |
| NEXUS `LICENSE` | **blocked on your decision** (doc 02 options A–D) | |
| Baseline capture | todo | run & store: idle RAM, wake FA soak (120 s), e2e fixture 49/50, `cargo test`/`vitest` counts → ledger "Before" |

## Phase 1 — Voice front-end: VAD + turn detection + barge-in  (targets: VAD/turn/barge-in ★2 → ★4) — **IMPLEMENTED 2026-10-05, live evaluation pending (see ledger 07)**

| # | Task | Source (Case) | Destination | Acceptance test |
|---|---|---|---|---|
| 1.1 | Silero VAD module | Handy `audio_toolkit/vad/{mod,silero,smoothed}.rs` + `silero_vad_v4.onnx` (C1-1) | new `src-tauri/src/vad/` + `resources/vad/silero_vad_v4.onnx` | unit tests with synthetic speech/silence; **silence soak: 0 false captures in 120 s**; hysteresis/pre-roll parity with current `STT_SPEECH_RMS_THRESHOLD` behaviour |
| 1.2 | Wire VAD into capture as an *additional* gate (flag `vadSilero`, default off) | — | `wakeword_oww.rs` capture loop (~L1601–1660), minimal diff | A/B on recorded clips: no more cut-offs than energy gate; noise-only clip does not start a turn |
| 1.3 | Smart Turn wire-in | prototype (C1-2) | worker thread fed from `STT_CAPTURE_BUFFER`; `turn_detect::should_end_turn(silence, 3, hard, prob, thr)`; flag `smartTurn` | **needs your 30 real recordings** (doc 03). Pass = fewer premature cuts on mid-thought set **and** median stop-latency ≤ today's 800 ms on complete set. If it doesn't beat the energy rule → leave off and record in ledger |
| 1.4 | Barge-in / "stop" instant tier | Pipecat `turns/user_start/{vad,min_words,transcription}` + `user_mute/*` design (C1-3) | `ghost.rs`/`orchestrator.rs` TTS-interrupt path | During TTS playback, bare **"stop"** halts speech (**corrected: ≈ 1–1.5 s via STT verify, not < 300 ms**; sub-300 ms needs an on-device stop-word spotter); echo of own TTS does not self-interrupt (mute-until-first-bot-complete strategy); regression test in orchestrator suite |
| 1.5 | `turnEagerness` setting low/medium/high | OpenAI `semantic_vad` spec (C2-1) | settings → mapped to Smart Turn threshold + min/hard silence | table-driven unit test of the mapping |

## Phase 2 — Local STT in Rust (spike, optional)  (targets: idle RAM, packaging)

| # | Task | Source | Acceptance test |
|---|---|---|---|
| 2.1 | Spike: `transcribe-rs` with the Moonshine model NEXUS uses | Handy `Cargo.toml` (C1-4) | Same 30 clips: WER within ±1 pt of the Python sidecar; cold-start & RAM measured |
| 2.2 | Decide: replace sidecar vs keep | — | Written decision in ledger with numbers; **no removal of the Python path in this phase** |

## Phase 3 — GPL-free offline TTS  (targets: TTS ★3 → ★4; licence blocker removed)

> **REVISED 2026-10-05 → see [doc 09](09-phase-3-kokoro-in-single-slot-architecture-2026-10-05.md).** Phase 3 must fit your Feature-83 cloud-primary + background single-slot swap design (voice change ⇒ background replacement). The table below is the original, superseded outline. Awaiting your decisions in doc 09 §7.

| # | Task | Source | Acceptance test |
|---|---|---|---|
| 3.1 | Vendor `pguso/kokoro` inference + `misaki-rs` (`default-features=false`) | C1-5 | `cargo tree` shows **no** `espeak-rs`/`espeak-rs-sys` when the Kokoro feature is on |
| 3.2 | New engine slot `Kokoro` in `tts.rs` tier chain (edge-tts → Kokoro → Piper) | — | first-audio latency measured; boundaries are `estimated: true` like Piper |
| 3.3 | OOV-word policy | — | list of 50 tricky words (names, apps: "WhatsApp", "Ghostwriter", "Nexus") pronounced acceptably — manual listen + lexicon overrides |
| 3.4 | Make Piper optional (cargo feature `piper`, default **off** in distributable builds) | — | build without it: `cargo tree` has no GPL; `cargo test` green |
| 3.5 | Verify misaki dictionary data licence upstream | — | recorded in `THIRD_PARTY_NOTICES.md` |

## Phase 4 — Skills, memory, scheduler  (targets: skills ★2 → ★4, memory ★3 → ★4, scheduler ★2 → ★4)

| # | Task | Source | Acceptance test |
|---|---|---|---|
| 4.1 | `SKILL.md` loader (front-matter parse, progressive disclosure, allow-list + SHA pin) | agentskills spec + Hermes/OpenJarvis (C1-7) | load 50 sample skills < 200 ms; unlisted/unpinned skill refused; **no code exec** path |
| 4.2 | Migrate `intents.yaml` open/say intents into skills (back-compat) | — | existing `agent_specs` tests (11) stay green |
| 4.3 | SQLite FTS5 memory search (episodic + facts) | Hermes `hermes_state*.py` design (C1-8) | seeded 100 facts: recall@3 ≥ 90 % on 30 paraphrased queries; p95 query < 20 ms; existing 8 memory tests green |
| 4.4 | Scheduler (persistent, missed-run policy) | Hermes cron design + `tokio-cron-scheduler` (C1-9) | fire-time error < 1 s; survives app restart; disabled by default |

## Phase 5 — MCP server + browser  (targets: MCP ★3 → ★4, browser ★2 → ★4)

| # | Task | Source | Acceptance test |
|---|---|---|---|
| 5.1 | Expose a **read-only** NEXUS tool set over MCP (`rmcp`, stdio) | C1-6 | An external MCP client lists tools & calls one; write/destructive tools **not** exposed |
| 5.2 | Spike: `browser-use` as lazy sidecar for "search/open/read page" | C1-11 | one scripted task ("open example.com, read the heading") passes; sidecar RAM + idle-kill measured; Python process absent when unused |

## Phase 6 — Safety depth  (targets: sandbox ★1 → ★3)

| # | Task | Source | Acceptance test |
|---|---|---|---|
| 6.1 | Adopt `execpolicy`-style prefix rules for any shell/exec tool | codex-rs `execpolicy` (C1-10) | refusal battery (existing Phase-4 ghost suite) + new rule tests |
| 6.2 | Spike: `windows-sandbox-rs` extraction cost | C1-10 | written go/no-go with dependency graph; no code merged in the spike |
| 6.3 | Optional directed-speech second pass | Apple papers (C2-2) | measured FA reduction on the soak set, else drop |

## Phase 7 — Grounding upgrade (spike)

| Task | Source | Acceptance test |
|---|---|---|
| OmniParser v3 detector vs current Groq/Gemini axis-grid on 20 UI screenshots | C1-12 | click-target accuracy table in the ledger |


## Case 2 phases (added 2026-10-05 — details in [doc 08](08-case-2-plan-2026-10-05.md))

| Phase | What | Gate before building |
|---|---|---|
| 8 | Directed-speech gate + TTS-echo rejection for hot-mic turns (`directed.rs`) — **IMPLEMENTED 2026-10-05, see change 82** | (done) labelled set + your OK on local logs |
| 9 | Proactive speech policy (breakpoints, urgency, access ritual, rate limits) for the Sentinel — **IMPLEMENTED 2026-10-05, see change 83** | (done) |
| 10 | Opt-in screen-context memory (UIA→OCR→FTS5, strict denylist/retention) — **design only** | your explicit scope + consent decision; Phase 4 FTS5 memory first |

## Deferred / parked

* Messaging channels (OpenClaw TS adapters) — security review first (R1).
* Moshi / full duplex, VAP, Unmute — GPU-class or unverified licences (C2-6..8).
* Proactive speaking rules (C2-5) and Screenpipe-style ambient capture (C2-4) — design later.
* Case 3 list (doc 04 §3) — waiting on your reel/handle names.

## Order & dependencies

`Phase 0 baseline → 1.1/1.2 → 1.4 (independent) → 1.3 (needs your recordings) → 3 (licence) → 4 → 5 → 2/6/7 spikes`.
Phases 1 and 3 are the highest value per effort and independent of each other.

## Star-rating targets (hypotheses, to be scored after testing — not promises)

| Pillar | Before | Target |
|---|---|---|
| VAD / turn / barge-in | ★★ | ★★★★ |
| TTS | ★★★ | ★★★★ |
| Skills | ★★ | ★★★★ |
| Memory | ★★★ | ★★★★ |
| Scheduler | ★★ | ★★★★ |
| MCP | ★★★ | ★★★★ |
| Browser | ★★ | ★★★★ |
| Sandbox | ★ | ★★★ |
| Average (18 rows) | 2.9 | ≈ 3.4 |
