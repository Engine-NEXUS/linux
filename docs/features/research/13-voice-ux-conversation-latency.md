# Voice UX, Conversation & Latency — Research Grounding

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** Turn-taking floor (+208 ms), the latency ladder, state signalling, error recovery, confirmation, and why not to build visemes.


**Date:** 2026-10-02
**Scope:** Turn-taking, endpointing, latency thresholds, state signalling,
listening-indicator problems, error recovery, confirmation/undo, lip-sync, and
wake-word UX. Peer-reviewed sources preferred; folklore explicitly flagged.

---

## 0. The numbers to build against

### Human turn-taking gap = **+208 ms**

**Stivers et al., "Universals and cultural variation in turn-taking in
conversation," PNAS 106(26):10587-10592, 2009-06-30.** 10 languages,
5 continents, hunter-gatherer through post-industrial.

| Metric | Value |
|---|---|
| Mean response offset | **+208 ms** |
| Cross-linguistic median | +100 ms |
| Overall mode | **0 ms** (most common transition is *no gap at all*) |
| Per-language mode | between 0 and +200 ms |
| Slowest / fastest | Danish +469 ms / Japanese +7 ms |
| Answer vs non-answer | answers significantly faster in all 10 |
| **Confirmation vs disconfirmation** | confirmations **100–500 ms faster** in all 10 |
| Visible vs vocal-only response | visible faster in **every** language (significant in 7/10) |

Load-bearing sentence: *"Speakers become hypersensitive to perturbations in
timing of responses, measured in 100 ms or less."*

**Practical:** any endpointing → STT → LLM → TTS chain lands above this. Moshi's
authors benchmarked 160 ms theoretical against Stivers' 230 ms pooled figure as
the design target.

### ⚠️ The uncomfortable companion number

**Heldner & Edlund, *J. Phonetics* 38(4), 2010** — **40–41.7% of human turn
transitions are overlaps**, and only **0.4–0.7%** are clean no-gap-no-overlap
handoffs.

⚠️ Single source, 2010, two of three corpora are task-oriented map dialogues. The
load-bearing corpus is the Spoken Dutch Corpus (321 speakers); the Swedish
sub-corpus is only 8 speakers. Stivers corroborates the *shape* (mode = 0,
systematic avoidance of long gaps) but not the percentage.

→ **You cannot set a silence threshold that both never cuts people off and never
feels slow. Pick which failure you prefer and instrument it.** For a
wake-word-gated, mostly-command pipeline, cut-off is the more damaging failure: it
forces re-utterance *and* the wake word has to re-fire. **Prefer the generous
end.**

---

## 1. Endpointing thresholds actually deployed

| Source | Threshold | Notes |
|---|---|---|
| **SRI Enhanced End-of-Turn Detection** (2021-12) | baseline fixed **~500 ms** | *"typically on the order of 500ms"* |
| **Raux & Eskenazi, ACL 2008** | baseline **700 ms** | Let's Go! system; ~4% cut-in rate |
| **Amazon far-field endpointing** (2018) | `T_min`/`T_max`, posterior δ = 0.7 | `T_max = ∞` → endpoint slips entirely; P99 = 2 s. Two-sided bounds mandatory. |
| **Chang et al., Interspeech 2022** | **100 ms**, 97% recall / 85% precision | E2E joint turn-taking+ASR; handles 4 disfluency classes |

### 🔴 The SRI result is the most actionable in this document

At **100 ms** response latency, pre-pausal acoustic features (**vowel energy
slope + mean, normalized per speaker**) fed to an SVM cut premature cut-off:

| Condition | Premature cut-off |
|---|---|
| 500 ms, fixed silence | ~100% |
| 500 ms + pre-pausal acoustics | ~36% |
| **100 ms + pre-pausal acoustics** | **20.3%** |

Two corpora, 4,409 non-final pauses (Template) + 8,061 (Freeform), natural
open-ended elicitation (reminders, calendar, SMS, voice search) under cognitive
load.

**This is our documented "Speech Onset Decapitation" bug, and it has a known
fix.** Our current approach is a fixed silence gate.

⚠️ Honest caveat: cross-corpus LOOCV was *significantly worse* than
within-corpus. **Train on spontaneous speech or it will not transfer.**

---

## 2. Turn-taking mechanics

**Levinson et al., PNAS 2015** — the central puzzle: gaps average ~200 ms but
speech production takes 600+ ms, so humans **predict** your turn end and
pre-encode their response before you finish.

- Turn-taking "beat" period: **80–180 ms**
- A gap or overlap **under 120 ms is not perceived** (Heldner 2011)
- **51–55% of transitions occur in under 200 ms**
- Intra-speaker pauses run **~140 ms longer** than inter-speaker gaps
- Voiceless stops average 60–80 ms — the floor of measurability
- *"An excessively long gap after a question may be taken to indicate that the
  recipient has some kind of problem with it."*

---

## 3. Barge-in

### (A) Production evidence — Alibaba, ACL 2022

*Duplex Conversation*, deployed to Alibaba customer service, **online A/B tested**.

Their rule-based barge-in (streaming ASR intermediate with confidence > threshold)
produced:

> **Only 11% of detected "barge-ins" were real. 89% were false.**

Taxonomy of the 89%: (1) no intent to interrupt — greeting or backchannel;
(2) ambient noise; (3) **echo** — the agent interrupts *itself*; (4) misplaced-turn.

Multimodal (audio + transcript + previous bot turn) model: **+16.2% recall,
+10.15% F1**, precision ~90%. Online: **50% latency reduction.**

Design decision worth copying: *"we consulted customer service professionals about
whether robots should be allowed to interrupt users. Since most human customer
services are not allowed to interrupt users, **we did not design the function for
robots to interrupt users when users are speaking.**"*

