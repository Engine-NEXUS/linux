# 139 — Production return review and agent task reconciliation

Date: 2026-10-17. Status: implementation in progress. The first build is recorded in `docs/changes/2026-10-17-return-review-first-build.md`; provider hooks, release packaging, and live acceptance remain open.

## 1. Owner contract

NEXUS should let the owner leave an agent task, work on something else, and return without having to inspect every agent window. The return response depends on the **length of this absence**, not the time since NEXUS last spoke.

| Return episode | Spoken result |
| --- | --- |
| Under 30 minutes, including 5–10 minutes away | No greeting. Review the agent tasks being tracked, with verified changes and anything needing the owner. |
| At least 30 minutes away | One local-time morning/afternoon/evening greeting, where the owner left off, and the same agent task review. |
| Duration unknown or near the 30-minute boundary with insufficient timestamp precision | No greeting; give the task review and mark uncertain status honestly. |

This applies to logout/login, shutdown/startup, sleep/wake, and lock/unlock. Treat the unmentioned 10–30 minute interval as the short-return case. A short interruption should not repeat an old daily digest. No voice should play on the lock screen, before the owner is present, during a meeting, or over a user/NEXUS turn. If no tracked task exists, show a quiet card instead of inventing an update.

“Response ready” means the agent produced a new response. “Task complete” requires separate evidence that the requested work finished. A quiet window, file inactivity, or a process merely exiting does not prove either claim.

## 2. Pre-implementation baseline and root causes

The findings in this section describe the code and supplied log **before the first build on 2026-10-17**. They are retained as the research record; the current implementation state is summarized below and detailed in the linked change record.

1. In the supplied 2026-10-17 log, startup emitted `[BOOT] hello (boot, hour 12)` and then `[BOOT] skip briefing: already briefed today`. The spoken line was “Good afternoon, sir.” The log contains no prior leave time or task-watch state, so it cannot establish the actual absence duration.
2. `memcore/briefing.rs`: `hello_due` compares `last_hello_ts` to a two-hour gap. `start_boot_briefing` submits the hello before checking `last_briefing_day`. Sleep has another rule: `on_resume` speaks a hello after 20 minutes but does not build a task review. These three rules cannot implement the owner contract.
3. `lib.rs`: sleep detection compares wall-clock time around a ten-second sleep. It is a fallback heuristic, not a unified source of lock, logon, suspend, and return events. No `WTS_SESSION_UNLOCK`/power-event bridge currently feeds the briefing.
4. The existing Memory Core has valuable resume points (`memcore/resume.rs`, every 30 seconds) and session snapshots (`memcore/session.rs`, every minute and on `WM_QUERYENDSESSION`). These can provide “where you left off,” but neither is an agent-task ledger. Session snapshots skip refresh after extended input idle and identical content may retain an older timestamp; they must not be used as an exact away clock.
5. `agent_watch.rs` stores active IDs and keep-awake leases only in `ACTIVE_WATCHES`. The Node sidecar stores watches only in an in-memory `Map`. `list_watches` returns an empty list when the sidecar is absent. Logout/shutdown therefore erase the current watch view.
6. `run_watch_agent_task` calls `start_watch(..., prompt=None)` for a recognized CLI. `/watch` in `server.cjs` always calls `manager.spawn`. Thus “watch over this” can start and watch a fresh blank CLI session instead of attaching to the already-running task. The acknowledgement implies coverage it has not established.
7. For desktop/IDE watches, `poll_desktop_task_quiescence` announces completion after roughly six seconds without a recently modified entry in the workspace root. If no workdir is known, the same false-completion path remains. Nested file changes and an agent composing an answer are not reliable signals.
8. `proactive_policy::Signals` tracks meeting, speech, work, and time since interaction with NEXUS, but has no session-locked or user-away signal. A completion alert can therefore be selected for speech when nobody is at the unlocked desktop.
9. The sidecar exposes `/watch`, `/watch/:id`, and `/watch/:id/stop` on fixed `127.0.0.1:39222` without caller authentication. Its current first-use `npm install` has no committed lockfile and depends on Node/npm and network availability. These are release and local-trust gaps, not just briefing issues.
10. The supplied log reports Google OAuth not connected. Email may be a secondary clue after authorization, but cannot currently certify an agent reply.
11. `user_present(None)` currently assumes presence, and `GetLastInputInfo` alone does not prove that the owner is present: Windows documents session scope and possible non-monotonic/injected input timestamps. Use unlock/session state as the primary privacy gate, with input only as a supporting signal.

The old feature 123/126 plans intentionally requested a hello at logon and keep-awake while watching. This document supersedes that **greeting policy** for returns. The keep-awake choice needs an explicit power policy below.

### Current implementation state (2026-10-17)

