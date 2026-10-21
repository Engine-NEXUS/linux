# FIFO Voice-Command Queue ("Real Ghost Mode") + Hotkey/Esc Realignment — Full Research, Comparison & Execution Plan

**Date:** 2026-09-30
**Status:** PLAN ONLY — no code changed as of this document. Awaiting user cross-check & approval.
**Author:** opencode agent (voice/ghost research lane)
**Reviewers:** user (admin), Gemini orchestrator-lane session (must re-read before touching `orchestrator.rs` / `intent_parser.rs` / `ghost.rs`)
**Companion docs:**
- `docs/research/ghost-mode/04-program-consolidation-and-hotmic-repair-2026-09-28.md` (hot-mic loop internals)
- `docs/features/70-ghost-mode-complete.md` (program guide)
- `docs/features/ghost-mode-architecture-reference.md` (symbol map + Send invariant)
- `docs/changes/55-ghost-waves-browser-search-typing-and-intent-isolation-implementation.md` (Phase 3 follow-up queue)

---

## 0. One-Paragraph Summary

The user wants **FIFO voice command queuing inside Ghost Mode**: while NEXUS is executing a command, the mic stays free, the next spoken command is captured and **queued**, and the queue drains strictly in first-in-first-out order — producing a true "hands-free script" feel with a target cadence of ~1 second between commands. Simultaneously, the global hotkey (Ctrl+Space) must become a **second-press cancel** (kill the call, hide the orb when idle) and, during TTS playback, must **stop the speech and immediately start listening**; window-closing moves off the hotkey onto **Esc**. This document: (1) audits the current code so we know exactly what already exists, (2) compares five candidate architectures against how Alexa / Google Home / GPT-4o Voice / Gemini Live / OpenAI Operator / ROS actionlib actually behave, (3) rates every approach on speed / delay / error / miscommunication / complexity with evidence, (4) exposes the four failure modes of *pure* FIFO, (5) specifies the recommended **Approach E = FIFO + 4 preemption rules**, and (6) gives a file-by-file, test-gated implementation plan.

---

## 1. Current State Audit (what exists today, with file:line evidence)

### 1.1 The gap anatomy — why turn-by-turn feels delayed (3–6s)

Measured chain for a ghost follow-up turn (all timings from code constants, not guesses):

| # | Stage | Where | Timing |
|---|-------|-------|--------|
| 1 | Drill step executes (open Brave, click, etc.) | `ghost.rs` runners / `live/commands/*` | 0.2–0.8s (varies; UIA + glide) |
| 2 | Narration TTS spoken ("Opened Brave, sir.") | `orchestrator.rs::speak_line` | 1.0–2.0s speech |
| 3 | Turn-end → `endGhostTurn()` | `recorder.ts` local/VAD branches, `orchestrator.ts` error + decline, Tier-3 listener, `stage:notice`, speaking failsafe, legacy `wsBridge.ts` | immediate |
| 4 | `maybeGhostRelisten()` → `waitForAudioIdle` **polls up to 3000ms** | `frontend/src/net/ghostHotMic.ts:53` | 0–3000ms (echo guard) |
| 5 | `triggerFollowupListen()` → `__NEXUS_WAKE__` + `start_stt_capture` | `frontend/src/main.tsx:37-48` | ~10ms |
| 6 | Rust endpoint: **400ms fast / 1000ms hesitant** silence | `wakeword_oww.rs:2549-2554` (`STT_SILENCE_CHUNK_LIMIT=5`, `_PATIENT=12`, 80ms chunks) | 400–1000ms |
| 7 | STT (Groq Whisper large-v3-turbo, temp 0) | `stt_groq.rs` | 250–2000ms |
| 8 | Parse + route + execute next step | `intent_parser.rs` → `center.rs` → runner | 10–500ms |
| — | **TOTAL perceived gap** | | **≈ 3–6s, highly variable** |

**Key insight:** stages 2+4 (narration + 3s echo poll) dominate the delay. The endpoint (stage 6) is already tuned to 400ms — the industry inter-turn target is ~200–300ms natural / ~800ms–1s where users start talking over the agent (Zylos 2026 research, §7 Source S6). We are **at or past the "users talk over it" threshold** — which is precisely why a FIFO queue (never re-wake, never wait for narration to fully clear) is the correct architectural answer rather than more latency shaving.

### 1.2 What already exists for queueing (partial FIFO — read this before building anything)

| Facility | Location | Behavior | Reuse verdict |
|---|---|---|---|
| `FOLLOWUP_QUEUE: VecDeque<String>` | `ghost.rs:158-160` | FIFO of raw transcripts captured mid-drill | **REUSE as the queue core** |
| `FOLLOWUP_CAP = 5` | `ghost.rs:164` | drop-oldest beyond 5 | **REUSE** (bounded) |
| `DRAIN_CAP = 10` | `ghost.rs:165` | max drained per pass (nested-drill ping-pong bound) | **REUSE** |
| `queue_followup()` | `ghost.rs:236-243` | push_back + drop-oldest, returns kept bool | **REUSE** |
| `take_followups()` | `ghost.rs:246-248` | drain-all → `Vec<String>` | **REUSE** |
| `drop_followups()` | `ghost.rs:252-259` | clear on abort/failure, logs count | **REUSE as "cancel clears pending"** |
| `drill_running()` / `drill_begin()` / `drill_end()` | `ghost.rs:210-231` | `GHOST_BUSY` counter gate for intercept + drain | **REUSE as executor-busy signal** |
| `request_stop()` / `stop_requested()` / `clear_stop()` | `ghost.rs:197-208` | `GHOST_CANCEL` atomic, observed at step boundary | **REUSE as priority-lane cancel** |
| `is_stop_phrase()` + `STOP_PHRASES` | `ghost.rs:171-193` | 17 exact-match stop phrases, parser-independent | **REUSE as priority-lane classifier** |
| Stop-word intercept during drill | `orchestrator.rs:803+` ("Ghost drill overlap: stop-words + follow-up queue") | routes stop words to cancel, others to queue | **REUSE, extend** |
| `is_cancelled_pub()` per-step guard | `command_center.rs:513, 712, 940` | compound steps check cancel flag | **REUSE as executor-side abort** |
| Relisten watchdog (3 pokes/session) | `ghost.rs:140-156` + `ghost:relisten` event | recovers dead frontend loop | **REUSE, keep budget** |
| Silent-miss anti-nag (3 quiet re-listens → 1 nag → park) | doc 44 | prevents nagging | **REUSE unchanged** |
| `GHOST_SILENT_CAP = 3` | `ghostHotMic.ts` | stop after 3 misses | **REUSE** |