⚠️ Their **echo** category is our problem directly. Local TTS plus a 180 px
always-on-top orb on a laptop with speakers is a crosstalk fixture.

### (A) Barge-in often means the *opposite* of what we assume

**Selfridge et al., ACL 2013** — live Let's Go! deployment:
> *"One of the striking findings was that **dialogues with barge-in are slower and
> less successful than dialogues without barge-in.** This suggests that, for
> current systems, dialogues with barge-in are more indicative of **environmental
> difficulty** than user pro-activity."*

Introduces **NUBI** (non-understood barge-in): if the partial recognition *will
not* be understood, stop prompting and open a clarification subdialogue instead.

### (A) The three canonical configurations

**Kaspar, Eurospeech 1997** (Deutsche Telekom pilot) — the load-bearing negative
result:
> *"Triggering on speech detection proved to be infeasible. False alarms as well as
> sudden reaction (by interrupting the speech output) lead to confusion on the
> user side. **Triggering on speech recognition** is a natural way of introducing
> this behaviour."*

| Config | Trigger | Output | For |
|---|---|---|---|
| **Barge-In** | NearEnd, recognition-based | not interrupted | general |
| **Talk-over** | prompt-content-dependent, recognition | interrupted | expert users |
| **Cut-through** | explicit keywords | interrupted | informed users |

### (A) Strom, ICSLP 2000 — the "reduce and listen" pattern

Three phases: detect → verify → recover. On suspected barge-in, **duck the TTS
volume** rather than hard-stopping. Verification success ⇒ terminate; failure ⇒
restore. Hard-mute fully only after **1000 ms**.

Advantages: the volume reduction *is* the signal; a false barge-in doesn't kill
the output; reduces crosstalk. Prohibited-barge-in states use **keep the floor**:
brief loudness increase plus pre-emphasis filter `s'ᵢ = 3.0·sᵢ − 0.75·sᵢ₋₁` —
users understand it *"intuitively, without explanation or training."*

---

## 4. State signalling (listening vs thinking vs speaking)

### Google Conversation Design

Six principles: persona; move the conversation forward; be brief/be relevant;
leverage context; **End-Focus**; don't teach "commands." Two relevant:
- **"Ask questions"** — *"Your persona should give clear signals when it's the
  user's turn."*
- **"Don't monopolize"** — *"Your persona should not monopolize the floor."*

⚠️ Industry practice, not peer-reviewed.

### The sharpest finding

**Porcheron et al., CHI 2018** — month-long Echo deployments, 5 households,
6+ hrs verified:
> *"Responses from the VUI themselves are analysed by members for the 'account' of
> sorts they provide on the state of the VUI device. Our data shows the
> **inadequacy of the responses** as resources to furnish this analysis."*

Recommendation: Dourish & Button's **"observable-reportable abstractions"** — cues
that state *"not only what the system was doing, but why it was being done, and
what was likely to be done next."*

**Reeves, Rogers, Blumler, Beneteau et al., ACM Interactions 2019** — the best
single sentence for our orb:
> *"Responses like 'interesting question' or **'I didn't understand the question'
> offer little purchase** for [further action].… responses enable certain kinds of
> possible next moves in the sequence but also shut down others."*

They argue "conversation" is the wrong frame; use **"sequentially organized moves
around request and response."**

→ **For our 4-state orb: a state change with no affordance attached is a state
*label*, not a resource.** "THINKING…" is a label. "THINKING… you can interrupt
me" is a resource.

---

## 5. Latency thresholds — what users perceive

| Source | Finding |
|---|---|
| **Porcheron, CHI 2018** | **4.5–5 s silence = failure** — users assume the system didn't comprehend |
| **Funk et al., AUI 2020** | Unnaturally long positive delay → **users assume an error occurred**. **Negative delay** (responding before the user finishes) perceived as **rude** |
| **Peng et al., CHI 2020** | Tolerate to **4 s**; satisfaction drops at **8 s**. Peak 4 s for retrieval, 2 s for chitchat. N=94 |
| **Scovell et al., 2015** | High tolerance to latency **up to 4 s** *if accuracy is high*; ratings a weak function of latency, a **strong function of accuracy**. Collapses at ≤70% accuracy. N=47 |
| **IEEE Access 2023** | Optimal **750 ms**; tolerance threshold **1850 ms**; 550–1850 ms "relatively appropriate"; >4150 ms all "too seriously delayed". n=20 |
| **In-vehicle expert eval, 2024** | Satisfaction peaks **1.5 s**, dissatisfied **5 s**. ⚠️ **n=6.** Anecdote-with-statistics |
| **Commercial reality** (Koni et al. 2021) | Echo / Google Assistant / Siri / Bixby respond in **0.77–3.09 s** |
| **Alexa Skills Kit** | Timeout 10 s originally; shortened to **~8 s** July 2021 |

### ⚠️ Debunked folklore

