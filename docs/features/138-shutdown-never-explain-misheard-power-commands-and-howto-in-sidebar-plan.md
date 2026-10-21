# 138 — "Shut down my laptop" must shut down; never read steps aloud; how-to answers go to the sidebar (PLAN ONLY, nothing built)

Date: 2026-10-17. Trigger: live log 2026-10-17 02:20 — the user said "shut down my laptop", STT heard "check down my laptop.", nothing matched, the cloud model answered with a how-to and NEXUS read 288 characters aloud for ~18 s.

## 1. What the owner asked (my reading — confirm item D1)
1. When the user gives the shutdown command, the laptop shuts down (through the existing graceful flow).
2. NEXUS never explains *how* to shut down by voice.
3. If steps are shown at all, they appear in the **sidebar window**, with one short spoken line.

## 2. What already exists (verified in code)
| Piece | State |
|---|---|
| `power_cmd::parse` strict parser, run before every close/open verb (`intent_parser.rs:604`) | works for exact verbs ("shut down", "power off", "turn off", ...) and a hand-kept mishearing list |
| `orchestrator::run_power_action` | an explicit shutdown is **obeyed at once** (no question) unless a meeting / running task blocks; goodbye line, graceful app close, 3 s cancel window |
| `Offer::Power` yes/no offer (`memcore/offer.rs`) | exists, used for the "shut down anyway?" question |
| Sidebar content API `show_sidebar_with_content(query, text)` (markdown) | exists, used by analysis/briefing |
| Voice-bias prompt for Groq (`NEXUS_VOCABULARY`) | 125 of 140 allowed words; has no power phrases |
| Any "how-to" handling | **none**: every unmatched sentence goes to the cloud and the whole answer is spoken |

## 3. Root causes
**R1 — whack-a-mole mishearing list.** `missed_intents.jsonl` already holds six different mishearings of the same sentence: "send on my laptop", "set down my laptop", "to set down my laptop", "check down my laptop", "tick on my laptop", "send down my laptop". An exact list fixes yesterday's miss and misses tomorrow's. (I added "check down / chuck down / shuck down" earlier today; that is the same whack-a-mole.)
**R2 — a miss leaks into the cloud.** An unparsed sentence about the computer is treated as a general question; the model answers it as a tutorial.
**R3 — no delivery rule for procedural answers.** Long how-to text is spoken in full, which is slow and exactly what the owner does not want.

## 4. Design

### A. Hear it right in the first place (cheap, biggest effect)
Add "Shut down my laptop. Restart my laptop. Lock my laptop." to `NEXUS_VOCABULARY` (11 words, budget 125 -> 136 of 140; the existing length test stays green). Whisper's prompt bias is what turns "check down" back into "shut down". Gate: replay is not possible offline, so measured by live `[STT]` lines after a rebuild (not claimed before).

### B. Structural "loose" power match for what still slips through (`power_cmd::parse_loose`)
Not a word list. A sentence is a *likely shutdown* when ALL hold:
* <= 5 words after `clean()`; ends with a computer word (laptop, computer, pc, system, machine, ...);
* contains a particle `down | on | off` right before the computer phrase;
* the leading verb is NOT a known benign verb (denylist: turn on, start, boot, wake, charge, clean, slow, calm, break, bring, back, update, write, track, wipe, dust, plug, ...);
* does not start with a question word (how, what, why, can, do, does, is, where).
Result: `Shutdown` with **weak** evidence.
Weak evidence never reaches the cloud and never powers off silently: NEXUS asks once, short and spoken: "Shut down your laptop, sir?" using the existing `Offer::Power` yes/no with the 10 s answer window. "Yes" runs the normal graceful flow.
**Learning (so it is asked only once per mishearing):** when the user answers yes, the heard phrase is stored in a local `power_aliases.json` (app data, never leaves the device); next time that exact phrase is a strong match and obeys at once. Cap 50 aliases, forget-able from the Memory page.
Hook: one call in `orchestrator.rs` at the point where an `Unknown` intent would be sent to the Worker (before `WorkerBackend`). No new `ParsedIntent` variant (that would touch the enum, `center.rs`, the frontend `Intent` union and every exhaustive match).

