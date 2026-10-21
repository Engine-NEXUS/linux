# 123 — Power-on to first spoken word: measured analysis + plan (PLAN ONLY, nothing built)

> See also [125](125-response-delay-and-whatsapp-learning-analysis-and-plan.md): the cached greeting/goodbye lines (P2) share its disk cache (R6), and the Edge 403 fix (R1-R3) comes first.

Asked for: "the time taken for the agent to speak whenever the laptop is powered on is too long ... make it perfectly
speak when the user logs in ... authentication is done, can't we start it in the background and wait for the user to type
the password so it works without any issue".

Method: code read (every claim names the file), a real startup log (`nexus_unified.log`, 2026-10-14), and read-only
measurements of THIS boot (CIM / scheduled-task queries). Numbers marked *(est.)* are estimates, not measurements.

---

## 1. What was measured

### 1.1 This boot (autostart instance, 2026-10-15, local clock)

| Event | Time | Since previous |
|---|---|---|
| Kernel boot (`LastBootUpTime`) | 06:10:47 | |
| Logon session created (password accepted) | 06:11:14 | 27 s (includes typing the password) |
| `explorer.exe` | 06:11:15 | 1 s |
| **`nexus.exe --background` (scheduled task)** | **06:11:15** | **same second** |
| first WebView2 process | 06:11:19 | 4 s |

**Launching is NOT the problem.** The `NEXUS` task (AtLogOn) starts the exe within ~1 s of authentication. Everything
that makes the first word late happens after the process exists.

Also measured on this boot: the task has **Priority 7**, and `nexus.exe` (plus its WebView2 browser process) runs at
**BelowNormal** priority class while 3 Run-key apps (OneDrive, Microsoft Lists, Figma Agent) and 29 other logon tasks start
at the same moment. `nexus.exe` had used 111 s of CPU in its first ~5.5 minutes.

Not measurable: the autostart instance writes only to stdout, so there is **no persisted log of an autostart boot**
(`nexus_unified.log` exists only for `nexus start` runs). The Task Scheduler operational log is disabled and the
Diagnostics-Performance boot event is empty on this machine.

### 1.2 Real startup log (`nexus start`, 2026-10-14, UTC; t0 = 19:00:47.35)

Caveat: a manual dev start (the WebView2 profile was cleared, so a worst-case cold WebView), and the user's presence during
the wait is unknown.

| +s | Event | Source line |
|---|---|---|
| 0.0 | process starts, EcoQoS set | `cleared WebView2 profile`, `power: EcoQoS` |
| 0.7 | stage window shown | `stage: shown` |
| **2.8** | `autostart: scheduled task created` — setup was **blocked ~2.1 s** by `reg delete` + a synchronous PowerShell | `lib.rs:441-478` |
| 6.3 | setup reaches orb rect / TTS pregen start | `emit: stage:orb_rect` |
| 7.7 | stage frontend alive | `stage: frontend alive` |
| 9.0 | wake engine capturing | `wake-engine: audio capture started in 2.2s` |
| **58.0** | TTS pregeneration of 9 ack phrases finishes — it **took 51.5 s** | `tts: cached 9 phrases in 51.47s` |
| 162.5 | health check done and boot greeting built | `[HEALTH]`, `[BOOT] greeting` |
| **162.9** | first speech attempt | `[TTS] speak ... "Hello, sir. You left OpenCode open ..."` |
| 163.4 | **edge-tts 403** after 0.5 s -> cold Kokoro load, greeting re-spoken in a different voice | `edge-tts rejected (... 403 Forbidden)` |

The mic was silent for the whole run (Intel SST, `silent_for~450s`), so no voice reply was possible either.

---

## 2. Why the first word is late (ranked; code facts)