| Area | First build | Remaining production gate |
| --- | --- | --- |
| Return decision | Native Windows session/power events, persisted checkpoint, 30-minute greeting rule, lock/presence gate, and Command Hub card | Real lock, sleep, sign-out, shutdown, and cold-login acceptance runs; clock-change hardening |
| Task tracking | Bounded local Memory Core journal with evidence and last-observed time; uncertain tasks remain unconfirmed | Dedicated versioned/migrated sealed table and full Memory Core forget/disable verification |
| Existing agent tasks | Foreground target is recorded without starting a blank CLI | Verified provider hooks or protocols with stable identity for independent Claude, Cursor, Antigravity, Codex, and other sessions |
| NEXUS-owned CLI | Authenticated lazy sidecar, bounded requests, parent-loss exit, pinned dependency lockfile | Offline first-use packaging and real process/version tests |
| Delivery | Return card and one spoken review through the existing speech path; spoken receipt after completion | System-visible quiet notification, live meeting/TTS/retry and active-typing tests |

**Release state:** feature 139 is a working first build, not production signed off. A sidecar output pattern is only a **possible response**; no independent agent response is currently certified by a provider hook.

## 3. Architecture

```text
Windows session/power events + last-user-activity checkpoint
                       |
                       v
              ReturnEpisode coordinator
       absent -> returned -> present -> reconciled -> delivered
                       |                     ^
                       |                     |
          sealed local task journal -> agent adapters
                       |             (hooks / owned process / protocol)
                       v
          single return brief -> Command Hub card + voice policy
```

### 3.1 One return episode coordinator

Create one service for startup, lock/unlock, suspend/resume, and session end. Windows event handlers should record tiny events promptly, then let the async coordinator perform I/O. Register the existing hidden top-level message window for `WM_WTSSESSION_CHANGE` with `WTSRegisterSessionNotification(NOTIFY_FOR_THIS_SESSION)`; handle lock, unlock, logon, logoff. On the same window handle `WM_POWERBROADCAST` suspend and resume. Keep the current time-gap watcher only as a diagnostic/fallback if native event registration fails. Unregister the window on teardown.

Persist `episode_id`, `away_started_at_utc`, cause, `last_confirmed_input_at_utc`, checkpoint precision, and delivery state in sealed Memory Core. For an explicit lock/suspend/logoff, start the absence at that transition; coalesce overlapping events into one episode using the earliest transition. For ordinary shutdown use the existing session-end hook. For crash or power loss, use a lightweight periodic last-input checkpoint and record that its precision is bounded by the cadence. Do not turn a process restart while the user remained active into a “return.”

On wake, do not speak at the automatic power-resume event. Wait for the user's unlocked session (and supporting recent input), stage/caption readiness, and a speech-safe breakpoint. If session state cannot be verified, show a card and wait to speak. Dedupe the wake, unlock, logon, and stage-ready signals by episode ID. At `away_seconds >= 1800` greet once; below it do not. If the confidence interval straddles 1800 seconds, omit the greeting. Wall-clock changes, timezone changes, daylight saving, and clock rollback must not create a false long absence; use monotonic time while the process survives and a guarded wall-clock checkpoint across restarts.

### 3.2 Durable task journal and evidence

Add a small sealed table to the existing Memory Core, with schema version/migration: `task_id`, provider, external session/turn ID, ownership (`started_by_nexus` or `attached`), project, brief title, started/observed/completed times, state, evidence type, source sequence, last checked, last delivered revision, and optional deep link. Do not store full prompts, transcripts, API keys, or terminal output by default. Retain a bounded recent history and honor Memory Core disable/forget controls. Persist each state transition transactionally *before* producing an alert.

Separate `running`, `response_ready`, `waiting_for_user`, `failed`, `stopped`, and `unconfirmed`. Keep `task_complete` as an additional verified outcome, not a synonym for `response_ready`. A provider event needs task identity and a newer sequence/time to change a record. Replayed hooks or a restarted sidecar must be idempotent. A missing connector, offline state, or laptop shutdown changes a previously running local task to `unconfirmed` until rechecked; it must not silently become `done`.

### 3.3 Adapter hierarchy and limits

| Task source | Preferred proof | Production rule |
| --- | --- | --- |
| NEXUS-owned CLI process | Child ID, stdout/PTY final response, exit state, stable task ID | Record before spawn; distinguish reply from exit; retain status after NEXUS restart. |
| Claude Code already open | Optional local `Stop`, `StopFailure`, `Notification`, and task hooks using session ID | `Stop` means response ended; inspect background tasks before saying all work is done. Hook integration must be opt-in and restorable. |
| Antigravity App/CLI/IDE | Optional official `Stop` hook with `conversationId`, `terminationReason`, `fullyIdle` | `fullyIdle=false` cannot be called complete. |
| Cursor | Optional official `stop` and `afterAgentResponse` hooks with reported status | `aborted`/`error` are not successful completion. |
| Codex task NEXUS itself starts via documented app-server protocol | Persist thread/turn ID; consume `turn/completed`, check `turn.status` | `completed` means the turn completed, not that code is correct. An existing independent Codex desktop chat requires a separately verified integration; never pretend the new app-server instance is attached to it. |
| Other IDE, terminal, MCP, or cloud task | A provider adapter with a documented event/API or a NEXUS-owned process | If no authoritative source exists, keep `unconfirmed` and identify the last observation. No workspace-quiet completion claim. |
| Email/GitHub | Correlated message/CI/PR event if connected | Supporting evidence only; not an agent completion substitute. No extra API key required for the core return feature. |