### C. How-to questions: sidebar, not voice (`howto.rs`, pure + tested)
1. **Detect** how-to shape on the *transcript*: "how do I / how to / how can I / steps to / show me how to / what is the way to / where do I click to".
2. **Power topics: no cloud at all.** "how do I shut down my laptop" -> static, version-correct steps card (Start > Power > Shut down; Alt+F4 on the desktop; Ctrl+Alt+Del > Power; `shutdown /s /t 0`; hold the power button as the last resort) + restart/sleep/lock variants. Instant, offline, no hallucination. Spoken: one short line ("The steps are in the sidebar, sir."), card opened with `show_sidebar_with_content`.
3. **Any other how-to** still asks the cloud (speed unchanged), but the *delivery* changes: if the reply is longer than ~160 characters or looks procedural (numbered/bulleted lines, step verbs: press, click, select, open, type), speak ONE short line and put the full markdown in the sidebar. Short answers and ordinary conversation are still spoken (speed priority).
Hook: one call where the Worker result is turned into `OrchestratorEvent::Result` (the analysis flow already does "emit Result, then open the sidebar", same pattern). Pure function `delivery_for(transcript, answer) -> Speak | SidebarWithShortLine{short, markdown}`.

### D. Safety rails
* Strong match keeps today's behaviour (obey; blocker question only for meetings/tasks).
* Weak match = confirm. Aliases only from an explicit "yes".
* The cancel window (3 s) and graceful close stay untouched.
* Kill switch: setting `powerCommands` (already exists) disables all of it.

## 5. Tests / gates (nothing is "done" without these)
1. Replay the six real mishearings: all six -> weak `Shutdown`; "shut down my laptop" -> strong.
2. 40-sentence negative corpus (slow down my laptop, turn on my laptop, charge my laptop, "what is a laptop", "how do I shut down my laptop", "restart" alone, ...) -> no loose match; mutation-check (drop the denylist / the particle rule -> tests must fail).
3. `howto` classifier: 20 positive / 20 negative phrasings; `delivery_for` thresholds; static power card content test (each of the five methods present, no network call).
4. Offer flow test: weak match -> question armed; "yes" -> `start_power`; "no" -> nothing powered off; alias saved only on yes; second time strong.
5. Existing: power_cmd 9, intent_parser power tests, orchestrator tests, frontend 316.
6. **Not testable here, needs you after a rebuild:** say "shut down my laptop" 5 times (does Whisper now hear it right?), say a deliberately garbled version, ask "how do I shut down my laptop", and one generic "how do I take a screenshot".

## 6. Effort and ownership
~1 day: `power_cmd.rs` (loose + aliases, ours), new `howto.rs` (ours), `stt_groq.rs` (one constant), `orchestrator.rs` (two narrow hooks, shared file: anchored patches, re-grep first), docs. GPT-sol's Ghost files untouched.

## 7. Decisions for the owner
* **D1 Scope of "never explain":** (a) only how-to questions go to the sidebar (recommended: conversation stays fast), or (b) every answer over ~160 characters goes to the sidebar with a short spoken line.
* **D2 Weak-match behaviour:** (a) ask once "Shut down your laptop, sir?" then learn the alias (recommended: powering off is destructive and a wrong guess loses unsaved work), or (b) obey at once even on a weak match.
* **D3 Spoken line wording** for the sidebar hand-off ("The steps are in the sidebar, sir.") — any change you want.

## 8. Not verified
Whisper's reaction to the prompt bias (needs live use); the 160-character threshold (a guess, tune from real answers); that the sidebar card renders the static steps in both themes (visual check after build).
