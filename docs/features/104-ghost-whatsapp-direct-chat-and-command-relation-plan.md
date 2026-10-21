# 104 — Ghost Mode: "Open <name> chat in WhatsApp" directly, and "replace the old command only if it's a retry — never if it's a continuation" (PLAN ONLY)

**Date:** 2026-10-09 · **Status:** PLAN. Nothing in this document is implemented. Requested as plan-only.
**Builds on:** `103-ghost-command-understanding-and-keyboard-first-implementation-plan.md` (grammar U1–U5, router E1), `102-…`,
research `10-command-understanding-…`, `11-p0-baseline-report-…`. Evidence below is from the user's live log (2026-10-09 00:15–00:17 session)
and from running the user's phrasings through the real parser today.

---

## 0. What the user asked (restated so it can be checked)
1. In Ghost Mode, **"open mummy chat in whatsapp"** and **"open thanmatee akka chat in whatsapp"** must *directly open that chat*.
2. If a command was **not understood** and the user repeats it, the **new command replaces the old one — if they are similar**.
3. But if the new command is **related to / dependent on** the previous one, the previous one must **not** be cancelled.
   Example: **"open brave" → "search dribble"**. "Search dribble" depends on Brave being opened; it must not cancel the open.

---

## 1. What the live log proves (and what it does not)
| Log line (time) | What it shows | Consequence |
|---|---|---|
| `open brave.` → ghost open → `open tab 3.` → `delete this tab.` (00:16:35–00:17:19) | Clear, well-formed commands already work end to end | The gap is phrasing breadth and relation handling, not the pipeline |
| `open google in new tab.` → `ghost open 'google photos'` (00:17:32) | **Wrong action executed.** The parser read it as `OpenApp("google in new tab")`; the registry fuzzy-matched *Google Photos* | "open X in new tab" is a missing grammar form; app matching accepts a poor match (research 10 §3.6) |
| `go to the moon.` → `open_app`, confidence 1 (00:16:05) | Everyday speech accepted as a command at **confidence 1.0** | Parser confidence says nothing about whether the *target* is valid |
| `there is this tab.` → *no* deterministic parse → **brain** `github_command` @0.95 → `subsystem: none` → silence → watchdog re-arm 27 s later (00:16:48→00:17:15) | A garbled "delete this tab" was mislabelled by the brain as a GitHub command, the turn ended with **no spoken result**, and Ghost sat dead for ~27 s | Brain/NLU labels must be proposals, not commands, inside Ghost; every turn must end with speech |
| `on me.` → `directed gate: ignored (no_cues)` → `followup listen skipped: duplicate hot_mic re-arm (+689ms)` (00:17:46) | The gate and my phase-R duplicate-listen guard both worked live | Positive: R verified in the field |
| `__NEXUS_WAKE__ invoked #1…#10 (source=…, +gap)` | Every wake is attributed, gaps are seconds, **no double wake** | Phase R + 117 instrumentation hold |
| `[ANIM] WebGL shader/context compilation error … 'bool' * 'highp float'` at startup | Unrelated bug: the orb shader does not compile | Filed as a separate task (not part of this plan) |
**Not proven by the log:** how often an in-flight command gets cancelled by the next one (the log shows one relisten during `thinking`, nothing about overlapping *commands*). §4.1 therefore starts with a measurement step.

### 1.1 What the parser does today with the user's exact phrases (run through `parse_deterministic`, 2026-10-09)
| Phrase | Result | Why |
|---|---|---|
| `open mummy chat in whatsapp` | `OpenApp("mummy chat in whatsapp")` ✗ | generic `open <anything>` arm runs before any WhatsApp arm |
| `open thanmatee akka chat in whatsapp` | `OpenApp("thanmatee akka chat in whatsapp")` ✗ | same |
| `open mummy chat on whatsapp`, `open mummy's chat in whatsapp`, `open the chat of mummy in whatsapp`, `open mummy in whatsapp`, `go to mummy chat in whatsapp`, `open mummy chat` | all `OpenApp(…)` ✗ | same |
| `open chat with mummy`, `chat with mummy`, `message mummy on whatsapp`, `whatsapp mummy` | `WhatsappChat{contact:"mummy"}` ✓ | the only forms `parse_whatsapp_command` knows (8 literal prefixes) |
| `open brave and search dribble` / `… then search dribble` | `OpenApp("brave and search dribble")` | no compound split at parse level (the command-center splits "then" later, "and" never) |
| `open brave` → `OpenApp`; `search dribble` → `Search` | each fine alone | |
So **item 1 is a grammar gap**: the execution path for `WhatsappChat` already exists and is what Ghost uses for "chat with mummy".