**Conclusion:** the *capture* half of FIFO exists and is battle-tested (Phase 3, doc 55). What is missing is the **execution half**: a formal serial executor that (a) keeps the mic hot while a step runs, (b) drains strictly FIFO, (c) applies priority/preemption rules, and (d) re-verifies grounding at dequeue time. This plan specifies exactly that.

### 1.3 Hotkey/Esc current truth table (`src-tauri/src/hotkey.rs:67-179`)

| Press context | Current behavior | Code | Target behavior |
|---|---|---|---|
| Ghost session live | End session + stop TTS | `hotkey.rs:75-77` (`cancel_active` + session end) | KEEP (already correct) |
| TTS speaking | `stop_tts()` + `cancel_active()` + **return — does NOT start listening** | `hotkey.rs:90-100` | **CHANGE → also start capture + show orb (barge-listen)** |
| Any NEXUS window visible | Close windows only, no wake | `hotkey.rs:134-150` | **CHANGE → move to Esc; hotkey falls through to wake** |
| Idle, orb hidden | Wake: show orb + `__NEXUS_WAKE__` + `start_stt_capture` | `hotkey.rs:151-179` | KEEP |
| 2nd press while listening, no speech | Frontend no-ops: `"already listening, ignoring wake"` (`main.tsx:121-124`); **no Rust capture-abort exists anywhere** (grep-confirmed: no `stop_stt_capture` symbol in `src-tauri/src`) | — | **CHANGE → abort capture; if no voiced audio, hide orb + reset; if speech in progress, let turn finish** |
| Esc (always-on) | Not registered globally. Registered **dynamically per ghost session only** (`ghost.rs:443-469`), unregistered on abort/exit/stand-down (`ghost.rs:428, 462-469, 571, 582`) | — | **CHANGE → also close NEXUS sidebar/settings windows (frontend-level, see Rule C3)** |

### 1.4 Known constraints that shape the design (do not violate)

