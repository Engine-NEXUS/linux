# 125 - Quality system Q0/Q1: honest CI, dead-code ratchet, IPC and event contracts

Date: 2026-10-12. Plan: `~/.claude` plan "make it stop breaking, then build the features properly" (part Q). The audit
behind it: 12 recurring regression classes, CI that did not run most tests, and ~3,600 lines of unwired Rust.

## Why

Features kept being "fixed" and then breaking again because nothing automated tied the pieces together: a Rust command
and the TypeScript call that uses it are linked only by a string, dead code was hidden by `#[allow(dead_code)]`, and CI
skipped the integration tests and never compiled the feature set that ships. The rule in `AGENTS.md` ("must stay in
`NexusSettings`" and so on) repeated for months. Checks that run on every push replace that rule.

## What was wrong with CI (all verified, then fixed)

| Finding | Fix |
|---|---|
| `scripts/check-turn-ends.mjs` listed `main.tsx` and `App.tsx`, both deleted, so the CI step crashed with ENOENT and had been hiding 13 unmarked `reset()` calls | Gate rewritten: scans **every** non-test `.ts/.tsx`, fails if a core file it depends on disappears (`REQUIRED`), and each deliberate raw reset needs `turn-end:keep-raw <reason>` (a bare marker fails). 13 sites reviewed and marked with a reason |
| `tests/phase2_integration.rs` called the deleted `tts_piper` (`cargo test --all-targets` could not compile) | Removed the two Piper tests (engine deleted in change 81) |
| `src-tauri/tests/*` (50-command voice fixture etc.) never ran in CI | CI runs `cargo test --tests` |
| `custom-protocol,admin-brain` (what you actually build) never compiled in CI | `cargo check --all-targets --features custom-protocol,admin-brain` |
| **`cargo check --features mock-wake` was broken since 2026-10-03** (`verify_turn_audio`, `verify_speaker_at_fire`, `SpeakerCheck`, `verify_candidate` used gated items) | Gated the same way as the items they use |
| Worker `tsc` had an error (`__listProfiles` not on `IdentityStore`) and Worker/Python tests were not in CI | Optional member added to the interface; CI runs Worker `tsc` + vitest (109) and agent/memory pytest (31) |
| No check that secrets, databases or logs are not committed | `scripts/check-repo-hygiene.mjs` (+ `--selftest` proving each of 10 secret rules and 4 path rules fires) |

Correction to the audit: `server/NEXUS_credentials.db` and the `.log` files are **not** tracked (`*.db` is git-ignored).

## Ratchets (CI fails on anything new; baselines may only shrink)

- **Dead code** (`scripts/dead-code-audit.mjs`, baseline `scripts/baselines/dead-code.txt`, **164 findings**).
  - Builds the lib with `--force-warn dead_code`, which **overrides `#[allow(dead_code)]`**, so hidden dead code is counted.
  - `lib.rs` now declares only six modules `pub` (`scripts/baselines/external-api.txt`: the ones tests and dev tools import);
    the other 41 are `pub(crate)`. rustc cannot report unused `pub` items in a lib crate, so before this the compiler was blind to them
    (it now reports 75 more even without force-warn).
  - Guard 0 fails if `lib.rs` re-publishes a module that is not on the reviewed list.
  - Mutation-checked: a new dead fn behind `#[allow(dead_code)]` fails; an extra `pub mod` fails.
  - Names starting with `_` are exempt from `dead_code` in rustc, so a probe named `__x` proves nothing (learned the hard way).
- **IPC + events** (`scripts/check-ipc-contract.mjs`, baselines `ipc-unused-commands.txt` (63) and `events-unmatched.txt` (7)).
  - Hard failures: command defined but not registered; registered but not defined; frontend invokes an unregistered command.
  - Parses real `emit*/listen*/once*` calls (first three arguments) instead of regexing literals, so window labels, payload keys and CSS classes are not mistaken for events.
  - Mutation-checked with four probes (unregistered invoke, unregistered command, new unheard event, new unemitted listener).
  - Found and removed a real dead call: `invoke("show_overlay")` in the Settings Display tab (no such command since the orb moved into the stage; the error was swallowed).

## Not done yet (still to come under plan part Q)

Settings round-trip (`#[serde(flatten)]` + TS type generation), the exhaustive intent `Lane` mapping and phrase corpus, fake-executor
tests, turn recorder, WebView console smoke test, mutation testing in CI, `nexus build` version stamp, per-window capability
allow-list, and the dead-code removal campaign itself (the 164 + 63 + 7 baseline lines are the work list).

## Verify

- `cargo check --all-targets --features custom-protocol,admin-brain` clean; default and `mock-wake` clean.
- `cargo test --tests --features custom-protocol,admin-brain` serial: lib 1352 passed / 13 ignored, e2e fixture 2, offline 10, user commands 7 (all pass).
- Frontend `tsc` clean, vitest 243/243; Worker `tsc` clean, vitest 109/109; Python 31/31.
- Gates: turn-ends OK (78 files), hygiene OK (1,062 tracked files), IPC OK, dead-code ratchet OK.
- **Not run:** the GitHub Actions workflow itself (YAML validated locally; every command in it was run locally).

## Pitfalls

- To pay down a baseline: delete/wire the item, then `node scripts/dead-code-audit.mjs --update` (or `check-ipc-contract.mjs --update`). The script
  refuses to pass if a baseline entry is stale, so the file cannot rot.
- Adding `pub mod` to `lib.rs` fails the audit; justify it in `external-api.txt` (say who imports it).
- Do not name a function with a leading underscore to "test" the dead-code ratchet.
