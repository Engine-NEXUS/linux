# Ghost Unknown-Turn Stall Fix — Local Retry, Invoke Timeouts, Relisten Watchdog (2026-10-08)

**Trigger:** live log 00:27–00:30 — two unknown-in-ghost turns produced 29s and 19s dead-air stalls (one forced a hotkey session-kill, one forced Ctrl+C app-kill), plus a wrong-app launch ("open the brief" → Intel Software Installer).
**Root cause (verified):** Unknown → WorkerBackend cloud chat (useless for control) + `orchestrator_process` awaiting the full turn + 120s Worker timeout + frontend invoke with no timeout + relisten chained exclusively to TTS-onEnd. No result → no speech → no relisten, forever.

## What changed
- **A1/A2 ghost-Unknown goes local:** extracted `try_unknown_local_fallbacks` (memory/specs, shared with the normal 2a slot); new ghost-branch `Unknown` arm → fallbacks else `run_ghost_unknown_retry` ("Didn't catch that — say it again", Result-only so speech re-arms the hot-mic). Normal-mode Unknown still routes to cloud chat.
- **A3 mishearing repair:** "shift/shift-to tap/tab to" → "switch to tab 2" in `normalize_phonetic_mishearings` (the exact live mishearing now parses `BrowserTab{2}` — covered by `test_tab_phonetic_mishearings`).
- **B timeouts:** frontend `processViaOrchestrator` gains `opts.timeoutMs` (`GhostInvokeTimeout` escapes to the existing "Didn't catch that" + relisten path; long-running callers omit it); recorder branch-4 passes 12s for ghost non-long turns; Rust Worker POST timeout 120s → 30s.
- **C watchdog:** `start/stopGhostWatchdog` on session enter/exit; open turn >12s with no completion → one quiet capture re-arm (no nag; silent-miss path still owns nagging); open/close marks on pipeline entry, result/done/error handlers, and turn end.

## Verify
- tsc 0; vitest **213/213** (5 new: 2 timeout + 3 watchdog); Rust **1183/1184** serial — the 1 failure is the pre-existing `turn_detect::model_loads_and_runs` timing flake (fails under suite load, passes isolated; untouched module).
- Release build clean, **zero warnings**, 93.5 MB.
- Uncommitted. Live replay of the log through the fix: both stalls become ~1s "Didn't catch that" + relisten with the session surviving; 'shift to tap to' resolves to tab 2 directly.
- Incidents: one tool-reported edit failure that had actually applied (duplicate block — removed, verified single); one concurrent-session file rewrite mid-task (re-applied lost lines, verified); one build blocked 30min on the shared cargo lock (retried clean); concurrent session's `memcore::Status` test breakage observed but already fixed by them before I touched it.
- P1 (app-match confidence floor) + P2 (double-wake audit) + P3 (log hygiene) scoped but NOT built — next.
