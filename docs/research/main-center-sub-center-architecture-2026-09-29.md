# Main Center + Per-Action Sub-Centers — Research (2026-09-29)

## 1. What the user asked

> Every single action gets its own center (a decision tree). All centers
> connect to one Main Center. The Main Center owns the UI (orb, loading,
> waves, ghost mode, all animation), owns STT and TTS, and judges command
> validity — telling the user clearly what happened instead of a bare
> "on it sir" / "ok sir" / "can you repeat". Industry standard. Any new
> action I create becomes a sub-center wired to the Main Center.

## 2. Industry standards mapped

### 2.1 Alexa Skills Kit — Dialog Model (the validity standard)

Amazon's production pattern for exactly our "is it valid, notify the user"
problem. Three mechanisms, per intent:

| Alexa mechanism | Purpose | Our equivalent (missing today) |
|---|---|---|
| **Required slots + elicitation prompts** | Intent can't run until slots filled; Alexa asks ("Which city?") | `NeedMoreInfo` exists for ONE case (WhatsApp body). No other intent declares required slots. |
| **Slot validation rules + failure prompts** | Value checked against rules (`IsInSet`, entity match); failure speaks a specific correction prompt ("I don't recognize X as a planet") | Nothing. `"open brave"` misheard as `"open brief"` sails through; `"close tab 5"` would have taskkilled an app named "tab 5". |
| **Intent confirmation** | Whole action repeated back + yes/no ("Saving trip from X to Y, OK?") | Only destructive GitHub/MCP flows confirm, via ad-hoc code paths. |
| **dialogState machine** | `STARTED → IN_PROGRESS → COMPLETED`; ambiguous "yes" routed by dialog position | Our `dialog_context` exists but only WhatsApp uses it. |

Key Alexa rule we adopt verbatim: **confirmations sparingly** — routine actions get a *repeat-back ack* ("Opening a new tab, sir"), never a yes/no gate; only destructive/ambiguous actions gate.

### 2.2 n8n Orchestrator-Worker (already our doc 12)

One planner, specialist workers, `{ok, data, error}` contracts, run summary,
retry/skip/fallback. Our `command_center.rs` implements the execution half
(parallel batches, checkpoints, resume). What's missing: the planner outputs
*routing JSON to named sub-centers* — today routing is a flat Rust `match`.

### 2.3 UI Director (game-engine / stage-manager pattern)

One owner for all visuals; every visual transition is a single
`UiDirective` through one function with transition guards. Today's failure
mode (wake animation killed by an Avatar phase-machine side effect, 2026-09-29)
is precisely what this pattern eliminates: no component may pause, replace,
or hide another center's visual except through the director.

## 3. Current-state gap analysis (measured, file:line)

### 3.1 No validity gate — garbage in, spoken nonsense out

Live log 2026-09-29 01:05: `transcript = 'Sers.'`, `'Ss. And accept.'`,
`'Dumb.'`, `'right.'` → all routed to cloud chat → Groq answers → spoken
aloud. There is no step that asks "is this a valid command?" — `Unknown`
means "ask the cloud", never "tell the user it wasn't understood".
`missed_intent_logger` records the miss for training but the USER gets a
confident-sounding hallucinated answer.

### 3.2 Acks don't say WHAT was understood

`pick_ack()` returns context-free "On it sir." / "Ok sir." The user cannot
distinguish "new tab opening" from "command queued" from "I heard
something". Alexa's repeat-back rule fixes this: every ack names the action
("Opening a new tab, sir", "Closing tab 5, sir", "Switching to tab 3, sir").

### 3.3 STT/TTS owned by five places

STT lifecycle: `wakeword_oww.rs` (capture), `lazy_stt.rs` (sidecar),
`stt.rs` (transcribe), frontend `recorder.ts`/`vad.ts` (dead chain),
meeting-mute gate (drops chunks with a warn). TTS: `tts.rs`,
`tts_edge.rs`, `tts_piper.rs`, frontend `ttsPlayer.ts`, generation
counter. No single owner can answer "is it safe to listen now?" or
"what should the user hear now?".

### 3.4 Animations owned by three places

Frontend store (`visible`, `state`), Rust emits (`show_loading`,
`ghost:ring`, `assistant:server` state), CSS/Avatar phase machine
(`ghostPhase`, `waveSource`). Any one of them can override the others —
which is how the wake animation died.

### 3.5 Complete action inventory (every future sub-center)

**Parser intents** (`intent_parser.rs` `ParsedIntent`, ~30 variants):
`OpenApp, OpenUrl, CloseApp, WhatsappChat, OpenArchitect, OpenSettings,
Search, AnalyseRepo, AnalysePr, AnalyseLatestPr, CheckBranch,
Media{PlayPause,Next,Previous,Stop}, Greeting, NluResult, GitHubCommand,
OrderFood, SearchProduct, SendWhatsAppMessage, NeedMoreInfo,
EnterGhostwriter, EnterGhostControl, ExitGhostControl, ScreenClick,
ScreenRead, BrowserTab, BrowserCloseTab, Unknown` + NLU live verbs
(`type_text, press_key, press_hotkey, confirm_send, cancel_action,
browser_new_tab, browser_navigate, browser_search, whatsapp_open,
whatsapp_search, focus_app`).

**Executor verbs** (`command_executor.rs`, ~28): app open/close/url,
Spotify/YouTube/Google/GitHub search, timers, screenshot, lock,
browser hotkeys (new/close/next/back), media, greeting.

**Live primitives** (`live/`, 17 commands): keyboard, mouse, WhatsApp
drill, browser, window focus, ghost click/calibrate.

**Ghost session** (`ghost.rs`): enter/exit, watchdog, drill guards,
ring, follow-up queue.

**UI surfaces**: orb (`Avatar.tsx` Lottie + waves), loading window,
stage shell, sidebars (response/settings/architect/PR-list), tray,
confirmation cards.

**Voice I/O**: STT (Groq cloud + Moonshine local + filters), TTS
(Edge cloud + Piper local + cache + barge-in).

## 4. Target architecture

```
                        ┌──────────────────────────────┐
                        │         MAIN CENTER          │
                        │  TaskState machine (+Validating,
                        │   +Confirming phases)        │
                        │  Validity gate (Alexa map)   │
                        │  Decision-tree router        │
                        │  STT director │ TTS director │
                        │  UI director (ALL animation) │
                        │  Confirm gates │ Run summary │
                        └──────┬───────────────────────┘
           ┌────────┬──────────┼──────────┬────────┬─────────┐
           ▼        ▼          ▼          ▼        ▼         ▼
       AppCenter BrowserCenter MediaCenter ... GhostCenter SystemCenter
       (each: validate → execute → receipt → ui-directive)
```

Decision-tree rule: intent → exactly one sub-center; sub-center returns
`{ok, data, error, ui}`; Main Center speaks, animates, summarizes. Adding
an action = adding a sub-center module + one router arm + its dialog
spec (slots, validations, prompts). Nothing else changes.

## 5. Non-goals (explicit)

- No new cloud spend (all policy is deterministic Rust).
- No wake/STT/DSP changes. No model retraining.
- No visual redesign — the director preserves current visuals, it only
  centralizes who may change them.
