# 125 — Slow replies ("Ok sir" after 7 s) and the WhatsApp agent that never learns (ANALYSIS + PLAN, nothing built)

Evidence: the `nexus start` log of 2026-10-15 11:23-11:29 pasted by the user, plus the code paths named below. Companion plans:
[123](123-boot-to-first-word-latency-analysis-and-plan.md) (boot + shutdown), [124](124-questions-keep-mic-open-and-follow-up-memory-audit-and-plan.md)
(questions + memory). Nothing here was built or run live.

User reports: (1) "ghost mode ... open brave ... it opens Brave but takes 4-5 s to say 'Ok sir'"; (2) "create a new tab does it
instant but takes 5 s to say 'tab sir'"; (3) "I still don't see NEXUS open a specific person's chat in WhatsApp; it doesn't learn; it
is a blind agent following orders".

---

## 1. Part A — why every spoken line is late

### 1.1 The log, line by line (all rows end with `edge-tts rejected ... 403 Forbidden, trying local fallback`)

| Command / line | Reply ready | Speech starts (`[TTS] engine=local`) | **Delay** |
|---|---|---|---|
| "hey" -> "Hello, sir." | 11:23:51 | 11:24:02 | **11 s** |
| "do something for me" -> cloud answer | 11:24:14 | 11:24:34 | **20 s** (orb already force-closed at 11:24:25) |
| "ghost mode" -> "Ghost mode initialized, sir" | 11:25:10 | 11:25:21 | **11 s** |
| **"open brave" -> "Ok sir."** | 11:25:38 | 11:25:45 | **7 s** |
| **"create a new tab" -> "New tab sir."** (Ctrl+T sent at once) | 11:25:53 | 11:25:59 | **6 s** |
| "hand mode" -> "Hand mode on, sir." | 11:26:15 | 11:26:22 | 7 s |
| empty-turn nag | 11:27:04 | 11:27:12 | 8 s |

So the problem is not "Ok sir" or "New tab": **every** line takes 6-20 s, and the actions themselves are instant.

### 1.2 Root-cause chain (each link is in the log or the code)

1. **Edge TTS is permanently broken, not "transient".** 403 on every request in this log (and in 3 earlier logs). `tts.rs:806` logs
   "network flag untouched" and never remembers. Probable cause (hypothesis): the pinned `edge-tts-rust 0.1.3` sends an outdated
   `Sec-MS-GEC-Version`; **0.1.6 exists** (doc 123 section 8.2).
2. **"The cloud is back" is judged by the wrong test.** `is_network_up()` is true because bing.com answers (`tts_edge.rs:255`),
   not because Edge TTS works.
3. **That makes the cached local-voice acks "stale"** (`tts.rs:50-68`, `cache_entry_usable`: `estimated && is_network_up()`), so the
   already-rendered "Ok sir." (which would play in < 5 ms) is **thrown away** and a refresh starts:
   `[TTS] cached ack is the local voice but the cloud is back; refreshing cache` (log 11:25:38).
4. **The refresh floods the CPU:** `pregenerate_cache` fires 9 Edge requests (9 x 403, 11:25:39-44) and then renders all 9 phrases
   with Kokoro (42-51 s of CPU, seen in boot logs) **while the live line is being synthesised**.
5. **The live line pays the whole slow path:** Edge attempt (~0.5 s, 403) + Kokoro (cold or contended) + synthesis. NEXUS also runs in
   **EcoQoS / BelowNormal** (`power: EcoQoS enabled`, task priority 7) on a **battery** laptop (`power=BATTERY 21 %`), so Kokoro runs
   on efficiency cores. *(Hypothesis for the 6-20 s; the plan measures it first.)*
6. **The orb/audio sync breaks:** the stuck-speaking watchdog (10 s of silence) fires during the synthesis wait
   (`11:24:25 stuck-speaking watchdog: silent 10s, forcing turn close`), hides the orb, and the voice then plays 9 s later with no orb.
7. **Nothing runs in parallel:** the keystroke happens immediately, but the acknowledgement is only synthesised afterwards.

### 1.3 Fix plan (Phase R)

