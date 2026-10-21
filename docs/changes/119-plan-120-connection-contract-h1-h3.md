# Plan 120 Built — Subcommand Connection Contract H1–H3 (2026-10-14)

Built phase-by-phase with two cross-checks per phase (targeted test + full
affected suite/warnings). No behavior change: test-only additions + dead-code
removal + one doc comment. Concurrent-session collision mid-Phase-1
(untracked `agent_watch.rs` broke the crate for ~20 min) resolved itself —
verified green before advancing, no files of theirs touched.

## Phase 0 — baseline
- `cargo test --lib center::`: 50 passed, 0 failed before any edit.

## Phase 1 (H1) — spelling test
- `center.rs`: new `test_center_for_names_are_registered` — one sample intent per
  center (17 samples incl. `Unknown`), asserts each `center_for` output equals the
  expected name AND ∈ `ALL_SUB_CENTERS`; asserts no duplicate/dead registry entries.
- Verify: targeted 2/2; full `center::` 51/51; zero new warnings.

## Phase 2 (H2) — side-effect audit test
- `center.rs`: new `test_side_effect_membership_is_pinned` — pins the must-gate set
  (WhatsApp send, OrderFood, GitHubCommand, ScreenClick, StartDictation,
  WatchScreenEmail, Search) and the pure set (Greeting, MemoryAudit, OpenSettings,
  Unknown) in both directions.
- **H2b finding (needs product approval, NOT built):** `RunOnLocalhost`,
  `CloseApp`, `Block/UnblockContact`, `MemoryForget(All)`, `TimetableCommit`,
  `CalendarAdd`, `CreateVoiceReminder`, `AnalyseChatDocs`, `ScanInternshipEmails`,
  `Directions`/`PlacesSearch`, `Enter/ExitGhostControl`, `CodeProject`/
  `CodingAgentTask` change state/quota by the `is_side_effect` definition but are
  unlisted → unenrolled weak-evidence turns Allow them today. `OpenApp`/`OpenUrl`
  deliberately unlisted (Ghost Oct-13 directive). Gating any of these changes live
  behavior — approve per item. Note embedded in the test.
- Verify: targeted 2/2; full `center::` 52/52; zero new warnings.

## Phase 3 (H3) — dead-policy cleanup
- Deleted unreachable `source_strength` duplicate branch (`center.rs`).
- Deleted `is_long_running` (zero production callers) + its 4 test fns
  (`orchestrator.rs`); left an explainer comment pointing at the live
  Ack/Loading-event flow so nobody reintroduces the dead oracle.
- `confirm_kind`: deliberately NOT deleted — the 4 impls hold reviewed
  repeat-back/GatePrompt policy a future confirm flow should reuse. Added an
  explicit not-yet-wired doc on the trait method instead (ends the false
  confidence with zero churn).
- Verify: `center::` + `orchestrator::` 114/114; zero references to
  `is_long_running` outside the explainer; zero new warnings.

## Open follow-ups (not built)
- H2b per-item gating approvals (behavior changes).
- Wiring `confirm_kind` into the confirm flow (behavior change, needs design).
- `Receipt` never-constructed warning (pre-existing, aspirational type — same
  family as confirm_kind; remove or wire in a later pass).
