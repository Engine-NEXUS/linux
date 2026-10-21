# Linux port — Linux CI for the linux repository

The port brought WINDOWS' workflows along with the Windows base, so the linux repository compiled and tested its Rust only on `windows-latest`. This change gives it the Linux counterpart of every Windows job, and fixes what kept correct code from passing.

## Jobs

| WINDOWS | Linux (this repo) |
|---|---|
| `ci.yml` › `rust-check-windows` | `ci.yml` › `rust-check-linux` (ubuntu-24.04): `cargo check` default / `mock-wake` / shipped features on all targets, dead-code ratchet, lib + integration tests |
| `ci.yml` › `windows-installer` (main only) and `build-windows.yml` (every push/PR) | `build-linux.yml` (every push/PR): the `.deb` through `scripts/build-prod.sh` (the path `release.yml` takes), installed with apt on a clean runner; every `Depends` must resolve, the binary must have no unresolved library, and the wake/VAD/turn/NLU resources must be in the package; uploaded as an artifact |
| `frontend-check`, `worker-check`, `python-check` | unchanged; they are platform-neutral and already ran on ubuntu |

`release.yml` (tags) was already Linux; it moved to Node 22 (`@saystack/core`, the aura, requires ≥ 22) and gained `libdbus-1-dev`.

The Worker deploy (`deploy-worker.yml`) is unchanged: it deploys on a push to `main` that touches `server/worker/**`. This repository has no `CLOUDFLARE_API_TOKEN` secret (its last runs failed on exactly that), so merging deploys nothing today. If the secret is ever added, a merge deploys the J1 gate, which needs the D1 migration first (`linux-port-j1-worker-deny-by-default-device-gate.md`).

## Why correct code did not pass, and the fixes

| Job | Cause | Fix |
|---|---|---|
| dead-code ratchet (both OSes) | `dtolnay/rust-toolchain` sets `CARGO_TERM_COLOR=always`; ANSI codes around `warning:` made every finding unparseable, so CI saw **zero** findings and passed blind. On Windows this blindness also led to removing 156 baseline entries that were never fixed. Reproduced: `CARGO_TERM_COLOR=always` → 0 findings | `--color=never`; the Windows baseline restored to WINDOWS main; one baseline per OS (`dead-code-linux.txt`, 316 findings, mostly shared code whose only callers are Windows-only; `dead-code.txt` stays the Windows one) |
| dead-code, Linux layer | `overlay_linux::compute_input_rects` was a test helper outside `#[cfg(test)]`; `session::SessionKind::describe` had no caller | fixed instead of recorded |
| IPC contract gate | its hard failure came from `return_review.rs`, which no `mod` line declares: feature 139 lost its wiring and its frontend card when WINDOWS main was squashed, so the file (and `memcore/agent_tasks.rs`) is not compiled | the gate scans only the module tree from `lib.rs`/`main.rs` (honouring `#[path]`) and lists unreached files in `rust-orphan-files.txt` (may only shrink), so a lost `mod` line now fails CI; the inherited one-sided `agent_watch_*` commands/events and `ghost:action_receipts` are recorded, two resolved entries removed |
| NLU Data Foundation | one live-log import (`d61c517`) wrote 15 train rows in the log's shape (`slots: null` or an `action` slot): seven overlap the frozen validation/calibration/test, and some carry intents that are not labels (`OpenArchitect`, `nlu_result`, `Unknown { raw: … }`), which `train.py` maps to `unknown` | those 15 rows removed from train only; the train split re-locked; validation, calibration, test and quarantine byte-identical; two stale tests fixed (a pinned 52-intent count, an old error wording) |
| `build-linux.yml` (first run) | the Windows-base `tauri.conf.json` runs `npm run build` / `npm run dev` from the root `package.json` shim, but `build-prod.sh`, `build.sh` and `dev.sh` started tauri from `src-tauri/` — `release.yml` would have produced nothing | the scripts run tauri from the repo root |

## Still open (owners)

- Feature 139 (return review, agent task journal, the watch UI): re-land the `mod` lines and the frontend in WINDOWS; the orphan and one-sided baselines shrink when it lands.
- The same two CI fixes (color, orphans) apply to WINDOWS' own CI.