---

## 2. Part A — "Open <name> chat in WhatsApp" opens that chat, directly

### 2.1 Understanding (belongs to track U of plan 103)
- **New forms** (all produce `WhatsappChat{contact}`), tried **before** the generic `open <app>` arm:
  `open {name} chat (in|on) whatsapp` · `open {name} (in|on) whatsapp` · `open the chat (of|with|for) {name} (in|on) whatsapp` · `open {name}'s chat (in|on) whatsapp` ·
  `go to {name} chat (in|on) whatsapp` · `{name} chat (in|on) whatsapp` · `show {name} chat` · `whatsapp {name} chat`, plus the polite/wake/filler wrappers from U1.
- **Names may be several words** ("thanmatee akka") and may be kinship words or nicknames ("mummy", "papa", "akka", "anna"). The text alone cannot tell "open mummy chat" (a chat) from "open notepad" (an app) when the word *whatsapp* is not spoken — so:
  - with the word **whatsapp**: always a chat command;
  - **without** it: a chat command only if `{name}` resolves to a known contact/alias (§2.2); otherwise the usual app path / clarification.
- **Name resolution is a closed-vocabulary problem** (a few hundred contacts), the case where phonetic matching is strongest (research 10 §5.2): "thanmatee" vs the real contact "Thanmayee", "mummy" vs "Mummy ❤", transcription variants. Candidates are scored by token overlap + phoneme distance + learned aliases + recency/frequency of past chats; accept only above a threshold **and** with a clear margin over the runner-up (two "Mummy"s → ask "Which Mummy?").

### 2.2 Where the contact names come from (ranked, all local)
1. **WhatsApp's own chat list on screen** (UIA list items) — the exact strings the user sees. With the E0.5 fast lookup this is ~20–80 ms; it needs no stored data. **Unmeasured for the WhatsApp desktop client** (WebView2 vs UWP) — prerequisite probe, §6 step 1.
2. The WhatsApp MCP bridge's read-only chat list (already built for the priority-people feature; the read allowlist is enforced in code).
3. Remembered facts / aliases ("remember that mummy is …") and **learned aliases** (after a successful open + the user not objecting, store `mummy → <contact>`; promoted after N confirmations).
4. `contacts.json` — **does not exist on this machine today**, so the current non-Ghost `whatsapp_chat` can only say "couldn't find … opening WhatsApp so you can pick a chat" (the file would also need phone numbers).

### 2.3 Execution — "directly open", keyboard-/pattern-first (ties to plan 102/103)
Today's Ghost path (`ghost_drill::whatsapp_drill`): launch/focus WhatsApp → **fixed `sleep(1000)`** → `search_contact` (type the name into search) — slow, and the 1 s wait is unconditional. Proposed ladder:
1. WhatsApp in front → fast UIA lookup of the chat-list item for the resolved contact → **Invoke** it (no cursor, no typing) → **verify** the chat header/title now shows the contact → speak "Mummy's chat is open, sir."
2. WhatsApp running but behind → focus it (registry, ms), then step 1.
3. Not running → launch, then **poll for the window** (UIA / foreground) instead of sleeping a fixed second, then step 1.
4. Contact not visible in the list → keyboard path via the Shortcut Map: `search` shortcut → type the resolved name → first result (Down/Enter) → verify. (This is what `search_contact` does today, with verification added.)
5. Deep link `whatsapp://send?phone=…` only when a phone number is actually known (and always for a not-running app if numbers exist) — last resort, not the default.
- Group chats and chats with no phone number are only reachable via 1/4, which is another reason not to make the deep link the primary path.
- Safety: the existing contact denylist (`safety_check("whatsapp_search", contact)`) stays; **opening a chat never types a message** (a separate "send" confirmation owns that); reading chat *contents* is out of scope.
- Expected latency (to be measured, not promised): parse < 1 ms; focus ≤ 150 ms; UIA find+invoke ≤ 120 ms; verify ≤ 100 ms ⇒ **≈ 0.4 s** when WhatsApp is already running, versus ≥ 1 s of fixed sleep + typing now.

