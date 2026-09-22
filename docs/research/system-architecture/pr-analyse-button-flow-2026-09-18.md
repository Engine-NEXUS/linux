# Analyse-PR Button Flow — Full Trace Research (2026-09-18)

**User expectation:** in the PR-list sidebar, clicking `Analyse` on a PR
should (1) close the list, (2) show the loading animation, (3) render the
AI analysis in a sidebar. **Actual (pre-fix):** the list stays open, the
orb speaks the summary, and the analysis is fetched then thrown away
(`console.log`). This doc traces every hop with file:line, then records
the fix.

---

## 1. Entry: the `Analyse` button (pre-fix)

Files: `frontend/src/pr-list/PrListApp.tsx`,
`frontend/src/pr-list/prListStore.ts`, `frontend/src/pr-list/main.tsx`.

- Card: `PrListApp.tsx:31-57` — `PrCard` with `Merge PR` (`:48`) +
  `Analyse` (`:51-53`, `onClick={onAnalyse}`).
- Wiring: `PrListApp.tsx:203-210` —
  `onAnalyse={() => handleAnalyse(pr)}`, `disabled={actionInProgress !== null}`.
- Handler (pre-fix, `PrListApp.tsx:166-179`):

```tsx
const handleAnalyse = async (pr: PrSummary) => {
  setActionInProgress({ prNumber: pr.number, action: "analyse" });
  await tauriInvoke("orchestrator_process", {
    transcript: `analyse pr ${pr.number} in ${pr.repo}`,
    dialogContext: null,
  });
} finally { setActionInProgress(null); }
```

- **What it did NOT do:** no `hide_pr_list_sidebar`
  (`commands.rs:1209`), no `destroy_window`, no event. Window stays open;
  only local `prListStore.ts:45 setActionInProgress` disables buttons.
- Listeners in this window (`PrListApp.tsx:65-132`):
  - `orchestrator:event` → handles ONLY `payload.type === "github_result"`
    + `result.type === "pr_list"` (`:74-83` → `showPrList`). **Ignores
    `result`/`loading`/`error` with `analysis`.**
  - `get_pending_pr_list` on mount (`:101`) — race-free init for the list.
  - `sidebar:backdrop` (`:123`) — blur only.
  - `Ctrl+Space` → `hide()` + `hide_pr_list_sidebar` (`:139-140`).

So the button is a thin voice-command synthesizer:
`orchestrator_process("analyse pr N in repo")`.

**Fix applied (`PrListApp.tsx:166-179`):** `hide()` +
`hide_pr_list_sidebar` BEFORE the invoke (same close path as Ctrl+Space).
The analysis renders in the response sidebar; the loading indicator
(Rust-owned) covers the wait.

---

## 2. Rust routing: why Analyse-PR is a `WorkerBackend` job

- `orchestrator.rs:1975-1981` `orchestrator_process` →
  `process_transcript` (`:479-484`).
- Parse: deterministic (`:496`) → brain/NLU (`:504`) →
  `ParsedIntent::AnalysePr{owner,repo,pr_number}`
  (`intent_parser.rs:44,310,679`; patterns e.g. `"analyse PR 23 servx"`,
  `"analyse pr 23 in servx"`, `"analyse pull request 23 servx"`).
- `route_intent` (`orchestrator.rs:387-438`):
  `AnalysePr|AnalyseRepo|AnalyseLatestPr → Subsystem::WorkerBackend`
  (`:430-436`). **Not `GitHub`, not `Architect`.** Pinned by tests
  (`:2650-2673`).
- `router::can_route` returns `false` for `analyse pr`
  (`router.rs:564`, test `:596`; also blocks `github`, `pull request`,
  `merge/approve/close pr`, `architect`, `check branch`) → 9Router is
  skipped, always POSTs to the Worker.

`dispatch_to_worker` (`orchestrator.rs:1244-1377`): 9Router fast path
(`:1254-1293`), else Worker POST with 120s timeout (`:1329-1346`),
returns `(reply_text, analysis, dialog_state)` — `analysis =
data["analysis"]` (`:1373`), `dialog_state` (`:1374`).

---

## 3. The `WorkerBackend` arm, step by step

`orchestrator.rs:695-796`:

```
:697-704  emit Ack{...}                       ("On it sir")
:707-714  emit Loading{visible:true} + show_loading()
:717-724  dispatch_to_worker(...).await
:727-734  emit Loading{visible:false} + hide_loading()
:737-765  Ok → [FIX] emit Result{text, analysis, dialog_state} (:745-753)
                [FIX] non-null analysis → show_sidebar_with_analysis (:759-775)
                clear_active_request
:766-794  Err → hide_loading AGAIN (:767) + emit Error + Done
```

Notes:

- Success emits `Result` FIRST so TTS starts immediately, then opens the
  sidebar (window creation never blocks speech).
