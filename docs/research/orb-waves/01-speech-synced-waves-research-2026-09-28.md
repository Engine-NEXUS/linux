# Speech-Synced Orb Waves — Research (2026-09-28)

Requirement (user): the wakeup orb must morph into live waves when
ghost mode enables; waves move ONLY with the user's speech (matched
to backend STT), show during NEXUS speech too (TTS), transition
wakeup → waves on entry, revert on Esc, and stay visible for the
whole session. Two LottieFiles references (wakeup animation +
waves) to be adopted via export/MCP and edited to our use case.

## 1. What exists today (audited, not assumed)

- `frontend/public/wakeup.json` (17 KB) + `lottie-web` drive the orb
  (`frontend/src/avatar/Avatar.tsx`). Segments: 171–260 loading
  circles, 261–316 smile arrival, hold frame 300. listening =
  one-shot loading → smile; thinking/speaking = loading loop
  (1.5x / 1.2x); idle = smile arrival → hold.
- Speaking gates on `ttsActive` (event-derived from real playback),
  never on bare `state` — no looping over silence
  (`shouldHoldSpeakingFrame`).
- Ghost waves shipped earlier (orb pinch 220 ms → 3 palette bars
  `#259ed6/#ef4f25/#fbdf38`, reverse on exit via `ghost:session`).
  **Gap found: the bars run a free CSS loop while the session is
  active — constant motion, NOT speech-gated.** This repair replaces
  that with audio drive.
- Esc revert already exists (`ghostPhase` → smile + `applyState` on
  `ghostActive` false). Kept, not rebuilt.
- `audioVolume` in the store is written ONLY by the dormant
  browser-VAD path (`vad.ts:118`) — always 0 under Rust capture.
  There was **no Rust→frontend level event** (console `RMS:` lines
  are log text parsed by `run.ps1`, not events). Added this turn:
  `audio:level` (§3).

## 2. LottieFiles references — acquisition analysis

Supplied links are Lottie Creator **editor** URLs
(`creator.lottiefiles.com/?fileId=…`):

- `8e62ba76-…` — wakeup animation (juggling while speaking,
  loading while thinking).
- `ffb68ecd-…` — waves visual for ghost mode.

A live fetch of the editor URL returns only page metadata — **no
direct JSON is exposed** (verified 2026-09-28). So the files cannot
be pulled by URL. Acquisition options, in order:

1. **Manual export (certain, recommended).** Open each file in
   Lottie Creator → Export/Download → `.json` (Lottie). Drop as
   `frontend/public/wakeup-v2.json` and
   `frontend/public/ghost-waves.json`. No code change needed —
   both are picked up automatically with fallback (§5).
2. **Lottie Creator MCP — pipe VERIFIED working 2026-09-28.**
   `@lottiefiles/creator-mcp@0.2.2` boots locally (WS
   127.0.0.1:3847 + stdio), exposes **108 tools**, and executes
   calls — proven with a stdio JSON-RPC probe
   (`initialize → tools/list → tools/call`). The ONLY blocker is
   a connected Creator tab: `read_scene` with no tab returns
   "No Creator tab is connected." User setup (2 min): open
   creator.lottiefiles.com → open the file → Settings → MCP
   Settings → Enable local MCP (bridge connects to 127.0.0.1:3847).
   Then the assistant can drive edits directly: `read_scene` →
   `list_segments` → palette swap to `#259ed6/#ef4f25/#fbdf38`
   (`set_fill`), staggered entrances (`stagger_layers`),
   `speaking`/`loading`/`waves` named segments (`add_segment`),
   ghost state machine (`add_state_machine` + states bound to
   segments) — then the user exports JSON (no export/download
   tool exists in the MCP; export stays a Creator-UI click) into
   the same two `frontend/public/` paths. Probe/caller scripts:
   `%TEMP%/opencode/mcp_probe.mjs`, `mcp_call.mjs`
   (`node mcp_call.mjs <tool> '<json args>'`).
3. **Hosted LottieFiles MCP** (`mcp.lottiefiles.com`, Streamable
   HTTP + OAuth, GraphQL workbench) — works with account data but
   needs an interactive OAuth sign-in the assistant cannot complete
   alone. Prefer route 1 or 2.
