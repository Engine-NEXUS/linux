# 10 — Ghost Mode Command Understanding: The Phrasing Space, Measured Accuracy, and What Limits "Perfect"

**Date:** 2026-10-08
**Status:** Research + **real measurements on the current parser**. No product code changed (one `#[ignore]` test harness added).
**Companions:** `09-keyboard-first-shortcut-knowledge-...md` (what to do with a command) · `docs/features/102-...` (execution plan) · `docs/features/103-...` (combined implementation plan).
**Question asked:** *how accurately can NEXUS understand a command perfectly; list all the ways a command can be said, how many ways there are, and research it.*

> Evidence legend: **[measured]** run today against the real `parse_deterministic` · **[code]** read in this repo ·
> **[lit]** published source (linked) · **[assumed]** my modelling choice — flagged because it can bias the numbers.

---

## 0. Executive summary (read this first)

1. **The number of ways is not countable in the useful sense, but it is *structured*.** Across 36 Ghost Mode actions, a modest 402-entry
   vocabulary plus 42 "wrapper" entries (wake words, fillers, polite frames, qualifiers) already spans **36,982,400 distinct surface forms**
   (4,064 for the bare core alone) **[measured, my grammar]**. Real speech is a power law over that space, so "all possible phrasings" is the wrong target;
   **compositional coverage of the dimensions** is the right one (§2).
2. **Today's deterministic parser is a hand-written list of literals, and it shows [measured]:**
   | Input condition (existing-intent actions) | Correct | Clean miss | **Wrong action** |
   |---|---:|---:|---:|
   | Bare core phrase (no wrapper words), n=594 | **43.1 %** | 45.1 % | **11.8 %** |
   | + 1 wrapper word, n=861 | 25.3 % | 67.0 % | 7.7 % |
   | + 2 wrappers, n=640 | 12.0 % | 83.0 % | 5.0 % |
   | + 3 wrappers, n=1,164 | 2.9 % | 94.6 % | 2.5 % |
   | + 4 wrappers, n=2,181 | 0.0 % | 99.8 % | 0.2 % |
   | "Realistic" mix (§3.2), n=2,040 | 25.8 % | 65.9 % | 8.2 % |
3. **Polite framing alone destroys recognition:** a single frame word ("can you…", "I need you to…", "go ahead and…") → **3.6 % correct**;
   a wake word alone → 45 %, a filler alone → 40 %. The parser has no *general* wrapper-stripping; each arm handles a few of its own.
4. **19 of the 36 actions have no intent at all** (refresh, back/forward, save/copy/paste/cut/undo/redo/select-all, scroll, zoom, switch-window, reopen/next/prev tab, find-in-page).
   They return a clean miss 96.5 % of the time and a **wrong action 3.5 %** of the time — e.g. "go to the right tab" → *open an app named "right tab"*.
5. **The dangerous failure is a wrong parse, not a miss.** Permissive arms (`open <anything>`, `close <anything>`, `search <anything>`, `type <anything>`) swallow ambient speech:
   5 of 65 adversarial everyday sentences (**7.7 %**) were accepted, e.g. *"close the window it's cold"* → `CloseApp("the window it's cold")`, *"open your mouth and say ah"* → `OpenApp(...)`.
   In an open-mic Ghost session that is an action on the user's PC.
6. **ASR damage is mostly *systematic*, not random** — and therefore fixable: the real log's recoverable misses are dominated by a few domain words ("command hub" ×≈9 variants,
   "ghost mode" ×≈8, "Brave" → "Wave"), by truncation ("Open." ×4), and by Whisper prepending a garbage sentence before the real command ("Ssort X2. Ghost mode.").
7. **"Perfect" has a hard ceiling set by the recogniser, not the parser** (§6). What is controllable: parser coverage → ≥ 98 % of in-grammar input, wrong-action rate ≤ 0.5 %,
   noise repair, clarification instead of guessing. What is not: words the ASR drops or hallucinates entirely. We report both, never one number.

---

## 1. How a spoken command reaches an action (Ghost Mode) [code]

