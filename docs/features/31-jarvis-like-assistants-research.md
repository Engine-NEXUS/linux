# JARVIS-like assistants research (steal list)

*Date: 2026-09-04*
*Status: research only. No NEXUS code touched.*
*Note: live search/fetch failed this run. Claims below from repo knowledge + project docs already in tree. Verify URLs before quoting externally.*

Fictional JARVIS traits map to real subsystems: wake -> listen -> understand -> act -> speak -> show. NEXUS already owns each slot. Goal here: similar projects, patterns worth copying, failures worth avoiding.

---

## 1. Fictional JARVIS decomposed

| Trait | Real subsystem | NEXUS slot |
|---|---|---|
| "Yes sir" voice | TTS + persona | `src-tauri/src/tts.rs`, Fish Audio / Web Speech fallback |
| Always listening | KWS on mic stream | `src-tauri/src/wakeword_oww.rs`, cpal -> 16k mono -> 80ms chunks |
| Understands orders | Intent routing | deterministic parser first, ONNX NLU fallback |
| Controls machines | Command execution | `src-tauri/src/command_executor.rs`, `app_registry` |
| Shows results | Overlay UI | `src-tauri/src/dyn_windows.rs`, `src-tauri/src/window_manager.rs` |
| Summons by voice key | Hotkey + wake | `src-tauri/src/hotkey.rs`, `CommandOrControl+Space` |
| Silent in meetings | Privacy gate | `meeting_detect`, TTS mute + 300ms post-TTS gate |
| Survives reboot | Autostart + tray | Scheduled Task Windows, LaunchAgent macOS/Linux, `src-tauri/src/tray.rs` |

Lesson: no single "JARVIS tech". Composition of narrow parts. Copy parts, not myth.

---

## 2. Open-source voice assistants (copy pipeline patterns)

### Leon AI
Modular Python bridge: hotword -> STT -> NLU -> skills -> TTS.
Steal: skill package layout (one folder per intent, manifest + runner).
NEXUS fit: map each `command_executor` action to skill folder later.
ponytail: ceiling = local file skills. Upgrade = signed skill store when sharing needed.

### OpenVoiceOS / Mycroft
Offline-first: Precise wake -> VAD -> STT -> Adapt intent -> Converse skill -> TTS.
Steal: fallback chain per stage (local first, cloud second), dialog context object across turns.
NEXUS fit: already local-first routing. Add dialog context already in `network.rs` events (`dialog_state`).
Avoid: plugin sprawl without sandbox.

### Home Assistant Wyoming + openWakeWord
Satellite pattern: tiny mic device streams frames, server runs KWS/STT, events back on socket.
Steal: audio framing (16k mono chunks, RMS health logs), multi-satellite IDs.
NEXUS fit: `wakeword_oww.rs` already OWW. Add per-device ID when second mic source appears.
Avoid: networked raw audio without consent UX.

### openWakeWord itself
`melspectrogram.onnx` -> `embedding_model.onnx` -> classifier, 80ms sliding window.
Steal: shared embedding across wake + command classifiers (Tier 3 pattern already in NEXUS).
NEXUS fit: keep. Threshold 0.35, AGC target RMS 0.03, silence gate 0.0005, 500ms confirmation.
Avoid: retraining without negative silence samples (causes silence false-positive).

### Whisper.cpp / faster-whisper, Silero VAD, Sherpa-ONNX, Piper / Kokoro / Coqui
Local speech stack proven combos.
Steal: VAD gates STT (no empty transcribes), STT lazy-load, TTS lazy-load.
NEXUS fit: already done — Silero preloaded, faster-whisper lazy port 39217, Kokoro lazy (~350 MB saved idle).
Avoid: holding all three resident. Keep lazy.

---

## 3. "JARVIS controls computer" projects (copy harness, not hype)

### Open Interpreter / 01 Light
LLM emits Python/shell, local runner executes, screenshot/text returns, loop.
Steal: code-as-action (not fixed click JSON), human approval before destructive run, full transcript log.
NEXUS fit: extend `command_executor.rs` with allowlist + confirm-before-write.
Avoid: free shell without gates. Never ship.

### Self-Operating Computer Framework (OthersideAI fork tree)
Screenshot -> vision model -> pyautogui click/type -> new screenshot.
Steal: coordinate normalization 0-1000, settle wait 200-500ms, verify-by-next-screenshot.
NEXUS fit: pairs with `docs/features/30-computer-use-vision-action-loop.md`.
Avoid: raw pixel coords. Always normalize.

