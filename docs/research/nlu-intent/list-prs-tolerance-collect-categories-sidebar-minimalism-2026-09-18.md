# Minimalist Sidebar + list_prs Tolerance + Collect Categories (2026-09-18)

Two user requests, one doc: (A) strip the sidebar gloss down to flat
minimal, and (B) make "show me the prs / show me the pull requests and
alll" work, train it with targeted phrasings, and let `nexus collect`
train by category. Part B's headline finding: **the data was fine (64
train rows) — the deterministic regex was the hole.**

---

## PART A — list_prs: data autopsy

Production `server/nlu/dataset.json` (measured 2026-09-18):

- TOTAL rows: 4084. Train intents: 52.
- `list_prs` total: **151** — train 64, validation 8, calibration 8,
  test 71. **Not thin.** (Thin intents for reference: `list_pr_files`
  7, `create_pr` 8, `remove_collaborator` 8, `search` 11,
  `update_branch` 14.)
- Sample train rows carry `slots: {"repo": ...}` (often `""`), e.g.
  STT-mishearing-robust rows like `"show me the pool request"`,
  `"so show pee ars"`.

Collect-script phrases (`scripts/collect_nlu_samples.py`, pre-fix) already
contained "show pull requests", "show the pull requests",
"show me the prs in nexus" — so even the voice-collection prompts
covered the noun. Conclusion: **50 more blind rows would barely move
anything.** The failure is upstream of the model.

---

## PART B — the regex hole (the actual bug)

`src-tauri/src/intent_parser.rs:1597` (pre-fix `:1594`):

```
^(?:give\s+me\s+(?:the\s+)?|show\s+(?:me\s+)?(?:the\s+)?|…)?   verb
(?:(open|closed|all|live|latest|active|merged)\s+)?            state (group 1)
prs?(?:\s+list)?                                              noun — pr/prs ONLY
(?:\s+in\s+(\S+))?$                                           repo (group 2) + END ANCHOR
```

Two defects:

1. **No `pull request` alternative.** "Show me the prs" matches;
   "show me the pull requests" NEVER matches deterministically, on any
   build (family or admin — admin only survives via the Qwen brain).
2. **`$` end-anchor with no trailing tolerance.** "…and alll" / "and all"
   / "all of them" / "everything" kills the match; the tail then confuses
   NLU (no such tails in the 64 rows) → `Unknown` → Worker.

Post-fix pattern (`:1597`): noun is
`(?:prs?(?:\s+list)?|pull\s+requests?(?:\s+list)?)`; optional `the` after
the state word ("give me all the pull requests"); trailing tolerance
`(?:\s+(?:and\s+)?all(?:\s+of\s+them)?|\s+everything)?` both before and
after the optional `in <repo>` group. Groups 1 (state) and 2 (repo) are
unchanged (all-new groups non-capturing), so downstream
`extract_repo`/auto-detect/empty-repo fallbacks (`:1608-1641`) behave
identically.

Tests (`intent_parser.rs:4644` `test_parse_list_prs_pull_request_wording`):
all of "show me the prs", "show me the pull requests", "show me the pull
requests and all", "show the pull requests in owner/repo", "give me all
the pull requests", "show prs and all of them", "pull up the pull request
list" parse to `ListPrs`; bare "show me everything" (no PR noun) is
asserted NOT to parse as `ListPrs`. Full module: **148/148 pass.**
(Test-writing caught a third gap live: "give me ALL THE pull requests"
failed until the optional `the`-after-state was added.)

---

## PART C — Phase 11 targeted data (45 rows, not 50 blind ones)

`server/nlu/add_phase11_pr_verbs.py` → `server/nlu/data/phase11_pr_verbs.json`:

- 39× `list_prs` (slot `repo` only, no new labels — no
  `train.py`/`nlu_server.py` sync needed): "pull requests" noun × 8 verbs
  (show me/show/give me/pull up/display/fetch me/get me/bring me),
  trailing tails (and all / and all of them / all of them / everything)
  on both noun forms, "pull request list" noun × 4 verbs, 8 repo-suffixed
  rows (zync/servx/nexus/shopkart), repo+tail combos, 5 state variants
  (open/closed/all/merged).
