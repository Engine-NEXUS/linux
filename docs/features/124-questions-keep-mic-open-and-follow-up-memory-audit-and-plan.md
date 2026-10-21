# 124 — When NEXUS asks a question: mic stays open, and "do it" still works later (AUDIT + PLAN, nothing built)

> See also [125](125-response-delay-and-whatsapp-learning-analysis-and-plan.md): "ask once, remember" for unknown WhatsApp contacts (W5) uses the pending-question mechanism of Q2.

Asked for: "whenever it asks a question it should always have the mic open so the user can speak the response ... even if
the mic gets closed, when the user calls it again and says 'do it' it should remember the last conversation. Check how great
the memory is working and see if it is perfect so I can plan the implementation."

Method: code read (every claim names its file), the repo's own memory tests (run), and a read-only look at the **shape** of the
stored memory on this PC (counts only; no personal text was read, and the sealed `memcore.enc` was not opened). Nothing was run
live with a real microphone.

**Verdict in one line:** structured offers work for 90 s; everything else (confirmations, clarifications, cloud questions) is
partly or not remembered, and the conversation memory is almost empty in practice. It is **not perfect**.

---

## 1. Every kind of question NEXUS asks, and what the mic does

| # | Question kind (examples) | Mic opens after the question? | Window | Still answerable after the mic closes? | Where |
|---|---|---|---|---|---|
| 1 | **Offers**: "Time for DSA, shall I start?", "In the app or the browser?", "Add these slots?", "Shut down your laptop?", "Reopen what you had open?", "Did you mean Google Chat?" | Yes, once the question has been **spoken** | **8 s** | **Yes, for 90 s** (wake word + "do it"); after that it is dropped silently | `memcore/offer.rs` (`LISTEN_WINDOW_MS`, `REPLY_WINDOW`), `net/orchestrator.ts:211` |
| 2 | **Confirmations** (GitHub merge/close, WhatsApp send, MCP order) | Yes | **5 s**, hard-coded | **Yes, forever** (see 2.1: never expires) | `net/orchestrator.ts:181-199, 811-830` |
| 3 | **Clarifications** ("say it again", "which one?", "hand mode or ghost mode?") | Yes | **8 s**, one try | **No.** Nothing links the answer back to the original request | `orchestrator.rs:298-320`, `net/orchestrator.ts:474-500` |
| 4 | **Cloud-LLM questions** ("Would you like me to ...?") | **No** | none | **No.** "do it" becomes a new cloud turn; nothing executes | no code path |
| 5 | **Worker follow-ups** (repo not found -> asks which repo) | Via the clarify path | 8 s | **No.** The Worker returns `dialog_state`; the frontend only logs it and **never sends it back** (`processViaOrchestrator(corrected, undefined, ...)`) | `net/orchestrator.ts:559`, `recorder.ts:993,1066` |
| 6 | Ghost mode | Hot mic is always open | n/a | n/a | `net/ghostHotMic.ts` |

Gates that also stop the mic from opening: a meeting is active (kinds 1 and 3), Ghost mode already owns it, and the known
Intel SST problem (mic delivers silence for minutes, so a "spoken" answer cannot be heard at all).

### 1.1 What happens to "do it" in each case
* **Offer, within 90 s:** works. `classify()` accepts "do it", "go ahead", "yes", "let's go", ... up to 9 words after removing
  "nexus"; a wake-word turn is routed to the offer first (`orchestrator.rs:748`). Owner check: a TV/other voice cannot approve
  shutdown, app launch, calendar add, timetable erase or session reopen (`may_answer_offer`).
* **Offer, after 90 s:** dropped; "do it" goes down the normal path as an unknown sentence.
* **Confirmation, any time later:** executed (see 2.1, which is a problem).
* **Clarification / cloud question:** "do it" has nothing to refer to.

---

## 2. Problems found (ranked)

