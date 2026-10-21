# Deployment Readiness: Failure-Mode Analysis for ≤20% Problem Rate (2026-09-28)

**Question.** What breaks, how often, and what must be true before
NEXUS can ship to real users who churn after repeated failures?
Target: problems on at most 10–20% of interactions — deployment
grade, not demo grade.

**Method.** Stage-by-stage inventory of every known failure mode
(from this repo's audits, logs, and timelines), anchored to 2026
industry benchmarks, closed with chained-reliability math, SLOs,
and a phased hardening roadmap. Research only — no code changed.

**Verdict up front.** The deterministic core (wake → STT → regex
parse → local execute) can reach the 80–90% band with the hardening
below. Vision-grounded clicks and long-horizon compounds cannot —
the entire industry sits at 20–55% there (OSWorld 2.0 best: 20.6%
binary). The deployment promise must be tiered, or the 20% budget
dies on the first "click the red icon" miss.

---

## 1. Industry anchors (all 2026, all cited)

- **STT.** Whisper large-v3: ~4–5% WER clean English (Coval 5.2%,
  FLEURS English 4.1%); turbo +0.3–1.4 pts in European languages;
  **Hindi 15.7–22.3%, Arabic 14.6–16%** (Vocova, Jul 2026).
  Noisy rooms: 90.7–97.3% accuracy (VoiceCheck). Repetition-loop
  hallucinations: ~0.2% read speech, ~1% conversational
  (Koenecke-linked audits). Our stack (Groq `whisper-large-v3-turbo`
  primary, Moonshine-medium 6.65% WER fallback) sits exactly on
  these curves — no better, no worse.
- **Intent classification.** Rhasspy-style deterministic+ML pipelines:
  92.8–94.8% intent accuracy, low FP (VoiceCheck, 1,125 commands).
  Our candidate BERT-Mini: **0.9004 test intent** (phase-11 verbs);
  production promotion is a separate, manual decision — the shipped
  model may lag the candidate.
- **Endpointing (age bias).** Fixed 700 ms VAD thresholds cut off
  older speakers 2–2.5× more often; semantic turn models halve the
  gap (Aug 2026 benchmark). Our adaptive endpoint (400 ms → ~1 s
  patient, pause-count aware) is directionally right but unmeasured
  against this bias.
- **Grounding.** ScreenSpot-v2 ~90% (saturated, easy); high-res Pro
  ~36–40%; OSWorld-G ~50–54% (JEDI-7B, NeurIPS 2025). Our vision
  path (Groq/Gemini VLM 0–1000 coords + axis-grid) has **no measured
  accuracy on real user screens** — calibration suite exists
  (Phase 4) but results were never recorded.
- **Computer-use agents.** OSWorld 2.0: best config 20.6% binary /
  54.8% partial on long-horizon work; short tasks 20–24%; agents
  spend <7% of budget on error repair; top killers are dropped
  constraints, missed mid-task info, guessing instead of asking,
  skipped verification (2026). OSWorld-Pro: keyboard ≥89%, scroll
  ≥84%, clicks 72–92% (strong models) vs 39–42% (weak).
- **Lesson for us.** Keyboard-first is not a preference — it is
  where the entire industry's reliability lives. Every cursor-only
  flow inherits the 40–60% miss band.

## 2. Chained-reliability math (English, quiet room, deterministic command)

Per-command success ≈ product of stage reliabilities:

| Stage | Our measured / anchored rate |
|---|---|
| Wake recall (audited positives) | 0.96–0.99 @ 0.68 |
| STT command-correct (4-word cmd, 5% WER) | ≈ 0.81 |
| Parse (deterministic + alias map) | ≈ 0.90–0.95 |
| Execute (focus-verify, safety gates) | ≈ 0.95 |
| **End-to-end** | **≈ 0.66–0.73** |

That is a 27–34% problem rate TODAY on the happy path — above the
20% budget before any real-world noise. Each hardening item in §5
is priced against closing this gap: STT vocabulary biasing + alias
map already recover roughly half the STT→parse losses (the "goes to
mode" class); the remainder needs the eval harness (§4) to find.

Hindi / Hinglish / accented English: STT alone drops to ~0.78–0.84
per word → command-correct ≈ 0.45–0.60. **The 20% budget is
unreachable for these users without accent-specific work**
(§5.4). This is the largest demographic risk in the Indian market.

## 3. Failure catalog (12 modes)

1. **Mic driver silence (Intel SST).** RMS → exact 0.000000 for
   minutes. Mitigated (5 s poll, silent restart, 12-restart Audiosrv
   nuke, Meet-parity classification). Residual: admin-only fixes,
   other vendors untested. Needs: driver matrix (Realtek, USB,
   Bluetooth) + FA/hr log.
2. **Wake false accepts on TV/conversation.** Stage-2 STT verifier
   exists — but its default is disputed in-tree (struct default
   true vs orchestrator fire-on-detect). Residual: unknown FA/hr in
   real living rooms. Needs: 24 h TV-noise soak with counter.
3. **STT hallucinations.** Filter list + phantom-capture guard +
   VERIFY_RING removal. Residual: ~1% conversational confabulation
   (industry floor) + vocabulary gaps for new entities. Needs:
   confabulation counter + per-user learned corrections (exists,
   unmeasured).
4. **Deterministic parse brittleness.** Every unseen verb is a
   full miss ("create a new tab" class, Bug A wake-prefix class).
   Mitigated per-incident; no generative fallback for family
   builds (brain is admin-only compile gate). Needs: miss-mining
   → phrase promotion SLA (improve.rs exists — record-only; close
   the loop on a cadence).
5. **NLU staleness.** Candidate 0.9004 vs unknown production;
   promotion is manual; OTA channel exists but unproven in the
   field. Needs: promotion runbook + on-device accuracy probe.
6. **Hot-mic loop death.** Fixed 2026-09-28 (endGhostTurn, 30+
   sites) — but the class (N call sites, amendment-by-memory)
   recurs by construction. Needs: single turn-end choke point +
   loop-liveness watchdog (Rust-side: no transcript × seconds
   while session live → re-emit listen + log).
7. **Vision grounding miss.** Unmeasured; quota-capped (14.4k/500
   per day); race mode doubles spend. Needs: calibration results
   recorded per machine + "can't see it" honest-abort rate SLO.
8. **UIPI elevation.** Clicks into elevated windows fail silently
   (open follow-up). Needs: elevation detect → spoken reroute.
9. **Fullscreen swallow.** Mitigated (auto-pause). Needs: soak
   across games/players with synthetic-input matrices.
10. **Credential drift.** OAuth expiry, 20-day WhatsApp rotation,
    keychain variance across machines (the Groq-key class),
    manual settings.json edits. Mitigated case-by-case. Needs:
    startup self-test reporting per credential (not just logs).
11. **Backend SPOF.** Cloudflare Worker + Groq + edge-TTS are hard
    dependencies; offline = local-only degradation (untested as a
    whole). Bridges (WhatsApp :8765, Amazon :8766) are user-run
    binaries. Needs: offline-mode drill + bridge health in
    `nexus check` with fix commands.
12. **Concurrency flakes.** `test_install_and_cancel` fails under
    parallel threads (shared ACTIVE_REQUEST) — passes serially.
    A test-only symptom of a production-shared-mutable pattern
    (barge-in vs drill vs follow-up drain). Needs: ownership audit
    of request-scoped globals under ghost overlap.

## 4. Measurement plan (SLOs that define "deployment")

| Signal | Instrument | Gate |
|---|---|---|
| Wake FA/hr (TV + silence soak) | counter + 24 h fixture | < 1 FA / 8 h |
| Wake recall (per mic profile) | existing audits, scheduled | ≥ 0.95 @ 0.68 |
| STT WER (EN + Hindi fixtures) | Groq vs Moonshine harness | EN ≤ 6%, HI tracked |
| Confabulation rate | filter-hit counter | < 1.5% captures |
| Parse miss rate | missed_intents.jsonl miner | week-over-week ↓, SLA 7 d |
| Ghost loop liveness | Rust watchdog (new) | 0 deaf sessions |
| Vision hit rate (post-calibration) | calibration log | ≥ 0.70 UIA-miss fallback |
| Quota exhaustion events | existing counters → surface | 0 silent denials |
| Credential self-test | startup probe per service | all green or guided |
| E2E scripted pass (50-command suite) | new harness, per release | ≥ 0.80 EN quiet room |

No release without the suite green. Today's repo has the unit
counts (645 Rust / 42 FE / 52 Worker) but no E2E gate — that is
the single biggest deployment gap.

## 5. Hardening roadmap (priced against the 20% budget)

- **P0 — harness.** 50-command E2E fixture + nightly FA/hr soak.
  (Finds the next ten "create a new tab" classes before users do.)
- **P1 — loop liveness.** Rust-side ghost watchdog + single
  turn-end choke point; elevation detect; credential self-test.
- **P2 — accent coverage.** Hindi/Hinglish STT fixture, alias-map
  expansion from mined misses, promotion SLA + runbook.
- **P3 — graceful degradation.** Offline drill, quota-aware
  narration, bridge health + fix commands in `nexus check`.
- **P4 — rollout.** Staged rings (self → family → 10 users) with
  the SLO board as the promotion gate; rollback = previous
  installer + model snapshot.

## 6. Trust mechanics (why users leave, and what keeps them)

Churn comes from *silent* failure, not failure: a missed command
with no explanation teaches "don't bother". The existing rails —
per-step narration, confirm gates on irreversible acts, honest
aborts ("couldn't find X"), spoken quota notices — are the actual
retention system. Deployment rule: **every failure must be spoken,
attributed (mic/heard/misparsed/action), and recoverable in one
utterance** ("I heard X, did you mean Y?"). Add the attribution
line to every abort path — cheapest trust-per-line in the repo.

## 7. Tiered deployment promise (the honest contract)

- **Tier 1 — deterministic voice commands (EN, quiet):** target
  85–90% after P0–P2. Shippable.
- **Tier 2 — vision clicks on custom UI:** target 60–75%
  post-calibration. Ship as "beta vision", never the default path.
- **Tier 3 — long-horizon compounds:** industry 20–55%; promise
  *supervised* execution (narrate + confirm per step), not autonomy.
- **Hindi/accented:** measure first (P2), promise after.

## References

- Coval STT benchmarks (Whisper large-v3, measured 2026-09-23/24).
- Vocova Whisper accuracy benchmark, 12 languages (Jul 2026).
- VoiceCheck surgical voice prototype (Dec 2025).
- Fixed-threshold endpointing age bias (Aug 2026).
- OSWorld 2.0, long-horizon computer use (arXiv 2606.29537).
- OSWorld-Pro, process-based CUA eval (Sep 2026).
- OSWorld-G / JEDI grounding (NeurIPS 2025).
- In-repo: ghost-mode timelines, wake-word audits (docs 43–52,
  57–61, 65–68), orb-waves research, AGENTS.md program notes.
