# Ghost Mode — Full Program Consolidation (2026-09-28)

One document for the entire arc: Phase 0 → 4, entry split, takeover
deletion, Phases A–D, vision, mic-chain audits, Groq-key repair,
settings-on-command, window architecture, settings needs, the
cursor-vs-keyboard verdict, and the hot-mic loop repair (live bug:
mic died after the first ghost command).

Prior art lives alongside: `01-clicky-and-grounding.md`,
`02-costs-and-limits.md`, `03-irregularities-and-fixes-timeline.md`,
`vision-grounding-plan-2026-09-27.md`, plus
`docs/features/70-ghost-mode-complete.md` and
`docs/features/ghost-mode-architecture-reference.md`.

---

## 1. Program history (condensed — full detail in AGENTS.md)

**Phase 0 (leash before hands).** `src-tauri/src/ghost.rs`: session
machine (Idle → Active → Yielded), Esc panic button (dynamic
register/unregister, never global), ring events, stage ring UI.
Zero AI motion — prove containment first.

**Phase 1 (keyboard ghost).** `live/commands/launcher.rs`: Win-search
open + focus verify; WhatsApp drill (open → search → type, send stays
confirm-gated); voice-stop via `live_cancel`. Primitives atomic —
no key tracker needed.

**Phase 2 (mouse ghost).** `live/commands/mouse.rs`: eased glide,
click/double/scroll/drag/restore + UIA-first resolver
(exact → starts-with → contains; never password fields) + focus
verify before AND after. Vision deferred until measured need.

**Phase 3 (overlap engine).** Drill-depth flag + follow-up queue
(cap 5, drop-oldest, drain-cap 10) + stop-word intercept. Stop aborts
spoken + unrouted; rest drains in order after clean runs only.

**Phase 4 (hardening).** Calibration suite, refusal battery,
exclusive-fullscreen auto-pause (`foreground_blocked`).

**Entry split.** "Ghost mode" enters cursor control
(`EnterGhostControl`); bare-mode phrases stripped from Ghostwriter
dictation entry after a live collision sent users to the wrong room.

**Waves / voice-entry / instant-response / messaging + trace.**
Orb pinch-to-waveform on session start (220 ms pinch, Lottie palette);
initial `ghost:ring` emit; ghost hot-mic loop (re-listen after every
turn); silent-miss anti-nag (3 quiet re-listens, then one nag + park);
follow-up listen starts Rust capture; in-session OpenApp routes to
ghost runners; WhatsApp messaging via desktop drill; `debug_trace`
p0–p4 taps.

**Takeover detection DELETED (2026-09-27, user directive — final).**
After three live misfires, `decide_takeover`, the observe_cursor
judgment, and `abort_task` were removed. Mouse use is ALWAYS free.
**Esc = the cancel button** (armed at session start). Other exits:
"exit ghost mode" voice, stage hide/kill. Entry narration carries
"Esc cancels any time". Ring rides commanded glides only.

**Phases A–D.** A: OS-keychain API keys, PII filter, speaker
verification, health dashboard, settings export/import. B: 3-tier
memory, streaming TTS, emotion prosody, parallel Worker batches,
crash-safe checkpoints. C: vision grounding fallback, proactive
diary, wire protocol v1, multi-voice Piper, localhost webhooks.
D: intent specs (`intents.yaml`), edge-case miner. Cross-phase audit
fixed 8 real issues.

**Vision v2 + speed mode.** Axis-grid overlay, Groq → Gemini fallback,
Pacific-day quota counters (14,400/500 RPD), `visionProvider` /
`visionRace` settings, spoken limit notices, quota UI. Keys stay in
OS keychain, never Cloudflare.

---

## 2. Mic command chain during ghost (how voice actually flows)

Verified against current code (2026-09-28):