### 2.4 Risks / unknowns specific to WhatsApp
- WhatsApp desktop's UIA tree is **unmeasured** (Chromium/WebView2 clients expose a tree lazily; the whole point of the pending Brave/VS Code probes). If list items are not exposed, rungs 1–2 degrade to rung 4.
- Duplicate or emoji-decorated names, "Mummy (You)", groups with the same first name.
- ASR garbles of names are *systematic* (research 10 §3.5) — resolution should use the on-screen list as the vocabulary, not a global dictionary.

---

## 3. Part B — Which new command replaces the old one, and which must not (the user's logic, formalised)

### 3.1 What exists today (read from the code)
- **Ghost FIFO** (`ghost.rs`): commands spoken *while a drill is running* are queued (cap 5, drop-newest). The only replacement rule is **lexical**: a pending item of the same *slot class* is overwritten — `AppOpen` (`open/launch/start/close …`) and `Navigate` (`search/go to/find …`). Nothing compares meaning or similarity, so **"open brave" followed by "open whatsapp" silently drops Brave**, while "open brave" followed by "search dribble" survives only because their slot classes differ.
- **Outside the queue**: `install_new_request` marks the previous request cancelled whenever a new one starts, regardless of how the two relate. Runners check the flag at their own points (command-center steps do; a single ghost open mostly does not).
- **Frontend**: an auto relisten during `thinking` aborts the *capture* ("turn-transition: cancelling current turn") but does not cancel the already-dispatched command.
- **No notion of "the last command was not understood"**: the Unknown path speaks "Didn't catch that — say it again" but records nothing a later command can be compared with.