| # | Cause | Where | Size |
|---|---|---|---|
| A | **Fixed 45 s wait** before the briefing even starts | `memcore/briefing.rs:21,440` (`BOOT_DELAY_SECS`) | 45 s, pure sleep |
| B | **Contradictory gating**: the briefing waits until there was input in the last 60 s (`PRESENT_WITHIN_SECS`), but the proactive policy for Medium urgency waits until the user has been **idle >= 30 s** (`MEDIUM_IDLE_MS`) and expires after 15 min | `briefing.rs:25-30,455`, `proactive_policy.rs:30,101-110` | Speaks only inside a 30-60 s idle band; an active user can wait 15 min and get nothing. Most of the 117 s unexplained gap in 1.2 *(est.)* |
| C | **Serial data gathering before the first word**: calendar (<= 5 s) + session offer + health (<= 12 s budget) | `briefing.rs:465-480`, `health.rs:20` | up to ~17 s *(est.)*; health is irrelevant to saying hello |
| D | **First utterance depends on the network at boot**: Edge-TTS 403 twice, 9-phrase pregen took 51 s, the network may not be up yet at logon; fallback = cold Kokoro + voice change mid-sentence | `tts.rs`, `lib.rs` pregen block | 1.5-3 s on a good run, a wrong-voice greeting on a bad one |
| E | **Blocking PowerShell in `setup()`** every boot (task re-registered although nothing changed) | `lib.rs:441-478` | 2.1 s measured, delays everything after it |
| F | **BelowNormal priority** at the busiest moment of the session (task Priority 7) | task settings in `lib.rs:452-456`, `commands.rs:2521` | unmeasured; every phase above runs slower *(est.)* |
| G | **Stage (WebView2) cold boot**: frontend alive at +7.7 s; speech is frontend-driven and, since the 2026-10-15 sync audit, `speak()` forces orb + caption, so sound cannot sensibly start earlier | `stage.rs`, `ttsPlayer.ts` | ~8 s floor today |
| H | **Mic deaf** (Intel SST) — the "Shall I reopen them?" question cannot be answered by voice | `wakeword_oww.rs` | separate known issue |
| I | **No persisted boot timeline** — none of this was visible without a dev run | — | blocks tuning |

Note: the old "Hello sir" greeting (`frontend_ready`, docs/features/04) no longer runs in the stage frontend; the **only**
boot speech today is the memory-core briefing path (A-C).

---

## 3. "Start before the password" — the straight answer

**Cold boot: not possible, and not desirable.** Windows has no user session until authentication succeeds; a logon-triggered
task cannot fire earlier, and it already fires ~1 s after (1.1). Anything running before logon would have to be a SYSTEM
service in session 0, which has:
* no access to your profile (Credential Manager keys, the sealed memory key, `%APPDATA%`, HKCU),
* no desktop for the WebView2 overlay, orb or captions,
* and a privacy problem: speaking "you left OpenCode open ..." at the logon screen tells whoever is at the machine.

This matches the Memory Core plan's earlier "honest NO: greeting before login".

What a pre-login helper *could* do is only generic work (read the exe and model files so the OS file cache is warm). That is
low value (Windows already prefetches) and adds a SYSTEM-level component to maintain, so **recommended: skip unless P0
shows the start is disk-bound**.

**The part of your idea that does work — lock screen, sleep, screen-timeout.** When you lock the PC or it sleeps and you
type the password again, NEXUS is *already running* in your session (models loaded, mic stream open). Today it only logs a
"system resumed" line. Plan item P4 makes it speak ~1 s after you unlock, stay silent while locked, and restart the mic
after resume. That is exactly "wait in the background for the password".

---

## 4. Targets

| | Today (est. from 1.2) | After P1-P3 | After P4 |
|---|---|---|---|
| Logon -> process | ~1 s (measured) | ~1 s | — |
| Logon -> **first word** | 60-160 s, or never for an active user | **<= 10 s** (stage alive ~8 s + cached greeting) | — |
| Logon -> full briefing | same as above | <= 20 s | — |
| **Unlock/resume -> hello** | none | — | **~1 s** |

---

## 5. Plan (phases, each ends with a gate)

**P0 — Measure (0.5 day).** Persist `boot_timeline.jsonl` in the app-data folder: process start, logon time (WTS session
start), `setup_done`, `stage_alive`, `wake_capture_started`, `tts_ready`, `briefing_ready`, `first_audio_out`, `mic_live`;
timestamp the `[BOOT]`/`[HEALTH]` lines. Gate: 5 real reboots recorded; rank A-I with real numbers.

**P1 — Quick wins (1 day).**
1. Task settings: `-Priority 4`, `-StartWhenAvailable` (Priority 7 -> normal).
2. Move the PowerShell registration off the setup path (background thread) and only re-register when the task is missing or
   the exe path changed; drop the `reg delete` unless the old Run value exists.
3. Replace `BOOT_DELAY_SECS` with readiness gating (stage alive + wake capture started + a 3 s settle).
4. New `Greeting` class in `proactive_policy`: speak when not in a meeting, not speaking, not working — **ignore the 30 s idle
   rule**; "present" = any input in the last few minutes, not a 30-60 s band.
5. Split the greeting: speak a short first line immediately; gather calendar / session offer / health **in parallel** with a
   3 s cap each and deliver details after, or as a card. Health never delays the hello.

