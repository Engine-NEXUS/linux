# 34 — Motion Fixes + PR-Analyse Flow + Minimal Sidebar + Phase 11 Data (2026-09-18)

Two user-reported defects ("stuck wakeup animation after long replies",
"Analyse button does nothing visible") plus two experience requests
("sidebar too glossy — make it minimal", "train list_prs with 50
possibilities + category-selectable voice collection"). Every fix was
traced to root cause via two full research passes before editing,
verified twice after (per house rule), and documented in three research
files in `docs/research/`.

---

## 1. Stuck wakeup animation — root cause, not symptom

### 1.1 Symptom and the false lead the user suspected

After a long WorkerBackend reply (PR #68 analysis: ~15s of chunked TTS
plus a second spoken segment), the orb never returned to idle — it held
the `speaking` state with the loading-loop Lottie segment indefinitely.
The user believed a "ghost mode" they had applied caused it. A full
case-insensitive grep of `frontend/` and `src-tauri/src` found **zero
ghost-mode UX**: the only "ghost" strings are the Ghostwriter dictation
mode (`ghostwriter.rs`, `intent_parser.rs::parse_ghostwriter_entry`,
`orchestrator.rs` session handling) and a "ghost-hands" comment in
`live/commands/window.rs` describing the `AttachThreadInput`
window-focus trick. **There is no ghost visual mode anywhere**; the
cause was structural (§1.3).

### 1.2 How the orb animation actually works (research pass 1)

- `frontend/src/avatar/Avatar.tsx` — Lottie `wakeup.json`;
  `SEG_LOADING=[171,260]`, `SEG_SMILE_ARRIVE=[261,316]`,
  `FRAME_HOLD_SMILE=300`; `resolveAvatarAnim()` maps
  `listening → wake-loading`, `thinking|speaking → loading-loop`,
  `idle → idle-smile`; a mode-unchanged guard skips restarts between
  `thinking→speaking`.
- `frontend/src/store/assistant.ts` — states
  `idle|listening|thinking|speaking`; canonical transition table allows
  `speaking → idle` **only via the `done` event path**.
- `frontend/src/net/orchestrator.ts` — the `orchestrator:event` listener;
  the `done` handler (`setLoadingVisible(false)`, brief visible beat,
  `reset()` after 550ms) was **the only orb-reset path** for the Worker
  flow.
- `frontend/src/App.tsx` — the native window show/hide (`show_overlay`/
  `hide_overlay` with 600ms delay); the 8s auto-hide fires for
  `listening` **only** — there was no backup for `speaking`.

### 1.3 Root cause: a one-sided `done` handshake

Rust deliberately withholds `OrchestratorEvent::Done` on the
`WorkerBackend` success path (`orchestrator.rs`, `Subsystem::WorkerBackend`
arm) — the comment explains that emitting `Done` immediately would cancel
TTS before the user hears the answer. The counterpart — frontend signals
`orchestrator_done` after TTS finishes — was never implemented:

- the `result` handler called `speak(ev.text)` fire-and-forget with no
  `onEnd`;
- `signalOrchestratorDone()` existed with **zero call-sites** (dead code).

Result: `speaking` was a terminal state for every long reply. Short
replies masked it (the next wake/barge-in reset the orb). Error paths
still emitted `Done`; `GitHub`/`Mcp` arms too — only `WorkerBackend Ok`
orphaned.

A secondary confirmed race: `show_loading` ran its window create inside
`tauri::async_runtime::spawn` while `hide_loading` synchronously
destroys. A fast Worker reply can destroy before the spawned create
completes, re-showing the spinner with no queued hide. (The 800ms
min-dwell in `store/loadingMachine.ts` widens the window for the
frontend half of the same inversion.)

### 1.4 Fixes

1. **`finishSpokenResult()`** (`frontend/src/net/orchestrator.ts`, new
   exported function; wired into the `result` handler): TTS completion
   (or TTS-promise rejection) → request-id-guarded reset + clean
   long-running flag + brief visible beat + `orchestrator_done` invoke.
   The guard makes a stale `onEnd` from a barged-in turn inert: it
   returns false and touches nothing when `currentRequestId` doesn't
   match, so a new turn's state is never clobbered (the cancel flow owns
   barge-in).