| # | Change | Why | Effort |
|---|---|---|---|
| R1 | **Edge circuit-breaker:** after 2 consecutive 403/timeouts mark Edge "degraded" for 10 min (skip it, no 0.5 s tax); re-probe with one cheap background attempt | ends the retry tax and the false "cloud is back" | 0.5 day |
| R2 | **Cache validity by real success:** a local-voice cached ack is stale only if an Edge synthesis **actually succeeded** in the last N minutes, never because bing.com answers | "Ok sir." plays from cache in < 50 ms again | 0.5 day |
| R3 | **Try `cargo update -p edge-tts-rust` (0.1.3 -> 0.1.6)** + a `nexus tts probe` command that prints the first success/403 | may remove the whole fallback problem and the voice swap | 0.5 day |
| R4 | **Single-flight cache refresh**, low priority, never started from the speak path, never repeated for the same engine | no CPU flood during a live line | 0.5 day |
| R5 | **Keep Kokoro loaded while Edge is degraded** (the monitor unloads it after 10 min "stable network"); log `[TTS-T] queued / edge_try / fallback / synth_start / synth_done / play_start` per line | measure where the 6-20 s really goes; check EcoQoS effect | 0.5 day |
| R6 | **Persist the common lines on disk** and widen the set: "Ok sir.", "New tab sir.", "Hand mode on, sir.", "Ghost mode initialized...", the 3 greetings, "Good night, sir." (shared with doc 123 P2) | most ghost acks never need synthesis | 1 day |
| R7 | **Speak in parallel with the action:** start synthesis at parse time; play the moment the action succeeds | "tab sir" no longer waits for the key press | 0.5 day |
| R8 | **Watchdog understands "speech requested, not yet audible"** (no force-close during synthesis; show a short "about to speak" state) | no orb-less voice (the 2026-10-15 sync rule) | 0.5 day |
| R9 | **Hand mode start-up:** log shows `no frames for 12s` and first `capture_to_landmarks p50=13148ms` right after "hand mode" (same moment as the TTS contention); instrument start -> first landmark and trim | first usable hand frame in the 18 s after the command today | 0.5 day |

Targets: acknowledgement audible **<= 300 ms** after the action; any short reply **<= 1.5 s** (Edge healthy) / **<= 2 s** (local, warm).
Order: **R1 + R2 first** (smallest, biggest effect), then R3, R4, R7, R8, R6, R5/R9.

---

## 2. Part B — WhatsApp: why a specific person's chat does not open, and why it never learns

### 2.1 What the log shows (11:28:12 -> 11:29:08, 57 s, failed)

1. STT heard **"start with mummy in whatsapp"** (meant "chat with mummy"). The parser still accepted it as `whatsapp_chat` (good), but
   nothing records that "start with" means "chat with"; a second utterance **"open mummy chart"** (chat -> chart) arrived 28 s later
   and went down the generic `open_app` path.
2. **18 x `[UIA] list_actionables: exceeded 2500ms budget - returning empty`** in 46 s. WhatsApp's chat list is not readable through UI
   Automation in time (the new WhatsApp is a WebView2 app whose tree is built lazily).
3. **The poll loop multiplies the timeout:** `open_chat` polls 11 times, or **23** while the list has "never shown an item"
   (`wa_open.rs` `POLLS_COLD`), each poll calling `list_items()` which now blocks 2.5 s: up to **~60 s per candidate x 3 candidates**.
   The loop was designed for a UIA call that takes milliseconds.
4. **Meanwhile the hot-mic watchdog interrupted the drill:** `ghost watchdog: turn open with no completion - re-arming listen`
   (11:28:34) then `barge-in/turn-transition: cancelling current turn`; and `[KEEPAWAKE] watchdog: no heartbeat for 10018 ms,
   force-clearing` (11:28:26): the blocked drill thread stopped beating.
5. The next foreground check failed -> **"WhatsApp lost focus, sir. I didn't type anything."**

### 2.2 Why it "doesn't learn" (confirmed in code)
* `wa_alias_<name>` and `wa_phone_<name>` are read (`learned_aliases`, `learned_phone`, `ghost_drill.rs:107,122`) but **nothing in the
  code ever writes them** except you dictating "remember that wa phone mummy is ...". After a *successful* open NEXUS knows the exact
  chat name it matched (`matched`) and throws it away.
* No success/failure memory per method: it tries the same fragile UIA path first, every time.
* `stt_learning: loaded 0 corrections`: mishearings like chart/chat and "start with"/"chat with" are never learned.
* No directory of contacts exists (the optional WhatsApp bridge that could list chats is OFF and was offline in the diagnostics).
* If not found it tells you to dictate "remember that wa alias ...": the burden is on the user.

### 2.3 Target design: a contact directory that learns, and a ladder of methods with budgets

**Contact book** (local, sealed memory store; nothing leaves the PC):
`{ spoken_forms[], display_name, phone?, last_ok_method, ok_count, fail_count, last_used }`, filled automatically from:
1. every **successful open** (the matched chat name becomes an alias for what you said: "mummy" -> "Mom <3");
2. the **read-only bridge** `list_chats` when it is connected (names + numbers in one step; opt-in, already built, default off);
3. what you say ("mummy's number is ...", or answering "what is she saved as?");
4. the **disambiguation answer** ("Mummy or Mummy Home?") through the pending-question mechanism of doc 124.