**P2 — Cached boot greeting (1 day).** Exactly three greetings (your decision), chosen by local time:
**"Good morning, sir."** (05:00-11:59), **"Good afternoon, sir."** (12:00-16:59), **"Good evening, sir."** (17:00-04:59,
so after-midnight logons also get "Good evening"). Friend persona says the name instead of "sir" (`persona.rs`). The three
clips are **persisted to disk** per voice (WAV + word timings), refreshed only when the voice changes, and played through the
normal `speak_cached` path (orb + caption + sound stay in sync): no network, no 403, no cold Kokoro, no voice change.
Today the ack cache is an **in-memory map re-rendered every boot** (`tts.rs:253`, 42-51 s of CPU/network each time, section 8);
make the pregeneration skip phrases already on disk and run after the hello with limited parallelism.

**P3 — Stage first (1 day).** Create the stage window at the very start of `setup()` so WebView2's cold start overlaps with the
Rust initialisation (memcore, app registry, diagnostics, recorders) instead of following it. Gate: `stage_alive` <= 5 s.

**P4 — Unlock / resume hello (1-2 days).** Register for session-change (`WTS_SESSION_UNLOCK`) and power-resume
(`PBT_APMRESUMEAUTOMATIC`) events; policy: hello only after >= N minutes away, never while locked, no personal content in the
hello; restart the mic stream after resume (also helps the Intel SST deafness).

**P5 — Skipped by default:** pre-login file-cache warmer. Revisit only if P0 shows disk-bound startup.

**P6 — Don't ask what you can't hear:** until the mic is live, offer the "reopen?" choice as on-screen buttons / hotkey
instead of a voice question (small; separate from latency).

---

## 6. Decisions

Decided by you: greeting wording = Good morning / Good afternoon / Good evening (section 5, P2); a "Good night, sir" on shutdown
(section 9).

Still open:
1. The hello on **every** logon and unlock (after N minutes away) with the long briefing once a day: recommended, because
   today `[BOOT] skip: already briefed today` means a second login the same day gets **no greeting at all** (log 2, section 8).
2. Is silence before you authenticate acceptable (privacy), as in section 3?
3. Shutdown: keep one safety step (a short spoken countdown with cancel) or shut down at once? Recommended: countdown (section 9).
4. P1-P3 first as one change, P4 and the shutdown flow as separate changes?

## 7. Not verified

All numbers in 1.2 and section 8 come from single dev runs; 1.1 is a single boot. The size of causes B and F is estimated until
P0 exists. The Edge 403 root cause (section 8.2) is a hypothesis. Nothing here was built or run live.

---

## 8. Second log (2026-10-15 00:41 UTC, `nexus start`): what it adds

t0 = 00:41:42.8.

| +s | Event |
|---|---|
| 1.0-1.9 | stage window created and shown |
| **5.9** | autostart PowerShell finished: **setup blocked ~4 s** this time (2.1 s last run) |
| **12.8** | setup reaches `orb_rect` (6.3 s last run); the gap 48.7 -> 55.7 in the log is unexplained, to be timed in P0 |
| **13.8** | stage frontend alive (7.7 s last run): the WebView floor is not stable |
| 14.0 | wake engine ready (ONNX 0.6 s) |
| 14.7-18.5 | **all 8 Edge-TTS phrases rejected with HTTP 403 within 5 s**, then fallback to cold Kokoro |
| **55.0** | `tts: cached 9 phrases in 41.91s`: Kokoro rendering on the CPU, competing with startup |
| 57.9 | `[BOOT] skip: already briefed today (2026-10-15)`: **no greeting at all** |
| whole run | mic `SILENT rms=0.0000` for 150+ s (Intel SST), max RMS 0.0014 |

New facts:
* **8.1 Nothing greets a second login in the same day** (`last_briefing_day`). The three-greeting design fixes this (P1/P2).
* **8.2 Edge TTS fails every time, in 3 logs from different runs** (403 on every phrase, never one success). The code calls it
  "transient" (`tts.rs:800`), but it does not look transient. `edge-tts-rust` is pinned at **0.1.3**, which sends
  `Sec-MS-GEC-Version=1-143.0.3650.75` (`constants.rs:9`); **0.1.6 exists**. Services that sign requests this way reject stale
  version/token constants with 403. **Hypothesis, not verified:** bumping the crate brings the cloud voice back, which also
  removes the Kokoro voice-swap problem. Cheap test: `cargo update -p edge-tts-rust`, then `nexus start` and watch for the first
  non-403 line. Also verify the PC clock (the token is derived from the system time).
* **8.3 The ack cache is an in-memory map rebuilt every boot** (`tts.rs:253-311`); with Edge down, Kokoro builds it every boot
  (42-51 s of CPU while the user is logging in). That alone slows everything else at the busiest moment.
