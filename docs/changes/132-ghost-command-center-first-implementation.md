# Ghost command center: first implementation

Date: 2026-10-16

## Built

- Added src-tauri/src/ghost_center.rs as Ghost Mode's desktop-action coordinator. The main orchestrator still owns turn identity, voice ownership, validity, cancellation, and spoken results; center.rs now routes GhostAction to GhostCenter.
- Added keyboard and accessibility sub-centers. A strict local grammar recognizes common shortcut commands and exact named-control clicks, including up to five ordered steps. Unknown or partially parsed sequences are refused.
- Reused the existing shortcut router and its foreground, protected-window, elevation, and stop checks. Its verified versus unconfirmed result now reaches Ghost speech honestly.
- Added live UIA pattern actions for toggle, selection, and invoke. A cursor action is used only for a current accessible element with a clickable point that still hit-tests to that same element after moving. Ambiguous names, secure fields, protected windows, elevation mismatch, and changed foreground windows block execution.
- Ghost voice ordinal clicks and named NLU clicks now go through this center. The older direct Ghost click entry point also joins it and no longer asks a vision model to find a control.
- Each step emits an outcome receipt. A later step does not run after an unconfirmed, blocked, or stopped step.
- Added explicit voice commands to teach, use, and forget a control alias in the current app. The local rule stores the executable name, a hash of its path, its build fingerprint, a unique UIA AutomationId and control type, and a hash of the accessible name. It stores no screen coordinates or text-field values. An app update, identity mismatch, duplicate control, or damaged store blocks reuse.

## Still open

- Teaching works only where the app exposes a unique usable AutomationId. Custom canvases and controls without stable accessibility identity remain unsupported.
- Invoke/cursor dispatch may be unconfirmed when the app exposes no observable postcondition; it is never announced as completed.
- The strict parser does not yet cover the full natural-language phrasing set or explicit target-app slots.
- App-by-app live acceptance and mixed-DPI/occlusion testing have not been completed.

## Verification

- Rust build: cargo check --features custom-protocol,admin-brain passed.
- Focused Ghost parser/receipt tests: 4 passed.
- Ghost learning store tests: 2 passed, including replacement and damaged-store refusal.
- Main-center and orchestrator Ghost routing tests: 2 passed.
- Mouse module unit tests: 9 passed.
- Speech-recognition vocabulary budget test: 1 passed.
- rustfmt check for the new Ghost center: clean.
- No live desktop control was performed, so per-app reliability and cursor hit-testing remain unverified in practice.
