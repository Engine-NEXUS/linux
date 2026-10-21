# Feature 74: Main Center + Per-Action Sub-Centers — Implementation Plan

## 0. Contract (read first)

- Every action = one sub-center module with one contract:
  `validate(slots) → execute() → {ok, data, error} + UiDirective`.
- The Main Center is the ONLY caller of STT/TTS/UI transitions.
- Every spoken line names the action or the exact problem. Bare "on it
  sir" / "ok sir" disappear except as a prefix to a repeat-back
  ("On it sir — opening a new tab.").
- No phase changes behavior until its tests are green twice (dual-gate).

## 1. New shared types (`src-tauri/src/center.rs`)

```rust
pub enum Validity { Ok, NeedSlot { slot, prompt }, Invalid { reason, prompt }, Unheard }
pub struct Receipt { ok: bool, summary: String, data: Value, latency_ms: u64 }
pub enum UiDirective {
    Orb { state: OrchestratorState, visible: bool },
    Loading(bool),
    Waves { mode: WavesMode },          // Off | Mic | Tts | Rest
    Ring { x: i32, y: i32 } | RingHide,
    Speak { text: String }, StopSpeak,
    Sidebar { kind: SidebarKind, payload: Value } | SidebarHide,
}
pub trait SubCenter: Send + Sync {
    fn name(&self) -> &'static str;
    fn validate(&self, intent: &ParsedIntent) -> Validity;
    fn execute(&self, app: &AppHandle, intent: &ParsedIntent) -> ReceiptFuture;
    fn confirm_kind(&self, intent: &ParsedIntent) -> ConfirmKind; // None|Repeat|Gate
}
```

## 2. Validity gate (Alexa map) — Main Center, `process_transcript`

Order per turn: parse → **validate** → route → confirm → execute → merge.

| Situation | Main Center speaks (exact template) |
|---|---|
| Empty/garbage transcript (`Unknown`, <2 alpha chars, confabulation) | "I didn't catch that clearly, sir — say it again." (NEVER cloud chat) |
| Missing required slot (`NeedMoreInfo` generalized: every intent declares `required_slots()`) | Intent's elicit prompt, e.g. "Which app should I open, sir?" |
| Invalid slot value (app not installed, tab index out of range, contact unknown) | "I couldn't find {value}, sir." + nearest candidate if any |
| Valid, routine | Repeat-back ack: "Opening a new tab, sir." / "Closing tab 5, sir." |
| Valid, destructive/ambiguous | Gate prompt with details, 30s timeout (existing confirm flow) |

Required-slot table (v1): `OpenApp{target}`, `CloseApp{target}`,
`WhatsappChat{contact}`, `SendWhatsAppMessage{contact,message}`,
`Search{query}`, `BrowserTab{index}`, `ScreenClick{ordinal}`,
`Analyse*{repo}`, `TypeText{text}`. Everything else: no required slots.

## 3. UI director — one transition function

```rust
pub fn direct_ui(app: &AppHandle, d: UiDirective)
```

- Owns: orb `visible/state`, loading window, waves mode, ring, stage,
  sidebars. Frontend keeps rendering; it stops DECIDING (no local
  `setVisible`/`setState` outside the `orchestrator:event` handler —
  audit lists every site to migrate).
- Transition guards (the wake-animation lesson): e.g. `Waves(Mic)` may
  only replace `Orb(listening)` after the wake choreography completes;
  `Loading(true)` never hides the orb mid-turn. Guard violations log +
  keep the old visual (fail-frozen, never fail-invisible).
- Ghost mode becomes `UiDirective` sequences owned by GhostCenter, not
  scattered emits.

## 4. STT/TTS directors

- **STT director**: owns capture start/stop, mute windows (meeting,
  post-TTS settle), barge-in policy, transcript filters. One predicate:
  `stt_ready()`. Replaces the five-owner sprawl; the dropped-chunks warn
  becomes a director decision with a spoken reason when it blocks a turn.
- **TTS director**: owns speak queue, stop, barge-in generation, and the
  spoken-feedback policy (validity/ack/result/error templates from §2).
  No module speaks except through it.

## 5. Sub-center registry (decision tree)

```rust
// center_registry.rs — intent → exactly one center. Exhaustive match;
// adding an action = one arm + one module. Unknown → KnowledgeCenter
// ONLY after the validity gate passes (garbage never reaches chat).
AppCenter:        OpenApp, OpenUrl, CloseApp, OpenSettings
BrowserCenter:    BrowserTab, BrowserCloseTab, browser_new_tab/navigate/search, OpenSite
MediaCenter:      Media*, VolumeMute
MessageCenter:    WhatsappChat, SendWhatsAppMessage (+ghost drill variant)
CommerceCenter:   OrderFood, SearchProduct
GitHubCenter:    GitHubCommand (existing typed system, wrapped)
ArchitectCenter:  OpenArchitect, AnalyseRepo
KnowledgeCenter:  Search, general Q&A (Worker/9Router) — gated, never garbage
DictationCenter:  EnterGhostwriter, Start/StopDictation, TypeText, press_*
GhostCenter:     Enter/ExitGhostControl, ring, watchdog, drills
SystemCenter:    Screenshot, Lock, timers, focus_app, window ops
GreetingCenter:  Greeting, NeedMoreInfo prompts
```