### OSWorld / ScreenSpot-Pro (benchmarks, not products)
Measure task success + grounding accuracy.
Steal: eval harness (scripted tasks, screenshot diff, retry count) before claiming speed.
NEXUS fit: build 10-task local eval (open app, type, submit) before adding vision loop.
Avoid: vendor numbers as promises. Re-measure locally.

### Raycast / PowerToys Run / Alfred / Ueli
Launcher pattern: hotkey -> fuzzy search -> action -> dismiss.
Steal: instant show (<50ms), fuzzy `app_registry`, single-window, keyboard-first.
NEXUS fit: hotkey already state-dependent (sidebar visible -> close only). Add fuzzy ranking already in Worker.
Avoid: second launcher. NEXUS orb IS launcher.

### Cluely / interview-copilot overlays
Undetectable overlay tricks: transparent always-on-top, `WDA_EXCLUDEFROMCAPTURE`, click-through toggle.
Steal: affinity flag already on NEXUS sidebar (value 17). Native show/hide (WebView2 caches frames).
NEXUS fit: keep in `dyn_windows.rs`. Extend to orb only if stealth explicitly requested.
Avoid: hiding from screen share without user consent. Default visible.

---

## 4. Wearables + ambient (copy UX, skip hardware)

| Product | Pattern | Steal | Avoid |
|---|---|---|---|
| Limitless / Omi / Friend / Plaud | Pendant records meetings, transcribes, mutes on consent | Meeting detection + TTS suppression already in NEXUS. Add visible recording indicator. | Always-on cloud upload default. |
| Ray-Ban Meta + Meta AI | "Hey Meta" + camera button + photo context | Multimodal context attach (screenshot/frame with query). | Closed API. Copy pattern, not SDK. |
| Rabbit R1 / Humane Ai Pin | Thin client, cloud brain | Failure lesson: latency kills. Keep wake + STT + TTS local fallback. | Cloud-only action path. |
| Siri / Copilot / Alexa+ | OS intents, routines, device graph | Intent schema + app registry + routines ("good morning" macro). | Store lock-in. Keep intents local. |

---

## 5. NEXUS steal list (ordered, cheapest first)

1. Skill folders. One intent = one folder + manifest. Fits `command_executor.rs`.
2. Dialog context. Keep `dialog_state` across turns. Already in events. Use it.
3. Eval harness. 10 scripted desktop tasks. Measure before vision work.
4. Screenshot tool. Read-only first. Use when text intent fails.
5. Grounding cache. Label -> x, y + layout hash. Reuse until hash shifts.
6. Confirm-before-write. Gate sends/deletes/submits/auth. Log screenshot per action.
7. Recording indicator. Orb state visible during mic capture. Privacy trust.
8. Routines. Named macro = list of existing intents. No new engine.

Lazier alternative: items 1-3 + 7 only. Skip vision loop until text path proven insufficient.

---

## 6. Failure modes from history

| Failure | Example | NEXUS guard |
|---|---|---|
| Cloud latency kills demo | R1 / Pin roundtrips | Local wake + STT + TTS fallback chain |
| Mic lock deadlock | Intel SST dual capture | cpal owns mic. Frontend `pause_wakeword` before `getUserMedia`. (`frontend/src/main.tsx`) |
| Overlay blocks clicks | Wayland ignores click-through | Render loading inside orb. No extra 80x80 window. |
| Global hotkey dead | Wayland blocks `XGrabKey` | Linux: DE keybind -> `nexus --wake` + single-instance catch. Plugin disabled on Linux. |
| Silent mic | PipeWire dummy sink | Probe all devices 5s RMS. `pavucontrol` fallback doc. |
| Stale webview shows old page | WebView2 profile cache | Windows pre-builder EBWebView cleanup (`lib.rs`). Native show/hide (`window_manager.rs`). |
| RAM blowout | 4 WebViews resident | `dyn_windows.rs` create-on-demand, `destroy_window` on close. |
| Double ack speech | Triple wake events | Single `__NEXUS_WAKE__` path. No parallel emit. |

---

## 7. Glossary

- KWS: keyword spotting. Detects acoustic pattern, no transcription.
- VAD: voice activity detection. Speech vs noise gate.
- AGC: automatic gain control. Normalize quiet/loud to same model input.
- Satellite: mic-only node streaming to brain server.
- Grounding: phrase -> screen `x,y`.
- Settle wait: pause after action for UI redraw before next screenshot.
- Barge-in: user interrupts TTS. Stop speech, resume listen.

---

## 8. Deliberate simplifications

- No per-project URLs listed (fetch blocked). Add links when network verified.
- No new deps proposed. Uses installed stack: `tract-onnx`, `cpal`, faster-whisper, Silero, Kokoro.
- ponytail: ceiling = single-user desktop. Upgrade path = multi-device satellite IDs + signed skills when sharing needed.