```
mic → VAD endpoint (ghost: 240/480 ms hang) → Groq Whisper-large-v3-turbo (or Moonshine local) → hallucination filter
    → directed-speech gate (hot-mic only: cues/echo/filler/repetition) → normalize_phonetic_mishearings (hand table)
    → strip wake / filler (leading only) → parse_deterministic_inner  (≈60 hand-written arms, first match wins)
    → miss → BERT-Mini NLU (in-process via tract, `nlu_local.rs`; accepted only at confidence ≥ 0.85) → miss → [Ghost: "Didn't catch that"] / [normal: cloud chat]
```
Properties that matter for accuracy:
- **First-match-wins ordering** with permissive open-slot arms late in the chain (`open_app`, `close_app`, `search`) is the source of most wrong parses (§4).
- **Wrapper handling is per-arm.** Only `strip_wake_prefix` and `strip_leading_filler` are generic, and both strip *leading* material only.
- **`normalize_phonetic_mishearings`** is a hand-maintained table of whole-phrase aliases (≈ "goes to mode"→"ghost mode"). It grows one live failure at a time.
- **`directed.rs`** already decides, for hot-mic turns, whether speech was addressed to NEXUS — the right place for stricter rules (§5.1).

---

## 2. "How many ways can a command be said?" — the model and the counts

### 2.1 Dimensions of variation (what the generator varies) [assumed structure, measured counts]
```
utterance = [wake] [filler] [frame] VERB [determiner] OBJECT [qualifier]        (+ ASR channel noise)
```
| Layer | Options used | Examples |
|---|---:|---|
| Wake / address | 5 | ∅, "nexus", "hey nexus", "okay nexus", "nexus," |
| Leading filler | 13 | ∅, so, and, ok, okay, um, uh, now, then, also, just, alright, well |
| Polite / intent frame | 14 | ∅, can you, could you, would you, will you, can you please, please, I want/need/I'd like you to, go ahead and, let's, you should, try to |
| Verb synonyms | 3–10 per action | open / launch / start / run / fire up / bring up / pull up / show me / go to / load … |
| Determiner | 1–5 | ∅, a, the, my, this, current, another, one more |
| Object synonyms | 1–6 per action | "new tab", tab, blank tab, fresh tab, empty tab, new browser tab … |
| Slot value | 1–12 | app names, queries, text, key names, element names, tab numbers (digit **or** word **or** ordinal) |
| Qualifier / suffix | 10 | ∅, please, for me, right now, quickly, now, sir, again, in the browser, here |

### 2.2 The counts [measured — `scripts/phrasing_probe/gen_phrasings.py`]
Across **36 actions** (10 existing, 25 keyboard-oriented incl. the ones the Shortcut Brain adds, 1 mouse):
- Lexicon needed to *write down* the grammar: **402 verb/determiner/object entries + 42 wrapper entries = 444 entries.**
- Distinct surface forms it generates: **4,064 core forms** and **36,982,400 with the four wrapper layers** — an **≈ 83,000 × compression**
  (10 × at the core alone). Counts add; coverage multiplies. **This is the central argument for a compositional grammar over literal lists.**
- Every extra synonym multiplies a layer; every action adds only additively to the vocabulary.

| action | tier | lexicon entries | core forms | full forms (× layers) |
|---|---|---:|---:|---:|
| `open_app` | existing | 16 | 1,080 | 9,828,000 |
| `close_app` | existing | 11 | 336 | 3,057,600 |
| `new_tab` | keyboard | 20 | 270 | 2,457,000 |
| `close_tab` | keyboard | 16 | 140 | 1,274,000 |
| `reopen_tab` | keyboard | 13 | 75 | 682,500 |
| `next_tab` | keyboard | 12 | 48 | 436,800 |
| `prev_tab` | keyboard | 13 | 56 | 509,600 |
| `goto_tab_n` | keyboard | 13 | 672 | 6,115,200 |
| `refresh` | keyboard | 13 | 75 | 682,500 |
| `go_back` / `go_forward` | keyboard | 9 / 8 | 7 / 6 | 63,700 / 54,600 |
| `search_web` | existing | 11 | 54 | 491,400 |
| `address_bar` | keyboard | 13 | 60 | 546,000 |
| `find_in_page` | keyboard | 9 | 96 | 873,600 |
| `type_text` | existing | 9 | 35 | 318,500 |
| `press_key` | existing | 10 | 168 | 1,528,800 |
| `save` / `copy` / `paste` / `cut` | keyboard | 13/11/10/8 | 32/21/18/10 | 291,200 / 191,100 / 163,800 / 91,000 |
| `undo` / `redo` / `select_all` | keyboard | 11/8/11 | 24/12/24 | 218,400 / 109,200 / 218,400 |
| `scroll_down` / `scroll_up` | keyboard | 10 / 9 | 8 / 7 | 72,800 / 63,700 |
| `zoom_in` / `zoom_out` | keyboard | 8 / 7 | 6 / 5 | 54,600 / 45,500 |
| `minimize` / `maximize` | keyboard | 11 / 11 | 45 / 45 | 409,500 each |
| `switch_window` | keyboard | 9 | 7 | 63,700 |
| `click_element` | mouse | 13 | 576 | 5,241,600 |
| `ghost_enter` / `ghost_exit` / `stop_cancel` | existing | 12 each | 10 each | 91,000 each |
| `confirm_send` / `media` | existing | 9 / 11 | 7 / 9 | 63,700 / 81,900 |