```
wake word (Rust cpal + ONNX, KWS skipped during capture)
  → start_stt_capture (wakeword_oww.rs) → Groq/local transcript
  → frontend "stt:transcript" (main.tsx:398)
  → processTranscript (recorder.ts:642)
    → local branches (greeting, open_app, whatsapp_chat, …) → execute_command
    → else processViaOrchestrator → orchestrator_process (Rust)
      → run_ghost_open / run_ghost_message / drills / Worker / …
  → turn end → reset() + (if wired) maybeGhostRelisten
    → triggerFollowupListen (main.tsx:37) → __NEXUS_WAKE__ + start_stt_capture
```

Load-bearing facts:

- `startListening` is UI-only while Rust capture is active — no
  `getUserMedia`, no browser mic fight (`main.tsx:116`).
- KWS suppression (meeting/TTS-mute) gates *detection only*; direct
  captures bypass it. A "detection suppressed" log line does NOT mean
  the mic is dead.
- `reset()` (`store/assistant.ts:123`) sets
  state/speakSeq/audioVolume/ttsActive/awaitingInput — it does **not**
  clear `ghostActive`. A session can therefore be Active-but-deaf:
  alive in Rust, nobody listening in the frontend. (Two orchestrator
  comments claimed otherwise — corrected in §8.)
- Three turn-functions share duplicated branch code:
  `processTranscript` (live Rust path), `finishCapture` + `finishCaptureFromVad`
  (browser-mic/VAD path, dormant while Rust capture runs).
  `abortCapture` is explicit user cancel — never a turn end.

---

## 3. Mic-audit bugs A/B/C — status

- **Bug A (wake-prefixed phrases parse to `None`).** Temp test proved
  `"nexus open chrome"`, `"hey nexus open chrome"`, `"ok nexus stop"`,
  `"nexus ghost mode"`, etc. all return `None` — `strip_leading_filler`
  (`intent_parser.rs:3502`) knows no "nexus" prefix, and during hot-mic
  capture KWS is skipped so the wake word lands verbatim in the
  transcript. Temp test removed. **Fix still open** (retry-parse with
  wake-prefix strip on `None`; must add bare "command center" to
  `is_settings_command` or "nexus command center" regresses).
- **Bug B (local branches skip relisten).** This document's §8 —
  **fixed 2026-09-28**.
- **Bug C (ghost runners unreachable from voice).** Frontend
  local-executes `open_app`/`whatsapp_chat` before the orchestrator,
  so `run_ghost_open` / `run_ghost_message` (`orchestrator.rs:705+`)
  are unreachable from the live voice path; `execute_command`
  (`command_executor.rs:99`) has no ghost awareness. App still opens,
  but with no ghost narration/safety/focus framing. **Fix still open.**

---

## 4. Groq-key false nudge (fixed 2026-09-28)

**Symptom.** Returning user with a valid Groq key got "add a Groq or
Gemini key in Settings" + settings sidebar popup on every ghost entry.

**Root cause.** `ghost_enter` (`ghost.rs:345`) was the ONLY key check
reading keychain-only (`auth_vault::get_api_key(...).is_none()`).
Every other consumer — STT (`stt.rs:46`), vision gate
(`live/commands/mouse.rs:341`), hotkey, prewarm — reads
keychain-first with `settings.json` fallback. A key living in
`settings.json` but missing from the keychain (boot migration
silent-fail in `auth_vault.rs:171`, keychain write denied, manual
edit — migration also never strips disk, so the split state persists)
worked everywhere except ghost entry.

**Fix.** `vision_keys_present(app)` helper — same fallback-aware read
path as the vision gate — plus self-heal (settings-only key written
forward into the keychain) and pure `needs_vision_key_nudge()`
decision with truth-table test. (Nudge itself later deleted per §5;
helpers retained for diagnostics + future Ghost tab.)

**Verify.** Rust 643/643 serial, clippy zero in touched files.

---

## 5. Settings-on-command + pure-init entry (user directive 2026-09-28)

**Rule.** The settings sidebar opens ONLY on explicit user command:
voice "open settings", tray menu, `Ctrl+Shift+S`, `--settings` flag,
`nexus://settings` deep link. Audit confirmed ghost entry was the
sole auto-popup — all others are user-initiated.

