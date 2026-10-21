# 110 — Memory Core P3: resume points and the boot briefing

Plan: `docs/features/100-memory-core-plan.md` (phase P3). Follows change 109 (P2).

## What was built
- **Resume recorder** (`memcore/resume.rs`). Every 30 s, while the user is at the PC (keyboard/mouse input in the last 2 min, read with `GetLastInputInfo`), the foreground window is sampled: process, title, and for browsers the page address reduced to scheme+host+path (YouTube collapses to `watch?v=id`). One record per *change* of window; the same window only refreshes `last_seen`, so `last_seen` ≈ when the user last used it. Sampling instead of a shutdown hook is deliberate: Windows gives no reliable exit event on logoff/power loss, so at most 30 s is lost. 7-day ring, capped at 500, stored in the sealed Memory Core (new tier `resume`). Never recorded: NEXUS's own windows, lock screen and shell chrome, empty titles, and anything the live-mode denylist treats as sensitive (banks, password managers, wallets). Resume points never enter the cloud context pack and are not shown in the facts list.
- **Briefing builder** (`memcore/briefing.rs`, pure + 9 tests). "Good morning, sir. You were on LeetCode: “Two Sum”, about 3 hours ago. 2 things are open. First: Prof. Rao emailed about “Assignment 3”, deadline Friday 5 PM. Say “show my briefing” for the rest." Built only from local data: the last session's resume points, emails the user asked to watch (deadline text taken verbatim), and requests NEXUS could not finish in the last 48 h (new `conversation::unfinished_requests`). Friend mode uses the first name. Late hours just say "Hello". Terminal windows (title = shell path) are described as "in Terminal". If there is nothing true to say it stays silent.
- **"How much is left"**: only ever a count of open items or a deadline copied from an email. No percentages, no estimates.
- **Boot delivery.** The previous session's resume points are captured at startup before the recorder writes anything. 45 s after startup, once a day, it waits until the user is actually present (input in the last minute; gives up after 20 min), then goes through `proactive_policy::submit` at Medium urgency, so it waits out meetings, speech and busy moments. The day is marked only when it was actually delivered.
- **On demand.** New intent `briefing` ("where did I leave off", "where was I", "what was I working on", "show my briefing", "brief me", "catch me up", "what are my priorities"): spoken short, full card in the sidebar, skipping the window the user is in right now. If nothing is recorded it says so, and says whether recording is off.
- **Controls** (Command Hub → Memory): switch for activity recording (`memcoreActivity`), switch for the boot briefing (`memcoreBriefing`), "Clear activity history" (`memcore_clear_activity`). Both default on, because the user asked for this behaviour; everything stays on this PC. "What do you remember" card also states whether recording is on and how many points are kept. "Forget everything" erases resume points with the rest.

## Verify
- `cargo test --lib -- --test-threads=1`: **1092 passed, 0 failed** — run in three groups (see below): 1071 (everything except two TTS modules) + 12 (`tts_kokoro`) + 9 (`tts_bench`). New this phase: resume (9), briefing (9), store tier/touch/clear, conversation `unfinished_requests`, intent parser (briefing phrases + misfire guards).
- `cargo check --features custom-protocol,admin-brain`: clean, 0 warnings.
- Frontend: `tsc --noEmit` clean, vitest 205/205 (+1), `npm run build` ok.
- **Live probe on this PC** (`live_probe_foreground`, ignored test): `GetLastInputInfo` returned the real idle time and the foreground probe returned `windowsterminal.exe` with its title, which exposed the shell-path-title problem fixed above.

## Known issue found during verification (not caused by P3)
Running the whole suite in ONE process now aborts (exit code 0xffffffff) inside the heavy TTS modules: `tts_bench` + `tts_kokoro` load the Kokoro model more than once in a single process, and this machine currently has ~2.4 GB free RAM (a 2.4 GB `link` process was running). Each module passes alone and `tts_bench::bench_summary_table` passes alone; it passed in one process earlier the same day (1091). Those files were not touched by P3. Worth fixing by making those model-loading tests share one engine or run in their own process.

## Not verified / limits
- Delivery was not heard end to end: a real boot, the 45 s delay, presence gate and policy timing need a reboot and listening.
- The recorder's real-world title quality across apps (Electron apps with generic titles, browsers with no readable URL) has only been checked on a terminal window.
- Priorities today come only from watched emails and unfinished requests. The timetable and email-priority sources arrive in P4/P5.
- A resume point is "where you were", not progress: NEXUS cannot know how much of a video, problem or document is done.