4. **LottieFiles API.** Same output files; needs an API token.

**Segment-map contract for `wakeup-v2.json`.** The player assumes
the v2 file keeps the v1 layout unless `LOTTIE_MAPS` (Avatar.tsx)
is updated: loading loop segment, smile-arrival segment, hold
frame. If the new file moves them, update that one map — the
sequencer (`applyState`/`onComplete`) follows the map, not
constants. Juggling-while-speaking vs loading-while-thinking then
falls out: speaking currently reuses the loading loop at 1.2x; give
the juggling section its own map entry and a `speaking` branch if
the v2 file ships it as a separate range.

## 3. Audio-reactivity architecture (built this turn)

**Mic source = the STT capture RMS itself.** The cpal callback
(`wakeword_oww.rs:1562`) already computes per-chunk RMS for VAD on
the exact samples Groq receives — waves and transcript share one
ground truth, so sync is structural, not tuned.

- Map: `level_from_rms` — 0.10 saturates, sqrt keeps quiet speech
  visible, non-finite → 0 (never NaN over the bridge). Unit-tested.
- Transport: bounded `sync_channel(4)` + `try_send` every 2nd 80 ms
  chunk (≈6 Hz) + final 0.0 on capture stop (waves rest, never
  freeze mid-speech). try_send never blocks the audio callback;
  a dedicated `audio-level-fwd` thread drains-to-latest and emits
  `audio:level {level}` (duplicate suppression < 0.02). The STT
  receiver thread was NOT reused — it blocks seconds in
  transcription.
- Frontend: `micLevel` store field (clamped, reset to 0) +
  `audio:level` listener (`main.tsx`) + rAF drive loop in Avatar
  (reads via `getState`, no re-render storm) with per-bar easing
  (0.35 lerp) and stagger.
- **Rejected: parallel getUserMedia meter.** It conflicts with the
  Rust cpal stream on Intel SST drivers (`warmMic` disabled at
  startup for exactly this reason) — a second mic tap risks the
  silence-recovery death spiral. Rust-side metering is the only
  safe source.

**TTS side (honest limit).** rodio plays opaque audio — no
playback-tap exists, so there is no TRUE TTS envelope on the
bridge. Speaking waves ride a deterministic speech-like rhythm
(`ttsWaveLevel`: 2.1 Hz + 3.7 Hz partials, per-bar phase) GATED on
`ttsActive` (event-derived from real playback start/stop):
motion exactly while NEXUS talks, still otherwise. Path to true
envelope: tap PCM in Rust before the rodio sink and emit
`audio:tts-level` on the same channel — recorded as follow-up,
not needed for the visual contract.

## 4. Transition + visibility contract

Full sequence (built): smile → **pinch 220 ms** (Lottie scaleX +
fade) → **waves grow 250 ms + per-bar entrance bloom** (opacity +
rise, 90 ms stagger left→right, 650 ms window — separate CSS
properties from the rAF-driven scaleY, so entrance and audio-drive
never fight) → live waves. Exit (Esc / voice exit / stand-down):
**leave beat 200 ms** (trio shrink + fade, rAF already stopped) →
smile re-applies → normal wakeup behavior. Exit beat is
instant-feeling, never a hard cut.

| Event | Visual |
|---|---|
| Ghost enter (`ghost:session` true) | smile → pinch 220 ms → waves grow (existing `ghost-grow`) |
| User talks (capture RMS ≥ floor 0.04) | bars scale with mic level; optional waves-Lottie plays, opacity 1 |
| Silence / thinking | rest scale 0.15, Lottie pauses + opacity 0.35 — still, by design |
| NEXUS talks (`ttsActive`) | stylized rhythm on bars (+ Lottie resumes) |
| Esc / exit / stand-down (`ghost:session` false) | waves unmount → smile re-applies → normal wakeup behavior |
| Whole session | waves layer mounted until exit — never hidden mid-session |

Drive-source decision is pure and tested (`waveSource`):
speaking+ttsActive → tts; listening+level ≥ floor → mic; else rest.
