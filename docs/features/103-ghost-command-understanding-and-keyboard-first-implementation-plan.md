# 103 — Ghost Mode: Command Understanding + Keyboard-First Execution — Combined Implementation Plan

**Date:** 2026-10-08 · **Status:** **Phase R done; P0 tooling built and partly run** (see §5.1 and `docs/changes/118-…`); everything else is PLAN.
**Merges:** `102-keyboard-first-shortcut-brain-plan.md` (what to do with a command) **+** the command-understanding findings in
`docs/research/ghost-mode/10-command-understanding-phrasing-space-and-measured-accuracy-2026-10-08.md` (how to *recognise* the command)
**Evidence base:** research docs 09 and 10 (measured on the real parser, 26,676 phrases, 126 real log lines, 65 adversarial ambient sentences).
Where this plan and 102's phase table differ, **this plan wins**; 102 remains the design reference for the execution side (§§2–6 there).

---

## 1. The problem in one table (measured today)
| What the user wants | Measured reality | Root cause |
|---|---|---|
| "Say it any way you like" | Bare core phrase 43 %; one polite frame word → **3.6 %**; four wrapper words → **0 %** | Literal lists per arm; no generic wrapper stripping; no synonym classes |
| Keyboard actions by voice (save, copy, undo, scroll, refresh, back, next/prev tab, zoom…) | **19 of 36 actions have no intent**; 96.5 % clean-miss, 3.5 % wrong action | They were never added; each addition is a code edit |
| Never do the wrong thing | Wrong action in 8.2 % of realistic inputs; **7.7 %** of everyday sentences accepted as commands | Permissive free-slot arms (`open/close/search/type <anything>`) run before validation |
| Survive the speech recogniser | Noise keeps only 53–76 % of baseline; wrong-action stays 6–9 % | No repair, no clarification, no confidence-aware behaviour |
| No delay, perfectly | Understanding stage is already ~0.2 ms (not the bottleneck); mouse path ≈ 1.5 s of designed motion (doc 09) | Ceremony, not compute |

## 2. Target architecture — one pipeline, two tracks

```
 mic ─► [U0 audio gate] ─► STT (dynamic prompt) ─► [U1 normaliser] ─► [U2 grammar over lexicon]
                                   │                     │                 │ miss
                                   │                     │                 ▼
                                   │                     │         [U3 phonetic repair on closed vocab]
                                   │                     │                 │ miss
                                   │                     │                 ▼
                                   │                     │         [U4 semantic tier (BERT-Mini, in-process) + OOS gate]
                                   ▼                     ▼                 ▼
                          confidence signals ───────────► [U5 decide]  ── EXECUTE / CONFIRM / CLARIFY / IGNORE
                                                                          │ EXECUTE: CanonicalCommand{action_id, slots, confidence, trace}
                                                                          ▼
 Track E (doc 102): [E1 Action Router] ► keyboard (Shortcut Map) ► UIA pattern ► fast mouse ► vision ► verify (+1 fallback)
```
**Contract between tracks:** `CanonicalCommand { action_id, slots, app_hint, confidence }`. `action_id` is the *same* closed vocabulary as the Shortcut Map's
(`new_tab, close_tab, reopen_tab, next_tab, prev_tab, goto_tab_n, refresh, go_back, go_forward, find_in_page, save, copy, paste, cut, undo, redo, select_all,
scroll_up/down, zoom_in/out, minimize, maximize, switch_window, address_bar, type_text, press_key, click_element, …` — 36 today, extensible).
Understanding never decides *how* (key vs pattern vs mouse); execution never re-interprets *what*.

