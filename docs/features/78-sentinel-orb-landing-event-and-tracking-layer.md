# Feature 78: Sentinel Orb-Landing Event & Tracking Layer

Tracking-first half of the Orb UI & Landing Specification: the backend now
emits a structured `orchestrator:sentinel-alert` event per detected change,
and the frontend observes it in a lifecycle store. The landing animation
itself (incoming_pulse → side pill, 7s auto-collapse) is a later phase —
this lands the wire contract + observability so tracking comes first.

## Wire contract (Rust → frontend)

Event: `orchestrator:sentinel-alert`, payload `SentinelAlertPayload`
(`google/types.rs`), keys mirror the spec 1:1:

- `alert_id` (`sentinel_{deadline|reply|attachment}_{unix_secs}`),
  `source` (`"Gmail"`), `account_email` (nullable),
- `organization{name, domain, avatar_fallback_initials, brand_color}`
  (domain-derived fallback until an org directory exists),
- `context{thread_id, subject, sender, snippet}`,
- `deadline{is_extended, previous_deadline, new_deadline, urgency} |
  null` (null for replies/attachments),
- `landing_animation{initial_state, docked_state, auto_collapse_after_ms}`,
- `spoken_notification`.

Emitted from all three `ThreadUpdateEvent` arms in the sentinel polling
loop (`google/sentinel.rs`). Existing behavior preserved exactly: same
`println!` lines, and only deadline changes speak (via the unchanged
`speak_proactive_alert`). Urgency mirrors synthesis semantics (deadline
High, reply Medium, attachment Low). `is_extended` defaults true —
direction needs date parsing (documented follow-up).

## Frontend tracking (`store/sentinel.ts`)

Append-only lifecycle store: `incoming → docked → collapsed | dismissed`,
dedupe by `alert_id`, `receivedAt` stamp. `initSentinelAlertListener()`
(wired in `App.tsx` beside the ghost session block) only tracks + traces —
never speaks (speech stays Rust-side, no double TTS), never touches the
orb (landing phase owns presentation).

## Console observability (diagnostics-driven)

`run.ps1` surfaces `[WATCH|SENTINEL|GMAIL|VISION|ALERT|GHOST|ACTION|TTS]`
lines; everything else INFO stays hidden. Coverage per turn/poll:

- Sentinel poll: 1 cycle header + 1 line per watch stage (token →
  fetch → diff outcome). No-token skip prints once per process; event
  arms print payload ids (`[SENTINEL-ALERT] emitted event id=…`).
- Every executed turn: `[ACTION] '<transcript>' → intent → subsystem`
  plus validity verdicts (`Unheard/NeedSlot/Invalid` + prompt).
- Browser/ghost runners: outcome lines (`Ctrl+T sent`, drill results).
- Every spoken line: `[TTS] speak (req=…, 80-char preview)`.
- Orb/waves/ghost-UI: `[ORB]`/`[WAVES]` frontend traces.

## Frontend tracking (`store/sentinel.ts`)

Append-only lifecycle store: `incoming → docked → collapsed | dismissed`,
dedupe by `alert_id`, `receivedAt` stamp. `initSentinelAlertListener()`
(wired in `App.tsx` beside the ghost session block) only tracks + traces —
never speaks (speech stays Rust-side, no double TTS), never touches the
orb (landing phase owns presentation).

## Verification

- Rust: 3 new sentinel tests (wire-shape keys, org derivation, urgency
  mapping) — full serial suite 725+2+10+7 twice, 0 failed, 0 warnings.
- Frontend: 4 new store tests (shape, dedupe, lifecycle, listener
  registration) — 57/57 twice, `tsc` fully clean.