**Honest limits of the count.** (a) These are *my* verbs/objects — a real user will produce words outside them (the space is open-ended); (b) not every combination is natural
("fire it off" is rare, "open" is common); (c) the ASR channel multiplies every form by its possible mis-transcriptions (§3.3). The count says *how big the problem is and why
enumeration fails*, not "the user will say N things". Real phrase frequency is a power law; the plan therefore measures **coverage of the head and robustness in the tail**.

---

## 3. Measured accuracy of the current deterministic parser

**Method [measured].** `tests/phrasing_probe.rs` (new, `#[ignore]`) feeds JSONL phrases to `parse_deterministic` and logs label, parsed intent, µs.
`scripts/phrasing_probe/gen_phrasings.py` builds 26,676 phrases (36 actions). Expected labels were **calibrated from what the parser calls each action's canonical phrase today**
(e.g. close-tab → `browser_close_tab`), so a "miss" is a true miss, not a naming mismatch. Verdicts: **ok** (right label) · **miss** (clean `none`, recoverable by asking) · **wrong** (a different action ran).
Slot check on `open/close` (294 correct-label rows): **294/294** — when it picks the right action, the app slot is right.

### 3.1 By number of wrapper words (the key result) — see §0 table.
Per-layer, single layer only: wake **45.0 %** · filler **40.4 %** · qualifier **27.0 %** · **polite frame 3.6 %**. The bare-core ceiling is only **43.1 %**, so even before any wrapper,
**more than half of reasonable verb/object synonyms are not understood** (e.g. `hit enter`, `tap tab`, `write hello world`, `abort`, `halt`, `stop that`, `shrink this window`, `maximise`, `bing cats`).

Bare-core accuracy by action [measured, n = 26–51 each]: `close_app` 100 % · `open_app` 84 % · `search_web` 79 % · `media` 71 % · `goto_tab_n` 63 % · `ghost_enter` 58 % · `press_key` 50 % ·
`ghost_exit` 46 % · `click_element` 28 % · `address_bar` 26 % · `type_text` 25 % · `confirm_send` 24 % · `new_tab` 20 % · `maximize` 18 % · `stop_cancel` 16 % · `close_tab` 11 % · `minimize` 4 %.

### 3.2 The "realistic" mix and its caveat
Realistic = each wrapper layer is empty with probability 0.65 / 0.75 / 0.62 / 0.78 (wake / filler / frame / qualifier) → ~24 % of utterances are bare. **[assumed — not measured from real usage.]**
It yields 25.8 % correct / 65.9 % miss / **8.2 % wrong**. If real users are blunter than assumed, accuracy rises toward the 43 % bare-core figure; the *ranking of failure causes does not change*.
The first improvement target is therefore independent of the mix: **strip wrappers generically, then widen the core.**

### 3.3 ASR-channel noise (applied to the realistic mix; baseline 25.8 %) [measured]
| Noise | Models | Correct | Retention vs 25.8 % | Wrong action |
|---|---|---:|---:|---:|
| capitalised + trailing punctuation | Whisper style | 30.4 % | ≈ 100 % | 8.6 % |
| last word truncated | endpoint cut ("Open.") | 19.6 % | 76 % | 8.3 % |
| first word clipped | onset loss | 14.7 % | 57 % | 6.1 % |
| one word → near-homophone | "tab"→"tap", "brave"→"wave", "to"→"two" | 16.2 % | 63 % | 7.5 % |
| mid-sentence filler | "uh", "um", "you know" | 16.1 % | 62 % | 8.6 % |
| stutter / repeated word | "open open chrome" | 14.7 % | 57 % | 8.2 % |
| sentence split | "Open. A new tab." | 13.7 % | 53 % | 7.2 % |
Everything except punctuation/case loses 24–47 % of the already-low baseline. The **wrong-action rate stays 6–9 % under every noise type**, i.e. noise does not make the parser *more
careful* — it keeps guessing.