### 2.1 Module layout (new, additive; legacy `parse_deterministic` stays as fallback behind a flag)
```
src-tauri/src/cmdu/                      (command understanding)
  mod.rs        understand(raw, &Ctx) -> Understanding   // the single entry point
  lexicon.rs    load/validate lexicon.json; compile token tables
  lexicon.json  THE source of truth (verbs, determiners, objects, slot types, layers, aliases, flags)
  norm.rs       U1  normaliser (pure, table-tested)
  grammar.rs    U2  compositional matcher
  phonetic.rs   U3  phoneme-distance repair (G2P cache) + learned aliases
  decide.rs     U5  thresholds, destructive gates, clarification state machine
  trace.rs      "[CMDU] …" log lines for run.ps1 + per-stage timing
src-tauri/src/shortcuts/                 (doc 102)
  map.rs  seed/*.json  router.rs  harvest.rs  probe.rs  learn.rs
src-tauri/tests/
  phrasing_probe.rs        (exists, #[ignore], research)
  phrasing_regression.rs   NEW, runs in CI: fixed corpus + ambient negatives + real-log list
  fixtures/phrasing_corpus.jsonl  ambient_negatives.txt  real_log_recoverable.tsv  heldout_paraphrases.jsonl
scripts/phrasing_probe/    (exists) → generator reads lexicon.json instead of its own lists
```
`Understanding { outcome: Execute(cmd) | Confirm(cmd, prompt) | Clarify(question, pending) | Ignore(reason), confidence, trace }`.
`Ctx { foreground_app, ghost_active, hot_mic, installed_apps, recent_apps, pending_clarification, last_verb_at }`.

## 3. Understanding track (U) — what each stage does

### U0 Audio gate & signals (exists in part)
- Keep VAD endpointing and the directed-speech gate (`directed.rs`). **Add:** log `avg_logprob` / `no_speech_prob` per command turn (the verbose path and `GroqSegment` already exist) and clip duration → used only to *tighten* thresholds in hot-mic, never to rescue.
- Very short clips (< 0.4 s) are flagged `maybe_truncated` (feeds U5 clarification).
- **Dynamic recogniser prompt:** `stt_groq` takes `prompt: Option<&'static str>`; change to an owned string built from: static vocabulary + foreground app name + top installed/recent apps + a few visible control names + action labels. Flag `sttDynamicPrompt`. *Prompt length limit and effect on 1–3-word commands are [unverified] → A/B in P0 before enabling.*