1. **Mic exclusivity:** WebView2 `getUserMedia` + Rust cpal wake stream conflict on Intel SST (`main.tsx:87-92` warmMic disabled). The mic is acquired by **one** owner at a time — the queue must never open a second capture while Rust owns the device.
2. **`cancel_active()` semantics:** sets the current request's cancel flag; observed at **step boundaries** (`command_center.rs:513/712/940`), not mid-syscall. So "cancel" latency = time to next boundary — this is a feature (no half-clicked buttons), must be documented to the user.
3. **No `stop_stt_capture` today:** the Rust capture self-terminates only via endpoint/max/no-speech (`should_stop_capture`, `wakeword_oww.rs:2571-2581`) — 400ms/1s, 10s max, 8s no-speech. A true second-press cancel needs a new abort command (§5.1 A1).
4. **Echo/self-hearing:** TTS playback energy can re-trigger capture. The existing `waitForAudioIdle` echo guard and the 300ms post-TTS DAC-drain mute gate (`main.tsx:161-164`) exist for measured acoustic reasons — any cadence change must preserve *a* mute window (may be shortened, never deleted).
5. **Drop-oldest is current policy** (`queue_followup` pops front at cap). Dropping the **oldest** is wrong for a script user (they said it first — it matters most); dropping the **newest** is wrong for responsiveness. §4 Rule 2 resolves this per-lane.
6. **Global Esc via `RegisterHotKey` consumes the keystroke** — cannot be passed through to other apps (documented Windows behavior; this is why ghost's Esc is dynamic and session-scoped). Never register an always-on Esc (§5.3 C1 rejected).

---

## 2. The User's Proposed Method — Formal Spec

**Verbatim user intent (2026-09-30):**
> "I want first-in-first-out: if I said 'open the brave' → task taken → sent to execution. While execution, the mic will be free, so I will take the next command and wait for the previous task to be executed before sending the next task."

### 2.1 Formal restatement

```
STATE: ghost session active, executor idle
USER SPEAKS cmd₁  → capture → STT → parse → ACK (short) → enqueue(cmd₁)
EXECUTOR idle & queue non-empty → dequeue head → re-verify grounding → execute
MIC: remains in capture-ready state during execution (hot-mic loop, no re-wake)
USER SPEAKS cmd₂ (while cmd₁ executes) → capture → STT → parse → ACK → enqueue(cmd₂)
cmd₁ completes → (gap τ ≈ 1s) → dequeue cmd₂ → execute → ...
ORDER: strict FIFO within the normal lane
CANCEL: never queued — preempts instantly (see Rule 1)
```

### 2.2 Grade of the proposal as stated: **8/10**

| Aspect | Verdict | Note |
|---|---|---|
| FIFO ordering | ✅ Correct | Right model for ordered scripts |
| Mic free during execution | ✅ Correct | Hot-mic already does capture; needs executor decoupling |
| Wait-for-previous before next send | ✅ Correct | Serialization is the safety property |
| Bounded queue | ⚠️ Implied, must specify | Unbounded = runaway mic builds zombie plans (existing comment `ghost.rs:233-234`) |
| Cancellation semantics | ❌ Missing | "cancel" must NOT queue behind the thing it cancels |
| Contradiction/supersede semantics | ❌ Missing | "open Brave… open Chrome" must not both run |
| Stale grounding at dequeue | ❌ Missing | Page/window may have changed while queued |
| Head-of-line blocking | ❌ Missing | A 60s task blocks a queued "stop" |
| User awareness of depth | ⚠️ Missing | Silent queue = "did it hear me?" doubt |

The missing 2 points become **Rules 1–4** in §4.

---

## 3. Industry Comparison — How Real Systems Handle "command while executing"

### 3.1 Master comparison table

| System | Method | Queue or Cancel? | Priority handling | Barge-in | Mid-action safety | Evidence / source |
|---|---|---|---|---|---|---|
| **Alexa (AVS)** | Strict serial state machine (Idle→Listen→Think→Speak); user utterance **always top priority** over TTS/alerts/media | **Cancel/replace** — customer utterance interrupts Alexa's speech; interrupted TTS is **not resumed** (scenarios 1–3), media is paused then **not resumed** if replaced | Documented 5-level precedence: 1) customer input, 2) TTS, 3) alerts, 4) notifications, 5) media. "Customer must always be able to dismiss" | ✅ wake word / Action button during TTS pauses TTS | Alexa **does not execute queued third-party commands** — one intent per turn | Amazon AVS UX Interrupts Guidance (S2) |
| **Google Assistant / Nest** | Turn-based; duplex conversation mode for pure Q&A | Cancel/replace | Customer > TTS > media | ✅ | Actions are single-turn; no cross-turn action queue | Industry baseline; covered indirectly by LSLM survey (S5) |
| **Siri (classic)** | Half-duplex turn-based; cannot listen while speaking | Cancel/replace | Customer > TTS | ✅ (button/silent) | Single intent per turn | LSLM survey characterization of turn-based assistants (S5) |
| **GPT-4o Advanced Voice / Realtime API** | Full-duplex: streams mic↔model; server VAD detects interruption → **cancel generation + flush playback + reconcile history** via `conversation.item.truncate(audio_end_ms)` | **Cancel + replan** (never queue — context goes stale) | Interruption token in-model; `activity_handling` can mark responses non-interruptible | ✅ native, ~232ms | Truncation edits conversation history so the model doesn't assume unheard words were heard | OpenAI Realtime docs via Zylos (S6); Zan (S4); Lohrbeer (S3) |
| **Gemini Live API** | Server VAD; `serverContent.interrupted`, generation canceled & discarded | Cancel + replan | `activity_handling` opt-out | ✅ (known race: interrupted event sometimes missing) | Discarded-turn handling incomplete per docs | Zylos (S6), Zan (S4) |
| **Amazon Nova Sonic** | `stopReason: INTERRUPTED` in `contentEnd` | Cancel + replan | — | ✅ | Client responsible for dropping unplayed chunks | Zylos (S6) |
| **OpenAI Operator / Claude Computer Use** | Plan → execute stepwise → **re-verify screen before every step**; pause-and-ask on ambiguity; never run a stale plan blindly | Not a conversational queue — single agent loop | Confirmation gates on destructive steps | n/a | **Best-in-class stale-state safety** (this is the pattern for dequeue re-verification) | Product docs / observed behavior (S7) |
| **ROS actionlib (robots/industrial)** | Goal-based action server; **single-goal policy**: later GoalID preempts earlier; explicit preempt cancels all ≤ stamp; accept-new-goal ⇒ old goal auto-preempted; states PENDING→ACTIVE→PREEMPTING→PREEMPTED/ABORTED/SUCCEEDED | **Queue + preemption policy (the industrial gold standard)** | Per-action preemption policy configurable | n/a | Client `sendGoalAndWait(execute_timeout, preempt_timeout)` bounds poison steps | ROS actionlib docs (S8, S9, S10) |
| **Print spoolers / build queues (Jenkins, CI)** | Pure FIFO jobs | Queue | Priority lanes / cancel-queued-job | n/a | Jobs are **independent** — no shared mutable state between jobs | General systems knowledge |
| **Kubernetes / Mesos schedulers** | Queue with **priority classes**, preemption of lower-priority pods, backoff, per-job timeout | Queue + priority + preemption | Native priority classes | n/a | Resource conflicts resolved by scheduler, not by luck | General systems knowledge |
| **Human teams ("send me a task list")** | Batch hand-off, worker serial | Queue | Verbal priority ("do this FIRST") | ✅ | Re-confirm ambiguous items | Common sense |
| **NEXUS today (B)** | Strict serial + barge-cancels + partial follow-up queue (capture half only) | Cancel between turns; queue exists but drains only at drill end | Stop-words preempt (`STOP_PHRASES`) ✅ | ✅ (`startListening` barge-in path) | Focus-verify in ghost runners ✅ | this repo, §1.2 |

### 3.2 What the industry consensus actually is

1. **Conversational AI: cancel, don't queue.** Alexa/GPT-4o/Gemini all treat the user's next utterance as *replacement* intent, because in pure conversation the old answer is worthless. Queueing raw dialogue turns is an anti-pattern.
2. **Action systems (robots, schedulers, jobs): queue with preemption policy.** ROS actionlib and Kubernetes both combine FIFO with (a) priority lanes, (b) explicit preempt/cancel that jumps the queue, (c) per-goal timeouts.
3. **Agentic UI automation: re-verify before each step.** Operator/Claude Computer Use re-ground on screen state before every action because the world changes between plan and execution.
4. **Barge-in always wins over TTS** (Alexa's documented 5-level precedence maps 1:1 onto our intended state machine).
5. **The "4-step barge-in" is industry-standard** (Zylos S6): detect overlap → cancel server generation → flush client playback → **reconcile state to what was actually heard**. NEXUS has steps 1–3 (`stopTts` + `cancel_active` + `setBargedIn`); step 4 is our `drop_followups()`/history-clear analog — must be explicitly wired in the hotkey path (§5.1 B1), not assumed.
6. **Latency bands:** ~200–300ms natural · ~500ms noticeable · ~800ms–1s users start talking over it · >1.5s "broken" (Zylos, S6). **Our target τ = 1.0s sits at the top of the tolerable band; chained/silent steps should hit 0.3–0.5s.** This sets the acceptance bar (§6).

### 3.3 Why NEXUS should be **hybrid** (not a pure clone of any one system)

NEXUS is simultaneously:
- a **conversational agent** (TTS replies → cancel semantics fit), and
- an **action executor** (clicks, keys, opens → queue semantics fit), and
- a **UI automation agent** (stale-state risk → re-verify fits).

No single industry system is all three. Alexa is 1+3-partial, Operator is 3-only, actionlib is 2-only. Therefore: **queue only actions, cancel only conversation, re-verify always.** That is Approach E.

---

## 4. The Five Approaches — Rated

### 4.1 Definitions

- **A. Pure FIFO** — user's proposal exactly: mic hot, serial executor, bounded queue, drop-oldest, no extra rules.
- **B. Strict serial + barge-cancels** — NEXUS today: one turn at a time, next utterance cancels current, no cross-turn action queue.
- **C. Full-duplex cancel+replan** — GPT-4o Voice style: always listen, interruption instantly cancels and replans; never queue.
- **D. Batch planner** — user speaks an entire chain in one utterance ("open brave then new tab then search almonds"), parser builds one plan, executor runs steps with sleeps, zero inter-step STT.
- **E. FIFO + preemption lanes (RECOMMENDED)** — Approach A plus Rules 1–4 below.

### 4.2 Master rating table (★ = 1 worst … 5 best)

| Criterion | A. Pure FIFO | B. Serial+barge (today) | C. Full-duplex cancel | D. Batch planner | **E. FIFO + lanes** |
|---|---|---|---|---|---|
| **Speed — 3 chained commands** | ★★★★☆ | ★★★☆☆ | ★★★☆☆ | ★★★★★ | ★★★★☆ |
| **Inter-command delay predictability** | ★★★★☆ | ★★☆☆☆ | ★★★★☆ | ★★★★★ | ★★★★★ |
| **Fastest possible cadence** | ~1.0s | 3–6s | ~1.0s | ~0.2s | ~0.3–1.0s (adaptive) |
| **Error risk (bad execution)** | ★★★☆☆ | ★★★★☆ | ★★☆☆☆ | ★★★★☆ | ★★★★★ |
| **Stale-grounding safety** | ★★☆☆☆ | ★★★★☆ | ★★★☆☆ | ★★★☆☆ | ★★★★★ |
| **Miscommunication risk** | ★★★☆☆ | ★★★★☆ | ★★★☆☆ | ★★★★☆ | ★★★★★ |
| **Cancel responsiveness** | ★★☆☆☆ (queued!) | ★★★★★ | ★★★★★ | ★★★★☆ | ★★★★★ |
| **Head-of-line blocking** | ★★☆☆☆ | ★★★☆☆ (serial anyway) | ★★★☆☆ | ★★★☆☆ | ★★★★★ (priority lane) |
| **User awareness / trust** | ★★★☆☆ | ★★★★☆ | ★★★★☆ | ★★★★★ | ★★★★★ (depth ACK) |
| **Implementation complexity** | ★★★★★ (simplest) | ★★★★☆ (exists) | ★★☆☆☆ | ★★★☆☆ | ★★★☆☆ |
| **Testability (unit-testable purity)** | ★★★★★ | ★★★★☆ | ★★☆☆☆ | ★★★★★ | ★★★★☆ |
| **Risk of regressions to existing flows** | ★★★★☆ | ★★★★★ (baseline) | ★★☆☆☆ | ★★★★★ | ★★★★☆ |
| **OVERALL (weighted, §4.4)** | 3.4 | 3.6 | 3.1 | 3.8 | **4.5** |

### 4.3 Detailed pros/cons per approach

#### A. Pure FIFO (user's proposal, as stated)
**Pros:** simplest mental model; no re-wake cost; predictable order; direct reuse of `FOLLOWUP_QUEUE`; great for scripted multi-step work.
**Cons (the 4 failure modes, §4.3.1):**
- F1 "cancel" queues behind its own target.
- F2 head-of-line blocking (60s task starves "stop").
- F3 contradictions both execute.
- F4 stale grounding executes blind.
- F5 (bonus) silent queue → user doubt "did it hear me?"

#### B. Strict serial + barge-cancels (NEXUS today)
**Pros:** every turn parsed fresh against current world state; nothing stale ever runs; cancel always works; safest default; already shipped.
**Cons:** 3–6s variable gap (§1.1); narration occupies the gap; chained scripts feel sluggish; no cross-turn memory of queued intent.

#### C. Full-duplex cancel+replan (GPT-4o style)
**Pros:** instant interruption; natural conversation; newest intent always wins.
**Cons:** cancelling **mid-click/mid-type corrupts UI state** (half-typed message, half-open dialog); "which version did it hear?" confusion; needs server-side VAD + history reconciliation (the truncate step — S4/S6) that NEXUS's local pipeline doesn't have; highest regression risk; wrong tool for *actions* (industry consensus §3.2-1).

#### D. Batch planner (one utterance = one plan)
**Pros:** machine cadence (~0.2s gaps, no inter-step STT at all); zero mid-chain mishearing; trivially testable (pure plan → run).
**Cons:** requires the user to speak the whole chain upfront (doesn't fit the user's stated "take next command while executing" flow); one mis-heard word poisons the whole plan; parser must handle conjunction ambiguity ("and then" vs "and"); no mid-course correction without cancel.

#### E. FIFO + preemption lanes (recommended)
**Pros:** keeps A's script feel, fixes all 5 A-cons, reuses ~10 existing facilities (§1.2), keeps B's per-turn freshness for *conversational* turns, keeps D's fast path for chains (via optional "and then" chaining → falls into a batch sub-plan), and keeps existing focus-verify safety (Operator rule).
**Cons:** most moving parts of the five (but each part already exists in the codebase); needs a small state enum + tests; must be gated behind `ghost::session_active()` to avoid touching normal mode.

### 4.4 Weighted scoring (edit the weights to cross-check my opinion)

| Criterion | Weight | A | B | C | D | E |
|---|---:|---:|---:|---:|---:|---:|
| Speed / cadence | 20% | 4 | 2 | 3 | 5 | 4 |
| Delay predictability | 15% | 4 | 2 | 4 | 5 | 5 |
| Error safety (execution) | 20% | 3 | 4 | 2 | 4 | 5 |
| Miscommunication / trust | 15% | 3 | 4 | 3 | 4 | 5 |
| Cancel responsiveness | 15% | 2 | 5 | 5 | 4 | 5 |
| Implementation simplicity / regression risk | 15% | 5 | 4 | 2 | 4 | 3 |
| **Weighted total (×20 → /100)** | | **68** | **72** | **62** | **76** | **90** |

*(Round to nearest integer from raw weighted sums; my hand-calc may be off by ±2 — recompute when editing weights. The ordering E > D > B > A > C is robust to ±10% weight changes.)*

### 4.3.1 Failure-mode scenarios (why A loses points) — walk these through yourself

| # | Script | Pure-FIFO outcome | Correct outcome (E) |
|---|---|---|---|
| F1 | "open Brave" … "actually, cancel" | *cancel* enters queue **behind** "open Brave" → Brave opens, then cancel runs (no-op / confusing) | Rule 1: cancel jumps queue → Brave step aborted at boundary, queue dropped |
| F2 | "analyse repo" … "stop" | stop waits ~60s for analyse to finish | Rule 1: stop is priority-lane, preempts at next boundary |
| F3 | "open Brave" … "open Chrome" | both execute → two browsers, focus fights | Rule 2: same-slot (app-open) → Chrome **supersedes** pending Brave; if Brave already running, it is closed or left per slot policy |
| F4 | "open Brave" … (page loads, user clicks elsewhere, 5s later) … "click Send" | Send clicked on wrong window | Rule 3: re-verify focus/element at dequeue; fail → one spoken line, skip |
| F5 | user speaks cmd while queue depth 3, gets no ACK | "did it hear me?" → user repeats → duplicate | Rule 4: depth ACK ("queued, sir — 3 waiting"), once per session-threshold |
| F6 | poisoned step (element never appears, 60s UIA wait) | blocks queue forever (head-of-line) | Rule 4: per-step timeout kills poison step, speaks "skipping, sir", continues |
| F7 | session abort / Esc mid-queue | stale commands execute after context gone | `drop_followups()` on abort (already exists — wire it into every abort path) |

---

## 5. Recommended Package — Approach E + Hotkey Realignment

### 5.0 Approach E — the 4 rules (formal)

| Rule | Name | Spec | Mechanism (reuse / new) |
|---|---|---|---|
| **1** | Priority lane | Any `STOP_PHRASES` match or `cancel_action` intent **never enters the queue**. It: (i) sets `GHOST_CANCEL`, (ii) clears the normal lane (`drop_followups()`), (iii) speaks "Stopped, sir." (or stays silent if nothing was running → "Nothing running, sir.") | REUSE `is_stop_phrase`, `request_stop`, `drop_followups`; NEW: a `lane()` classifier fn (pure, unit-tested) |
| **2** | Supersede-by-slot | Pending commands are keyed by **slot class**. Same slot ⇒ replace in place (`app_open`, `navigate`, `focus` are singleton slots; e.g. "open Brave" then "open Chrome" ⇒ Chrome replaces Brave in the queue). Different slots ⇒ append (`new_tab`, `search`, `type_text` chain onto the app-open). In-flight (already dequeued) step is NOT superseded — it completes or is cancelled by Rule 1. | NEW: `slot_class(intent) -> SlotId` pure fn + queue entry becomes `QueuedCmd { id, slot, transcript, parsed, spoken_at_ms }` |
| **3** | Re-verify at execution | On dequeue, re-run grounding **before** acting: window/process focus check + element resolution (ghost runners already do focus-verify before+after — `live_ghost_click`). If grounding fails: skip step, speak one line ("Couldn't find it, sir — skipped."), continue queue. Never execute blind. | REUSE runner focus-verify; NEW: `verify_grounding(&QueuedCmd) -> Result<(), SkipReason>` hook point in executor |
| **4** | Bounded + announced + timed | Keep `FOLLOWUP_CAP = 5`. Policy at cap: **drop-newest for normal lane** (oldest = user's first intent, most valuable; newest is a repeat/refinement of a thought they may restate) — NOTE: this INVERTS current drop-oldest; gate the change behind tests. Depth ACK: when queue depth ≥ 2, speak a short "Queued, sir." once per session (never per-command). Per-step timeout: default 15s (config `ghostStepTimeoutMs`), poison step → abort step, log, continue. | REUSE cap; NEW: depth-ACK flag + timeout wrapper |

**Gap τ policy (ties to §1.1):**
- Between two **queued** steps with narration: τ = `ghostTurnGapMs` (default **1000ms**) measured from narration-end, not narration-start.
- Between two **silent/fast** steps (no TTS): τ = **250ms** (just the drain beat).
- `waitForAudioIdle` cap: 3000ms → **1000ms** in ghost mode (echo guard retained, shortened); skipped entirely if the completed step was silent.
- Keep 300ms DAC-drain mute after TTS (acoustic necessity), but it overlaps τ rather than stacking on it.
- **Target: chained silent steps 0.3–0.5s; narrated steps ≤1.5s** (within/below the "users talk over it" 800ms–1s band for the *usable* part of the gap; narration is information, not dead air).

**Mic ownership invariant (§1.4-1):** during executor-busy, exactly one capture owner. The hot-mic loop (`maybeGhostRelisten` → `triggerFollowupListen`) remains the *only* capture starter while `drill_running()` is true; the normal wake path must check `drill_running()` and defer (or become the queue producer rather than a second capture).

### 5.1 Feature 2a — Ctrl+Space second-press cancel

| Option | Description | Verdict |
|---|---|---|
| **A1. Rust abort command (RECOMMENDED)** | New Tauri command `stop_stt_capture() -> { had_speech: bool, elapsed_ms: u32 }`: (i) set `STT_CAPTURING=false` (same store the capture loop reads at `wakeword_oww.rs:1651`), (ii) flush any buffered samples without STT call, (iii) return whether ≥`STT_MIN_VOICED_CHUNKS` voiced chunks were seen (`STT_VOICED_CHUNKS`, `wakeword_oww.rs:2523`). Frontend on 2nd press while `state==="listening"`: call `abortCapture()` + `invoke("stop_stt_capture")`; if `had_speech==false` → hide orb + `reset()`; if `true` → let the in-flight transcript complete (never kill mid-word). Guard: transcript-sequence number so a late `stt:transcript` after cancel is dropped. | ✅ RECOMMENDED — precise, testable, no double-capture |
| A2. Frontend-only hide | Hide orb without aborting Rust capture | ❌ REJECT — capture continues, late transcript executes a "cancelled" command (exact bug class of doc 44) |
| A3. Rely on 8s timeout | Do nothing, wait `STT_NO_SPEECH_CHUNK_LIMIT` | ❌ REJECT — orb sits listening 8s after explicit cancel; violates "cancel the call" |

**Tests (A1):** Rust unit ×4 (abort flag flips; had_speech true/false arms; late-transcript drop; mock-wake no-op) + frontend ×3 (2nd press before speech hides; after speech lets finish; 1st press unaffected).

### 5.2 Feature 2b — Press during TTS → stop speech + start listening

| Option | Description | Verdict |
|---|---|---|
| **B1. Reuse the wake path (RECOMMENDED)** | In `hotkey.rs` speaking branch, after `stop_tts()` + `cancel_active()`: show `main` window + `eval __NEXUS_WAKE__` + `start_stt_capture` — identical to the idle-wake branch (`hotkey.rs:151-179`). `startListening` (`main.tsx:139-164`) already implements speaking→listening barge correctly: `setBargedIn()`, `clearDialogContext()`, `stopTts()`, `abortCapture()`, **300ms DAC-drain mute**. | ✅ RECOMMENDED — one proven path, no fork |
| B2. New dedicated "listen-only" command | Separate code path that only starts capture | ❌ REJECT — forks `startListening`; that function's races produced 3 historical bugs (docs 44–47). Do not duplicate. |
| B3. Stop TTS but stay idle (today) | Current `return` after stop | ❌ REJECT — user explicitly wants listening to begin. |

**Mandatory reconciliation (Zylos 4-step barge-in, §3.2-5):** after stopping playback we MUST also reconcile state: `setBargedIn()` + `clearDialogContext()` (existing) **and** `drop_followups()` if the interrupted turn had queued follow-ups (wire explicitly — this is the step teams skip). **Tests (B1):** hotkey-branch unit (mock speaking → assert wake eval + capture invoke) + barge-in-mute preserved test + queue-drop-on-barge test.

### 5.3 Feature 2c — Esc replaces window-close hotkey — SAFETY-CRITICAL

**Hard constraint:** a global `RegisterHotKey` **consumes** the keystroke — it cannot be forwarded to other apps. An always-on Esc would break Esc in every editor/browser/game on the system. Ghost survives this only because Esc is registered **dynamically per session** (`ghost.rs:443-469`) with explicit user consent ("Esc cancels any time") and unregistered on every exit path.

| Option | Description | Verdict |
|---|---|---|
| **C1. Always-on global Esc + foreground check** | Register Esc at boot; check foreground window before acting | ❌ **REJECT — unsafe.** Consumed keystroke can't be passed through; foreground checks race; breaks Esc system-wide when NEXUS misjudges focus. Never ship. |
| C2. Dynamic global Esc (register while any NEXUS window visible) | Mirrors ghost's proven per-session pattern, extended to sidebar/settings | ⚠️ POSSIBLE but high lifecycle complexity: must register/unregister on every show/hide/focus transition; a visible-but-unfocused window would still eat Esc in the user's other app. Regression surface: every window code path. |
| **C3. Frontend-level Esc, no global grab (RECOMMENDED)** | Each sidebar/settings/stage app adds `window.addEventListener("keydown")` for `Escape` → invokes its own close command. No OS registration at all. Ctrl+Space retains its window-close role for "close from anywhere" (or falls through to wake per §1.3 — decision point D3 below). | ✅ **RECOMMENDED** — zero OS risk, zero registration lifecycle, correct exactly when the user is already interacting with NEXUS. |

**Decision point D3 (user input needed):** after moving close-to-Esc, does Ctrl+Space with a window visible (a) keep closing the window (current, now redundant), or (b) **fall through to wake** (listen through the window)? Recommendation: **(b) wake** — Esc already covers close; hotkey becomes uniformly "talk to NEXUS".

**Tests (C3):** frontend keydown tests ×2 (Esc closes sidebar; Esc does NOT close when focus is in a nested modal/confirm dialog — must close the dialog first), settings window same, plus a manual checklist item: Esc in VS Code/Chrome unaffected (trivially true with no global grab).

### 5.4 Esc-conflict note (must coordinate with Gemini lane)

`ghost.rs` registers a **dynamic global Esc** during ghost sessions. If C3 adds per-window Esc handlers, during a ghost session BOTH exist: global handler → aborts ghost session; window handler → closes sidebar if focused. Define precedence: **global ghost Esc wins when session active**; window handler no-ops if `ghost::session_active()`. Add a test/grep-gate so a future sidebar Esc can't kill a ghost session accidentally.

---

## 6. Implementation Plan (file-by-file, phased, test-gated)

> Order chosen to land user-visible value fastest while keeping each phase independently shippable. **No phase merges without its gates green.**

### Phase 1 — Feature 2b + 2a (hotkey correctness, ~½ day)
| Change | File |
|---|---|
| Speaking branch: after stop, run wake sequence (B1) | `src-tauri/src/hotkey.rs:90-100` |
| New `stop_stt_capture` command + `had_speech` return + transcript-seq guard | `src-tauri/src/wakeword_oww.rs` (+ register in `lib.rs`) |
| 2nd-press cancel handling in `startListening` | `frontend/src/main.tsx:116-168` |
**Gates:** Rust `cargo test --lib --test-threads=1` (all, not just new) · frontend `tsc` + full vitest · manual matrix: idle/2nd-press-before-speech/2nd-press-after-speech/TTS/ghost-live/window-visible (8 rows).

### Phase 2 — Feature 2c (Esc, C3) (~¼ day)
| Change | File |
|---|---|
| Esc keydown → close sidebar | `frontend/src/sidebar/SidebarApp.tsx` |
| Esc keydown → close settings | `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` (and settings app entry) |
| Esc precedence guard vs ghost global Esc | as above |
| Decision D3 wiring (hotkey falls through to wake) | `src-tauri/src/hotkey.rs:134-150` |
**Gates:** vitest keydown tests; grep-gate that no `Escape` global registration was added outside `ghost.rs` (`check-turn-ends.mjs`-style script or CI grep).

### Phase 3 — Approach E core: serial executor + priority lane (~1 day)
| Change | File |
|---|---|
| Queue entry struct `QueuedCmd`, `lane()` classifier, depth-ACK flag | `src-tauri/src/ghost.rs` (extend `FOLLOWUP_QUEUE` section, lines 158–259) |
| Executor loop: `drill_running()` gate → dequeue → verify → run → τ sleep; stop-words preempt (Rule 1) | `src-tauri/src/ghost.rs` + drain wiring at `orchestrator.rs:803+` |
| Route mid-execution transcripts to queue instead of new turn (only when `drill_running()`) | `src-tauri/src/orchestrator.rs` (Gemini lane owns this file — coordinate!) |
| Depth ACK + per-step timeout + `ghostTurnGapMs` setting | ghost executor + `commands.rs::NexusSettings` + settings UI |
**Gates:** new Rust unit tests ≥12 (FIFO order, cap policy, stop-lane jump, timeout kill, drop-on-abort, ACK once) · existing 550+ serial suite green · `cargo clippy` clean in touched ranges.

### Phase 4 — Gap tightening + supersede + re-verify (~1 day)
| Change | File |
|---|---|
| `waitForAudioIdle` cap 3000→1000ms (ghost-only) + skip-if-silent | `frontend/src/net/ghostHotMic.ts` |
| Reset beat 550→250ms (ghost-only branch) | `frontend/src/net/orchestrator.ts` |
| `slot_class()` + in-place supersede (Rule 2) | `src-tauri/src/ghost.rs` |
| `verify_grounding()` hook before each dequeued step (Rule 3) | ghost runners (`live/commands/*`) |
**Gates:** unit tests for slot supersede (F3 scenario) + grounding-fail skip (F4) + timing tests (poll cap, beat) · e2e stopwatch log.

### Phase 5 — Acceptance (live, user-run) (~½ day)
- Script: "open brave" → wait → "create a new tab" → "search for almonds" → assert FIFO order, gap ≤1.5s narrated / ≤0.5s silent, zero duplicates.
- Failure drills: F1–F7 scenarios from §4.3.1, each must produce the "Correct outcome" column.
- Hotkey matrix (8 rows, §Phase 1) re-run with queue active.
- Regression: normal (non-ghost) mode unchanged — same e2e fixture as doc 55/73.

### Estimated totals
| | Rust | Frontend | Manual |
|---|---|---|---|
| New/changed LOC (est.) | ~450 | ~250 | — |
| New tests (est.) | ~20 | ~15 | 15 checklist rows |
| Calendar | 3–4 days | | + 1 live acceptance |

---

## 7. Research Sources (for cross-checking)

| ID | Source | URL | Used for |
|---|---|---|---|
| S1 | ROS actionlib — SimpleActionServer single-goal & preemption policy | https://docs.ros.org/en/api/actionlib/html/classactionlib_1_1simple__action__server_1_1SimpleActionServer.html | Preemption semantics, GoalID ordering, P→A→PREEMPTING state machine (§3.1, Rule 1) |
| S2 | Amazon AVS UX — Interrupts Guidance (5-level precedence, TTS non-resume, "customer must always dismiss") | https://developer.amazon.com/en-US/docs/alexa/alexa-voice-service/ux-design-interrupts.html | Barge-in precedence table; cancel-doesn't-resume semantics (§3.1, §3.2-4) |
| S3 | Trevor Lohrbeer — "An Analysis of Voice Mode in GPT-4o" (Medium/FastFedora) | https://medium.com/@FastFedora/an-analysis-of-voice-mode-in-gpt-4o-cc0ab4c8a2c0 | Full-duplex vs 6-stage pipeline; interruptability behavior (§3.1 C) |
| S4 | Sara Zan — "Can you really interrupt an LLM?" (2025-06-02) | https://www.zansara.dev/posts/2025-06-02-can-you-really-interrupt-an-llm/ | `conversation.item.truncate` + `audio_end_ms`; Gemini interruption gap; "interruption is app-level plumbing" (§3.2-5) |
| S5 | Stream — "Using a Speech Language Model That Can Listen While Speaking" (2024-09-26) | https://getstream.io/blog/realtime-speech-language-models/ | LSLM/full-duplex taxonomy; half-duplex limits of turn-based assistants (§3.1) |
| S6 | Zylos Research — "Turn-Taking and Barge-In Mechanics in Realtime Voice Agents" (2026-07-17) | https://zylos.ai/research/2026-07-17-turn-taking-barge-in-realtime-voice-agents/ | **4-step barge-in convergence**; latency bands 200-300/500/800ms-1s/1.5s; Nova Sonic/Gemini/OpenAI specifics; echo-cooldown (1.5s/0.03) precedent (§3.2, §4.4) |
| S7 | OpenAI Operator / Anthropic Claude Computer Use product behavior (stepwise screen re-verification, pause-and-ask) | product docs (platform.openai.com/docs, docs.anthropic.com — verify links before citing in public docs) | Rule 3 (re-verify at dequeue) |
| S8 | ROS actionlib main page (preemptable tasks, status values) | http://docs.ros.org/en/indigo/api/actionlib/html/index.html | Action state vocabulary (§3.1) |
| S9 | ROS actionlib — SimpleActionClient (`sendGoalAndWait(execute_timeout, preempt_timeout)`) | https://docs.ros.org/en/api/actionlib/html/classactionlib_1_1simple__action__client_1_1SimpleActionClient.html | Per-goal timeout pattern (Rule 4 poison-step timeout) |
| S10 | ROS wiki — actionlib DetailedDescription | http://wiki.ros.org/actionlib/DetailedDescription | Architecture overview (note: wiki currently bot-gated; use docs.ros.org mirrors) |
| S11 | OpenAI Realtime API interruption handling (via S4/S6 descriptions) | platform.openai.com/docs/api-reference/realtime | Truncate-to-reconcile pattern (state reconciliation after barge-in) |

**Verification note:** S1/S2/S6 were fetched and confirmed on 2026-09-30. S3/S4/S5/S8/S9 confirmed via search snippets — open them yourself to confirm exact wording before quoting publicly. S7 URLs deliberately not fabricated — locate the official pages before citing.

---

## 8. Glossary

| Term | Meaning here |
|---|---|
| **Lane** | Queue class: `priority` (stop/cancel — never queues) vs `normal` (actions — FIFO) |
| **Slot class** | Singleton resource key (app-open, navigation, focus) used for Rule-2 supersede |
| **τ (tau)** | Inter-command gap budget measured narration-end → next-step-start |
| **Barge-in** | User speaks over TTS → stop playback, cancel, begin new turn |
| **Reconcile** | After barge-in, align recorded state with what the user actually heard (Zylos step 4) |
| **Head-of-line blocking** | One slow/failing queue item starving everything behind it |
| **Stale grounding** | Executing a click/type against UI state that changed since the command was spoken |
| **Drop-oldest / drop-newest** | Cap-overflow policy: discard the front (first-spoken) vs back (latest) entry |
| **`GHOST_BUSY` / `drill_running()`** | Existing atomic counter marking executor busy — the "execution" flag in user's flow |
| **`GHOST_CANCEL` / `stop_requested()`** | Existing atomic cancel observed at step boundaries |
| **DAC-drain mute** | 300ms post-TTS mic mute so playback echo doesn't self-trigger (`main.tsx:161-164`) |

---

## 9. Open Decisions — user input needed before implementation

| # | Decision | Options | My recommendation |
|---|---|---|---|
| D1 | Approve **Approach E** (FIFO + 4 rules) vs pure A? | E / A / other hybrid | **E** |
| D2 | Cap-overflow policy for normal lane | drop-newest (inverts today's drop-oldest) / keep drop-oldest | **drop-newest** (Rule 4) — but today's `queue_followup` test asserts drop-oldest, so the test changes |
| D3 | Ctrl+Space with window visible | keep close-window / fall through to wake | **fall through to wake** (Esc covers close) |
| D4 | Default `ghostTurnGapMs` | 1000 / 750 / 1200 | **1000** (meets your "1 second" spec; silent steps auto-run at 250ms) |
| D5 | Depth-ACK phrasing + threshold | "Queued, sir." at depth ≥2 / at every enqueue / never | **at depth ≥2, once per session** |
| D6 | Depth-ACK voice | always / settings toggle | **settings toggle, default on** |
| D7 | Per-step timeout default | 10s / 15s / 20s | **15s** (`ghostStepTimeoutMs`) |
| D8 | Supersede scope for app-open | replace pending only / also close the already-open app | **replace pending only; in-flight completes** |
| D9 | Does queue drain continue after Esc/abort? | drop (current `drop_followups`) / keep pending | **drop** — context is gone (F7) |
| D10 | Coordinate with Gemini lane on `orchestrator.rs` edits now or later? | parallel / sequential | **sequential — Phase 3 touches orchestrator; get Gemini lane's green light first** |

---

## 10. Definition of Done

1. All Phase gates green: Rust full serial suite, frontend `tsc` + full vitest, clippy clean in touched ranges.
2. Live acceptance (§6 Phase 5) passes: FIFO order correct on all F1–F7 drills; gaps ≤1.5s narrated / ≤0.5s silent; hotkey 8-row matrix green; Esc closes NEXUS windows and **never** leaks to other apps (no global Esc registration outside `ghost.rs` — CI grep).
3. Normal (non-ghost) mode byte-identical behavior on existing e2e fixture.
4. Docs updated in lockstep (AGENTS.md rule): feature doc + changes entry + this research doc's status flipped to "IMPLEMENTED" with measured numbers.
5. User re-runs `nexus build` + `nexus start` and confirms personally (all live acceptance is user-run by standing rule).