### 2.1 SAFETY: a pending confirmation never expires and a loose "ok ..." confirms it
`pendingGithubCommand` is cleared only on yes/no (`net/orchestrator.ts:828,877,905`); `reset()` does not clear it
(`store/assistant.ts:231-246`). The confirm test is
`^(yes|yeah|yep|yup|confirm|ok|okay|sure|go ahead|do it|proceed|approved?|agreed?|approve)\b`: it has **no end anchor**.
I ran it: *"okay so what is the weather"*, *"ok open notepad"*, *"sure what time is it"*, *"proceed with the merge"* all test
**true**. So while a WhatsApp send, a PR merge/close or a food order is pending, **any later sentence that starts with
"ok / sure / yes"** (an hour later, in another conversation) would execute it. Only a rejected or uncertain voice is blocked.
This is the first thing to fix and is small.

### 2.2 The mic window is short and fixed
8 s for offers/clarifications, 5 s for confirmations, measured from the end of the question audio, cut at the deadline whether
or not the user has started speaking (`hadSpeech` only prevents the cut if speech was already captured). People often need 2-4 s
to think; the answer to a long question ("In the app or the browser?") is easily missed.

### 2.3 Only some questions open the mic, and the logic is duplicated 3 times
Offers, confirmations and clarifications each have their own state, own timer, own window length and own reply parser (Rust
`offer.rs`, frontend `pendingGithubCommand`, frontend `clarificationRequestId`). Cloud questions have none.

### 2.4 Clarifications do not complete the original request
`speak_prompt_and_hold` emits `Clarify { prompt, expected_slot }`; I found no state that remembers the request being clarified,
so the answer is parsed as a brand-new sentence. The Worker's own follow-up state (`dialog_state` / `pending_intent`) is
dropped by the frontend (row 5 above).

### 2.5 The conversation memory is almost empty in practice
Real data on this PC (counts only):
* `conversation.jsonl`: **23 turns in 10 days**, **22 of 23 have intent "unknown" and relevance < 40**, 0 with an unresolved
  question, 0 failed/clarification turns. `conversation_brief.json` is **121 characters**.
* `conversation_prompt()` **drops completed turns with relevance < 40** (`conversation.rs`, "low-signal ... STT-noise tail"),
  so **the recent-thread block sent to the cloud is effectively empty** for normal chat.
* Only **cloud turns** are recorded (`record_conversation_turn` has one live caller, `record_worker_turn`,
  `orchestrator.rs:2558`). Local commands ("open Chrome", "start hand mode", shutdown offers) never enter it.
* `episodes.jsonl`: 54 lines; `memcore.enc`: 260 KB sealed (not read).
So even a perfect "do it" resolver would have no recent context to read.

### 2.6 Cloud answers can claim success they did not do
With no executor behind a cloud suggestion, a "do it" reaches the cloud model, which can answer "Done" with nothing done (the
same family of "spoke success that did not happen" problems listed in the G0 audit).

### 2.7 Related open bug (already noted in AGENTS.md)
The second back-to-back auto-relisten after an already-answered turn (field test 5) lives in the same relisten code; fix it in the
same change so the new mic window does not loop.

---

## 3. How good is the memory itself? (evidence)

| Check | Result |
|---|---|
| Unit tests: conversation, offer, hygiene, store, recall (`cargo test`, serial) | **41 / 41 pass** |
| What those tests cover | components in isolation (classification, hygiene, ranking), **not** the end-to-end "ask -> mic closes -> wake -> do it" |
| Offer reply classifier | strict and well tested (yes/no/later families, ownership gate) |
| Retrieval | lexical (BM25 + recency); documented known miss ("where do I work" does not find `employer`, `memcore/mod.rs:709`) |
| Hygiene filter | keeps junk out, but also leaves the live thread nearly empty (2.5) |
| Live check | none run (no real mic here) |

**Grade:** structured offers **good** (within 90 s); confirmations **works but unsafe**; clarifications and cloud follow-ups
**missing**; conversation memory **not useful for "do it"**. Not perfect.

---

## 4. Target behaviour (what to build)

**Rule 1: asking = listening.** Every question NEXUS speaks that expects an answer opens the mic when the speech ends.
Window by endpointing, not a fixed cut: wait up to ~10 s for speech to **start**; once it starts, stay open until the normal
end-of-speech silence. Exceptions stay: meeting active (show buttons instead), Ghost (already hot). If the mic is silent/deaf
(Intel SST), say so once and show on-screen **Yes / No** buttons so the answer is never lost.