Define an adapter contract (`discover`, `attach`, `poll/reconcile`, `event`, `open`, `capabilities`) so new providers do not require changes to the return policy. `attach` must prove the target ID matches the current task. If that cannot be done, explain that NEXUS can observe only the app/window or ask the owner to enable a provider hook. Never start a new agent merely because the owner said “watch this.”

### 3.4 Reconciliation and delivery

At return, load the journal immediately and show a card with cached statuses and `last checked` times. Reconcile tracked tasks concurrently with a short per-adapter timeout; update the card as results arrive. Speak once from a bounded snapshot after a short wait, then use a later silent card update for slow connectors. Prioritize newly ready responses, approvals needed, and failures; summarize continuing tasks next. Where no status can be refreshed, say “I cannot confirm its current state” and show the last observation time. Cap spoken items and place the full list in Command Hub.

The return brief must be routed through the existing orb/caption/TTS path. The proactive policy needs explicit `session_locked`/`owner_present` gates; terminal alerts arising while absent should be queued in the task journal and merged into the one return brief. Mark `delivered_revision` only after speech actually completes or the user explicitly dismisses the card, so a TTS failure can be retried without duplicate claims.

While the owner is actively doing another task, post a quiet Command Hub/toast update as soon as a response is verified. Speak routine completions at a natural break or on return, rather than after a fixed timer that interrupts typing in another app. Waiting for approval can be more prominent, but still respects meeting, locked-session, and user-speaking gates. Make the voice-versus-quiet preference visible in settings; do not confuse “NEXUS has not been addressed recently” with “the owner is idle.” Merge several agent completions into one update and mark each revision delivered only once.

The long-return brief uses existing session/resume data for “where you left off,” with the latest meaningful project/window and agent task. It must not be blocked by `last_briefing_day`; that key may continue to limit only the broader calendar/mail daily digest. Short returns omit the greeting and old digest entirely.

### 3.5 Power, packaging, cost, and security

- Keep a local agent awake only while NEXUS can verify it is actively running and the owner has enabled that behavior. Explicit sleep, lid close, and shutdown remain the owner's choice. Remove `ES_AWAYMODE_REQUIRED` for portable devices; Microsoft warns that continuous sleep prevention and away mode can drain a closed laptop. A sleeping/off laptop cannot continue a local CLI task; remote tasks may continue and must be checked on return.
- No vision model, LLM request, or always-resident Node process is needed for the return decision or status wording. Use event records and deterministic templates. Keep the sidecar lazy for NEXUS-owned PTY tasks. Measure idle RAM, wake latency, and battery effect on a low-memory laptop.
- Do not run unlocked `npm install` during the first user command. Pin and package audited dependencies or replace the sidecar with a native implementation; make offline installation work. Test the installer and upgrade path.
- Protect any local watcher IPC with an unguessable per-launch secret or OS-restricted named pipe, validate method/body sizes, and bind task control to the current user. Do not log raw agent output or secret-bearing prompts. Hooks should send only task ID, provider, state, timestamp, and an optional short user-safe title to NEXUS.
- On NEXUS crash/restart, reconcile existing journal records without auto-relaunching agents, sending prompts, or repeating terminal alerts. Only owner-authorized task creation should spend provider credits.

## 4. Implementation order and gates

1. **Return clock and events.** Add pure `ReturnEpisode` policy tests at 0/5/10/29:59/30:00/30:01 minutes and unknown duration. Wire Windows session/power events, persisted checkpoints, presence gating, and episode dedupe. Gate: real sleep, lock, sign-out, and restart runs show one correct decision; no speech before unlock.
2. **Task identity and journal.** Add migration, bounded retention, privacy controls, transition receipts, and crash recovery. Gate: kill NEXUS during a task, restart, and see the same task ID with an honest status.
3. **Fix NEXUS-owned watcher.** Separate `start_task` from `attach_to_existing`; remove the no-prompt fresh-spawn path for “watch this”; remove file-quiescence as completion evidence. Gate: a silent but still-thinking IDE never announces ready; a real completed reply does.
4. **Verified provider adapters.** Implement and exercise Claude Code, Antigravity, Cursor hooks and Codex app-server for tasks NEXUS owns, with capability and version checks. Generic providers use `unconfirmed` until an adapter can prove completion. Gate: duplicate/out-of-order hook events do not regress state.
5. **One return brief.** Connect journal reconciliation to briefing and Command Hub; implement greeting threshold, priority order, locked-session queue, TTS completion receipt, and silent late updates. Gate: a short return speaks only task review; long return greets once and gives where-left-off; no daily gate suppresses new results.
6. **Release hardening.** Package dependencies, secure IPC, run low-RAM/battery measurements, and test offline and provider-disconnected behavior. Gate: clean install works without npm/network for the base feature, and the watcher clearly reports unavailable integrations.