2. **60-second speaking failsafe** (`frontend/src/App.tsx`, new effect):
   when `state === "speaking"` and 60s pass without Rust TTS actually
   playing (observed via the exported `isRustTtsPlaying()`), the store is
   force-reset the same way `done` does it. A genuinely long reply
   re-arms the timer instead of cutting audio mid-sentence.
3. **Single-owner loading window** (`src-tauri/src/orchestrator.rs`,
   `show_loading`): the spawned-async create was replaced by an inline
   call — the create always completes before the paired destroy can run,
   eliminating the re-creation wedge.

### 1.5 Verification

- New suite `frontend/src/net/orchestrator.test.ts` (vitest, jsdom-stubbed
  `@tauri-apps/api/core`, wsBridge, sidebar store):
  - current-turn completion resets to `idle` and invokes
    `orchestrator_done` with the right id;
  - a stale id (barge-in replaced it) touches nothing;
  - a stale id after cancel touches nothing.
- Full frontend suite 14/14, `tsc --noEmit` clean — run twice.
- Rust: `cargo check` clean; `orchestrator::` mod 43/43 with
  `--test-threads=1`.
- Known pre-existing flake documented (unrelated): `test_install_and_cancel`
  fails under parallel threads (shared `ACTIVE_REQUEST` global, the test
  itself notes the dependency); passes isolated and serially.

### 1.6 Live follow-ups (state at time of writing)

Handshake + failsafe are unit-verified; a live long-reply run (Analyse a
PR with a big diff, watch the orb settle to `idle` after audio) was still
pending user confirmation. The durable lesson: **state was stuck in
speaking after Long replies — if any recurrence appears after this
build, look at the TTS `onEnd` path first (a `speak()` that never
resolves), not at anims.**

---

## 2. Analyse-PR button flow — from dead-end to dashboard

### 2.1 Baseline behavior (what the user saw)

The PR-list sidebar card's `Analyse` button (PrListApp.tsx) only invoked
`orchestrator_process({transcript: "analyse pr N in repo"})`. It never
closed the sidebar, and the `WorkerBackend` Ok path emitted
`Result { text, analysis }` where the frontend's `result` handler
**literally `console.log`ged the `analysis` field and dropped it** — the
Worker had already produced a full structured analysis; it never got
rendered.

### 2.2 Why the fix needed no new AI call (verified in-tree)

`dispatch_to_worker` had been parsing `data["analysis"]` from the Worker
reply since before Phase 5 of the PR program; the `analysis` field
already existed end-to-end and was just discarded. "Use Gemini to analyse"
was therefore a routing misunderstanding, not a missing model — the fix
was plumbing the already-arriving payload into the already-existing race-
free sidebar rails (`PENDING_SIDEBAR` →
`show_sidebar_with_analysis` → `get_pending_sidebar_content` →
`AnalysisDashboard`). A model swap (GLM/Mistral → Gemini) remains a
separate Worker-side decision, listed-but-not-built.

### 2.3 Fixes

1. **Button pre-close** (`frontend/src/pr-list/PrListApp.tsx`, `handleAnalyse`):
   calls `hide()` + `invoke("hide_pr_list_sidebar")` **before** the
   orchestrator invoke (same path as Ctrl+Space), so the list gives way
   immediately and the loading indicator covers the wait.
2. **Backend routing** (`orchestrator.rs`, `WorkerBackend` Ok arm): after
   emitting `Result` (TTS starts first, never blocked on window
   creation), if the Worker returned non-null `analysis`,
   `show_sidebar_with_analysis(app, transcript, text, analysis)` opens the
   response sidebar with the dashboard via `PENDING_SIDEBAR` — the exact
   race-free pattern the response sidebar already uses for fresh windows.
3. Voice-initiated "analyse pr N in repo" and button-clicked analysis
   converge on the same arm, so both render the dashboard.

### 2.4 Verification

