# Research Part 2 (deep dive): Agent-Completion Watch Without Screen Polling (2026-10-14)

**Status**: Research only — no code touched. Companion to `voice-tts-01` (Part 1 =
voice twin; this doc = the "second part": keep-awake + "tell me when done").

## Headline finding

**Nobody needs to watch the screen.** All four agents the user named already emit
exact machine-readable completion events. Screen-polling (screenshots → vision
every N seconds) is the worst design on every axis: least reliable, most API,
most battery. The correct architecture is **hooks → localhost webhook → NEXUS
proactive alert**, and NEXUS already owns the webhook
(`webhook.rs`: `127.0.0.1:39220`, keychain bearer token, diary-logged).

## Per-agent completion signal (verified against current docs)

| Agent | Exact signal | Where to install | Payload NEXUS gets | Cost |
|-------|--------------|------------------|--------------------|------|
| Claude Code (CLI) | `Stop` hook event — fires when Claude finishes responding (not on interrupt). `Notification` + matcher `agent_completed`/`idle_prompt` for precision | `~/.claude/settings.json` → `hooks.Stop[].hooks[{type:command, command: curl localhost:39220/…, async:true}]` | JSON on stdin: session_id, transcript_path, cwd, last_assistant_message | $0, 0 LLM calls |
| opencode | `session.status` event (agent finished work); `session.idle` deprecated but still emitted | `.opencode/plugins/nexus-watch.js` → `event()` hook → fetch webhook | sessionID | $0, 0 LLM calls |
| Antigravity (IDE) | `Stop` hook in `hooks.json` — richest signal of the four: `terminationReason` (model_stop / max_steps_exceeded / error) + `fullyIdle` bool (true = agent AND background tasks done) | `.agents/hooks.json` or `~/.gemini/config/hooks.json` | conversationId, workspacePaths, transcriptPath, terminationReason | $0, 0 LLM calls |
| Devin (cloud) | No local hook (runs in their cloud). Two options: (a) Sessions API poll `status ∈ {exit, error, suspended}`; (b) Devin Automations → email on success/failure → NEXUS Gmail watch (mailwatch already polls) | NEXUS-side poller or existing mail watcher | session status / email | (a) ~480 plain HTTPS calls/day — hits Devin API, NOT our AI keys; (b) $0, reuses Gmail poll |

Caveats (honest): Claude `Stop` fires per *response*, not per *task* — treat it as
"needs eyes", and prefer `Notification/agent_completed` where available; a `Stop`
with `stop_hook_active=true` is a continuation, not completion. Antigravity `Stop`
with `fullyIdle=false` means background commands still run — keep watching.

## NEXUS wiring (no new processes, no new windows)

1. `webhook.rs` gains one route, e.g. `POST /agent_done {agent, project,
   summary?, status}` (same bearer-token auth as existing routes).
2. Route → `proactive_policy::submit` (Medium if user idle ≥30 s, else card +
   heads-up; High/escalate if the payload reports error/max-steps).
3. Spoken line on delivery: "Claude finished in <project>, sir." TTS reuses the
   sticky-voice rule from plan 119 — no new voice work.
4. **Keep-awake**: `SetThreadExecutionState(ES_CONTINUOUS \| ES_SYSTEM_REQUIRED \|
   ES_DISPLAY_REQUIRED)` armed when a watch starts (or any ghost/TTS activity),
   cleared on completion/timeout. ~20 lines in `power.rs`. Note: Windows honors
   this over sleep timers but NOT a closed lid on most laptops — say so in UI.
5. "Remind me when it is done" voice flow: `WatchTask{agent?, project?}` intent
   ("tell me when Claude is done", "watch this task", "remind me when it's done")
   → if hooks installed: arm webhook watch, speak "Watching, sir — screen stays
   on" → hook fires → alert. If hooks NOT installed (Devin without API key):
   fall back to local OS-signal watch (window title / CPU / file mtime, $0) with
   ONE vision confirm — the only design that ever spends API, ~1–3 calls per task.

## API-limit answer (exact numbers)

| Path | Cloud AI calls per 8-h agent run | Against free tiers | Verdict |
|------|----------------------------------|--------------------|---------|
| Hook → webhook → alert (Claude/opencode/Antigravity) | **0** | Nothing spent | Default |
| Devin API poll @60 s | 0 AI calls (plain HTTPS to api.devin.ai; spends Devin seat/quota, not ours) | N/A | Fine |
| Devin email → Gmail watch | 0 extra (existing 90 s/8 min mail poll) | — | Fine |
| Fallback OS-signal + vision confirm | 1–3 | Trivial (Groq 14.4k/d, Gemini 1.5k/d, vision RPD 500) | Bounded |
| Banned alternative: screenshot→Gemini q30 s | ~960 vision/day | Dead at 500 RPD | Never build |

**Direct answer to "can it do that or will API keys blow up": it can, and keys
don't move** — as long as completion comes from hooks (push) rather than vision
polling. The only recurring cost is zero.

## Papers / prior art specific to this part

| Work | Relevance |
|------|-----------|
| Claude Code hooks (Anthropic docs, 2025–26) + community notify.sh patterns | The exact `Stop`/`Notification` mechanism; Windows MessageBox variant documented |
| opencode plugin events (`session.status`, bus architecture) | The exact opencode mechanism; community `opencode-hooks` plugin catalogs all 28 events |
| Antigravity hooks.json (`Stop` + `fullyIdle`) | Best-in-class signal; `decision:continue` even lets NEXUS keep an agent working |
| Devin Sessions API + Automations (docs.devin.ai) | Poll-shape and email-shape for cloud agents |
| UFO (Microsoft 2024), OSWorld, WindowsAgentArena | Where hooks don't exist: control-state diffs as done-detectors (fallback only) |
| Calm-tech / proactive-agent notification literature | Alert discipline: batch, speak only on breakpoints (already `proactive_policy.rs`) |