### U1 Normaliser (`norm.rs`) — the highest-ROI, lowest-risk step
Pure functions, each unit-tested with tables, applied in this order:
1. Case/punctuation/whitespace; Whisper trailing `.`/`?`.
2. Disfluencies and stutters (`uh/um/like/you know`, `open open`).
3. **Segmenting:** split on sentence boundaries; evaluate **last segment first**, then earlier ones, then the whole (generalises today's `new tab`-only `rfind(". ")` hack and fixes `"Ssort X2. Ghost mode."`).
4. **Wrapper stripping (generic, annotated, reversible):** wake (`nexus`, `hey nexus`, `okay nexus`, `nexus,`) · filler · **polite/intent frame** (`can/could/would/will you (please)`, `please`, `I want/need/I'd like you to`, `go ahead and`, `let's`, `try to`, `you should`) · qualifier tail (`please`, `for me`, `right now`, `quickly`, `now`, `sir`, `here`). Words that change meaning (`again`, `twice`, `all`, `last`, `next`, `back`) are **not** wrappers.
5. Numbers: digit/word/ordinal unify; homophone-in-context (`tab to` → `tab 2`, `for` → `4` only after `tab|number`).
- *Expected effect (measured baseline in doc 10 §3.1):* the 1-, 2-, 3-, 4-wrapper buckets (25 % / 12 % / 3 % / 0 %) should converge to the bare-core figure; the core figure itself is U2's job.

### U2 Lexicon + compositional grammar (`lexicon.json`, `grammar.rs`)
```json
{ "layers": { "wake": [...], "filler": [...], "frame": [...], "qualifier": [...] },
  "slots":  { "app": {"resolver":"app_registry"}, "query": {"free":true,"max_tokens":12,"reject_aux":true},
              "text":  {"free":true}, "key": {"enum":["enter","escape",...]}, "number": {"range":[1,9]}, "element": {"resolver":"uia_names"} },
  "actions": { "new_tab": { "verbs":["open","create","make","start","add","launch","give me","get me"],
                            "dets":["a","the","another","one more",""], "objs":["new tab","tab","blank tab","fresh tab","empty tab","new browser tab"],
                            "bare":["new tab"], "destructive":false, "needs":"browser" }, ... },
  "aliases": { "tap":"tab", "wave":"brave" }       // seed; learned aliases live elsewhere
}
```
- Matching = longest-match over token tables (multi-word verbs), optional determiner, object class with typed slots, plus explicit `bare` forms (`refresh`, `undo`, `scroll down`). Entries **add**; coverage **multiplies** (444 entries span 37 M forms in the research model).
- **Ordering rule that removes most wrong parses:** closed-lexicon actions are tried **before** open-slot arms; open-slot arms (`open/close/search/type <tail>`) must pass **slot validation**: app tails resolve via the registry with coverage ≥ floor (`assess_open_target` already exists for `open`; add the same for `close`); free-text tails must look like a noun phrase (no auxiliaries/pronouns: *"engines have changed a lot"*, *"your mouth and say ah"*); `type` followed by a preposition (`type of …`) is not dictation unless the user said it with a wake/explicit cue.
- Migrating existing arms: **no big-bang rewrite.** New actions are grammar-only; existing arms are re-expressed in the lexicon one at a time, each with the existing 197 intent-parser tests + e2e fixture as the regression net.
- The Python generator and the Rust matcher read the **same** `lexicon.json` → test corpus, NLU training paraphrases and Whisper prompt vocabulary can never drift from the grammar (the failure that already happened with `nlu_server.py` hard-coded intent lists).

### U3 Phonetic repair on the closed vocabulary (`phonetic.rs`)
- On a U2 miss: for each token / bigram / trigram of the (normalised) text, find lexicon entries and context names (installed apps, contacts, UIA names) within a **normalised phoneme edit distance** (threshold set empirically, Q5); candidate repairs must (i) re-parse to a valid action, (ii) change ≤ 2 spans, (iii) win by a margin, (iv) satisfy context (e.g. `wave`→`brave` only if `brave` is an installed/running app).
- Phonemes via the English G2P already in the build for Kokoro (`misaki-rs`, **[reuse unverified]**) cached to disk; fallback Double Metaphone if reuse is awkward.
- Hand table `normalize_phonetic_mishearings` stays as a seed; **learned aliases** (user-confirmed, ≥ 3 occurrences) go to a per-user `aliases.json` and never overwrite the shipped lexicon.
- Targets the measured clusters: *command hub* (≈ 9 variants), *ghost mode* (≈ 8), *Brave* (`Wave browser/Brawler`), `tab to`→`tab 2`.

### U4 Semantic tier (exists — keep, retrain, gate)
`nlu_local.rs` already runs BERT-Mini in-process (tract). Changes: retrain/extend with grammar-generated paraphrases (sampled, not all 37 M) + **hard negatives** (ambient set, real-log noise) + the 19 new action labels; add an **out-of-scope gate** (margin + similarity threshold; calibrate at a fixed false-accept budget, not accuracy). In Ghost hot-mic it may only return `Confirm`, never silent `Execute`, until Q2 shows its false-accept rate.

### U5 Decide & clarify (`decide.rs`)
| Condition | Outcome |
|---|---|
| Full-span grammar match, validated slots, reversible action | **Execute** |
| Match after wrapper stripping / segmenting | **Execute** (same) |
| Match only after phonetic repair, or semantic tier | **Confirm** in hot-mic ("Brave — right?"), Execute with wake word |
| Destructive/irreversible (delete, send, close with unsaved state, purchase) | **Confirm** always (existing gates) |
| Dangling transitive verb ("Open."), clip < 0.4 s | **Clarify**: "Open what?" — remember the verb 5 s; the next utterance fills the slot |
| Nothing matches / OOS / ambient-looking in hot-mic | **Ignore** + `missed_intents.jsonl` entry; **never** guess |
- Truncation is also attacked at source: if the final token is a transitive verb, extend the endpoint once by ~300 ms (the Smart-Turn model in `turn_detect` can arbitrate).
- A foreground-aware check closes the loop with Track E: `save` is accepted only if the foreground app's Shortcut Map (or UIA tree) has a save; otherwise NEXUS says "I don't see a save here" instead of firing `Ctrl+S` blindly.

## 4. Execution track (E) — from doc 102, re-sequenced
E0 probe & timing · E1 Shortcut Map + seed packs + Action Router + foreground gate + verify + layout-safe keys · E2 cached window-rooted UIA snapshot + AcceleratorKey/AccessKey harvest + pattern tier · E3 fast mouse (`ghostMouseMs`, no per-click ceremony, instant home) · E4 deep learning (menu crawl opt-in, VS Code keybindings reader, user teaching, exec feedback) · E5 "ask NEXUS" queries.
Details, safety rails and decisions: doc 102 §§3–9 (unchanged).

## 5. Phases — ordered by return on risk

| # | Phase | Deliverable | Gate (must pass to continue) | Est. |
|---|---|---|---|---|
| **R** ✅ | Reliability prerequisites (from the earlier ghost audit) | turn closed on `GhostInvokeTimeout`; watchdog threshold 20 s + guarded relisten; duplicate-listen and duplicate-capture-start guards | tests green (done); **no double-wake in a live session — not yet verified live** | done 2026-10-08 |
| **P0** ◐ | Baseline & ground truth | harness + scripts ✅; UIA probe + timing spans ✅; Groq confidence logging ✅ (opt-in); baseline report ✅ (`11-p0-baseline-report`); **needs the user:** Chromium/Electron probes, held-out phrasings, 2 h ambient, dynamic-prompt A/B | written baseline report ✅; per-app coverage for **Brave/VS Code/WhatsApp still missing** | built; user steps pending |
| **E0.5** ✅ | **Fix the UIA lookup** (new, from P0) — done 2026-10-08: Notepad 5,097 ms/0 controls → 24–44 ms/23 of 23; Calculator/Explorer not re-measured | `screen::list_actionables()` rooted at the foreground window, one cached query, depth raised; legacy fallback kept behind a flag | probe re-run: ≥ 95 % recall on leaf focus (was 0 %), ≤ 100 ms (was 2.5–5.1 s); `screen`/`mouse` tests green | 0.5–1 d |
| **U1** | Normaliser | `cmdu/norm.rs` + table tests; flag `cmdu` | bucket 1–4 accuracy ≥ 90 % of bare-core; **no new ambient accepts**; existing 197+e2e tests green | 2–3 d |
| **U2** | Lexicon + grammar + slot validation | `lexicon.json`, `grammar.rs`; 19 new actions; fix permissive arms | held-out paraphrase coverage ≥ 90 % (see §6); generated-corpus ≥ 98 %; wrong-action ≤ 0.5 %; ambient false-accept ≤ 1 % of the 65 | 5–6 d |
| **E1** ✅ | Shortcut Map + Router (keyboard tier) — done 2026-10-09, `docs/changes/119-…`: 23 actions, 6 app families + global fallback, pre-send foreground gate, title-change verify, 4 legacy call sites rewired; corpus gate 100/100, mutation-checked, live-verified on Notepad (42 ms verified / 3 ms unverified) | doc 102 P1 | corpus pass; 0 wrong-window sends (met in tests); **real-browser + live-Ghost run still to do** | done |
| **U3** | Phonetic repair + learned aliases | `phonetic.rs` | homophone-noise retention ≥ 90 % of clean; **0 new ambient accepts**; real-log recoverable replay ≥ 80 % | 3–4 d |
| **U5** | Decide & clarify | `decide.rs`, truncation handling, confirm policy | "Open." family → Clarify, never Execute; hot-mic FAR within budget (Q2) | 3–4 d |
| **E2/E3** | Fast UIA, pattern tier, fast mouse | doc 102 P2/P3 | warm resolve ≤ 60 ms p95; mouse ≤ 250 ms p95 | 4–5 d |
| **U4** | Retrain semantic tier + OOS gate | new model + calibration | OOS recall at fixed FRR budget; no regression on gates above | 4–5 d |
| **E4/E5** | Deep learning + "ask NEXUS" | doc 102 P4/P5 | crawl never mutates app state | 4–6 d |
Dependencies: U1 → U2 → (E1 ∥ U3) → U5; E2/E3 parallel after E1. Estimates are single-developer working days and **unvalidated**.

### 5.1 What P0 measured and how it changes the plan (2026-10-08, report 11)
- **UIA lookup is broken for leaf focus**: `list_actionables()` returned **0 controls in 5.1 s** (Notepad, Calculator) and 64/68 in 2.5 s (Explorer); one cached window-rooted query returned everything in 18–79 ms. → new phase **E0.5**, to be done *before* E1 (it also removes most vision fallbacks).
- **AcceleratorKey is nearly empty (0–3 %)**; AccessKey up to 70 % with `"Alt, F"` sequence syntax; shortcuts in control names 0–22 %; **Invoke 52–91 %**. → doc 102's shortcut-learning weight moves to seed packs, name parsing, AccessKey sequences, published files and teaching; the **pattern tier** becomes the main way to avoid the mouse.
- **Still unknown (needs the user)**: Chromium/Electron (Brave, VS Code, WhatsApp, Discord) coverage and cold/warm tree cost; real STT confidence distribution; held-out phrasing accuracy; ambient false-executions/hour.
- Research doc 10's parser measurements stand unchanged (bare command 43 %, polite frame 3.6 %, wrong-action 8.2 %, 7.7 % ambient accepted).

## 6. Test strategy — and the circularity trap
**Trap:** a test corpus generated from the same lexicon the grammar uses will pass by construction (and says nothing about real speech). Therefore:
1. **Held-out paraphrases** written/collected *independently of the lexicon author*: (a) the user records their own phrasing of each action in ~3 ways; (b) a separately-prompted set of "30 natural ways to say X" per action, deduplicated and hand-labelled; (c) family/colleagues' phrasing if available. The lexicon author never sees (b)/(c) before the number is reported. **This held-out coverage is the headline accuracy.**
2. **Generated corpus** (fixed seed, committed fixture, ~3 k rows) → regression only, not accuracy claims.
3. **Ambient negatives** (65 now → hundreds): everyday sentences containing command words; **zero accepted** is a hard CI gate. Plus a recorded 2 h ambient audio corpus through the *real* STT + gate chain → false executions per hour (Q2).
4. **Real-log recoverables** (`real_log_recoverable.tsv`): the ≈ 25–30 recoverable lines from `missed_intents.jsonl` with their intended action; grows every week from live use.
5. **Channel-noise suite** (the 7 noise types, now in Rust) for U1/U3.
6. **Audio-level checks:** multi-voice TTS (incl. Indian-English voices) → real STT → pipeline, and the user's own recordings (Q1). TTS is a proxy, not real speech; reported separately.
7. **Execution tests** (doc 102 §8): wrong-window, password field, elevated app, remapped shortcut, layout change.
**Metrics reported together, always:** intent accuracy · slot accuracy · wrong-action rate · clean-miss rate · clarification rate · false executions / ambient hour · p50/p95 latency per stage. One number alone is never a claim.

## 7. Targets (release gates for the `cmdu` + `shortcutBrain` flags becoming default-on)
| Metric | Today (measured) | Target |
|---|---|---|
| Held-out paraphrase coverage (in-scope actions) | not measured | ≥ 90 % (stretch 95 %) |
| Generated-space coverage, any wrapper count | 0–43 % | ≥ 98 % |
| Wrong-action rate on in-scope input | 8.2 % | ≤ 0.5 % |
| Ambient sentences accepted | 7.7 % (5/65) | 0 on the committed set; ≤ 1 / hour on recorded audio |
| Noise retention (homophone / truncation / split / stutter) | 53–76 % of a low baseline | ≥ 90 % of clean accuracy, or Clarify (never a wrong Execute) |
| Recoverable real-log commands handled | 14 of ≈ 28 | ≥ 90 % |
| Understanding latency p95 (release) | ~0.4 ms (debug) | ≤ 5 ms deterministic path; ≤ 25 ms incl. semantic tier |
| Keyboard path decision→keys | n/a | ≤ 40 ms · pattern ≤ 80 ms · mouse ≤ 250 ms (doc 102) |
"Perfect" is reported as the pair *accuracy on understood speech* and *false-execution rate*; the speech recogniser's own error is measured separately and not claimed away (research doc 10 §6).

## 8. Flags, settings, logging
- `NexusSettings` (must be added there or `save_settings` drops them): `cmdu` (default off until gate), `cmduHotMicStrict`, `sttDynamicPrompt`, `shortcutBrain`, `ghostMouseMs`.
- Every decision logs one line: `[CMDU] raw='…' norm='…' seg=… stage=U2 action=new_tab conf=0.95 outcome=Execute t=0.3ms` (picked up by `scripts/run.ps1`, colour = new prefix). Misses/clarifications/ignores append to `missed_intents.jsonl` (existing) with the stage that gave up.
- Learning never stores audio or free-typed text; aliases and counters only.

## 9. Risks
| Risk | Mitigation |
|---|---|
| Broader grammar ⇒ more false accepts in hot-mic | slot validation, noun-phrase check, ambient CI gate at zero, `Confirm` for repaired/semantic matches, `Ignore` default |
| Phonetic repair repairs the wrong word | closed vocabulary only, margin + parse-validity + context checks; measured before enabled |
| Lexicon rot / drift between Rust, Python, NLU labels | single `lexicon.json`; CI check that generator, matcher and NLU `labels.json` agree |
| Dynamic prompt makes Whisper hallucinate prompt words | A/B in P0; flag; per-turn fallback to static prompt |
| Over-clarifying annoys | clarify only on dangling verbs/short clips; cap one clarification per turn; measure rate |
| Test circularity gives false confidence | held-out discipline (§6); report both |
| Concurrent sessions editing `intent_parser.rs` / `orchestrator.rs` / `wakeword_oww.rs` | new code lives in new modules; patches to shared files stay narrow and anchored |

## 10. Decisions needed from you
1. **Hot-mic strictness (Ghost):** for *non-trivial actions* (anything beyond stop/exit), require (a) the wake word, (b) a directed cue, or (c) neither if the match is exact? I recommend **(c) for exact matches of safe actions, (b)/(a) otherwise**, with `Confirm` for repaired matches.
2. **Clarification:** OK that a bare "Open." makes NEXUS ask "Open what?" and wait 5 s, instead of staying silent?
3. **Learned aliases:** auto-accept after 3 identical corrections, or ask you each time?
4. **Held-out data:** will you record your own phrasing of each action (≈ 36 × 3 short clips, ~10 min), and allow a 2-hour local ambient recording (stored only on this PC) so false-executions/hour can be measured?
5. **Apps/shortcuts first** (doc 102 §9.4) and **cursor speed / announcements / menu crawl / invisible Invoke** (doc 102 §9.1–3, 5) — still open.
6. **Scope:** English only (previous directive) — keep Hinglish contact/app names (`mummy`, `papa`) as lexicon slot values?