`cargo check` clean; orchestrator module 43/43 serial; frontend tsc +
vitest 14/14 — twice. Live click-through still pending build
(button → list hidden → loading → dashboard + speech → orb settles,
which §1's handshake now guarantees).

---

## 3. Minimal sidebar — the gloss inventory

User rule: flat panels, no extra colors, buttons not glossy. The buttons
themselves were already flat translucent; the gloss lived in the
containers. Deleted across `frontend/src/pr-list/pr-list.css` and
`frontend/src/sidebar/sidebar.css`:

- container top-highlight `background-image` gradient;
- double colored borders (incl. `border-top` highlight) → single neutral
  `1px solid rgba(255,255,255,.12)`;
- the 4-layer inset `box-shadow` stack → single outer drop shadow;
- the `::before` specular sheen (masked gradient border) — entire block;
- the `::after` gradient overlay → flat `rgba(8,8,10,.45)` scrim (the
  blurred desktop backdrop image is kept — readability over the glass —
  just no color overlay on top);
- badge inset shadow, pulsing dot keyframes + glow, action-button drop
  shadow, hover `translateY` lift/press transform, red active glow
  (`--active` keeps its tint color, glow removed), red close-button
  hover → neutral hover;
- `nexus-hr` gradient → flat `var(--apple-border-default)` 1px;
- all 8 unused `--gradient-*` custom properties (verified zero
  references repo-wide);
- dead `backdrop-filter`s (zoom button, scroll-top, lightbox overlay) —
  documented no-ops in transparent WebView2 (`WebView2Feedback#4945`,
  per `AGENTS.md` sidebar notes);
- `--lg-sheen-opacity: 0`, `--lg-rim-width: 0px` retained as documented
  no-ops so retired rules can't misbehave.

Kept deliberately: chart/diff colors in `AnalysisDashboard` (data-viz),
state tints (success/active), code-theme colors. Settings sidebar left
glossy (out of scope per user instruction). PR-list buttons became flat
solids: Merge `#2ea043` → hover `#388e3c`; Analyse `#3a3a3d` → hover
`#48484c`, white text.

Verification: frontend tsc clean + vitest 14/14, twice. Visual QA
(panels flat, text over dimmed backdrop readable, solid buttons) is the
live-build step.

---

## 4. list_prs "and alll" — regex hole, not data hole

### 4.1 The data autopsy (do the boring check first)

Counts from production `server/nlu/dataset.json`: `list_prs` total **151**
rows — train 64, validation 8, calibration 8, test 71. Not thin (thin =
7-14 train rows elsewhere). Existing rows even covered "pull requests"
and STT-garble forms ("show me the pool request"). Conclusion: 50 more
blind rows would barely move metrics — the failure was upstream.

### 4.2 The regex (the real bug)

The deterministic `list_prs` pattern in
`src-tauri/src/intent_parser.rs` only accepted `prs?` and anchored at
`$`:

1. no `pull request(s)` alternative — "show me the pull requests" never
   matched deterministically on any build (family or admin);
2. `$` with no trailing tolerance — "…and alll"/"and all"/"all of them"/
   "everything" killed the match, and those tails also weren't in the
   BERT rows, so NLU fell to Unknown → Worker.

**Fix:** noun is now `(?:prs?(?:\s+list)?|pull\s+requests?(?:\s+list)?)`;
optional `the` after the state word ("give me all the pull requests");
trailing tolerance `(?:\s+(?:and\s+)?all(?:\s+of\s+them)?|\s+everything)?`
both before and after the optional `in <repo>` group. Capture groups
(state, repo) are unchanged in position — downstream repo cleanup,
auto-detect, and empty-repo account-wide fallback behave identically.

A negative guard: bare "show me everything" (no PR noun) must NOT parse
as list_prs — asserted in tests.

### 4.3 Phase 11 — targeted rows, not blind rows
`server/nlu/add_phase11_pr_verbs.py` generates **45 fresh rows**
(39× `list_prs` + 6× `unknown` OOS negatives: "pull the door open",
"request a refund for my order", "pull up a chair", …):
"pull requests" noun × 8 verbs ("show me/show/give me/pull up/display/
fetch me/get me/bring me"), both noun forms × 4 trailing tails, "pull
request list" noun × 4 verbs, repo-suffixed rows (zync/servx/nexus/
shopkart) including repo+tail combos, 5 state variants. No new labels
(55/51 unchanged — no `train.py`/`nlu_server.py` sync needed). Wired into
`build_candidate_dataset.py` as `PHASE11_FAMILIES` (dedupe + frozen-test
family-overlap check + extend + logging, mirroring phases 8-10 exactly).

### 4.4 Candidate results (production untouched)

- Candidate train 2742 → **3482** (+45; 0 duplicates skipped — the forms
  were genuinely absent). Locked splits unchanged (validation 438 /
  calibration 429 / test 452; hashes verified).
- `train_candidate.py` 50 epochs → frozen-test intent **0.9004** (from
  0.8850), slot 0.9466. `evaluate_candidate.py`: gated OOS (0.85)
  **0.9871**; supported-mappings gated 0.4000 (5-row micro-benchmark,
  pre-existing).
- Production `dataset.json` (2765 train rows) untouched — the device
  runs the bundled model, so NLU-side gains land via a future production
  merge; the user's exact phrases work today because the **regex** fix
  ships in the next build.

### 4.5 Tests

`test_parse_list_prs_pull_request_wording` covers the seven phrasings
above; intent_parser module 148/148 after the change (150/150 after the
2026-09-20 sound-alias work landed in the same module). The test writing
itself caught a third gap live — "give me all the pull requests" needed
the optional `the`-after-state.

---

## 5. Voice collection — categories + the merged-key bug

### 5.1 Duplicate `list_prs` key (silent data loss)
`scripts/collect_nlu_samples.py` had `list_prs` twice as a dict key
(first 20 phrases, later 12 more). Python keeps only the **last** key, so
21 phrases were dead on every run. Merged into one 32-phrase list (union
+ 4 gap prompts: "show me the pull requests", "…and all", "give me all
the pull requests", "pull up the pull request list").

### 5.2 Category menu
Interactive numbered menu on bare `nexus collect` (TTY, not `--yes`):
`github / mcp / apps / messages / live / random` (+ `--category <name>`
for scripts). Categories resolve onto the internal stratified `TIERS`
(the scheduler already round-robins tiers, least-covered first, resuming
from gaps via `collect_progress.json`). `messages` bundles the dictation
tier (`type_text`, `confirm_send`, ghostwriter start/stop).

**Why 6 and why no google/research category** (user asked "how many"): a
category may only name intents that exist — the model has 52 (55 labels
in candidate) and zero google/drive/maps/research intents; collecting
into an empty category would mean faking rows under `unknown`. Those are
named follow-ups, each needing parser + NLU rows + a backend (the MCP
program pattern), not collection rows.

A "say-it-your-way / paraphrase" option was built after a first
discussion, then **deliberately reverted** the same day: the user
clarified the need is pronunciation tolerance (same word, many sounds →
one entity), not rewording — replaced by the sound-alias map in §6.
The `q`-path counter bug found during that fix was kept: quitting now
recomputes the run count from saved progress instead of printing 0, and
the scope line prints the category name rather than the raw digit.

### 5.3 Verification
Module-level harness: exactly one `list_prs` key with 32 phrases; every
menu choice resolves to non-empty intents that all exist in `PHRASES`;
`random`→None; unknown→KeyError. `--list` output shows the menu. Run
twice.

---

## 6. Same-week adjacent fix — repo sound aliases (2026-09-20, same pillars)

(Recorded here because it was born from the same collect-session log;
implementation details live in doc 35's shape): the collection session's
real STT transcripts ("cervix", "srvx", "service", "Zinc", "incognito")
became `canonical_repo_name()` in `intent_parser.rs` — a per-segment
alias table wired into `clean_repo_name` (all deterministic repo
consumers) and `nlu_client::repo_slot` (all 22 repo arms). Side effect:
"analyse pr 254 in zink" resolves exact at 1.0 instead of fuzzy at 0.8.
Unknown names pass through untouched.

---

## 7. Files touched (high-level map)

| Area | Files |
|---|---|
| Motion handshake | `frontend/src/net/orchestrator.ts` (+ new test file), `App.tsx`, `src-tauri/src/orchestrator.rs` |
| PR analyse flow | `frontend/src/pr-list/PrListApp.tsx`, `orchestrator.rs` (WorkerBackend Ok arm), existing `commands.rs` sidebar rails |
| Minimal sidebar | `frontend/src/pr-list/pr-list.css`, `frontend/src/sidebar/sidebar.css` |
| list_prs + data | `src-tauri/src/intent_parser.rs`, `server/nlu/add_phase11_pr_verbs.py`, `server/nlu/build_candidate_dataset.py`, `scripts/collect_nlu_samples.py` |
| Repo aliases | `src-tauri/src/intent_parser.rs`, `src-tauri/src/nlu_client.rs` |

Research companions (same-day deep dives, filed 2026-09-18 in
`docs/research/`): `orb-stuck-animation-root-cause-2026-09-18.md`,
`pr-analyse-button-flow-2026-09-18.md`,
`list-prs-tolerance-collect-categories-sidebar-minimalism-2026-09-18.md`.