- `Done` is still withheld on success (would cancel TTS) — the frontend
  now closes the handshake via `finishSpokenResult` (see companion doc
  `orb-stuck-animation-root-cause-2026-09-18.md`).
- `Architect` arm for contrast (`:798-834`): shows loading at `:815` and
  never hides in Rust (frontend-owned).
- No `show_pr_list_sidebar`, no `architect:*` emit on this arm.

---

## 4. The dropped analysis (pre-fix) and the reuse fix

Pre-fix frontend — main orb window only
(`net/orchestrator.ts:197-206`, pre-fix numbering `:172-206`):

- `:181` `setLoadingVisible(false)`, `:184` `setVisible(true)`,
  `:194` `speak(text)`.
- `:199-201`: `if (ev.analysis) console.log(...)` — **logged, dropped.
  No `show_sidebar_with_analysis`, no `sidebar:show`, no `set_pending_*`.**
- `github_result` case (`:290-297`) only logs; `PrListApp` filters to
  `pr_list` type.

**Net effect pre-fix:** result spoken in the orb + appended to orb
transcript; `pr-list-sidebar` shows nothing new (button just re-enables
via `finally`); response `sidebar` never opens.

The pending-content system (race-free event delivery for on-demand
windows — `docs/features/21-liquid-glass-sidebar.md`, "How to Reuse")
existed but was **only used by the legacy `wsBridge` path**
(`net/wsBridge.ts:529-535`):

```
show_sidebar_with_analysis{query,text,analysis}  (commands.rs:764-814)
  → PENDING_SIDEBAR static (:38, :785-791)
  → show_sidebar_inner + (window_existed ? emit sidebar:show (:798-801)
                                              + emit sidebar:analysis (:802-806)
                                              : mount pulls via get_pending_sidebar_content)
SidebarApp mounts → get_pending_sidebar_content (:821-836, via SidebarApp.tsx:105-106)
  → show() / showAnalysis() (:119-125)
  + fast-path listeners sidebar:show (:132-134), sidebar:analysis (:137-144)
```

PR-list pending is a separate static: `PENDING_PR_LIST`
(`commands.rs:45`), set at `orchestrator.rs:1074` for `PrList` results
only. There was no `PENDING_*` for PR analysis — and none was needed.

**Fix applied:** the `WorkerBackend` Ok arm calls the EXISTING
`show_sidebar_with_analysis(app, transcript, text, analysis)` whenever
the returned `analysis` is non-null. No new window system, no new event
— the `AnalysisDashboard` renderer already exists in the response
sidebar. Voice-initiated "analyse pr …" gets the same treatment as the
button (same arm), which is the consistent behavior.

---

## 5. Why no new AI call was needed (Gemini question)

User asked for "gemini or other ai" to produce the analysis. Finding: the
Worker **already returns** the PR analysis in the `analysis` field of its
JSON reply (GLM/Mistral-class models today). The app fetched it, logged
it, and discarded it. The fix is routing, not inference. If the analysis
must come specifically from Gemini, that is a Worker-side model swap in
`server/worker/` (separate decision with quota/cost implications — the
Worker tracks per-user daily quotas: 500 requests, 3000 neurons, 10 deep
analyses; see `server/worker/src/quota.ts`), NOT an app change.

---

## 6. End-to-end flow (post-fix diagram)

```
pr-list-sidebar (PrListApp.tsx:166-179)
  Analyse click → setActionInProgress{analyse}
  → hide() + invoke hide_pr_list_sidebar      [NEW — list gone first]
  → invoke orchestrator_process{"analyse pr N in repo"}
        │
Rust orchestrator.rs:479 process_transcript
  → parse_deterministic → AnalysePr{repo,pr_number}
  → route_intent → WorkerBackend
  → emit State{Thinking} + Ack + Loading{visible:true}
  → show_loading (inline create+show, top-right 80×80)
        │
  dispatch_to_worker: POST Worker, parse reply_text/analysis/dialog_state
        │
  → emit Loading{visible:false} + hide_loading (sync destroy)
  → emit Result{text,analysis} on orchestrator:event
  → [NEW] analysis non-null → show_sidebar_with_analysis
        → PENDING_SIDEBAR set → response sidebar opens with dashboard
        │
Main orb (net/orchestrator.ts:197+)
  → setLoadingVisible(false) → hide IPC
  → setVisible(true) + speaking + speak(text, onEnd → finishSpokenResult)
  → onEnd: guarded reset + orchestrator_done   [companion-doc fix]
```

## 7. Verification

- `cargo check` clean; orchestrator 43/43 serial.
- Frontend `tsc` clean, vitest 14/14 (incl. 3 handshake tests).
- Live verification still needed (running app): click Analyse →
  list closes + spinner shows → response sidebar opens with the
  analysis dashboard while the orb speaks → orb settles to idle after
  audio. Voice "analyse pr N in repo" should behave identically.