**Rule 2: one question object.** A single `PendingQuestion` in Rust replaces the three mechanisms:
`{ id, kind, prompt, asked_at, expires_at, risk, action (what runs on yes), options, owner_required }`.
Offers, confirmations, clarifications and Worker follow-ups all create one; the frontend only mirrors it.

**Rule 3: the question outlives the mic.** After the window closes it stays pending with a TTL by risk:
low (open an app, start a study session) 10 min; medium (calendar add, timetable) 5 min; **high (send a message, merge/close a PR,
order, shutdown, erase) 2 min**. While pending, the orb shows a small "waiting for your answer" mark. A later wake +
"do it / yes / go ahead / that one / the first one / no / later" resolves it, and the reply is told apart from a new command:
short (<= 4 words), starts **and ends** with an answer word, owner voice required for high-risk.

**Rule 4: remember the last conversation properly.**
* Record **every** turn (local commands and cloud) in the conversation store with its outcome and the structured action.
* Live thread = the **last 6 turns of the last 15 minutes regardless of relevance score** (relevance only prunes the older
  durable brief), so "do it" and "what did I just ask" always have context.
* Keep a small structured `last_proposal` (what NEXUS last offered or suggested, with its action) for 10 min.
* If "do it" arrives with **no** pending question, ask: "Do what, sir? A moment ago I mentioned X." instead of guessing.

**Rule 5: cloud questions become real.** The cloud reply may include a structured `proposed_action`; when present NEXUS registers
it as a `PendingQuestion`. When it is plain prose, a bare "do it" must not be answered with "Done": NEXUS says it has nothing
runnable, or re-asks with the specific action.

**Rule 6: observable.** `[ASK]` log lines (asked / spoken / mic_open / mic_closed / answered / expired / rejected_voice) with ids,
and a scripted test harness (below).

---

## 5. Plan (each phase ends with a gate)

* **Q0 - Reproduce and measure (0.5 day).** A scripted harness that drives the real code path with fake audio events:
  ask -> silence -> wake "do it" at +3 s, +30 s, +100 s, +11 min; "okay so what's the weather" while a confirmation is pending;
  meeting active; deaf mic; wrong voice. Record today's pass/fail table. Gate: the table exists.
* **Q1 - Safety fix (0.5 day, do first).** Anchor the confirm/deny test (a sentence must **be** an answer, <= 4 words, not merely
  start with one); give pending confirmations a TTL (2 min high-risk); clear them on `reset`/cancel; add 50 negative sentences.
  Gate: 0 false confirms in the 50; mutation-checked.
* **Q2 - `PendingQuestion` + mic rule (2 days).** Unify offers/confirmations/clarifications; endpoint-based window; on-screen
  Yes/No buttons; pending mark on the orb; meeting and deaf-mic handling; fix the second-relisten bug. Gate: all Q0 scenarios pass.
* **Q3 - Conversation memory (1 day).** Record local turns; recency-based live thread; `last_proposal`; "Do what, sir?" fallback.
  Gate: a scripted 10-turn conversation where "do it" / "the second one" / "what did I just ask" are answered correctly.
* **Q4 - Structured cloud proposals (1-2 days, Worker + app).** `proposed_action` in the Worker reply, round-trip of
  `dialog_state`, no fake "Done". Gate: 10 cloud-suggested actions execute or are refused honestly.
* **Q5 - Polish:** `[ASK]` logging in the unified console, settings for window length and TTLs.

Order: **Q1 now** (it is a standing risk), then Q0, Q2, Q3, Q4.

## 6. Decisions needed from you
1. TTLs: high-risk 2 min, medium 5 min, low 10 min: OK, or longer?
2. Window: wait up to 10 s for speech to start, then follow normal end-of-speech: OK?
3. A bare "do it" with nothing pending: ask "Do what, sir?" (recommended) or stay silent?
4. After a high-risk question times out, should NEXUS say "I'll forget that, sir" or drop it silently?
5. Should Q1 (the safety fix) go ahead now, ahead of the rest?

## 7. Not verified
No live microphone test; the real stored memory content and the sealed store were not read; the 41 passing tests do not exercise
the end-to-end flow; durations in section 1 are code constants, not measured user behaviour; the "relisten" bug is taken from
AGENTS.md, not re-reproduced here.
