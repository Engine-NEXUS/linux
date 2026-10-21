# 133 — Does the other assistant's Ghost "no-vision" plan affect our voice + RAM plan? (IMPACT + OWNERSHIP RULES, nothing built)

Date: 2026-10-16. Context: another assistant (GPT-sol) is implementing "Ghost Mode: reliable desktop control without a vision model" (new `ghost_center.rs` 752 lines, `ghost_learning.rs` 319 lines, edits in `orchestrator.rs`, `center.rs`, `intent_parser.rs`, `live/commands/mouse.rs`, `lib.rs`, `commands.rs`, `ghost.rs`, frontend `ghostHotMic.ts`). Owner instruction: **proceed with our plan without touching GPT-sol's changes.**

Baseline checked now: `cargo check --features custom-protocol,admin-brain` on the shared working tree = **clean** (their work included), 35 s.

## 1. Short answer
**Yes, it touches our plan in five places, but nothing in it blocks the plan, and it does not change the RAM target.** Two of the five need a small agreement so the two plans do not collide. No Ghost-plan feature has to be dropped or delayed for our plan.

## 2. Impact, item by item
| # | Where their plan meets ours | Effect | What we do |
|---|---|---|---|
| 1 | **RAM**: their UIA discovery cache (bulk element snapshots), learned-controls store (`ghost/learned_controls.json`, small), receipts, probes. `ghost_center.rs` has **no static caches** (checked: no `Lazy`/`OnceLock`/`HashMap` caches). They REMOVE the vision fallback from the Ghost click path, which removes screenshot buffers (a 1080p capture is ~8 MB raw + JPEG/base64 copies). | **Neutral to slightly positive.** Ghost budget in doc 132 (20 MB) still holds **if** the UIA snapshot cache is bounded. A browser/Electron window can expose thousands of elements. | Ask for one rule in their Phase 4: snapshot cache capped (e.g. <= 2 000 elements, 5 s TTL, dropped when the Ghost session ends). We register it as an evictable item in our governor. We do not edit their code. |
| 2 | **Voice latency**: their receipts make NEXUS speak far more often in a Ghost session ("Done", "I couldn't verify that", "That window changed, so I stopped", clarifications). | Without care, each line could trigger a Kokoro reload (our idle-unload) or a slow first sentence. | (a) Rule in our governor: **keep the voice warm while a Ghost session is active** (unload only after the session + 120 s). (b) Add the receipt phrases to the stock-line clip bank, **generated from their outcome strings** by a test that reads them, so we never copy or edit their wording. |
| 3 | **Questions**: their teach/accept flow, clarification ("which one?") and risky-action confirmations are all **spoken questions waiting for a short answer** - exactly the faults found in doc 126 ("do it" heard as "Dread.", question destroyed on a miss, no memory of the last question). | Their flows will inherit those faults unless our R1-R3 land, or they build a parallel question system (two systems = conflict). | One owner for question state: **`memcore/offer.rs` + the answer-aware STT (doc 126)**. Their flows should add **new variants appended** to `Offer` (e.g. `GhostTeach`) rather than a second mechanism. We tell them in the plan and implement the shared API; they plug in. |
| 4 | **Settings**: they add settings (shadow mode, rollback switch for the planner, per-app enablement). | The stale-snapshot overwrite bug (doc 128 Part B) can silently revert their rollback switch when a Settings window saves. | Our settings hot-fix (S0/S1 patch-based save) protects their flags. We touch `commands.rs` only to add the patch command, with narrow anchored edits that leave their fields alone. |
| 5 | **STT**: their commands need Ghost verbs in the decoder vocabulary; our plan adds `DecoderBias::Answer` for yes/no turns. Both edit `stt_groq.rs` (it already has +33 uncommitted lines from someone). | Same-file collision risk, no logical conflict. | They own `NEXUS_VOCABULARY`; we add only a new enum variant + prompt constant + selector. Re-read the file before each edit; never reformat it. |
No impact on: the Kokoro engine work (`tts_kokoro.rs`, `tts_swap.rs`, `voice_catalog.rs`), the compact dictionary, the ORT settings, NLU unload, the governor module, the orb GPU release. Those files are ours.