- 6× `unknown` OOS negatives for near-misses ("pull the door open",
  "request a refund for my order", "pull up a chair", "request time off
  tomorrow", …).
- Freshness proof: **0 dupes skipped** against all active splits — these
  forms were genuinely absent. Frozen-test family overlap check passes
  (raises on clash).

Wired into `server/nlu/build_candidate_dataset.py` as `PHASE11_FAMILIES`
(loader + dedupe + overlap verify + extend + logging, mirroring phases
8–10 exactly).

Results (2026-09-18):

- Candidate train: **3482** (+45). Validation 438 / calibration 429 /
  test 452 unchanged (locked hashes verified). No test-family overlap.
- `train_candidate.py` 50 epochs → frozen-test intent **0.9004** (was
  0.8850), slot 0.9466. `evaluate_candidate.py`: gated OOS (0.85) 0.9871
  (11 failures), supported-mappings gated 0.4000 (3 failures — pre-existing
  tiny benchmark, 5 rows).
- **Production `dataset.json` untouched (2765 train rows).** The device
  runs the bundled production model, so NLU-side gains reach the device
  only via a future production merge+retrain. The user's exact phrases
  work TODAY via the deterministic fix (ships with the next build) —
  promotion of phase 11 (+ phases 8–10, likewise staged) is a separate
  merge+retrain+rebuild decision.

---

## PART D — `nexus collect` category training

Pre-existing (already worked): `--intent`, `--count`, `--list`,
`--text-only`, `--yes`, `--status`; stratified tier scheduler
(`build_job_queue`: round-robin over tiers, least-covered first, resume
from gaps). Internal `TIERS`: mcp, github, messages, live, local, modes,
other.

Two changes (`scripts/collect_nlu_samples.py`):

1. **Duplicate-key bug fix.** `"list_prs"` appeared TWICE as a dict key
   (`:548` and `:692`) — Python silently keeps only the last, so **21
   phrases were dead** on every collection run. Merged into ONE 32-phrase
   list (union, incl. 4 new gap prompts: "show me the pull requests",
   "show me the pull requests and all", "give me all the pull requests",
   "pull up the pull request list"). Verified: exactly 1 key, 32 phrases.
2. **Interactive category menu** (`CATEGORY_MENU`, `:788`) + `--category`.
   Bare `nexus collect` on a TTY prints:

```
1. github     GitHub — merge/approve/close/list PRs, branches
2. mcp        Food / shopping / chat — order, search products, WhatsApp
3. apps       Apps & windows — open/close, search, settings, media
4. messages   Messages & dictation — type, confirm, ghostwriter
5. live       Live control — keys, browser tabs, focus
6. random     Random mix — stratified across every tier (default)
7. paraphrase Say it YOUR way — your own wording, random mix
```

   `--category <name|number>` for scripts; Enter = random mix;
   non-TTY or `--yes` never blocks on `input()`. `--list` now prints
   categories too.

**Why 6 categories, and why no google/research (user asked "how many categories"):**
a category may only name intents that exist — the model has 52 and zero
google/drive/maps/research-app intents, so a google/research category
would be empty (collecting into it would mean faking rows under
`unknown`). Six content categories = the five natural tier groups +
random. `messages` bundles `messages+modes` (type/confirm + ghostwriter
start/stop belong to one dictation session). Google/Drive/Maps/Research
are named follow-ups: each needs deterministic + NLU rows + a backend
(the MCP program pattern), not collection rows.

**Say-it-your-way mode — considered and REJECTED (user verdict same day).**
A paraphrase menu option was built, then removed: the user clarified the
need is NOT rewording ("way of saying it") but same-word/many-sounds →
one entity (`servx` = `cervx` = `srvx`), and that this must live inside
EVERY category, not as a separate option. The replacement is Part F
below. The `q`/Ctrl+C count fix and the scope-name fix from that pass
were kept (independent bugs).

Verified twice: `--list` output + a harness asserting the merged key
(32 phrases), every menu choice resolving to non-empty intents that all
exist in `PHRASES`, `random`→None, and `google`→KeyError.

---

## PART E — sidebar gloss inventory and removals

Pre-existing gloss (buttons themselves were already flat translucent —
the gloss lived in the containers):

**PR list (`frontend/src/pr-list/pr-list.css`):**
- `:45` top-highlight gradient → deleted.
- `:49-50` double colored borders → single neutral
  `1px solid rgba(255,255,255,0.12)`.
- `:52-57` 4-layer inset shadow stack → single outer
  `0 24px 64px rgba(0,0,0,0.6)`.
- `:69-83` `::after` gradient overlay → flat `rgba(8,8,10,0.45)` dim
  (backdrop photo kept for the glass look, readability preserved).
- `:88-112` `::before` specular rim (masked gradient border) →
  entire block deleted.
- Buttons `:268-286` translucent tints → flat solids: Merge `#2ea043`
  (hover `#388e3c`), Analyse `#3a3a3d` (hover `#48484c`), white text,
  1px solid borders. No gradients/insets/`backdrop-filter` on `.pr-btn*`
  (none existed).

**Response sidebar (`frontend/src/sidebar/sidebar.css`):**
- Card `:139-151`: top-highlight gradient, double borders, inset stack →
  same flat treatment as PR list.
- `::after` gradient overlay → flat dim; `::before` specular rim
  (`:187-201` + fallback ref) → deleted.
- `:238` badge inset shadow → deleted; `:241-253` pulsing dot glow +
  `@keyframes apple-pulse-dot` → flat static dot.
- Action buttons `:296/:304/:307-310/:316`: drop shadow, hover lift
  (`translateY`), active press, red active glow → flat color-change-only
  hover; state tints kept (no glow).
- `:333-337` red close-button hover → neutral hover (matches all other
  buttons).
- Dead `backdrop-filter`s (documented no-op in transparent WebView2,
  `MicrosoftEdge/WebView2Feedback#4945`): `:747` zoom-btn, `:851`
  scroll-top, `:1155-1156` lightbox overlay → deleted.
- `:510` `nexus-hr` gradient → flat `var(--apple-border-default)` 1px.
- `:46-53` unused `--gradient-*` vars (defined, zero references
  repo-wide) → deleted; `--lg-sheen-opacity/--lg-rim-width` zeroed with
  a "retired" comment (pr-list `:8-12` likewise).
- Deliberately kept: chart colors in `AnalysisDashboard` (data-viz needs
  color), state tints (success/active), code/markdown theme colors.
- Untouched: settings sidebar (still glossy — out of scope).

Verified: `tsc` clean, vitest 14/14 (CSS has no unit tests — visual check
is the live step: open PR list + response sidebar, confirm flat panels,
readable text over the dimmed backdrop, solid buttons).

---

## PART F — repo sound-alias map (same word, many sounds, one entity)

User correction: the need is pronunciation tolerance, not rewording.
`servx` said as `cervx`/`srvx`/`service` (all real Groq outputs from the
collection log) must resolve to the same `servx` — inside every category,
not as a separate option.

- `intent_parser.rs`: `repo_sound_alias()` table (servx ×9 aliases, zync
  ×5, congi, meet, shopkart, ledger-ai) + `pub canonical_repo_name()`
  (per-segment mapping on `owner/name`, unknown segments pass through —
  never rewrites a real name). Wired into `clean_repo_name()`, so every
  deterministic repo consumer (list/get/merge/approve/close…) inherits it.
- `nlu_client.rs`: `repo_slot()` helper applies the same map; all 22
  `slots.get("repo")` arms route through it — deterministic, NLU, and
  brain paths agree on the entity.
- Side effect (intended): "analyse pr 254 in zink" now resolves exact at
  1.0/deterministic instead of fuzzy at 0.8 — the canonical match beats
  the fuzzy guess. Test updated to pin the better outcome.
- Tests: alias table (every logged mishearing), per-segment paths,
  pass-through of unknown names, end-to-end "list prs in cervix" →
  `ListPrs{repo: servx}`. intent_parser 150/150, github_cmd 61/61,
  orchestrator 43/43 serial.