**Open ladder, each rung time-boxed, whole attempt <= ~6 s; the first *verified* rung wins and is remembered per contact:**
1. **Deep link by number** `whatsapp://send?phone=` (instant, no UI) when the number is known.
2. **Keyboard only:** Ctrl+F, type the name, wait ~600 ms, Down, Enter: needs no UI Automation at all.
3. **UIA chat list**, only if a 300 ms probe returned items; otherwise UIA is skipped for WhatsApp for the rest of the session
   (circuit-breaker) so the 2.5 s timeouts stop stacking. Poll limits become **time-based**, not count-based.
4. **Local OCR of the chat header** (Windows OCR, free, on-device) to **verify** that the right chat is open; a screenshot to a cloud
   vision model only as a last resort and only with your consent.
If a rung is verified, record `last_ok_method`; next time start there.

**Don't be interrupted:** while a drill runs, the hot-mic relisten watchdog and a new utterance must **queue** (existing FIFO) instead of
cancelling it, unless the words are "stop/cancel"; the drill thread must call `keep_awake::heartbeat()`.

**Learn the words:** accept "start/chart/chats with <name> (in whatsapp)" as "chat with"; bias the speech recogniser with saved contact
names; when a command fails and the next utterance within ~20 s succeeds with similar text, store the correction.

**Ask once, remember:** unknown name -> "I can't find 'mummy'. What is she saved as?" with the mic open (doc 124 rule 1); the answer is
stored as an alias.

**Honest and quick failure:** at most ~8 s, then one sentence saying what failed and the single next step.

### 2.3a General principle ("not a blind agent")
Every ghost action records `(app, action, method, ok, ms)`. The next time the same action runs, the method with the best recent record
goes first, and a method that failed 3 times in a row is skipped for a while. Start with WhatsApp and app-launching; extend later.

### 2.4 Phases (Phase W)

| # | Change | Gate | Effort |
|---|---|---|---|
| W0 | `[WA] rung=... ok=... ms=...` logging; reproduce today's failure in a test with a fake slow UIA | failing test = the 57 s case | 0.5 day |
| W1 | Time-based polling + UIA circuit-breaker + total budget; drill not cancellable by relisten; drill heartbeats | the case above fails in <= 8 s with an honest line instead of 57 s | 1 day |
| W2 | Keyboard-only rung + OCR header verification | "chat with mummy" opens the right chat 10/10 on this PC without UIA | 1.5 days |
| W3 | Contact book + automatic learning from successes + deep-link rung | after one success the 2nd run skips discovery (<= 3 s) | 1 day |
| W4 | Word learning: "start/chart with", contact-name vocabulary bias, correction store | the two mis-hearings above resolve with no user action | 1 day |
| W5 | Ask-once flow (needs doc 124 Q2) + bridge directory import (opt-in) | unknown contact -> asked once -> remembered | 1 day |

Order: **W1 -> W2 -> W3** first (they make the feature work), W4/W5 after.

---

## 3. Combined order across plans 123 / 124 / 125

1. **R1 + R2** (instant acks come back), **Q1** (stale-confirm safety), **R3** (Edge crate test).
2. **W1-W3** (WhatsApp works and learns), **R4, R7, R8, R6**.
3. **P1-P3 of doc 123** (boot hello), **Q2-Q3 of doc 124** (mic opens on every question + memory), shutdown flow, rest.

## 4. Decisions needed
1. While Edge is broken, play the **local-voice** cached acks instantly (different speaker from the cloud voice) rather than waiting for the
   cloud voice. Recommended: yes; swap to the cloud voice once Edge really works.
2. WhatsApp contact directory: allow the **read-only bridge** to import names/numbers (opt-in)? Or learn only from what you say and what works?
3. Verify the open chat with **local OCR** of the header (on-device) — OK? Cloud screenshot only if you allow it.
4. When a contact is unknown, should NEXUS ask "what is she saved as?" (recommended) or just say it failed?

## 5. Not verified
Everything in 1.2 items 5 is hypothesis until R5 logs exist (the 6-20 s could also be Kokoro cold-load or antivirus); the Edge fix
(R3) is untested; why UIA cannot see WhatsApp's list is inferred from the timeouts, not proven; nothing was run live.

---

## 6. Is this the best method? A more deterministic alternative (RECOMMENDED over sections 1.3 / 2.3)

Sections 1.3 and 2.3 **tune** the existing path (Edge TTS, then Kokoro, UI Automation, chat-list scraping). That improves things but
keeps their failure modes. A literal 100 % does not exist for anything that depends on a third-party endpoint, a speech recogniser or
another app's UI. What *can* be made deterministic is to **remove the unreliable step from the hot path**.

