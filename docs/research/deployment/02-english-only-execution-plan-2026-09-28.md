# English-Only Execution Plan to Deployment Grade (2026-09-28)

Scope directive: **English only. No other languages.**
Companion to `01-deployment-readiness-failure-mode-analysis-2026-09-28.md`
(the ≤20% problem-rate paper). Plan only — no code changed.

What rescoping removes: Hindi/Hinglish STT fixtures, multilingual
alias work, non-English NLU considerations, language auto-detect.
What stays: everything else, with fixtures and gates in English
(including a light en-IN accented-English sample set — still
English, still cheap, still the actual user base).

Non-English input policy (deterministic, one rule): STT pinned to
`language="en"` everywhere; non-English speech that mistranscribes
into Unknown routes to a single honest line ("I work in English
for now, sir") instead of the NLU lottery. No detection model, no
extra dependency.

Target: Tier-1 deterministic EN commands at 85–90% end-to-end
(from ≈0.66–0.73 today), vision beta + supervised compounds per
the tiered promise. No release without its phase gate green.

---

## P0 — E2E harness + nightly soak (first, unlocks everything)

| # | Task | Touches | Test | Gate |
|---|---|---|---|---|
| P0.1 | 50-command EN fixture (open/search/type/whatsapp/browser/ghost entry/exit/stop, quiet room) | `scripts/e2e_voice_fixture/` (new), CI job | fixture pass rate | ≥ 0.80 to exit P0 |
| P0.2 | Wake FA/hr soak: 8 h TV/conversation audio + 8 h silence, counter per profile | harness + `wakeword_oww.rs` counters | soak log | < 1 FA / 8 h |
| P0.3 | Confabulation counter (filter hits / total captures) | `stt.rs` | unit | tracked, < 1.5% |
| P0.4 | Miss-mining SLA job: `missed_intents.jsonl` → weekly top-10 → phrase candidates | `improve.rs` (close the loop) | miner test | SLA 7 d mine→candidate |

Effort: M. Exit: fixture + soak green on the dev machine.

## P1 — Loop liveness + environment honesty

| # | Task | Touches | Test | Gate |
|---|---|---|---|---|
| P1.1 | Rust-side ghost watchdog: session live + no transcript × N s → re-emit listen + log (kills the Active-but-deaf class structurally) | `ghost.rs`, `wakeword_oww.rs` | 3 Rust tests | 0 deaf sessions in fixture |
| P1.2 | Single turn-end choke point: route remaining raw `reset()` turn-ends through `endGhostTurn`; forbid new raw resets by grep-gate in CI | frontend | tsc + vitest | grep-gate clean |
| P1.3 | UIPI elevation detect → spoken reroute ("needs admin, sir") instead of silent click-fail | `live/commands/mouse.rs` | 2 Rust tests | honest-abort on elevated fixture |
| P1.4 | Startup credential self-test: per-service pass/fail + guided fix (not log-only) | `diagnostics.rs`, settings UI badge | 2 tests | all-green or guided |

Effort: M. Exit: P0 suite still green + 0 deaf sessions.

## P2 — EN parse robustness (the miss-killers)

| # | Task | Touches | Test | Gate |
|---|---|---|---|---|
| P2.1 | Bug A: wake-prefix strip retry-parse (`strip_leading_filler`), bare-"command center" guard | `intent_parser.rs` | 9 phrase tests | fixed set green |
| P2.2 | Bug C: in-session `open_app`/`whatsapp_chat` → ghost runners (narration + focus framing) | `recorder.ts`, `orchestrator.rs` | FE + Rust tests | ghost drill path hit in fixture |
| P2.3 | STT EN pin audit: `language="en"` + `temperature=0` on every Groq/local call; vocabulary prompt review | `stt_groq.rs`, `lazy_stt.rs` | unit | audit checklist signed |
| P2.4 | en-IN accented-English sample set (15–20 cmds) into P0 fixture; alias-map expansion from mined misses | fixture + `intent_parser.rs` | fixture | tracked separately, no gate |
| P2.5 | Endpointing self-check on EN fixtures (hesitant speakers, mid-thought pauses) | fixture report | — | cutoff rate tracked |

Effort: M. Exit: fixture ≥ 0.85.

## P3 — Graceful degradation

| # | Task | Touches | Test | Gate |
|---|---|---|---|---|
| P3.1 | Offline drill: kill Worker/Groq → local-only mode walkthrough, record gaps | manual + doc | checklist | gap list, no silent hangs |
| P3.2 | Quota-aware narration audit (every denial speaks + guides) | `vision.rs`, `router.rs` | unit | 0 silent denials in fixture |
| P3.3 | Bridge health + fix commands in `nexus check` (WhatsApp :8765, Amazon :8766) | `nexus.mjs`, `diagnostics.rs` | manual | check reports + fixes |

Effort: S. Exit: offline run completes without wedges.

## P4 — Rollout rings

Self (1 machine, 1 week SLO-green) → family (3–5 machines, mic/
driver variety) → 10 external users. Promotion gate = SLO board
(§4 of paper 01) green for the full ring duration. Rollback =
previous installer + model snapshot (both versioned before each
ring). **Non-goal at every ring: vision autonomy, non-English.**

## Order + dependencies

P0 → (P1 ∥ P2) → P3 → P4. P1.1 and P2.2 both touch ghost turn
flow — land P1.1 first (watchdog makes P2.2 verifiable). Total
effort ≈ M+M+S across ~4–6 focused sessions; verification each
landing: `cargo test --lib -- --test-threads=1`, clippy touched
ranges, `tsc`, `vitest`, P0 fixture.

## P0 status (built 2026-09-28)

- **P0.3**: `FILTER_TOTAL`/`FILTER_HITS` + `filter_transcript_counted`
  at all 4 STT call sites, `hallucination_stats()` + `stt_filter_stats`
  IPC, counter test. Rate = hits/total vs 1.5% SLO.
- **P0.4**: `WEEKLY_TOP_N = 10` + `top_suggestions()` + test;
  `improvement_report` already returns the ranked list (miner runs
  at boot, writes `suggested_phrases.json`).
- **P0.1**: `src-tauri/tests/e2e_voice_fixture.rs` — 50 EN commands,
  49/50 (98%) with one documented miss (`close pr 5` → close_app
  order gap, P2). Also fixed live: "stop please" → cancel_action
  (STOP_PHRASES claimed it, parser gave None).
- **P0.2**: `scripts/soak_wake_fa.py` (streaming trigger +
  3 s refractory semantics, --smoke/--full/--silence). Smoke:
  20 files, 0 triggers, peak 0.1033. Silence 120 s: 0 triggers.
  Full 8 h run scheduled (not run in-session).
- Verify: Rust 647/647 serial, clippy clean in touched ranges.

## P1 status (built 2026-09-28)

- **P1.1**: ghost relisten watchdog (`ghost.rs`: pure
  `should_watchdog_poke` + `note_ghost_activity` on enter/capture/
  drill-end + `spawn_relisten_watchdog` thread; `stt_capturing()`
  accessor incl. mock-wake stub; frontend `ghost:relisten` listener
  → guarded `maybeGhostRelisten`). Bounded: 15 s idle, 3 pokes/
  session. 3 Rust tests.
- **P1.2**: turn-end choke point — last raw turn-end wired
  (stage:notice failure path); deliberate raw resets carry
  `turn-end:keep-raw` (park, abortCapture, 8 s timeout, first-run
  greeting); `scripts/check-turn-ends.mjs` CI gate (negative-tested:
  fails exit 1 on unmarked reset).
- **P1.3**: UIPI elevation detect (`window_process_elevated` +
  `our_process_elevated` via TokenElevation; `Win32_Security`
  feature added) with pure `elevated_block_reason` gate hooked into
  `ghost_click` (spoken reroute, announced by existing error path).
  2 Rust tests.
- **P1.4**: API-keys row in boot diagnostics (informational, never
  alarms; keychain + settings.json fallback) + 2 tests;
  `HealthStatus.groqKey/geminiKey` → Connections Keys badge +
  Vision-keys guidance section.
- Verify: Rust 653/653 serial, e2e 49/50, clippy pre-existing only,
  tsc clean, FE 42/42, turn-end gate OK.

## P2 status (built 2026-09-28)

- **P2.1**: wake-prefix retry — `parse_deterministic` runs the full
  pipeline, then retries ONCE with `strip_wake_prefix` (word-boundary
  guarded; bare wake words never strip to empty) only when the first
  pass returned None — zero regression by construction. Added bare
  "command center" to `is_settings_command` so "nexus command center"
  survives the strip. 9-phrase matrix test (all previously None) +
  strip-guard test + Worker-owned remainders pinned ("nexus what's
  the weather" → none → NLU, unchanged).
- **P2.2**: `shouldGhostRoute` (pure, 2 tests) — live ghost sessions
  route `open_app`/`whatsapp_chat` to the orchestrator's ghost
  runners (narration + focus verify + session stays open), falling
  back to local execute + endTurn on orchestrator failure (never
  silent). `recorder.ts` ghost-route branch before local-execute.
- **P2.3**: STT EN-pin audit — all 3 Groq call sites already pinned
  `language="en"`; added `temperature=0` to the 2 missing call sites
  (transcribe_with_groq, transcribe_bytes_with_groq; verbose path
  already had it). Moonshine sidecar defaults `MOONSHINE_LANG=en`.
  CJK guard + hallucination filter already enforce EN-only actioning.
- **P2.4**: en-IN tracking fixture (15 rows, truth-encoded) — 15/15,
  tracked, NO gate. Two permissive-fallback truths documented
  ("open spotifai"/"open new tap" → open_app by design).
- **P2.5**: hesitant-speaker endpointing test — fast limit cuts the
  first pause, patient limit (pause_count ≥ 2) survives it, speech
  resume resets, patient limit still endpoints (no hang).
- Verify: Rust 656/656 serial, e2e 49/50 + accent 15/15, clippy
  clean in touched ranges, tsc clean, FE 44/44, gate OK.

## P3 status (built 2026-09-28)

- **P3.1**: offline drill walkthrough written
  (`03-p31-offline-drill-walkthrough-2026-09-28.md` — stage-by-stage
  audit table + 5-item gap list). One code fix shipped: Worker-dropped
  turns while offline now speak the attributed offline line
  ("Network's down, sir — I've gone local…") instead of raw error
  JSON; loading indicator still hides before the error emits.
- **P3.2**: quota narration audit — vision quota switches already
  spoken (mouse.rs quota_hit announce); keyless vision gate spoken;
  exhausted-limits abort now speaks the limit + recovery; Worker
  denials verified speakable (quota.reason is a full sentence — new
  Worker test pins 4 denial classes; 53/53). `quota_exceeded` flag
  logged Rust-side for badging.
- **P3.3**: bridge health + fix commands — `scripts/mcp_check.py`
  DOWN lines now carry per-bridge fix hints (verified live); probe
  wired into `nexus check` (10 passed + live bridge section, doesn't
  gate the tool check). `nexus mcp check` unchanged for deep probes.
- Verify: Rust 656/656 serial, e2e 2/2 green, Worker 53/53,
  clippy clean in touched ranges, tsc clean, FE 44/44, gate OK.

## P4 status (operational, 2026-09-28)

- **Soak hardening + finding**: per-file fresh state is now the
  soak default (the old cross-file embedding carryover MANUFACTURED
  a trigger: hey_0007 stitched 0.733, isolated <0.09; `--stream`
  kept for continuity work). Real finding: `necess_0003.wav`
  false-accepts at 0.821 in isolation (1/500 clips, soundalike
  class) — with `verifyWake` defaulting false this reaches the fire
  path. Ring-1 config: enable `verifyWake` + promote the TRIGGER
  file list into the next retrain's hard negatives.
- **SLO collector**: `scripts/collect_slo.py` assembles the ring
  gate board from on-device artifacts (parse misses, weekly top-10,
  watchdog pokes, recovery restarts, hallucination hits, quota
  denials) + prints the release-gate commands. Verified live.
- **Runbook**: `04-p4-rollout-rings-runbook-2026-09-28.md` — ring
  structure, promotion gates, per-machine checklist, rollback,
  ring-1 findings, artifact hashes.
- **Ring-1 artifact built + hashed**: MSI 201.7 MB +
  release `nexus.exe` 50.7 MB (NSIS broken locally — plugin cache;
  MSI is the artifact). Rust 656/656 at freeze.

## Explicit non-goals

Non-English STT/NLU, language detection, Hindi fixtures,
multilingual TTS voices, vision autonomy promises, long-horizon
unsupervised compounds. Anything in this list arriving as a
request routes to the tiered promise (§7 of paper 01), not into
these phases.