## 3. File ownership (so we never overwrite each other)
| Owner | Files |
|---|---|
| **GPT-sol (do not touch)** | `ghost_center.rs`, `ghost_learning.rs`, `ghost.rs`, `live/commands/mouse.rs`, `shortcuts/*`, `screen.rs`, `frontend/src/net/ghostHotMic.ts`, the Ghost section of `intent_parser.rs` / `center.rs` / `orchestrator.rs`, `docs/**/12-no-vision-*`, `103-*`, `119-*` |
| **Ours (voice, RAM, questions, settings store)** | `tts.rs`, `tts_kokoro.rs`, `tts_swap.rs`, `tts_network.rs`, `tts_edge.rs`, `tts_health.rs`, `tts_system.rs`, `tts_cache_disk.rs`, `voice_catalog.rs`, `nlu_local.rs`, new `ram_budget.rs`, `memcore/offer.rs`, `memcore/briefing.rs`, `power_cmd.rs`, `power_close.rs`, `conversation.rs`, `proactive_policy.rs`, settings Memory card, docs 126-133 |
| **Shared (narrow, anchored edits only)** | `orchestrator.rs`, `lib.rs`, `commands.rs`, `center.rs`, `intent_parser.rs`, `stt_groq.rs`, `wakeword_oww.rs`, `frontend/src/net/orchestrator.ts`, `AGENTS.md`, `frontend/src/settings*/` |

## 4. Working rules for the shared files
1. **Compile baseline first** (done: clean). Re-run `cargo check` before and after each of our edits; if it breaks in a file we do not own, stop, report, do not fix it.
2. **Read the exact lines immediately before editing**; use anchored replacements only (never rewrite a whole shared file, never a formatter pass, never `git checkout/restore/stash/reset`).
3. **Append, do not reorder**: new enum variants, new settings fields, new `mod` lines go at the end of their list.
4. Our new behaviour lives in **our own modules**; shared files only get a one-line hook that calls into them.
5. Tests: run ours plus the full suite; failures in their tests are reported to the owner, not edited.
6. No commits unless the owner asks; AGENTS.md gets one appended entry per finished step.
7. Anything of theirs that must change for our plan is written up as a request in this doc, not implemented by us.

## 5. What changes in our plan (small)
* Doc 132 governor: add "keep voice warm while a Ghost session is active" and "UIA snapshot cache = evictable".
* Doc 129 stock-line bank: add the Ghost receipt phrases via an outcome-string-reading test.
* Doc 126: `Offer` becomes the single question API; publish `Offer::*` extension points for Ghost teach/confirm.
* Doc 128 S1: the patch-based settings store must keep unknown/new fields (their flags) intact.
Order of our work stays: P0 measure -> R1/R4/NLU unload quick wins -> dictionary -> governor -> questions (126) -> settings store -> the rest. Nothing waits for their work and nothing they do invalidates it.

## 6. Requests to hand to the other assistant (do not implement ourselves)
1. Bound the UIA snapshot cache (count + TTL + drop at session end).
2. Expose receipt/outcome phrases as constants (or one function) so our bank can read them.
3. Use `memcore::offer::Offer` (append a variant) for teach/confirm questions instead of a new mechanism.
4. Keep `stt_groq.rs` edits limited to `NEXUS_VOCABULARY`; we add only the new answer-bias variant.
5. Put any new settings in `NexusSettings` with serde defaults (so the patch-based save keeps them).

## 7. Not verified
I have not read their implementation line by line (only module sizes, statics, file paths, git status and a clean `cargo check`); the snapshot-cache size, the exact outcome strings and the final settings keys are unknown until their code settles. Overlap in shared files is judged from `git status` and file names; `git diff` cannot separate their uncommitted edits from earlier sessions' edits.