### 6.1 Voice: no synthesis in front of the user
**Measured on this laptop (16 % battery, read-only test with Windows' built-in offline speech, System.Speech/SAPI):**
"Ok sir." 22 ms warm (442 ms first call), "New tab sir." 10-20 ms, a 9-word line 43 ms. The log shows the current path takes 6-20 s.
Five offline Windows voices are installed (David, Zira, Mark, ...). The app does not use them today.

| Layer | What it does | Latency | Guarantee |
|---|---|---|---|
| 1. **Pre-rendered files** for every fixed line ("Ok sir.", "New tab sir.", "Hand mode on, sir.", the 3 greetings, "Good night, sir." ...), rendered **once** by the best available engine (Edge when healthy, else Kokoro) and kept on disk **in your chosen persona voice** | play a file | **< 50 ms** | **deterministic**: nothing is synthesised at runtime |
| 2. **Dynamic replies** (cloud answers, names, numbers) | Edge if its circuit-breaker says healthy, else warm Kokoro | ~1-2 s | best effort |
| 3. **Safety net:** if nothing audible within ~1.5 s, speak through the **Windows built-in voice** | SAPI | **10-45 ms** | **never silent, never 6-20 s late** (different speaker; used only when 1-2 are late) |

The fixes R1-R4 and R8 of section 1.3 still apply (circuit-breaker, correct staleness test, no cache stampede, watchdog). R6 becomes the
core (persisted fixed lines) and R5 shrinks to "log the timings". Trade-off to accept: the safety-net voice sounds different from the persona.

### 6.2 WhatsApp: open by NUMBER, not by scraping
UI scraping (Ctrl+F, chat-list items, clicking) can never be 100 %: the log shows WhatsApp's chat list is not readable through UI Automation in
time. The one documented, deterministic way to open a specific person's chat is the **phone-number link** (`whatsapp://send?phone=<number>`,
the same family as WhatsApp's own "click to chat" `wa.me/<number>` links). It needs no search box, no UI Automation, no focus tricks. The app
already contains `deep_link()` (`wa_open.rs`) but the number is never known unless you dictate it.

**How NEXUS gets the numbers without you dictating them one by one (all official, on-device):**
1. **One-time contacts import** from a `.vcf` file (export Contacts from the phone or from Google Contacts; one tap): NEXUS reads names + numbers
   into its local sealed contact book. (Google Contacts over OAuth is the alternative, but Google is "not connected" in your diagnostics.)
2. **One question per alias:** "mummy" is saved as "Mom"? NEXUS asks once with the mic open ("Do you mean Mom?"), stores the alias.
3. **Learn from use:** a chat opened successfully by the old rungs records the display name as an alias.
4. The unofficial bridge stays OFF (ban risk, it is also offline).

After that, "chat with mummy" = **look up number -> open link -> verify the header with on-device OCR** (<= ~3 s). The UI-driven ladder of
section 2.3 stays only as the fallback for contacts that have no number.

**What still cannot be guaranteed:** the speech recogniser mis-hearing the name (mitigated: contact-name vocabulary bias, learned corrections,
ask-back), a number that is not on WhatsApp, and WhatsApp itself not installed/updated. In those cases NEXUS says exactly which one, within ~8 s.

### 6.3 Comparison
| Option | Reliability | Effort | Downside |
|---|---|---|---|
| A. Tune what exists (sections 1.3 / 2.3) | good, not guaranteed | ~8-9 days | still depends on Edge's unofficial endpoint and WhatsApp UI scraping |
| **B. Remove the fragile steps (6.1 + 6.2)** | **fixed lines and number-based WhatsApp are deterministic** | ~7-8 days (B reuses most of A's small fixes) | one-time contacts import; the safety-net voice differs from the persona |
| C. Switch the cloud voice to a paid/keyed provider (e.g. Azure Speech) | high for dynamic replies | 1 day + key | needs an API key/billing; does not fix local latency or WhatsApp |
| D. WhatsApp official Business API | high | weeks | sends from a business number, cannot open *your* chats in the desktop app: wrong tool |

**Recommendation: B**, built in this order: R1+R2 (+ the Edge crate test) -> persisted fixed lines + safety-net voice -> WhatsApp number
deep link + contacts import + header verification -> contact book learning -> the remaining items of 123/124.

### 6.4 Decisions for B
1. Is it acceptable that if a dynamic line is late (> ~1.5 s) it is spoken once in a Windows voice rather than waiting?
2. May NEXUS import a contacts `.vcf` (stays on this PC, stored in the sealed memory)?
3. Which voice should the fixed lines use: the equipped persona voice (recommended)?