**`ghost_enter` is pure init.** No sidebar, no extra speech. The
orchestrator's single entry line is the only speech. Key status is a
debug log; keyless users are guided verbally at the click-time vision
gate (`mouse.rs:343` error text now names Settings → Accounts).
That error path already speaks (`mouse.rs:453`), so guidance arrives
with no popup.

---

## 6. Window architecture research (2026-09-28)

**Answer to "are we using one window for everything": no — 8
separate windows, and `tauri.conf.json` declares zero of them**
(`"windows": []`). Even the orb is created in code (`lib.rs:391`).
All windows are on-demand create + destroy (each WebView2 tree
≈250 MB; destroy-on-close is the RAM strategy,
`dyn_windows.rs:259`):

| Window | Geometry | Role |
|---|---|---|
| `main` | 200px orb, transparent, always-on-top, no-focus | Voice UI, wakeup visual |
| `setup` | 520×680 centered, decorated | First-run wizard |
| `settings-sidebar` | 520×1000 LEFT edge, non-activating liquid glass | **Live settings UI** — Display / Audio / Accounts / Connections |
| `sidebar` | 400×1000 | Response / confirm cards |
| `architect-sidebar` / `pr-list-sidebar` | 900px / 500px | Mapper + PR list |
| `loading-indicator` | 80×80 top-right, permanently click-through | Lottie spinner (`commands.rs:1457`) |
| `stage` | Fullscreen transparent overlay | Step-1 shell: **empty + hidden, parallel-run** (`stage.rs:1`) |

Wakeup = Rust wake engine → STT capture → orb states in `main`.
Loading = `loading-indicator` + orb animation. Neither lives in the
stage yet. The single-stage plan (`docs/features/63`) moves orb →
loading → panels into the stage one by one as DOM layers at exact
legacy pixel positions (`stage/geometry.ts`), deleting one
`PENDING_*` system per step. Only ghost sessions call `stage_show`
today.

**Dead code:** the OLD centered settings window
(`WindowConfig::settings()`, 600×720) is unreachable —
`open_settings_window` (`commands.rs:1504`) is not registered in
`lib.rs` and has zero callers. Old `SettingsApp.tsx` +
`settings.html` still build but can never open. Recommend deletion.

---

## 7. Settings needs (derived from `NexusSettings` + consumers)

Edited today: orb position/size, TTS voice/volume/emotion,
local-STT-only, speaker verification, Moonshine model,
Google/GitHub OAuth, Groq/Gemini/Cerebras keys, vision
provider + race + quota, vault tokens, Telegram chat id, health.

**No editor (settings.json-by-hand only):** `verify_wake`,
`mic_keep_alive`, `autostart`, `hotkey`, `autoHideDelay`,
`wakePhrase`, `wakeSensitivity`, `meetingModeAuto`,
`suppressTtsInMeetings`, `serverUrl`, `userId`, `deviceId`.
No Ghost section (Esc-as-cancel undiscoverable; vision controls sit
under Accounts).

**Recommended shape:** Display / Audio / Accounts / Connections +
new **Ghost tab** (Esc hint, entry note, vision provider/race/quota
moved here, confirm-gate toggles if ever configurable) +
**Advanced tab** (verify_wake, mic_keep_alive, hotkey, autostart).

---

## 8. Hot-mic loop repair (fixed 2026-09-28 — the live "first command only" bug)

**Live repro.** `ghost mode.` → session ACTIVE → two quiet
re-listens → `open WhatsApp` → `QUIET [░░░░]` forever.

**Root cause.** `maybeGhostRelisten()` had exactly **2 emitters**,
both in `net/orchestrator.ts` (`finishSpokenResult` :155, `done`
:298). `open WhatsApp` parses to `whatsapp_chat` →
`isLocalExecutableIntent` (`recorder.ts:81`) → local-execute branch
(`recorder.ts:790`): `execute_command` → speak → `reset()` → return.
**Zero relisten calls existed anywhere in `recorder.ts`**
(grep-verified). Mic never reopened; `reset()` preserves
`ghostActive`, so the session stayed Active-but-deaf.