### 3.4 Actions with no intent [measured]
19/36 actions parse as `none` 96.5 % of the time and as a *wrong* action 3.5 % of the time (worst: `find_in_page` 21.9 % → Search, `next_tab` 15.0 % → OpenApp("…tab"), `prev_tab` 11.2 %, `reopen_tab` 5.9 %).
These cover exactly the keyboard-first actions the Shortcut Brain (plan 102) wants to use — **the understanding layer is the first blocker, before any shortcut is learned.**

### 3.5 Real-log replay [measured] (126 logged `missed_intents.jsonl` lines, 2026-10 sessions)
- Today **14 / 126 (11 %) now parse** — all from recently added alias patches ("goes to mode", "shift to tab to", "type", "comment hub"…).
- **112 still clean-miss.** My reading (judgement, not measured): **≈ 25–30 are recoverable commands**; the rest are noise/hallucination/side-talk that should stay rejected.
  Recoverable clusters still failing: `Cancel the ghost mode` ×3 · `command hub` family ≈ 9 variants (*Command hop / Sermon Hub / Command Roker / Comment up / command out / come and hope…*) ·
  `Wave browser / Wave Brawler` (= Brave) · bare `Open.` ×4 (truncated) · `scroll down` · `Post more / First more` (= ghost mode) · screen-analysis phrasings with a leading garbage sentence
  ("In other words, what is on the screen?") · `Ssort X2. Ghost mode.` / `Open's got to look at me. Ghost mode.` (**garbage sentence + real command**).
- Pattern: the same ~6 domain words are corrupted in many *different* ways. A phonetic closed-vocabulary matcher fixes the whole family; a hand alias table fixes one variant per live failure.

### 3.6 False accepts on everyday speech [measured] (65 adversarial non-commands)
5/65 = **7.7 %** accepted: `type of person who never listens` → type_text · `open your mouth and say ah` → OpenApp · `click here to subscribe to the channel` → click_element ·
`search engines have changed a lot` → Search · `close the window it's cold` → CloseApp. (The hot-mic directed-speech gate sits *before* this stage and removes some, but it is cue-based and was
not part of this measurement.) **Target is a rate per hour of ambient audio, which needs a recorded corpus (§7).**

### 3.7 Speed [measured]
Parse stage p50 179 µs · p95 410 µs · p99 701 µs (debug build) — **the deterministic stage is not a latency problem; it can afford to be 10–50× more thorough.**

---

## 4. Why the current design fails the way it does [code + measured]
| Failure | Mechanism (code) | Evidence |
|---|---|---|
| Polite phrasing missed | no generic frame stripper; arms enumerate a few literal forms (e.g. `NEW_TAB_OPENERS` ≈ 20 exact strings) | frame-only 3.6 % |
| Synonyms missed | verb/object lists are literal; each new word = a code edit | bare core 43 % |
| Wrong action | permissive arms accept any tail; `open_app` ran for "next tab", "right tab", "previous page" | §3.4, §3.6 |
| Garbage-prefix sentences | only `new tab` has a `rfind(". ")` special case | `Ssort X2. Ghost mode.` |
| Truncated command | endpoint hang shortened for Ghost (240 ms) → "Open." | 4× in log |
| Homophones of domain words | hand alias table, one variant per failure | 3.5 clusters |
| No cross-action consistency | each arm invents its own label (`browser_close_tab`, `system_click_element`, `NluResult{…}`) | calibration table |

---

## 5. What the literature says we should do about it

