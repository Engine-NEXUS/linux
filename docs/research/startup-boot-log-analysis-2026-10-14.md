# Startup Analysis — Boot Log 2026-10-14 07:12–07:13 UTC (read-only, no code changed)

Source: user-pasted `nexus start` log fragment. Method: every claim below is tied to a
log line + the code that emits it. Open hypotheses are labelled as such — nothing is
asserted from memory.

## 1. Reconstructed timeline (UTC)

| Time | Event | Evidence |
|------|-------|----------|
| 07:12:56 | Mic callbacks ~3990–4000, RMS exactly 0.000000 for ~66 s | `SILENT rms=0.0000`, `silent_for~66s` |
| 07:12:56 | Engine warns: Intel SST driver may need restart | `mic has been silent for ~66s (callbacks 1793-4000, max RMS ever seen: 0.073716)` |
| 07:13:00 | Health 4/4 OK; boot-session offer fires | `[HEALTH]`, `[BOOT] greeting: session_offer=true` |
| 07:13:01.03 | Offer Result emitted (full text intact in event) | `orchestrator: emitted Result { ... "Shall I reopen them?" }` |
| 07:13:01.05 | Frontend `stopTts()` runs BEFORE the greeting (routine pre-speak cut — see §4) | `tts: stop requested (generation 1)` + `tts.rs:328` + pre-speak-stop comment |
| 07:13:01.06 | Wake suppressed for greeting duration (by design) | `suppression reason: NEXUS speaking (TTS mute)` |
| 07:13:01.06–01.57 | Greeting "speaks" for ~0.5 s on the Edge path… | `TTS started` → `TTS ended` 511 ms later |
| 07:13:01.68 | …then Edge rejects with **403** → cold-load Kokoro, speak greeting locally | `edge-tts rejected (…403 Forbidden), trying local fallback` + `Initializing session` (ONNX) |
| 07:13:41–44 | Mic stream goes LIVE (rms 0.004 → 0.1379); wake scores stay 0.00–0.05, no fire | `LIVE rms=0.1379`, `model probability=0.022 (below threshold 0.680)` |
| 07:13:45–47 | WhatsApp bridge probe fails (expected — bridge offline) | `whatsapp probe unavailable`, `pairing state: Unavailable` |

## 2. Finding A — NEXUS was deaf for the first ~1–2 min (Intel SST, known issue)

- The capture stream delivered **bit-exact zeros for 66 s** (`rms=0.0000`, callbacks 1793→4000).
  `max_RMS_seen=0.073716` proves the mic *can* work — the driver just stops delivering
  (AGENTS.md: Intel Smart Sound Technology burst-then-silence bug).
- The 5 s silence-recovery poll (`wakeword_oww.rs`) evidently did **not** resurrect the
  stream here — audio only returned at 07:13:41+, ~45 s after the warning. Either the
  recovery restarts kept landing in silent windows, or the stream recovered on its own.
  **Open item:** the log does not show a `try_device_silent` restart line in this window —
  confirm whether recovery fired and failed, or never fired.
- Consequence: **no wake word, no hot-mic, no STT** until 07:13:41. The boot greeting at
  07:13:01 spoke into a room where the user could not answer by voice yet — and the
  session-offer's 8 s mic window (`openPendingOfferWindow`) likely expired in silence.

## 3. Finding B — the greeting changed voices mid-sentence (the "words not understandable" prime suspect)

Sequence from the log:
1. `speak_line` sends the FULL greeting text to the frontend (`orchestrator.rs:6522`;
   the `"you had Bra"` cut is only the **80-char console preview** — the event payload
   is complete, verified in the `emitted Result` line).
2. Frontend starts Edge (Ava) playback; **511 ms later** the turn "ends" and Edge
   returns **403 Forbidden** → engine falls back to a **cold Kokoro load** (~1–2 s per
   `tts_bench.rs`) → the greeting is re-spoken by the **local twin voice**.
3. So the user heard: ~half a second of Ava → cut → a pause → the same sentence
   restarted in a different speaker (Kokoro `af_sky` by default, or the persona twin).

Why 403 on a free endpoint: Edge's unofficial endpoint throttles/blocks on network
shape (proxy, sudden burst at boot, IP reputation) — the code already treats it as
transient (`network flag untouched`, `tts.rs:800`). It is **not** an auth problem.

Secondary suspect for "not understandable": the local twin is a *different speaker*
from the cloud persona by construction (`voice_catalog.rs`: e.g. JARVIS =
`en-GB-RyanNeural` cloud / `bm_george` local — different humans). After a fallback,
**every** subsequent cached phrase is the wrong voice until Edge recovers. Perfect
matching is impossible with two different speakers — see plan §2.

## 4. Finding C — `stop requested (generation 1)` is routine, not a bug

`stop_tts` (`tts.rs:328`) is called by the frontend **before every speak** to cut
prior audio (comment at `tts.rs:331-335`). At boot it cut nothing (generation 1) —
harmless. It is logged because the generation counter makes every cut provable.

## 5. Finding D — autostart path is already "at logon"; readiness is what lags

- Autostart = Windows **Scheduled Task `NEXUS`, trigger AtLogOn, `--background`**
  (`commands.rs:2534`, `lib.rs:470`; old HKCU-Run entry is deleted on every boot).
  So NEXUS *launches* at login by design — but "launch" ≠ "in control":
- Time-to-ready budget after process start (code facts):
  - Wake ONNX + audio capture: in-process, fast — **but deaf until Intel SST delivers
    (66 s+ in this log)**. This dominates everything.
  - STT server (Moonshine): lazy on first transcription (~2 s model load); first
    voice command pays this.
  - Kokoro: lazy on first fallback (~1.7 s); paid during the greeting in this log.
  - NLU server: lazy on first unparseable command.
  - Boot greeting/session offer: fires once health is OK (~07:13:00 here).
  - `STARTUP_GRACE_MS = 1500` (`wakeword_oww.rs:380`) — negligible.
- Ghost/screen control additionally needs a settled interactive desktop (foreground
  lock, UIPI); at the logon instant the shell may still be starting — bids for
  `SetForegroundWindow` can fail in the first seconds.

## 6. What this log does NOT show (do not assume)

- No wake fire, no STT transcript, no ghost turn — so **nothing in this fragment
  diagnoses recognition accuracy**. "Some words are not understandable" cannot be
  attributed to STT from this log; the attributable defect is the §3 voice swap.
- No evidence the greeting audio itself was truncated (only the console preview is).
- WhatsApp `Unavailable` is the expected idle state with no bridge running.
