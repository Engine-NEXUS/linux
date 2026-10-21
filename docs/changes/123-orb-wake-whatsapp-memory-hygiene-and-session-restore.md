# 123 - Stuck orb, wake policy, WhatsApp chat open, memory hygiene, session restore + shutdown command

Date: 2026-10-10. Triggered by a boot log: the greeting said "You were in Opencode on “OpenCode”", the orb never
disappeared, "open my chat with mummy in WhatsApp" did nothing in Ghost Mode, and memory was mostly junk.

Everything below was built and tested offline. **What was NOT run live is listed at the end - read that first.**

## A. Orb + wake

| Problem | Cause | Fix |
|---|---|---|
| Orb never hides | No "idle means hidden" rule; hiding relied on one-shot timers that several turn types never armed (boot greeting, silent turns, stage notices) | `frontend/src/net/orbIdleWatchdog.ts` (pure machine, injectable clock), 500 ms tick in `OrbFrame.tsx`: hides ~2.5 s after idle unless ghost / caption / TTS / awaiting input / confirm / loading / calibration; stuck `thinking` (60 s) and `listening` (30 s) are reset. `recorder.ts` 8 s dead-end reset for `handled_locally` turns. `hideOrbAfterSpeech` caption wait capped. |
| Wake events lost when the stage window was rebuilt | Tauri events are fire-and-forget; stage destroyed 0.7 s before its replacement existed | `stage.rs`: `is_ready`/`wait_until_ready(2 s)`, `PENDING_TRANSCRIPT` stash + `stage_take_pending_transcript` replay on mount, grace 8->45 s |
| False wake on "That's a poem", real wakes rejected ("Hello, Nico.") | Stage-2 verify timed out (cold local STT) and **failed open at any probability**; "nico" was not a known soundalike | `wakeword_oww.rs`: fail-open only at prob >= 0.9, high-confidence soundalike tier (<= 6 tokens), greeting + soundalike accept, cooldown fix; tests `verify_policy_tests` |
| `localSttOnly=true` still sent audio to Groq | `if` instead of `else if` in `stt.rs` | `groq_allowed()` + test `privacy_mode_never_reaches_the_cloud`; setting switched to cloud as you chose |

## B. WhatsApp chat in Ghost Mode

* Parser (`intent_parser.rs`): `strip_wa_tail` handles "in/on WhatsApp", "wa", "please"; new shapes "open my chat with X in
  whatsapp", "X chat in whatsapp", "open X in whatsapp"; the `on wa` bug fixed. `parser.ts` guard so the TS fallback never turns
  these into `open_app`.
* Execution (`live/commands/wa_open.rs`, `ghost_drill.rs`): process-checked foreground (`WhatsApp.Root.exe`, never title
  substring), Ctrl+F search, then UIA `ListItem` match (`pick_match`) before any Enter; honest spoken failure
  ("I couldn't find X in WhatsApp"); kinship names (mummy/mom/amma...) tried only after a miss; learned aliases through
  "remember that wa alias mummy is Amma".
* Normal mode: no fake success. `dispatch_to_mcp` probes the bridge before the confirm prompt and points to Ghost Mode.

## C. Memory hygiene

`memcore/hygiene.rs` admit gate (STT hallucination filter reuse, filler/stop-word content, unhelpful replies), retrieval filter in
`context_pack` and `conversation_prompt`, one-time backed-up cleanup (`memory/_backup_hygiene_v1/`, flag `hygiene_v1_done`),
audit pruning (30 days / 2000 rows), no-flush `touch`, `purge_junk`, `wipe_memory` covers every store, and a real STT bug fixed
(`COMMON_WORDS` blanked "I work at Google as a software engineer"). Greeting text: "OpenCode" no longer repeated as its own title.

## D. Session restore, power command, boot health (this round)

* **`memcore/session.rs`** snapshots what is open every 60 s (30 ms per capture): apps (exe path + sanitised args), VS Code /
  Cursor / Antigravity folders (from `storage.json`, cross-checked against window titles), Explorer folders (PowerShell,
  cached 5 min), browser windows as a count. Never stored: NEXUS, shells and terminals, system chrome, sensitive windows,
  private/incognito windows, protected (elevated) processes, any argument that is not an existing non-sensitive path.
  Sealed in the Memory Core `meta`, rewritten only when content changes or every 10 min. Switch: `memcoreSession`.
* **Session end:** a hidden top-level window (message-only windows do not get the broadcast) writes a final snapshot on
  `WM_QUERYENDSESSION`, so a manual power-off is covered; the periodic snapshots cover power cuts.
* **`power_cmd.rs`**: "shut down / restart / sleep / lock my laptop", "sign out of windows". Parsed BEFORE the close verbs
  ("shut down" was a close-app verb, so this used to mean "close the app named my laptop"); "shut down notepad" still
  closes Notepad. Shutdown/restart/sign-out ask first, snapshot (`voice_power`), say what was noted, count down 12 s ("say
  cancel to stop"), then `shutdown /s /t 0` (no `/f`: apps with unsaved work still prompt). Kill switch `powerCommands`.
* **Boot greeting** (`briefing.rs`): "Good evening, sir. Welcome back. When you left about 22 hours ago, you had VS Code on
  ULTRON, Brave and 2 folders open. [calendar] [open items] [Heads up: ...] Shall I reopen them?" Spoken as
  `offer_boot_session` so the reply window arms only after the question is spoken. Yes -> `restore.rs` reopens (cap 8,
  skips programs already running, browsers via `--restore-last-session`, folders via the shell); No clears it; Later keeps
  it ("reopen my session" works any time).
* **`memcore/health.rs`**: connections the user actually relies on (internet, NEXUS cloud, Groq key unless privacy mode,
  Google sign-in, WhatsApp bridge only if that watcher is on). Only new or changed problems are spoken (max 3); the card lists
  all. Switch `memcoreHealth`. Memory page has switches for session and health.

## Tests
Rust lib 1313/1313 serially (`--skip tts_bench --skip tts_kokoro`; one stale test updated: it used a low-signal `unknown`
intent that the new prompt filter correctly drops), vitest 243/243, `tsc` clean, `npm run build` ok, `cargo check` clean.
Mutation-checked (all killed): power target check, argument sanitiser, boot-hold decision, running-skip, health relevance and
new-or-changed rules, orb watchdog ghost/visible guards. Live capture on this PC: 28 ms, 3 apps.

## NOT verified live (needs a real run: `nexus build`, `nexus start`)
* The orb actually hiding after each kind of turn (inferred from code; the pasted log had no frontend lines).
* Reboot greeting -> spoken question -> mic window -> "yes" -> reopen. The offer round trip has never run live.
* `WM_QUERYENDSESSION` snapshot on a real shutdown/logoff; "nexus shut down my laptop", cancel during the countdown.
* Brave / Edge honouring `--restore-last-session` (documented for Chrome only); Store apps via the app registry.
* WhatsApp: Ctrl+F on build 2.2639 and the UIA list-item names (probing needs your consent: it reads chat names).
* The one-time memory cleanup has not run on your real data yet (runs at the next start; backup is kept).
* Greeting day is marked when the greeting is queued, not when spoken; if it is dropped, say "reopen my session".

## Pitfalls
* New settings (`memcoreSession`, `memcoreHealth`, `powerCommands`) must stay in `NexusSettings`.
* Power parsing must stay ahead of `parse_close_command`.
* The snapshot thread must start AFTER `session::at_boot` (it moves the ended session to "pending" first).
* Never add arguments to restored apps other than existing non-sensitive paths.
