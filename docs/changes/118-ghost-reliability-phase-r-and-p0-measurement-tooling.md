# 118 — Ghost reliability (phase R) + P0 measurement tooling (2026-10-08)

Implements phases **R** and **P0** of `docs/features/103-ghost-command-understanding-and-keyboard-first-implementation-plan.md`.
Measured results: `docs/research/ghost-mode/11-p0-baseline-report-2026-10-08.md`.

## Phase R — reliability
- `frontend/src/net/ghostHotMic.ts`: `GHOST_INVOKE_BUDGET_MS` (12 000, shared with the recorder) and `GHOST_STALL_AFTER_MS` 12 000 → **20 000** (must exceed the budget; invariant test); the watchdog re-arms through the guarded `maybeGhostRelisten()` (ghost still live, no meeting, TTS idle) instead of a raw `triggerFollowupListen`; `__testGetGhostTurnTimes` test hook.
- `frontend/src/net/orchestrator.ts`: a `GhostInvokeTimeout` now closes the watchdog turn immediately.
- `frontend/src/net/listenDedupe.ts` (new, pure) + `frontend/src/stage/orbRuntime.ts`: duplicate-listen guard in `triggerFollowupListen` (same origin within 1.2 s is one request; different origins never blocked).
- `frontend/src/audio/recorder.ts`: uses the shared budget constant.
- `src-tauri/src/wakeword_oww.rs`: `is_duplicate_capture_start` + `DUPLICATE_START_WINDOW_MS` (400 ms) guard in `start_stt_capture_with_origin` — a start while a young capture is live is ignored (an explicit `Direct` start may still take ownership of a hot-mic capture); **fails open** if the clock reads earlier than the recorded start; stale "direct eval" comment corrected.

## P0 — measurement tooling (no behaviour change unless a flag is set)
- `src-tauri/dev_tools/uia_probe.rs`: read-only UIA probe (coverage of AcceleratorKey / AccessKey / name-embedded shortcuts / Invoke etc., legacy `list_actionables` timing + recall vs one cached window-rooted query).
- `src-tauri/src/live/commands/mouse.rs`: `PhaseTimer` + `[GHOST-TIMING]` line per `ghost_click` (enter, focus, resolve_uia|vision, glide+dwell+click, verify, restore, exit, total).
- `sttLogConfidence` setting (default **false**, in `NexusSettings`; reader `commands::read_stt_log_confidence`): command turns use Groq `verbose_json` and log `[STT-CONF]` (`stt.rs: confidence_summary / format_confidence_line`).
- `src-tauri/tests/phrasing_probe.rs` (`#[ignore]`) + `scripts/phrasing_probe/{gen_phrasings,analyse,analyse2,build_reallog_and_ambient,collect_heldout,record_ambient}.py` (research doc 10).

## E0.5 — UIA lookup fix (added the same day, after the P0 measurements)
- `src-tauri/src/screen.rs`: `list_actionables()` now = one cached `FindAllBuildCache` rooted at the **foreground window**, OR-filtered server-side to the clickable control types, run on a worker thread under `FAST_LOOKUP_BUDGET_MS` (2 500 ms). Outcomes: results; **timeout → empty list** (no legacy retry — it is slower; the caller falls through to its next tier); UIA unusable (no foreground window / COM error) → automatic fallback to `list_actionables_legacy()` (the previous 10-query implementation, unchanged). Kill switch: env `NEXUS_UIA_LEGACY=1`.
- Pure `accept_actionable(enabled, offscreen, name, w, h)` (same rules as before + **off-screen controls are rejected**) with unit tests; `[UIA] list_actionables: N controls in Xms` log line.
- Measured live (Notepad, the only app where the foreground stayed put): **5,097 ms / 0 controls → 24–44 ms / 23 of 23**. Calculator/Explorer not re-measured (foreground contention).
- `dev_tools/uia_probe.rs` now times old vs new side by side, has `--expect-exe` (refuses to read any other app) and reports `foreground_stable`.
- Behaviour change: ordinals ("3rd option") count across the whole foreground window; off-screen controls no longer offered.

## Verify
- Frontend: `tsc --noEmit` clean; vitest **224/224** (+9 new: dedupe ×5, watchdog invariant + meeting guard, timeout-closes-turn regression, updated watchdog tests). The timeout-close regression test was **mutation-checked** (fails when the fix is removed).
- Rust: new unit tests (duplicate-start guard, phase timer ×2, confidence summary/line ×2, accept_actionable, lookup budget) pass; main lib group **1188 passed, 0 failed, 8 ignored** (`--test-threads=1 --skip tts_bench --skip tts_kokoro`); `cargo check --features custom-protocol,admin-brain` clean.
- The Rust guard test **found a real bug in the first draft** (fail-closed on a backwards clock) — fixed in the code, test unchanged.
- Probe runs: Notepad / Calculator / Explorer, twice, matching results (report 11).

## Not done / not verified
- **Not run live:** the double-wake and "Didn't catch that" race with a real Ghost session; `[GHOST-TIMING]` and `[STT-CONF]` produce data only once used.
- **Not measured:** Chromium/Electron apps (Brave, VS Code, WhatsApp) — user must run the probe (report 11 §5); held-out phrasings and ambient false-execution rate (need the user); dynamic-prompt A/B.
- Duplicate doc numbers `115*`/`116*` left as is (referenced by number from `AGENTS.md`).
