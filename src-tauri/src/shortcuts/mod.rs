//! Shortcut brain, phase E1: the Shortcut Map and the keyboard-tier Action Router.
//!
//! "Do ACTION in the app that is in front" resolved to a documented key chord, sent only after the
//! foreground window is re-checked, with an optional cheap verification. Design and phasing:
//! `docs/features/102-keyboard-first-shortcut-brain-plan.md`, `103-…-implementation-plan.md`;
//! measurements behind it: `docs/research/ghost-mode/09-…`, `11-p0-baseline-report-…`.
//!
//!   keys.rs   layout-independent chords + the exact `SendInput` event sequence      (pure)
//!   map.rs    `Action` vocabulary, seeded `(app family, action) → chord` table      (pure)
//!   router.rs gates + pre-send foreground re-check + send + verify, over `Env`      (pure, fake-testable)
//!   exec.rs   Windows adapter (`WinEnv`), `do_action`, spoken-line helpers
//!
//! Kill switch: `NEXUS_SHORTCUTS_LEGACY=1` restores every call site's previous implementation.

pub mod exec;
pub mod keys;
pub mod map;
pub mod router;

pub use exec::{do_action, do_action_spoken, enabled, run_or_legacy, Sent, Spoken};
pub use map::Action;
pub use router::Outcome;
