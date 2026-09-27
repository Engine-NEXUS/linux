# Loading Choreography + LLM Cascade Refresh (2026-09-19)

Two additions shipped together: the loading spinner finally tells the truth,
and 9Router routes to live model IDs again.

## 1. Loading: single owner (fixes hallucinated spinner)

**Root causes (all verified in code):** `setLoadingVisible` had ~20 writers
across `recorder.ts` (10+ sites), `wsBridge.ts`, `orchestrator.ts`, `main.tsx`
— any missed path left the spinner stuck or flashing. The orb restarted its
Lottie segment on every state tick (`playSegments(force)`), so rapid
listening→thinking→speaking looked possessed. Orb-hide delays (600/1500/550/
400ms) were scattered across three files.

**Fix (frontend-only):**
- `store/loadingMachine.ts` (new, pure, clock-injectable): minimum 800ms
  dwell (no sub-second flashes), 120s failsafe (a lost hide can never wedge
  it; covers deep PR analysis), no-op transitions, show refreshes failsafe.
- `store/assistant.ts`: `setLoadingVisible` is now the single owner — all
  existing writers funnel through it unchanged, with transition debug logs.
- `avatar/Avatar.tsx`: `resolveAvatarAnim()` extracted (pure, tested);
  same-mode transitions update speed only instead of replaying the segment
  (thinking→speaking no longer restarts the loop).
- Tests: `vitest@2` added (`npm test`), 11 tests
  (`loadingMachine.test.ts` ×7, `avatarAnim.test.tsx` ×4).

## 2. LLM cascade refresh (Sept 2026 model churn)

Free-tier model IDs die silently industry-wide this year (Groq 404'd
`llama-3.3-70b-versatile` — observed live in NEXUS logs; Cerebras free became
a card trial; GitHub Models retired). Changes in `router.rs`:
- Groq: `openai/gpt-oss-120b` → `openai/gpt-oss-20b` → `qwen/qwen3-32b`
  rotation (multiplies per-model daily quota; 404s fall through).
- Order: Groq → Gemini → Cerebras (demoted to key-only last resort).
- `probe_provider_health()` at startup: checks each configured `/models`
  menu, logs which IDs are dead (zero inference cost). Standing rule:
  re-check menus whenever a new 404 appears.
- Tests: cascade order, no-dead-llama, menu-diff detection (16 router tests).

## 3. Verification
- Rust: 431/431 lib tests (incl. 4 new router tests), zero warnings.
- Frontend: 11/11 vitest, `npm run build` clean.
- Live matrix pending (P4 soak): fast query, slow analysis, error path,
  cancel mid-flight — spinner must track each exactly once.