Each center wraps existing code first (no rewrites): e.g. BrowserCenter
wraps `run_browser_tab/run_browser_close/run_browser_new_tab`;
GhostCenter wraps `ghost.rs` + drill runners. Contracts added, logic moved
only when a center is migrated.

## 6. Phases (each independently shippable, tests ×2)

- **P0 — Contracts + registry (no behavior change).** New `center.rs`,
  `center_registry.rs` mapping every intent to a center; all arms delegate
  to current code paths. Tests pin the mapping table. Ship gate: full
  suite green, zero behavior deltas in live test.
- **P1 — Validity gate + spoken feedback.** `required_slots()` per intent;
  garbage → "say it again"; missing slot → elicit; invalid → name the
  problem; acks become repeat-backs. Removes the confident-nonsense-chat
  path. Ship gate: gibberish matrix (`'Sers.'`, `'Dumb.'`, `'right.'`,
  1-char, symbols) → all prompt, none chat; every routine action's ack
  names the action (grep-asserted in tests).
- **P2 — UI director.** `direct_ui()` + transition guards; migrate Rust
  emit sites, then frontend decision sites; wake-animation regression test
  (choreography completes before any waves/loading transition). Ship gate:
  visual walkthrough of wake → listen → speak → idle + ghost enter/exit.
- **P3 — STT/TTS directors.** Centralize lifecycle + policies; spoken
  reason when a turn is blocked. Ship gate: meeting/TTS/barge scenarios.
- **P4 — Center migration.** Move logic into centers one family at a time
  (Browser → App → Message → …); retire old paths as each center takes
  over. Ship gate per family: intent matrix green + live test.
- **P5 — Docs + cleanup.** Update feature 73/62 docs, AGENTS.md, intents
  catalog; delete retired code.

## 7. Acceptance (perfect-execution bar)1. Gibberish NEVER gets a cloud answer — always "say it again".
2. Every action's ack repeats the action; every failure names the value.
3. No animation can be killed by another center (guard test).
4. New action = new module + one router arm + dialog spec (checklist in §0).
5. Full suite green ×2 per phase; live walkthrough per phase.

## 8. Build log

- **P0 (done, verified ×2):** `src-tauri/src/center.rs` — `SubCenterName`
  (12 centers), exhaustive `center_for()` (compiler forces every
  `ParsedIntent` variant + every NLU live verb into exactly one center),
  per-turn `main-center: {intent} → sub-center {name}` trace log.
  Classification only — execution paths untouched, behavior identical.
  Full suite 667+2+10+7, 0 failed, 0 warnings, identical across two runs.
  Next: P1 validity gate + spoken feedback.

- **P1 (done, verified ×2):** `Validity` + `validate()` in `center.rs`
  (garbage → "say it again", missing slot → elicit by name, bad value →
  name the problem), gate wired in `process_transcript` with exemptions
  for dictation/ghostwriter-room/running-drill/follow-up context,
  WorkerBackend acks repeat the action (`ack_for`). Confirmations safe:
  frontend intercepts yes/no before `process_transcript`. Full suite
  670+2+10+7 serially green, 0 warnings. One parallel-only flake
  (`test_pending_compound_roundtrip`, shared-global class, passes alone
  ×2 and serially) — pre-existing, untouched by P1.
  Next: P2 UI director.

- **P2 (done, verified ×2):** `UiDirective::{Loading, Session}` +
  `direct_ui()` choke point with guards (orphan-spinner refusal,
  ghost-suppression kept); all 4 ghost session transitions + 16 loading
  sites covered (wrappers, zero call-site edits); ring stays in
  `ghost.rs` (hot-path, documented); frontend `shouldShowWaves()`
  extracted + matrix-tested (waves only in visible ghost sessions —
  the exact invariant that broke twice). Full serial suite
  707+2+10+7, 0 failed. Notes: (a) concurrent `WatchScreenEmail`
  variant proved the exhaustive-match design — the build broke until
  it got a center (GoogleCenter) + route (WorkerBackend until its
  executor lands); (b) one warning (`extract_gmail_thread_id_from_url`)
  and the google env-test parallel flake belong to concurrent work,
  flagged not fixed.
  Next: P3 STT/TTS directors.

- **P3 (done, verified ×2):** `SttGate`/`SttMuteReason` + `stt_gate()` (mirrors
  `should_suppress_wake` priority, now with reasons in the suppression log),
  `TtsEngine` + `tts_engine_for()` wired into the synthesis fallback's
  upfront decision (identical branches, pinned by matrix tests). Full serial
  suite 709+2+10+7 twice, 0 failed. Notes: concurrent `WatchScreenEmail`
  work is adjacent — its route arm was dropped mid-merge (build
  broken), restored to WorkerBackend until its executor lands; one warning
  + parallel env flake belong to concurrent work, flagged not fixed.
  Next: P4 center migration (per-family, starting with Browser).

- **P4 (Browser family done, verified ×2):** `browser_center.rs` —
  `BrowserCenter` implements the shared `SubCenter` contract (adapt +
  delegate: family rules for tab ranges live here, everything else
  delegates to the Main Center gate — zero duplicated rules; unknown
  actions fail closed). No execution moves (runners stay; trait has no
  `execute` yet — joint decision, not unilateral). Full serial suite
  712+2+10+7 twice, 0 failed, 0 warnings. Next: P5 docs + cleanup,
  then App/Message families by the same pattern.