## 5. Acceptance matrix

Run the same scenarios with 0, 1, and several tracked tasks: 5- and 10-minute sign-out/login; 5-, 10-, 29-, and 30-minute sleep; 30-minute lock/unlock; short and long full shutdown; repeated restart without absence; automatic wake while locked; wake+unlock duplicate events; meeting on return; owner actively typing in another app; several completions in quick succession; audio failure; NEXUS crash; OS power loss; network down; OAuth expired; task finishes while absent; task waits for permission; provider reports error; connector misses an event; clock/timezone jump; third-party window has no adapter.

Required observed outcomes: **zero false “response ready” claims**, one return response per episode, no greeting below 30 minutes, one greeting at/above 30 minutes when duration is known, no speech before verified presence, and no lost tracked task across process restart. Test with real installed agent versions as well as deterministic event replays. Record p50/p95 return-to-card and return-to-first-audio times plus idle/active RAM; set release budgets from measured baseline rather than guessing them. Re-run the tests after provider updates because hook schemas can change.

Provisional release targets to validate on low-memory hardware: local cached status card within 2 seconds of confirmed unlock (8 seconds after a cold logon), first spoken status within 8 seconds of a warm return (15 seconds after cold logon when a voice engine is available), no always-running watcher sidecar when there are no NEXUS-owned PTY tasks, and no more than 10 MB additional idle working set for the coordinator/journal. These are targets for measurement, not results already achieved. If a remote adapter exceeds its three-second reconciliation budget, deliver the honest cached status and refresh the card later instead of delaying the whole return.

Example short return: “Claude has replied on ULTRON. Cursor is still working. Codex needs your approval.” Example long return: “Good evening, sir. You left VS Code open on ULTRON. Claude has replied; Codex needs your approval.” If NEXUS cannot verify the result: “I last saw Antigravity working before shutdown; I cannot confirm its status yet.”

## 6. Source register

### Local evidence

- Pasted startup log: `C:\Users\Chitkul Lakshya\.codex\attachments\f6a2f758-88d7-45c1-8d9f-2b6b58b71d9d\Pasted text.txt` (startup hello, daily skip, Google OAuth status).
- `src-tauri/src/memcore/briefing.rs` (hello, resume, daily briefing, inputs).
- `src-tauri/src/memcore/resume.rs`, `src-tauri/src/memcore/session.rs` (current resume/session snapshots and shutdown hook).
- `src-tauri/src/agent_watch.rs`, `src-tauri/resources/agent_watch_sidecar/server.cjs`, `src-tauri/src/orchestrator.rs` (watch lifecycle and identity gaps).
- `src-tauri/src/proactive_policy.rs`, `src-tauri/src/keep_awake.rs` (current speech and power policies).
- `docs/features/123-boot-to-first-word-latency-analysis-and-plan.md`, `docs/features/126-universal-agent-watcher-and-cross-platform-music-control-plan.md`, `docs/changes/110-memory-core-p3-resume-points-and-boot-briefing.md` (older intent and built baseline).

### Official external research (checked 2026-10-17)

- Microsoft: [session notification registration](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsregistersessionnotification), [session lock/unlock events](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change), [power suspend/resume messages](https://learn.microsoft.com/en-us/windows/win32/power/wm-powerbroadcast), [last input scope and caveats](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getlastinputinfo), [sleep and execution-state power guidance](https://learn.microsoft.com/en-us/windows/win32/power/system-sleep-criteria), [execution-state flags](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate).
- Anthropic: [Claude Code hooks](https://code.claude.com/docs/en/hooks), including `Stop`, `StopFailure`, `Notification`, and background-task fields.
- Google: [Antigravity hooks](https://antigravity.google/docs/hooks), including `Stop`, `fullyIdle`, and `terminationReason`.
- Cursor: [agent lifecycle hooks](https://cursor.com/docs/hooks), including `stop` status and `afterAgentResponse`.
- OpenAI Docs: [Codex app-server integration](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server), including `turn/completed` and its `turn.status`. This documents NEXUS-owned app-server turns; it is not evidence of an API for arbitrary independent Codex desktop chats.