There is **no single "acceptable latency" number**, and anyone quoting one
(including Google's page-one "300 ms") is not citing a study. Two independent
2026 teardowns ([talk-about.ai](https://talk-about.ai/2026-07-29-what-are-the-primary-sourced-thresholds-for-human),
[sabato.ai](https://www.sabato.ai/blog/measure-your-voice-agent-latency)) traced
the popular "300 ms" and "~1200 ms causes hangup" figures and found **both
terminate in vendor pages citing nothing.**

**Explicit evidence gap:** *no peer-reviewed source links measured voice-agent
latency to abandonment or conversion* on European/American calls. If we need a
defensible abandonment number, **we must run the experiment ourselves.**

### ⛔ ITU-T G.114 — flag this if anyone cites it

150 ms one-way mouth-to-ear = effectively transparent; **>400 ms "unacceptable
for general network planning purposes."** This measures **audio transport + codec
+ packetisation + de-jitter only.** No ASR, no endpointing wait, no inference, no
synthesis. **A floor under your budget, not a target.** The single most common
misapplication of this standard.

### What survives scrutiny, as a band

| Phase | Target | Basis |
|---|---|---|
| Wake word → VAD onset | ~0 | stream always running |
| **Endpointing silence wait** | **100 ms** | SRI (100 ms → 20.3% cut-off); Chang 2022 |
| ASR finalize | 165–270 ms | Moonshine table in our `AGENTS.md` |
| Intent parse (deterministic) | <20 ms | we already have this |
| Worker/LLM cold | 300–1500 ms | 9Router cascade |
| TTS first audio | 200–800 ms | ⚠️ no clean primary source |
| **Total p50 fast path** | **~700–900 ms** | |
| **Total p90** | **2.5–4 s** | at which point Porcheron's 4.5–5 s "failure" threshold is 1–2 s away |

### 🎯 The design rule that falls out

**We need a fluency floor, not just a fast path.** Humans hold the floor with
backchannels ("mm", "one sec", an audible breath) while thinking. **We currently
emit silence in `thinking`.** Every source says silence is the failure mode, not
speed. A cheap non-factual backchannel at ~700 ms costs nothing and buys the
entire 700–1850 ms "relatively appropriate" band.

Google Conversation Design, on record (I/O '17):
> *"First, we can't possibly know what a user's timing out means.… **Making people
> wait in silence is disrespectful of their time. It's abusive.**"*

---

## 6. The "is it listening?" problem

### Measured false-activation rates

**Cheng et al., PoPETs 2020** — "When Speakers Are All Ears": 2 rounds × **134
hours**, 12 TV shows, US + UK.

| Metric | Value |
|---|---|
| **Misactivation rate** | **0.95 per hour** |
| Normalized | **1.43 per 10,000 words** |
| Long-activation tail | on some devices **10% lasted ≥10 s** |
| **Reproducibility** | **majority NOT repeatable** — same audio, different outcome |

The non-reproducibility is the key design constraint: **you cannot precompute
your false-wake set.** A passing test suite tells you almost nothing.

**Schönherr et al., USENIX Security 2022** — "Unacceptable, where is my privacy?"
11 smart speakers, 8 manufacturers. Automated trigger crafting via weighted
Levenshtein distance over CMU Pronouncing Dictionary:
- **>1,000 sequences** identified that incorrectly trigger
- **>350 verified triggers released as a public dataset** ← use as our negative floor
- ~75% medium-to-highly reproducible
- Confirmed: "Alexa" ← "unacceptable", "election"; "Google" ← "OK, cool"; "Siri" ←
  "a city"; "Computer" ← "Peter"; "Echo" ← "tobacco"
- Also found **acoustic fingerprinting of TV-commercial audio** as a vendor
  mitigation — i.e. vendors keep a blacklist of specific recorded fingerprints

**Combs et al., JAHH 2022** — 8 weeks, 4 Echos: "Alexa" **6 FPs** vs "Amazon"
**65 FPs**; false positives fell **60%** (103 → 41) over the study.

⚠️ **Contested:** "Alexa" is both best and worst. Combs: fewest FPs. Schönherr:
>60% of English triggers relate to "ALEXA." Different corpora. **There is no
reliable cross-study law.** Measure on our own hardware.

**Siegart, Interspeech 2021** — counterintuitive and directly actionable: F0
range SD was **70.2 Hz (no-activation), 119.5 Hz (low), 100.1 Hz (high)**.
**Higher** intonation variety → **more** accidental activation. Speaking rate and
recording quality showed **no** substantial effect.

**Mahmood et al., 2024** — one-month in-home: **unintentional activation
triggered a user reaction 40% of the time.**

### ⚠️ Invalid extrapolation to flag

**Gómez Ortega et al., 2023** — 8,375 real donated Google Assistant records:
**only 1.05% were unintended**, ~10× below every TV-condition study.

Why: TV audio is continuous, dense, full of near-homophones, played at high SPL
across a room. Live domestic speech is none of those. **Do not extrapolate
TV-condition rates to a desktop orb.** Expect substantially better than 0.95/hr
for a near-field laptop mic.

### What users do (not what they say)

**Malkin et al., PoPETs 2019**, n=116: recordings that were just noise 2.93%;
**speaker was not addressing the device 6.33%**; total unintentional >10%.
28.3% experienced privacy concerns, with **accidental activations a leading
cause**. Only **5%** ever used the mic-off button; **<3%** reviewed recordings;
about half did not know recordings were stored forever.

**Kim et al., NDSS 2024**, n=100: 85.9% believed both intended and unintended are
kept; **76.9% never review their history**; **58% were surprised and concerned**
when shown unintended recordings. Their worst finding for us:
> *"For some of these I remember hearing random responses back from Alexa but for
> the others, **I didn't notice the activation.**"*

They prototyped a **smart-light notification**; users preferred light because
*"it works all the time, even if I'm not on a computer or phone."*

**The "creepy" factor** — actual user language:
- *"I don't use it a whole lot because I think it is **very creepy that it
  literally listens to everything you say**. This is why I keep it unplugged."*
- *"just the ambient listening about what we talk about scares me."*

### 🔴 Where proactivity's trust ceiling is

**Kazakova et al., SOUPS '22** (UC Berkeley, incl. Dan Weld/Dawson), proactive
assistants:
- *"Concerns center on **actions and consequences**. […] people are most worried
  about impactful activities: an assistant taking autonomous actions that carry
  financial, social, or personal consequences. […] **Looking up information for
  ambient suggestions was seen as safe.**"*
- **"The majority of our participants were not comfortable with always-on
  continuous listening,"** despite acknowledging convenience.
- **None** of their four permission modes were acceptable. Ask-every-time:
  everyone hated it. Learning/black-box: a sizeable minority refused to surrender
  control. Approve-then-review: preferred by the majority but they conclude it
  would produce poor privacy outcomes **because people never actually review**.
- Recommendation: confirm/notify over **multiple** modalities — *"in case they are
  too far away to see the display, or the environment is too loud."*

**Kostakos et al., IMWUT 2021**, n=13, 3-week field study — preference order:
> **utterance starter ("Hey, are you available?") > 4-second earcon chime >
> baseline (abrupt)**

But the utterance starter had the **lowest response rate**; **7/13 found baseline
awkward**; one participant preferred the earcon: *"the Earcon music was
comforting while the utterance starter was annoying."*

**Reicherts et al., UCL HCI** — proactive behaviour rated more positive **when
the user was alone**; *"If I am in the middle of an interaction with one or more
persons, I do not want Jay to interrupt."* **Proactive step-by-step task guidance
was the most favoured type.**

⚠️ **"Walking companion problem" is not an established term.** I searched for it
specifically and found no such canonical paper. Don't build on it.

⚠️ **Bernstein & Resnick UIST privacy paper could not be verified.** Do not cite
it. The adjacent verified work is Bernstein et al., *"Everyone's In: The End-to-End
Evaluation of a Privacy-Preserving Voice Assistant,"* CHI 2020.

---

## 7. Error recovery and "didn't catch that"

### (A) The canonical paper — 20 years old and still correct

**Bohus & Rudnicky, SIGdial 2005** — source of the phrase *"Sorry, I didn't catch
that."* 10 recovery strategies: AskRepeat, AskRephrase, Reprompt,
DetailedReprompt, Notify, Yield, MoveOn, YouCanSay, FullHelp, GoToAQuieterPlace.

| User response after non-understanding | Share |
|---|---|
| **Rephrase** | **~45%** |
| Repeat | ~20% |
| Contradict | ~0 |

Recovery rate by response: **Change 63%** (best); repeat and rephrase not
significantly different.

### 🔴 The counterintuitive finding: don't tell the user

**Skantze, Speech Communication 45(3), 2005** — wizard-of-oz:
> *"Unlike most spoken dialog systems, **human wizards often did not signal the
> non-understandings to the user when they occurred.** Instead, they asked
> different task-related questions to advance the dialog. **This strategy
> generally led to a speedier recovery.**"*

Bohus & Rudnicky tested this as the **MoveOn** strategy and it was the
best-performing strategy in their domain.

**Henderson, 2011** — smarter strategies (Subsume, Subsume Split, Fake):
**Enjoyment +1 point, highly significant**; more enjoyable, better flowing, less
frustrating; **fewer total questions asked.**

⚠️ Henderson's own caveat: *"Misunderstanding Detection converts previously
invisible non-understandings into visible misunderstandings. The average number of
errors per person **increases** when MD is switched on."* He also finds hard-coded
confidence thresholds unreliable.

### (A) Deployed-system error rates

| System | Task success | Misunderstandings | Non-understandings | Turns engaging a strategy |
|---|---|---|---|---|
| **RoomLine** | 75% | 17% | 13% | **41%** |
| **Let's Go! Public** | 52% | 28% | 27% | **53%** |

**41–53% of all turns engage an error strategy.** Our retry loop feeling normal is
the industry base rate — but if our engagement is near 41%, something upstream is
too aggressive.

### (A) Google's shipped guidance contradicts "didn't catch that"

**Dialogflow ES voice agent design best practices**, "Conversation repair":
- **Avoid** *"I didn't catch that,"* *"I don't understand,"* *"I'm having trouble."*
- **Avoid** *"try rephrasing"* and *"you can say x, y, or z"* — these remove agency
- **Do** use **"You mean X?"** — repeating **only** the information needing
  confirmation
- Prioritize Who/Where/What/When/How over Yes/No
- Make the conversation **balanced**; a dominant system removes agency

**Google's shipped reprompt ladder:**
```
attempt 0: "What was that?"
attempt 1: "Sorry I didn't catch that. Could you repeat yourself?"
final: "Okay let's try this again later."   [conversation ends]
```

**Dialogflow enforces a hard cap of 3** combined no-match + no-input events, then
terminates. That is a shipped product decision about how many retries are
tolerable. We have the right number (3) but should cite this rather than folklore.

### (B) The sharpest current critique of LLM repair behaviour

**"You're Not Listening": LLM 'Smart Speakers' Need to Learn When to Shut Up,
CUI 2026** — nine provocations. The most important:

> *"What is distinctive about LLM-based systems is their tendency to
> **elaborate where uncertainty would warrant brevity**, in part because **RLHF
> rewards 'helpful' expansive answers** and **verbosity compensates for low
> confidence**."*

→ **Architectural implication for us: if NLU confidence is low, that is exactly
when the LLM layer produces its longest, most confident-sounding output. Invert
that. Low confidence must *shorten* the turn.** This is a measurable property of
our intent layer.

Also: leave the repair slot (don't keep talking); one option at a time; *"Huh?"
works fine* — open-class repair is the most successful minimal strategy in
cross-linguistic repair literature; recognise moves to close; a single nudge is
enough — *"Persistent reminders are experienced not as support, but as a violation
of her agency."*

### (A) What actually kills satisfaction

**Mavrina et al., 2022** — 9 families, 5 weeks, log files + questionnaires:
- **Significant negative effect: the number of ABANDONED failed requests.**
- **Successfully repaired requests had NO significant effect on satisfaction.**
- Use of **repetition as a repair strategy decreased significantly** over time for
  both children and adults. No other strategy changed.
- **Situational cues dominate** — the assistant's immediate response matters more
  than the user's reasoning.

**Mahmood et al., 2024** — month-long in-home:

| Metric | Value |
|---|---|
| One-turn queries with errors | **24.76%** (632/2,552) |
| Errors resolved in immediate next attempt | **25.47%** |
| Intent recognition errors | 32.3% of errors, **lowest resolution rate 20.44%** |
| Human error (incl. saying "Alexis") | 3.6%, **highest resolution 38.9%** |
| Of 238 retries: resolved 1st | 44.96% |
| Of 238 retries: **user gave up** | **15.19%** |
| Of 238 retries: kept retrying | 36.97% |

**Beneteau et al., CHI 2019** — 10 families, 59 breakdowns analysed:
> *"While Alexa makes some attempts to repair communication breakdowns through
> specific clarification responses, **neutral clarification responses are far more
> common**, which do not aid in helping the human communication partner repair the
> breakdown."*

**Fix:** `"Do you mean 10:45 in the morning or the evening?"` ≫ `"I didn't
understand"`.

---

## 8. Confirmation and undo

### ⚠️ Three named sources could not be verified

| Source | Status |
|---|---|
| "Unless I Say Otherwise," HCI 2021 | ❌ **Not found** |
| Ranchordas et al., voice-assistant safety | ❌ **Not found** |
| **Confirmation fatigue threshold (voice-specific)** | ❌ **No quantitative threshold exists in the literature** |

The last is a genuine hole. The only instance found is qualitative (below).

### (A) The nearest real evidence — authentication as a proxy

**Renz, Neff, Baldauf, Maier** — n=696 survey + n=18 lab, 6 methods.

| Method | Security | Ease | Efficiency | Error susceptibility |
|---|---|---|---|---|
| Card reader | **3.4** (highest) | 1.9 (worst) | 3.2 | 3.8 |
| App + button confirm | 3.3 | 3.0 | **4.2** (highest) | **4.0** (best) |
| Voice confirmation in app | 3.1 | 3.2 | 3.8 | 3.4 |
| Spoken PIN | 2.8 | **3.8** (highest) | 3.6 | 2.9 |
| Biometric (speaker ID) | **2.5** (worst) | 3.1 | 3.7 | 2.9 |
| Sound authentication | — | appreciated | — | rated **worst for security** |

Three transferable findings:
1. **"Media disruption" was criticized** — *"participants resented being made to
   reach for a phone."* For an orb-based assistant, **a screen-only confirmation
   that requires clicking dismisses the assistant and may be worse than a spoken
   "yes."**
2. **An active manual task creates perceived security.** *"The absence of such an
   active task and thus a certain lack of control might have further impacted the
   respective assessments."*
3. **Novelty destroys perceived security.** *"It's strange because it's so
   unfamiliar."* → **If we ship an unusual confirmation mechanic, it will be rated
   insecure no matter how well it works.**

**Ponticello et al., SOUPS 2021**, n=16 — the one confirmation-fatigue instance:
> *"I wouldn't use any of the skills described here because the **effort-to-risk
> ratio is not profitable for me**."*

Users build trust by trial and error. Their recommendation: **ship a
"demonstration mode"** — a sandbox where authentication can be tested unlimited
times without penalty. Eaveshedding by bystanders was the **prime** concern.

### Confirmation is cheap for the human

**Stivers 2009**: confirmations are **100–500 ms faster** than disconfirmations in
all 10 languages, and far more common (70–89% of answers).

→ **Confirmation is cheap for the *speaker* but expensive for the *designer*,**
because of confirmation fatigue on the system side. The human-cost ledger is fine.

### 🔴 Users skip consent screens reflexively

**"Do Users Really Know Alexa?" ASIACCS 2023**:
- **71%** thought Amazon, not the third-party skill, requested permissions
- Only **1 of 28 (3.6%)** correctly answered all Alexa-specific permission questions
- **24/28** got the Reminders-vs-Notifications distinction wrong
- *"I just keep clicking. The skill should be safe I think."*

→ **If the destructive action isn't named in the first ~10 words, it will be
approved reflexively.** Keep confirmation text minimal and specific — the opposite
of most "helpful" prompting instincts.

### Deceptive voice patterns to avoid

**Owens et al., SOUPS 2022**, n=93 — **22 of 93 had felt tricked, manipulated, or
deceived**:

| Pattern | Quote |
|---|---|
| Unchangeable notification sound | *"Constant annoying sound you can't change."* (P7) |
| Volume manipulation | *"the 'ad' part was louder than the part where she actually responded to me wanting to cancel"* (P51) |
| Verbosity as obstacle | *"In the amount of time it took to explain that it can tell me the weather itself, it could have just told me the weather."* (P8) |
| Modal switching to block the goal | *"I would be annoyed to have to get my phone or computer to cancel a subscription I was using with my smart speaker instead of canceling through my smart speaker."* (P91) |
| Blocking exit | *"until I actually acknowledged her 'suggestion'… she literally wouldn't close the app. I hate that."* (P14) |
| Manipulative loss framing | *"The AI seemed to try to maneuver and manipulate me not to cancel by stating benefits I'd be losing."* (P38) |

### Practical guidance for `requires_confirmation` / `is_destructive`

**Our architecture is ahead of the published evidence base.** No study
demonstrates an optimal confirmation policy for a desktop assistant with
GitHub-write and WhatsApp-send capabilities. What the evidence supports:

| Decision | Recommendation | Basis |
|---|---|---|
| Which actions gate | Irreversible + externally-visible + socially/financially costly | Berkeley SOUPS'22 |
| Gate style | Offer **both** spoken and clicked | Renz n=696 |
| Gate text | Name the action in the **first ~10 words**; keep short | Owens SOUPS'22 |
| Gate mechanic | Familiar, not novel | Renz: novelty → perceived insecurity |
| Retry count | **Hard cap 3**, then exit gracefully | Dialogflow ships exactly this |
| Rehearsal | Ship a **dry-run / demo mode** | Ponticello SOUPS'21 |
| Prefer undo over confirm | Undo is strictly better on the fatigue axis | *(inference, flagged)* |
| Never | Un-dismissible chimes, volume manipulation, exit-blocking, modal switching | Owens SOUPS'22 |

---

## 9. Lip-sync / visemes — do not build these

### Oculus OVR 15-viseme set

`sil, PP, FF, TH, DD, kk, CH, SS, nn, RR, aa, E, ih, oh, ou`

Meta's rationale: *"These 15 visemes have been selected to give the **maximum range
of lip movement**, and are **agnostic to language**."*

⚠️ **CRITICAL FOR 2026: the Oculus Lipsync plugin is END-OF-LIFE.** From Meta's
docs: *"The Oculus Lipsync Plugin is in **end-of-life stage and will not receive
further updates or support**… The canonical reference for why these 15 is now the
**MPEG-4 Facial Animation viseme standard**."*

### Rhubarb / Hanna-Barbera / Preston Blair

**Rhubarb Lip Sync**: 6 basic shapes {A}–{F}, *"invented at the Hanna-Barbera
studios… they have evolved into a **de-facto standard for 2D animation**, widely
used by Disney and Warner Bros."* Plus extended {G}, {H}, {X}.

⚠️ Rhubarb's `--datUsePrestonBlair` mapping is internally inconsistent with the
actual Preston Blair series (`H→L` is a coincidence, not semantic). The author
flags this. **Use Rhubarb's alphabetic names.**

The real Preston Blair 10-shape series: `A/I` · `E` · `O` · `U` ·
`C,D,G,K,N,R,S,Th,Y,Z` · `F/V` · `L` · `M,B,P` · `W/Q` · `rest`

### 🔴 The decisive study — audio sync doesn't matter

**Böck et al., TU Dortmund, 2023** — **N=44**, VR, same personalized photoreal
avatar, four 30-second performances.

1. **Audio synchronization had NO significant effect on any rating.** χ²(1) = 0.25,
   **p = .62** for nonverbal naturalness; χ²(1) = 1.19, p = .28 for match.
   No interaction with animation method (p = .78, p = .33).
2. Static faces rated **less natural and less plausible** than animated
   (b = 0.65, t(86) = 9.74, p = .006).
3. ⭐ **Synthesized expressions rated MORE natural and MORE plausible than tracked
   capture** (b = −0.5, **t(86) = −5, p < .001**).

> *"Read this carefully, because it cuts against the whole premise of viseme-driven
> avatars."* You do not need sample-accurate lip sync; synthesize rather than
> capture; **some motion beats a static face**; the dominant rating driver was the
**audio**, not the face.

⚠️ The authors note a small audiovisual skew was used between groups.

### Corroborating

- **Salehi et al., 2025**, N=70: *"**Silencing clips improved perceived realism by
  removing mismatches** between voice and animation."* → **If we ship visemes,
  ship them correctly or ship a generic pulse.**
- **Abdulrahman et al., 2024**: participants **preferred the cartoon look over
  human-like, and no-lip-sync over lip-sync**. **Lip-sync negatively affected UX
  on several dimensions.** Their shipped outcome: a lip-sync toggle.
- **IJMRS 2015**, N=113: increasing asynchrony increased uncanniness (monotonic),
  and **participants were more sensitive when AUDIO preceded VISUAL** → **render
  the mouth ahead of the audio, never behind.**
- **Gurung et al., Int. J. Social Robotics 2023**: non-speaking avatars sometimes
  rated *higher* than speaking ones; *"a machine-like voice causes an uncanny
  valley effect."*

### Does audio-only suffice?

**Aspöck et al., DAGA 2021**, N=39 German speakers, 3 speech × 2 embodiment:
- H1 confirmed: human > human-with-TTS-prosody > synthetic TTS (p < .001)
- **H2 NOT confirmed: no significant effect of embodiment (ECA video vs
  audio-only) on perceived naturalness.** *"the results indicate only a **minor
  role of the visual representation of the ECA**."*

→ **When the user isn't looking, audio-only feedback does not degrade perceived
naturalness.** The visual is load-bearing for **state legibility**, not
naturalness — a different job, done by *any* motion.

### Sync target (if ever needed)

A 2026 VR-NPC paper defines **Viseme-Audio Synchronization Error (VASE)**:
mean **32 ms (SD 8)** over 50 cycles — 28 ms standard, 36 ms complex — with a
claimed perception threshold of **<45 ms**. ⚠️ Single small-n study, threshold
asserted not derived. Engineering target, not a citation.

Its user finding is more valuable: **"thinking" silence of 2–3 s caused social
awkwardness and 40% of participants were unsure whether the system had heard
them.** Time-to-audio was 1.42 s simple / 2.90 s complex.

### The right shape: three independent axes

**OpenAI "ChatGPT Dots"** (Rive-based) decomposes into **independent** channels:

```
activity: idle | listening | processing | working | speaking
emotion:  neutral | happy | empathetic | concerned | surprised
speech:   mouthOpen 0–1, lookX, lookY, celebrate, reset
```

*"The critical architectural insight: **activity, emotion, and speech are three
independent inputs.** Changing the expression must not stop the mouth; a happy
expression must not auto-trigger speaking."*

**Our 4-state orb is exactly the `activity` axis.** That is the fix for the
three-states-share-one-segment bug: **orthogonality, not three more animations.**

⚠️ Announcement coverage is second-hand; verify against OpenAI's docs before citing.

---

## 10. Wake word UX

### How many false activations are tolerable?

**Nobody has published a tolerance number.** Measured rates exist; tolerances do
not. Our engineering target should come from Cheng et al.'s 0.95/hour **scaled
for a far more favourable acoustic environment** (near-field laptop mic, no TV,
no across-room speaker). A defensible target is **<0.1 false wakes/hour** — ~1 per
10 hours of ambient listening, so a user should essentially never experience one
in a normal session.

**Practical consequence: you cannot validate this with a test suite.** Cheng
showed the majority of misactivations are **not reproducible**. We need a
long-running ambient soak test plus the **Schönherr 350-trigger dataset** as a
floor. Real-audio corpus ≠ test suite.

### Push-to-talk vs wake word

⚠️ **Direct evidence is thin — flagged.** No peer-reviewed head-to-head
user-preference study exists. Ship both (the global hotkey already exists) and
treat this as an open question to test ourselves.

Supporting signals:
- Schönherr frames the tradeoff: *"In contrast to a push-to-alk model, where
  speech recognition is only active after a physical button is pressed, smart
  speakers **continuously record their surroundings**."*
- Renz shows token/button methods rate higher on **security**, lower on **ease**.
- **FrownOnError CHI 2020**: frowning to interrupt beat **both** wake-word and
  button-press on timeliness and intuitiveness (precision 97.4%, recall 97.6%).
  ⚠️ Needs a webcam — but our 180 px orb is already on screen, so a visual
  interrupt affordance is available at zero hardware cost.

### 🔴 The best available improvement: add a prosody gate

**"Aware: Intuitive Device Activation Using Prosody for Natural Voice
Interactions," UIST 2021.**

> *"The pitch is precise. Keyword spotting uses only **semantic** information — does
> the audio contain the keyword — so it cannot distinguish 'Alexa, what's the
> weather' from 'Do you know Alexa?' or 'Alexa is very helpful!' The user's own
> study of calling vs. not-calling voices found **distinctive prosodic patterns**
> (pitch variation, harmonics-to-noise ratio, intensity, duration)."*

| Result | Value |
|---|---|
| Calling-voice classifier F1 | **0.869** |
| **Head-to-head, N=14: Aware F1 = 0.93 vs Amazon Echo F1 = 0.56** | |
| Generalization | F1 = 0.985 on 72 samples of four unseen words |

It directly attacks the two failure modes Schönherr identified (phonetic
similarity, and the keyword spoken *about* the device), and sidesteps the
utterance-length problem (*"users tire of repeating 'Alexa stop!'"*).

⚠️ Lab prototype, N=14, English only. But it is open-designable over features our
pipeline already computes.

### Touch/hotkey is harder than voice

**arXiv 2110.04656** — streaming on-device detection of device-directed speech:

| Invocation | Operating point | FAR |
|---|---|---|
| Voice-triggered | FRR = 1% | **4.2%** streaming |
| **Touch-based** | FRR = 3% | **25.1%** |

**~5× more false accepts for button/hotkey**, because there's no keyword to focus
on. → **The global hotkey is a ~25% false-accept design in literature terms.**
Treat hotkey-initiated turns as lower-confidence; don't let them skip the
confirmation gate the way wake-word turns do.

### Speculative execution

**(A) Production evidence — Alibaba.** Full-duplex + multimodal barge-in → **50%
latency reduction**, substantially from starting work during overlap.

**(A) Porcheron CHI 2018**: users **already hold the floor** during pauses —
intra-speaker pauses are ~140 ms *longer* than inter-speaker gaps, and **40–41.7%
of transitions are overlaps.** Systems reacting only after a clean endpoint are
structurally slower than the human baseline.

**(A) Funk et al., AUI 2020** name the hazard: *"a **negative delay** (responding
before the user finishes) is perceived as **rude**."* Safe **if** you don't
visibly jump the gun.

**(A) A patentable mechanism that is our exact false-wake problem.** Google US
Patent 11,557,293, "Contextual suppression of assistant command(s)": on a call
where the user says "answer", the system suppresses the action because the
**preamble** *"I don't want to answer that"* is present. It runs ASR on a buffer
**before** the wake word plus the postamble after, plus an NLU model, and
optionally speaker ID.

→ **Wake word + contextual preamble/postamble + optional speaker verification →
suppress the action.** Directly implementable.

### Wake-word recommendations

| # | Recommendation | Basis |
|---|---|---|
| 1 | **Add a prosody gate after the keyword** (calling vs mentioning) | Aware: F1 0.93 vs 0.56 |
| 2 | **Add a 300–500 ms pre-roll buffer** and run ASR+NLU on it to suppress the wake if context contradicts | Google patent 11,557,293 |
| 3 | Prefer longer, more distinctive wake words — but ⚠️ contested evidence on which | Schönherr vs Combs |
| 4 | **Ship the Schönherr 350-trigger dataset as a regression floor**, plus a long ambient soak | Cheng: non-reproducibility |
| 5 | **Do not shorten the 3000 ms post-TTS re-arm based on research** — no published number exists. If made adaptive, drive it from TTS audio energy, not a constant | ⚠️ No source found |
| 6 | Treat hotkey turns as lower-confidence than wake-word turns | arXiv 2110.04656 |
| 7 | Ship both hotkey and wake word | No head-to-head study exists |

---

## 11. Not yet available

### Moshi / Kyutai — shipping, and it removes our cascade

[arXiv 2410.00037](https://arxiv.org/abs/2410.00037) · weights **CC-BY** (verify)

| Property | Value |
|---|---|
| Latency | **160 ms theoretical, 200 ms practical on an L4** |
| Design target | explicitly *"lower than the 230 ms average in natural conversations"* (Stivers) |
| Mimi codec | 24 kHz → 12.5 Hz, **1.1 kbps**, 80 ms frame, fully streaming |
| Architecture | Multi-stream: user + Moshi audio + "Inner Monologue" text. 7B Temporal Transformer + small Depth Transformer |
| Full duplex | No explicit speaker turns. Overlap and interruption are **learned, not detected** |
| Silences | Decodes to *"natural silence"* — a near-silent waveform, not a fixed value |
| Deployments | PyTorch (research), **Rust (production)**, MLX (on-device) |

→ **Eliminates the VAD→ASR→LLM→TTS cascade, so there is no endpointing wait** —
the 100 ms SRI budget vanishes. Only model inference remains.

**Limits:** English only at that release; **5-minute conversation cap**;
knowledge **2018–2023**; **no tool calling**; Mimi suppresses music.

### Alibaba Duplex Conversation — in production

The only full-duplex system in this set with published **production A/B numbers**.
Modular (not end-to-end), audio+text multimodal barge-in, and deliberately
**does not let the agent interrupt the user**.

### Announced / academic only

| Item | Status |
|---|---|
| **FireRedChat** (arXiv 2509.06502, Xiaohongshu) | Modular pluggable full-duplex: **streaming personalized VAD** to suppress false barge-in from non-primary speakers; **semantic end-of-turn detector**. Proposes three eval metrics — *"adopt this as your eval harness"* |
| **dGSLM** (2023) | Prior full-duplex; proof of concept |
| **CONCORD** (arXiv 2604.13348, 2026) | Proactive always-listening capturing only the owner's speech via real-time speaker verification. Gap detection 91.4% TPR @ 6.8% FPR; relationship classification 96%; disclosure decision 97% TNR; context resolution 78% TPR but only 58% semantic similarity. ⚠️ Synthetic data, few-shot GPT-4.1, single-speaker — **design sketch, not evidence** |
| **ChatGPT Dots** | Announced expressive characters, Rive-based, three-axis decomposition. ⚠️ Second-hand coverage |
| **Gemini Spark** 24/7 agent | Beta for US AI Ultra since 2026-05-19; no GA confirmation |
| **Gemini Intelligence** (Pause Point, Rambler, Create My Widget, Noto 3D) | *"later this year"* — **absent as of 2026-06-26** |
| **XREAL AURA** glasses | 70° FOV, Sony Micro-OLED 1920×1200/eye, ≤120 Hz, electrochromic dimming ×5, 4455 mAh — *"Fall 2026"* |
| **Android XR SDK** | Developer Preview 4 (2026-05-19); XR spatial typography floor **14 dp** |

---

## 12. Contested or thin — do not build on these

| Claim | Status |
|---|---|
| "Acceptable voice latency is 300 ms" | ❌ **Unsourced** |
| "~1200 ms causes abandonment" | ❌ **Unsourced** |
| ITU-T G.114 400 ms | ⚠️ Real standard, commonly misapplied. Transport only |
| "Alexa is a safe wake word" | ⚠️ **Contested** (Combs 6 FPs vs Schönherr >60%) |
| In-vehicle 1.5 s optimal / 5 s ceiling | ⚠️ **n = 6** |
| **Confirmation fatigue threshold (voice)** | ❌ **No study found** |
| Ranchordas voice-safety paper | ❌ **Not found** |
| "Unless I Say Otherwise," HCI 2021 | ❌ **Not found** |
| Bernstein & Resnick UIST privacy paper | ❌ **Not found** |
| "Walking companion problem" | ❌ **Not an established term** |
| Push-to-talk vs wake-word preference | ❌ **No head-to-head study** |
| Post-TTS wake re-arm grace period | ❌ **No published number anywhere** |
| TV-condition false-wake → desktop orb | ❌ **Invalid extrapolation** (~10× gap) |
| Oculus Lipsync plugin | ⚠️ **End-of-life** |

---

## 13. Priority for NEXUS

1. **Replace the fixed silence gate with pre-pausal-acoustic endpointing at
   100 ms.** Our truncated transcripts have a documented fix (SRI: 100% → 20.3%).
   Do this before touching a single visual.
2. **Barge-in on recognition, not energy** (Kaspar 1997), with self-echo
   handling (Alibaba taxonomy). Our local TTS plus an always-on orb is a
   crosstalk fixture.
3. **Ship a ~700 ms backchannel** so `thinking` is not silence. Cheapest possible
   latency win; buys the whole 700–1850 ms acceptable band.
4. **Make state changes carry affordances.** What is the system doing, **why**,
   and **what is available next**. Move to `activity` ⊥ `emotion` ⊥ `speech`.
5. **Do not build visemes.** Use amplitude/luminance for state legibility.
6. **Add the prosody gate** to the wake word (F1 0.93 vs 0.56).
7. **Pre-roll suppression** buffer, 300–500 ms (Google patent).
8. **Adopt Google's reprompt ladder** including the hard 3-retry cap, and invert
   the LLM verbosity problem: low confidence must shorten the turn.
9. **Confirmation:** name the action in the first ~10 words; offer spoken *and*
   clicked; ship a dry-run mode.
10. **Validate with a soak test and the Schönherr dataset**, not the unit suite.