# Log Completeness P0–P5 — Every Loophole Surfaces (2026-10-06)

**Plan:** cross-check + improvement plan presented 2026-10-06 (chat); all 6 phases executed with per-phase proof.
**Goal:** every runtime breakage — UI or backend — leaves a log line; cross-check by single-grep.

## What Gemini built (verified, kept)
debug_trace p0–p4 markers, stt:turn_stats, CDP all-target console+exception forwarding, run.ps1 unified color console, micHoldersSummary, missed_intents.jsonl.

## Gaps closed
- **P0 STT stderr drain** (`lazy_stt.rs`): dedicated `stt-stderr-drain` thread on the previously-never-read pipe → `[STT-PY]` lines in the unified log (traceback/error/warn escalate). Fixes lost Moonshine/uvicorn tracebacks AND the 64KB pipe-fill hang-with-no-evidence class.
- **P1 burst-proof console** (`run.ps1`): after the 50-line cap trips, prints `… [+N more lines in nexus_unified.log — burst]` instead of silently dropping. Position bookkeeping untouched (file was always complete).
- **P2 logged IPC boundaries** (frontend): new `src/ipc.ts` (`invokeLogged`/`emitLogged` — warn + rethrow, dynamic imports keep tests green) + 3 unit tests; wired into ~24 critical sites (all calibration commands, orb interactive + loading show/hide, hitbox registration ×3, annotation commit/done/append, pin-focus, card-highlight, abortCapture ×3 with context). Benign no-ops (hide-if-hidden) stay silent. Deliberately skipped: 2s heartbeat (watchdog rebuild is its signal).
- **P3 checked emits** (Rust): new `commands::emit_logged` (debug line per emission = file-complete, console-quiet; warn on backend failure) wired into 25 turn-critical sites (stt:transcript/turn_stats ×12, orb rect/visible/wake/position/loading-rect, loading_visible ×2, spatial pins ×2, show_spatial ×2, show_annotation, relisten watchdog, session, stage:notice). Ring/point + audio:level untouched (high-frequency, self-healing).
- **P4 ghost drill step logs** (`drain_ghost_followups`): structured `drain step #N id= slot= 'transcript'` + completion ms + grounding-failure ids + distinguished abort points (loop-top vs gap) + cap-purge warn.
- **P5 turn-ID propagation** (`net/orchestrator.ts`): result/done/error handlers log `req=` — links to Rust install/result logs (`new request {id}`, `[TTS] speak (req=)`) for single-grep turn stories (frontend p0 transcript → p4/result req → Rust req lines).

## Verify
- tsc 0; vitest **185/185** (3 new ipc tests); cargo **969/969** serial; release **91.4 MB**; warnings 22 (concurrent session cleaned house; zero from this change — only hit is the pre-existing vite chunk note).
- Uncommitted. Live drills (`nexus start`): kill STT mid-turn → `[STT-PY]` traceback visible; wake-storm → burst indicator; failing calibration save → `[IPC]` warn; dead pin click → warn; ghost drill fail → structured step line; any turn → grep `req=<id>` both logs.
- Incidents during build: one bad edit (stray `void 0`) caught by tsc, reverted; one `|`-chain split + two `&AppHandle` mismatches + one Clone bound, all caught by cargo check. Never stashed again after noting the shared-tree risk.