**Same kill-chain, same fix — full site inventory:**

| Site | Kind | Fix |
|---|---|---|
| `recorder.ts` processTranscript branches (greeting, open_app/chat, settings, architect, need_more_info, fallback, …) | turn-end | `endTurn()` |
| `recorder.ts` finishCapture / finishCaptureFromVad duplicates + watchdogs | turn-end | `endTurn()` |
| `orchestrator.ts` `error` path | turn-end | `endGhostTurn()` |
| `orchestrator.ts` confirm-decline (`handled_locally`) | turn-end | `endGhostTurn()` |
| `main.tsx` Tier-3 listener (2 sites) | turn-end | `endGhostTurn()` |
| `App.tsx` stage:notice + 60s speaking failsafe | turn-end | `endGhostTurn()` |
| `wsBridge.ts` legacy result/done/error (6 sites) | turn-end | `endGhostTurn()` |
| `recorder.ts:675` silent-park after cap | park by design | **left alone** (+ comment why) |
| `abortCapture`, App 8s no-input timeout, first-run greeting | cancel/timeout | **left alone** |

**Design.** `endGhostTurn()` (`net/ghostHotMic.ts`): relisten
BEFORE reset (gate reads ghostActive; order must not depend on
reset preserving it). Outside ghost it is exactly `reset()` —
`maybeGhostRelisten` no-ops without IPC (test-pinned). Two stale
comments claiming "reset clears ghostActive" corrected.

**Verify.** `tsc` clean; vitest **35/35** (33 + 2 new `endGhostTurn`
tests: outside-ghost = reset with zero IPC; in-ghost = relisten
fires once, state idle, `ghostActive` survives). Rebuild + run fresh
binary to take effect.

---

## 9. Cursor-only vs keyboard-first — verdict (2026-09-28)

**Recommendation: keep the hybrid — keyboard-first, cursor where
keyboard cannot reach.** Going cursor-only would make ghost slower,
costlier, and less reliable. Detail:

**Keyboard stays primary.** Win → type → Enter is pixel-free
(DPI/monitor-independent), ~3s, zero grounding cost, with
focus-verify and no silent lies (`launcher.rs:13`). Vision costs
quota every miss (Groq 14.4k/day, Gemini 500/day, race mode 2×) —
on a ₹200/mo budget vision must stay a fallback. Grounding tops out
at Pro 61.6 / OSWorld 42.5 (coin-flip on hard UI); UIA bounds are
exact and free. Failure mode favors keyboard: a wrong keystroke lands
in a visible text field (undoable); a wrong-pixel click lands on
whatever is there (Delete, Send, X).

**Cursor stays (not deleted).** UIA-blind custom/canvas UI is the
case vision was built for (`vision.rs:609`, axis-grid + fallback +
race). No keyboard equivalent exists for hover menus, drag, scroll,
sliders, "click there" pointing. UIA click IS the cursor path done
right: exact bounds → eased glide → click → verify (`mouse.rs:303`).

**Doctrine (already in code — formalize, don't replace):**
UIA bounds → keyboard sequence → vision fallback. Cursor-only inverts
this into the worst option first.

**Gaps before expanding cursor use:** UIPI elevation (clicks into
elevated windows fail silently — still open), multi-monitor/DPI
coord verification, Phase-4 calibration re-run. Cleanup:
`mouse.rs:4` header still references the deleted takeover detector.

---

## 10. Open follow-ups

1. Bug A: wake-prefix strip retry-parse (`intent_parser.rs:3502`).
2. Bug C: route in-session `open_app`/`whatsapp_chat` to ghost
   runners instead of frontend local-execute.
3. Delete dead centered settings window + `settings.html` input.
4. Ghost + Advanced settings tabs (§7).
5. UIPI elevation detection; calibration re-run; `mouse.rs:4`
   stale comment.