### 3.2 The principle
Two consecutive commands are related in one of five ways; the right behaviour differs, and *similarity alone* is not enough (the user's own example: "open brave" and "search dribble" are *related* yet must both run).

| Relation | Meaning | Action on the OLD command | Action on the NEW command |
|---|---|---|---|
| **Retry / correction** | The user is re-saying something that was not understood, failed, or was misheard ("open bread" → "open brave"; "search dibble" → "search dribble") | **Discard** (cancel if not started; ignore its result) | Run |
| **Duplicate echo** | Same command again within ~3 s while the first is still pending/running (impatience, double STT) | Keep | Drop |
| **Dependent / continuation** | The new command *needs the state the old one creates* ("open brave" → "search dribble"; "open whatsapp" → "open mummy chat"; "new tab" → "open google") | **Keep — never cancel** | **Wait** for the old to finish, re-check grounding, then run |
| **Independent** | Unrelated ("open brave", "open whatsapp") | Keep | Queue after it (FIFO) |
| **State-overwriting supersede** | Both set the *same piece of state* and the old one has not started ("search cats" → "search dogs") | Replace | Run |

### 3.3 How the relation is decided (deterministic, tiny, testable)
Inputs per command: `CommandRecord { id, raw, normalized, canonical{action, slots}, requires: Set<Ctx>, provides: Set<Ctx>, status, t_spoken, t_done }` where `status ∈ {Pending, Running, Done, Failed, NotUnderstood, Cancelled}`.
Decision order (first match wins):
1. **No previous**, or previous finished > ~20 s ago → Independent.
2. **Explicit correction marker** at the start of the new utterance ("no", "not that", "I said", "I meant", "sorry", "wait", "actually") → Retry/correction of the last *unfinished or failed* command (if it already completed, it is a new command with that context — never an undo).
3. **Duplicate echo**: same canonical action + equivalent slots, previous Pending/Running, gap < ~3 s → drop new.
4. **Retry**: previous `NotUnderstood`/`Failed` (or an unfinished one the user is clearly redoing), within ~15 s, and the new utterance is **similar** → replace. Similarity = max of (content-token overlap after U1 normalisation, normalised edit distance, **phonetic** similarity from U3, same canonical action with a similar slot). Threshold tuned on a labelled pair corpus; a repeat of a *successful* command is **not** a retry (it just runs again — "scroll down" twice means twice).
5. **Dependent**: `new.requires ⊄ current world state` **and** `new.requires ⊆ old.provides` → keep old, queue new behind it, and at dequeue re-verify grounding (`ghost::verify_grounding` already does this for browser steps).
6. **State-overwriting supersede**: both are "state-setting" actions of the same family (search, navigate, go-to-tab, address-bar) and the old one is still Pending (not started) → replace.
7. Otherwise **Independent** → FIFO append.

`requires/provides` (initial table, extensible, derived from the Shortcut Map's app families so there is one source of truth):
| Command | provides | requires |
|---|---|---|
| `open_app X` | foreground = app X (and its family, e.g. *browser* for brave) | — |
| `whatsapp_chat c` | foreground = WhatsApp, chat c open | — |
| search / new tab / go-to-tab / address bar / back / refresh / zoom | — | foreground ∈ *browser* family |
| `type_text`, paste, send | — | a text-capable foreground that is not NEXUS (send also: a draft/chat open) |
| copy / undo / save … | — | any non-protected foreground |
**Worked examples**
| Previous → New | Previous status | Relation | Result |
|---|---|---|---|
| "open brave" → "search dribble" | Running (launching) | **Dependent** (search requires browser; open provides browser) | Brave keeps opening; search runs after Brave is foreground; wait ≤ ~8 s, else say so and drop it |
| "open brave" → "open whatsapp" | Running | Independent | both run, in order (**changes today's D8 supersede — decision 1**) |
| "search dibble" (NotUnderstood) → "search dribble" | NotUnderstood | **Retry** | old discarded, new runs |
| "open bread" (failed) → "open brave" | Failed | **Retry** | same |
| "open brave" → "open brave" (1.5 s later) | Running | Duplicate echo | second dropped |
| "search cats" → "search dogs" | Pending | State-overwriting supersede | only dogs |
| "scroll down" → "scroll down" | Done | Independent | scrolls twice |
| "open brave" → "no, open chrome" | Running | Correction marker | brave discarded if not yet up; chrome runs |

### 3.4 Where it plugs in (small surface, new pure module)
- New `cmdu/relate.rs` (pure functions + table tests) and a small **CommandLedger** (last N records with status) owned by the Ghost session.
- `ghost.rs::enqueue_command` — replace the lexical `classify_slot` supersede with `relate(...)`; keep the cap and `verify_grounding`.
- `orchestrator::install_new_request` — cancel the previous request **only** on Retry/Correction/Supersede; Dependent/Independent wait their turn.
- The Unknown/“Didn't catch that” path and the directed-speech gate's “addressed but not understood” outcome **write a `NotUnderstood` record** (they currently write nothing a later command can see).
- The frontend relisten keeps not cancelling in-flight commands (only an explicit stop/hotkey barge-in does).
- Every decision logs one line: `[CMD-REL] prev='open brave'(Running) new='search dribble' -> Dependent (requires browser, provides browser) wait`.

### 3.5 Start by measuring (cheap, no behaviour change)
Add the ledger and the `[CMD-REL]` log **in shadow mode** (compute the relation, log it, change nothing) for a few days of real Ghost use, then review the log to tune thresholds and confirm which cancels actually occur. This is consistent with how phases R/P0 were done and it protects against encoding a wrong guess about the existing cancel points.

---

## 4. Part C — Fixes the log also calls for (feed plan 103)
1. **Missing grammar forms** that currently fall into `open <anything>`: `open {site} in (a )?new tab` (⇒ new tab + address bar + navigate — a router/shortcut sequence, not an app launch), the WhatsApp forms in §2.1, `go to {site}`. Add to the U2 lexicon with the ordering rule "closed-lexicon actions before open-slot arms".
2. **Target validation before confidence**: a deterministic parse that ends in an open-slot arm (`OpenApp`) must not report confidence 1.0 unless the target resolves in the registry with sufficient coverage (`go to the moon`, `google in new tab`). This is the app-match gate (change 117) applied at the parser, not only at execution.
3. **Inside Ghost, brain/NLU labels are proposals.** A label coming only from the brain/NLU (here `github_command @0.95` for a garbled "delete this tab") may not start long-running or external intents (GitHub, MCP, architect…) without an explicit cue/wake word; otherwise the turn ends in the local "Didn't catch that — say it again" path.
4. **Turn-completion invariant:** a Ghost turn must always end with speech or an explicit "ignored" outcome; `subsystem: none` with no spoken result is a bug class (here: 27 s of dead air). Add a test that drives every ghost-routed outcome and asserts a spoken line or a logged ignore.
5. The E1 router already covers the *execution* side of `delete this tab`, `open tab 3`, etc. (foreground-checked, 2–3 ms unverified / ~42 ms verified in the live run).

---

## 5. Test and acceptance plan
| What | How | Gate |
|---|---|---|
| WhatsApp grammar | table of ≥ 40 phrasings (the user's two exact phrases + wrappers, kinship names, multi-word names, "in/on", possessive) → `WhatsappChat{contact}`; and ≥ 20 non-chat look-alikes ("open notepad", "open chat gpt") that must **not** | 100 % on the user's phrases; 0 false chat-parses in the look-alike set |
| Name resolution | contact-list fixtures with garbles ("thanmatee"→"Thanmayee"), duplicates, emoji, groups | correct on clear cases; asks on ambiguous; never opens a wrong contact silently |
| Execution | scripted run against a real WhatsApp window (user-run): ladder step reached, header verified, latency recorded | opens the right chat or says why not; never types a message |
| Relation corpus | ≥ 60 labelled (previous, new, status, gap) pairs incl. every example in §3.3 | **100 % on the user's examples**; ≥ 95 % overall; **never cancels a Dependent**; false-replace ≤ 1 % |
| Shadow run | `[CMD-REL]` log over several days of real use | review before enabling; any "would have cancelled a useful command" case becomes a test |
| Ghost turn invariant | enumerate outcomes of the ghost branch | no silent end |

## 6. Phases (each small, each gated; depends on 103's U1/U2 for the grammar parts)
| # | Phase | Notes |
|---|---|---|
| 0 | **Probe WhatsApp's UIA tree** (user runs `uia_probe` with the WhatsApp window in front; names of chat items are *not* written to the file) | decides whether rungs 1–2 of §2.3 are possible |
| 1 | **Ledger + shadow `[CMD-REL]` logging** (no behaviour change) | answers "how often does a useful command get cancelled today?" |
| 2 | **WhatsApp grammar forms** (U2) + parser-confidence-with-target-validation (§4.2) | pure parser work, table-tested; fixes the user's two phrases for the *understanding* half |
| 3 | **Contact resolution** (on-screen list + MCP list + aliases) with phonetic matching | closed vocabulary |
| 4 | **Direct-open ladder** (UIA invoke → keyboard search → deep link) with verification and the poll-instead-of-sleep launch | replaces the fixed 1 s sleep |
| 5 | **Relation engine live** (`relate.rs`, ledger-driven queueing, selective cancel) | behind a flag, default off until the shadow log looks right |
| 6 | **Ghost brain-proposal gate + turn-completion invariant** (§4.3–4.4) | removes the silent 27 s stall class |

## 7. Decisions needed from the user
1. **"open brave" then "open whatsapp"** — run **both in order** (recommended: opening apps accumulates), or keep today's rule where the newer open replaces the pending one (decision D8)?
2. **"search cats" then "search dogs"** while the first has not started — replace (recommended; the second overwrites the first anyway) or run both?
3. **Dependent timeout:** how long should NEXUS wait for Brave to come up before giving up on "search dribble" (proposal 8 s), and should it say "Brave didn't open, sir"?
4. **Tell me when I discard a command?** Silent, or a short "Okay — <new command> instead"?
5. **Nicknames:** learn `mummy → <contact>` automatically after a successful open, or ask once ("Is Mummy the contact 'Amma'?")?
6. **WhatsApp probe:** are you OK running the UIA probe with WhatsApp in front? (It records control *counts and shortcut labels only* — not chat names or messages — and refuses to read any other app.)