### 5.1 Reject by default; accept by evidence (closed vocabulary + out-of-scope)
Whisper's own confidence signals (`avg_logprob`, `no_speech_prob`, `compression_ratio`) are weak hallucination detectors on their own — a logistic regression on them reached **F1 = 23.6 %**
([arXiv:2606.23060](https://arxiv.org/pdf/2606.23060)); 1-second clips hallucinated **52.1 %** of the time versus 11.6 % at 10 s on non-speech audio ([arXiv:2501.11378](https://arxiv.org/html/2501.11378v1)).
Short Ghost turns sit exactly in the bad regime. Matching against a **closed command vocabulary and rejecting everything else** is the stronger filter (this is the published OOS-detection stance
for assistants: threshold on similarity/probability, tune on in-domain data — [CLINC150](https://arxiv.org/pdf/1909.02027), [Conformal intent/clarification](https://arxiv.org/pdf/2403.18973)).
NEXUS already requests `verbose_json` for the wake-word verifier and deserialises `avg_logprob` and `no_speech_prob` per segment (`stt_groq.rs: GroqSegment`) **[code]**, so the signals exist on this path; they are **not yet used for command turns** and their usefulness on 1–3-word commands is **unmeasured** (the literature says they are weak on their own).

### 5.2 Fix ASR errors on a *known* vocabulary with phonetics, not hand tables
Entity/name mistranscription is the standard industrial ASR failure; the proven post-processing fix is phonetic similarity (Double Metaphone / G2P phonemes) over a bounded candidate list,
reported **+40.8 %** on a production voice-collaboration assistant ([Cisco, EMNLP-IJCNLP 2019](https://aclanthology.org/D19-3011/)); the follow-up literature adds n-gram spans and context
([voice-search rescoring, arXiv:2506.06117](https://arxiv.org/pdf/2506.06117)). Our candidate list is tiny (≈ 450 lexicon words + installed app names + contacts) — the favourable case.
NEXUS already ships an English G2P (`misaki-rs`, used by Kokoro) — a phonemizer is available without a new dependency **[code, to be verified for reuse]**.
I found **no source** benchmarking phoneme-edit-distance for closed *command* sets specifically; the design is my inference and must be measured.

### 5.3 Bias the recogniser toward the active vocabulary
Prompt/context biasing of Whisper helps rare terms; trie-based hotword decoding gave ≈ 19–22 % hotword error reduction on LibriSpeech ([arXiv:2508.17796](https://arxiv.org/pdf/2508.17796)).
Caveats from the sources: evaluated on names/terms not short commands; biasing can raise false triggers on look-alikes. NEXUS already passes a static vocabulary prompt (`NEXUS_VOCABULARY`);
the improvement is a *dynamic* prompt (foreground app, installed apps, visible control names). Gain on commands is **[unverified]** → measure.

### 5.4 Don't commit to one hypothesis blindly
Pooling several ASR hypotheses gave **14–25 % relative** intent/domain gains over 1-best in a production assistant ([arXiv:2001.05284](https://arxiv.org/pdf/2001.05284));
word-confusion-network input narrows the gap to oracle text ([arXiv:2401.02921](https://arxiv.org/pdf/2401.02921)); SLURP shows large speech-vs-text gaps (median WER 25 % with a commercial API).
**Groq's API returns one hypothesis**, so n-best is unavailable on the cloud path; the practical substitutes are (a) phonetic repair against the lexicon, (b) a second cheap local pass only when
the first is low-confidence, (c) clarification. All three are in the plan.

### 5.5 A few examples per intent go a long way — if there is an OOS gate
SetFit-style few-shot classifiers reach ≈ 86 % (CLINC150, 8 shots) and ≈ 78 % (BANKING77, 8 shots) in one 2026 evaluation ([arXiv:2602.18922](https://arxiv.org/pdf/2602.18922)); intents that are close in embedding space
(BANKING77) score lower — our 36 actions include close pairs (next/prev tab, copy/cut, zoom in/out), so **a classifier alone will not give ≥ 98 %**; the grammar must carry the head, the model the tail.
NEXUS already runs BERT-Mini in-process (`nlu_local.rs`, 64-token fixed input, labels read from `labels.json`) — the semantic tier is **built**; what is missing is training data that matches real phrasing and an out-of-scope gate. Static-embedding distillation ([Model2Vec](https://pypi.org/project/model2vec/)) claims up to 500× CPU speed-up (vendor figure, task/hardware unspecified) — worth benchmarking against the existing BERT-Mini ONNX.

### 5.6 Agent action-space evidence
OSWorld/OpenCUA/Agent S all include `hotkey` actions; OS-Symphony argues action-space design can outweigh architecture ([arXiv:2601.07779](https://arxiv.org/pdf/2601.07779)). No paper benchmarks keystroke-vs-click for desktop agents; NEXUS measures its own (plan 102).

---

## 6. What "perfect" can mean (and cannot)
End-to-end success for an intended command factorises:

`A  =  P(audio passes gate) × P(ASR keeps the command-critical words) × P(parser/recovery gets the right action+slots | words) × P(execution + verify succeeds)`

- Parser/recovery term: **controllable**; this document's measurements show it is currently the *weakest* term (43 % bare, 26 % realistic) while the ASR term is plausibly much higher on clear speech.
  Target ≥ 98 % on in-grammar text, ≤ 0.5 % wrong-action.
- ASR term: **bounded by the recogniser.** Whisper-class models drop/insert words on short clips ([arXiv:2504.05702](https://arxiv.org/pdf/2504.05702) reports turbo with many extraneous tokens);
  single-word WER is extremely noisy as a metric (one error = 100 %). Public benchmarks do not cover 1–3-word commands — **NEXUS must measure its own** with recorded commands.
- What can be done about words the ASR gets wrong: phonetic repair (§5.2), context biasing (§5.3), and **asking** ("Open what?") rather than guessing.
- So the honest product claim is a pair: **(a) accuracy on understood speech** and **(b) false-execution rate per hour of ambient speech** — both reported, both gated.

---

## 7. What must be measured next (unverified items that gate the plan)
| # | Question | Method |
|---|---|---|
| Q1 | Real end-to-end accuracy per condition (quiet/noisy/accent) | Record the user's own ~36 actions × 3 phrasings × 3 conditions; run through real STT; compare to text-only results here |
| Q2 | False-execution rate per ambient hour in a hot-mic Ghost session | Record 2 h of TV/meeting/room audio (local only, with permission) → STT → full gate chain → count accepted actions |
| Q3 | Distribution of `avg_logprob`/`no_speech_prob` on real command turns vs hallucinated ones (fields already parsed by `GroqSegment`; `compression_ratio` is not) | Log them for ~1 week of Ghost use, then pick thresholds |
| Q4 | Does dynamic prompt biasing help or cause prompt-word hallucination? | A/B on Q1 audio |
| Q5 | Can `misaki-rs` G2P be reused for phoneme edit distance, and what thresholds separate repair from false repair? | Offline experiment on the lexicon + the 126-line log + ambient set |
| Q6 | Real phrase frequency (what do users actually say?) | Mine `conversation.jsonl` / `missed_intents.jsonl` over time; replace the assumed wrapper weights |

## 8. Reproduce
```
python scripts/phrasing_probe/gen_phrasings.py <outdir>                         # writes probe_in.jsonl + meta.json
cd src-tauri && NEXUS_PROBE_IN=<outdir>/probe_in.jsonl NEXUS_PROBE_OUT=<outdir>/probe_out.jsonl \
   cargo test --test phrasing_probe -- --ignored --nocapture
python scripts/phrasing_probe/analyse.py <outdir>      # tables A–H (needs probe_in2/out2 for the real-log/ambient section)
python scripts/phrasing_probe/analyse2.py <outdir>     # wrapper-layer breakdown + failing examples
```
(`probe_in2.jsonl` = the 126 real transcripts + 65 ambient sentences, built by `scripts/phrasing_probe/build_reallog_and_ambient.py <outdir>`; run the test a second time with `NEXUS_PROBE_IN/OUT` pointing at `probe_in2/out2`.)
Seeded (`random.seed(7)`), deterministic. Results above are from the 2026-10-08 run on the working tree at HEAD `bddd1a4` + uncommitted changes.

## Sources
- Whisper hallucination detection — https://arxiv.org/pdf/2606.23060 · non-speech hallucinations — https://arxiv.org/html/2501.11378v1 · PennSound STT evaluation — https://arxiv.org/pdf/2504.05702
- Entity resolution for noisy ASR (Cisco) — https://aclanthology.org/D19-3011/ · phonetically-augmented rescoring — https://arxiv.org/pdf/2506.06117
- Whisper contextual biasing (trie, hotwords) — https://arxiv.org/pdf/2508.17796 · CB-Whisper — https://arxiv.org/abs/2309.09552v2
- ASR n-best for SLU — https://arxiv.org/pdf/2001.05284 · word confusion networks + LLM — https://arxiv.org/pdf/2401.02921
- CLINC150 OOS — https://arxiv.org/pdf/1909.02027 · conformal intent classification — https://arxiv.org/pdf/2403.18973 · OOS for assistants — https://arxiv.org/pdf/2301.06544
- SetFit few-shot comparison — https://arxiv.org/pdf/2602.18922 · Model2Vec — https://pypi.org/project/model2vec/
- Groq speech-to-text docs — https://console.groq.com/docs/speech-to-text · OpenAI TranscriptionSegment fields — https://developers.openai.com/api/reference/resources/audio/index.html
- OS-Symphony (action-space design) — https://arxiv.org/pdf/2601.07779 · OSWorld — https://arxiv.org/pdf/2404.07972