* **8.4 App registry refresh = 38 s** in the background (`Apps indexed: 0` at preflight, 829 entries when done): an early "open X"
  right after logon can miss until it finishes. Separate item: defer / lower its priority.
* **8.5 Mic silence again** for the whole run. The unlock/resume restart in P4 is the first real fix; the driver advice in the log
  (disable audio enhancements) is worth trying once.

---

## 9. "Nexus, shut down my laptop" -> close everything -> "Good night, sir"

### 9.1 What exists today (`power_cmd.rs`, `orchestrator.rs:5591-5630`)
"shut down / power off my laptop" is already parsed strictly (the target must be the computer itself). The flow today:
1. NEXUS **asks** ("Shut down your laptop, sir? I'll note what's open first so I can offer to reopen it.") and waits for a yes.
2. On yes: snapshots the open session (so the next boot can offer to reopen it), says "Powering off in 12 seconds, sir. Say
   cancel to stop."
3. After 12 s runs `shutdown /s /t 0`, **without** `/f`, so Windows closes the apps itself and apps with unsaved work show their
   own "save?" prompt.
It does **not** close the apps itself, does not say good night, and needs a yes **and** a 12 s wait.

### 9.2 Your wish
"Shut down / power off my laptop" -> close all running applications -> say **"Good night, sir."** -> power off.

### 9.3 Proposed flow (keeps the safety, removes the extra question)
1. **Hear the command** ("shut down my laptop", "power off my laptop", "turn off the pc"...). No yes/no question.
2. **Safety check, spoken only if needed:** an agent task still running (`agent_watch`), a Ghost drill in progress, a meeting
   active -> "Claude Code is still working on a task, sir. Shut down anyway?" (needs a yes). Otherwise go on.
3. **Snapshot** the open apps (existing, silent) so tomorrow's greeting can offer to reopen them.
4. **Speak the goodbye** and **wait until the audio has finished** (the process must not die mid-sentence): at night
   (20:00-04:59) **"Good night, sir."**; in the day **"Shutting down, sir. See you soon."**
5. **Close the apps gracefully** (`WM_CLOSE` to each top-level window, the same as the X button; never kill): skip the shell,
   NEXUS itself, system and security processes. Wait up to ~8 s.
6. **If a window refuses** (a "save your work?" dialog appears), **stop and say so**: "Notepad is asking to save, sir. I've left
   it open." No forced close, no data loss. (Today Windows would show its "this app is preventing shutdown" screen instead.)
7. **Power off** with `shutdown /s /t 3`, with a 3 s window where "cancel" / `Esc` / the hotkey runs `shutdown /a`. NEXUS
   releases its volume and keep-awake leases first.
Restart ("Restarting, back in a moment, sir."), sign-out ("Signing out, sir.") and sleep/lock keep their short lines; no "good night".

### 9.4 Phrases (better options)
* Goodbye by time: after 20:00 **"Good night, sir."**; 17:00-20:00 **"Good evening, sir. Shutting down."**; otherwise
  **"Shutting down, sir. See you soon."**
* With a note when apps were open: **"I've noted what's open, I'll offer to reopen it in the morning. Good night, sir."**
* Friend persona: **"Good night, Lakshya."**
* More ways to ask (confirmed once, because they are less explicit): "good night nexus", "I'm done for today", "call it a night",
  "that's it for tonight" -> **"Shall I shut down, sir?"**. The explicit forms ("shut down", "power off", "turn off") never ask.

### 9.5 Risks and how the plan handles them
| Risk | Handling |
|---|---|
| Unsaved work lost | graceful close only; a save dialog aborts the shutdown and is announced |
| Wrong windows closed | exclude shell, system, security, NEXUS; log what was closed |
| Voice cut off by the power-off | wait for the end of the audio before step 5 |
| Mic deaf at night (Intel SST), so "cancel" is not heard | cancel also by `Esc`/hotkey and an on-screen Cancel card |
| Long agent task killed | step 2 warns first (uses `agent_watch` watches + Ghost drill state) |
| Accidental trigger by TV/room speech | explicit phrases only, strict parser, shutdown still has the 3 s cancel window |

### 9.6 Effort and order
About 1.5 days: new close-apps step, goodbye line, safety checks, tests with a fake window list; the close step is tested
against a throwaway Notepad with unsaved text. It pairs with P2: both need the small set of cached lines (3 greetings +
"Good night, sir." + "Shutting down, sir. See you soon.") rendered once to disk.

### 9.7 Not verified
Nothing here was run: `shutdown /a` timing, which apps refuse `WM_CLOSE`, and how Brave/Edge session restore behaves after a
graceful close are unmeasured.
