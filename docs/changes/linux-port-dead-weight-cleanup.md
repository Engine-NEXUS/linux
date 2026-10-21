# Linux port — dead code and dead weight removed

User request: *"if there is any dead code or dead weight? if yes get rid of it"*. Only code that is dead on **every** platform was removed; code that is dead on Linux because its callers are Windows-only stays (the repository tracks the Windows base).

## Method

- **Rust:** start from rustc's own findings (`scripts/dead-code-audit.mjs`, `--force-warn dead_code`, shipped features), then keep only items whose name appears nowhere else in the repository's code — no other file, no `#[cfg]` branch, no test, no dev tool, no frontend `invoke` — and whose only mention in their own file is the definition. Repeated until nothing new appeared (two rounds). Struct fields were left alone (removing one can change serialized JSON), and so were GPT-sol's Ghost files.
- **Frontend:** an import graph from every Vite HTML entry (static, dynamic and `new URL` imports, `import.meta.glob`): files no page reaches, plus files only their own tests reach.
- **Files:** large or unreferenced tracked files, junk patterns (logs, temp, caches — none were tracked).

## Removed

Rust, 25 items (24 were also on the Windows dead-code baseline; `policy::contains_ci` is a Linux-layer helper Windows never had):

- `auth_vault.rs: function migrate_api_keys_to_keychain`
- `brain_client.rs: function brain_get_pronunciation_map`
- `brain_client.rs: function is_brain_available`
- `brain_client.rs: struct PronunciationMapResponse`
- `brain_monitor.rs: function approved_phrasings_count`
- `brain_monitor.rs: function gaps_count`
- `brain_monitor.rs: function pending_count`
- `browser_center.rs: function read_page_markdown`
- `browser_center.rs: function to_intent`
- `google/auth.rs: associated function is_authenticated`
- `google/mail.rs: associated function parse_threads_json`
- `google/types.rs: struct TimeRange`
- `lazy_brain.rs: function is_brain_running`
- `lazy_brain.rs: function kill_brain`
- `live/commands/whatsapp.rs: function full_send_flow`
- `live/mod.rs: enum LiveIntent`
- `nlu_update.rs: function downloaded_model_dir`
- `policy.rs: function contains_ci`
- `system_center.rs: function inspect_window_via_bridge`
- `tts_network.rs: constant NETWORK_CHECK_TIMEOUT`
- `vision.rs: function build_email_extraction_prompt`
- `vision.rs: function locate_via_vision`
- `vision.rs: function resolve_named_target`
- `voice_catalog.rs: function read_manifest`
- `voice_profile.rs: method acceptance_threshold`

Plus four `screen:tour_phase` emits in `orchestrator.rs` (their only listener, below, was never started) and the imports the removals left unused.

Uncompiled Rust files (no `mod` line reached them): `return_review.rs`, `memcore/agent_tasks.rs` (feature 139; WINDOWS still has them). `rust-orphan-files.txt` is now empty.

Frontend:

| File | Why it was dead |
|---|---|
| `stage/EntranceBurst.tsx` | retired stub returning `null`; nothing imports it |
| `stage/geometry.ts` (+ test) | legacy window-to-stage rect mapping; only its own test used it |
| `net/screenTour.ts` (+ test), `stage/tourState.ts` | the orb/TTS side of the narrated screen tour; `initScreenTourListener` has never been called in any commit since it was written (2026-10-06) |
| `public/wakeup.json`, `public/waves-2.json` | Lottie animations of the retired avatar; nothing loads them, yet `public/` ships them in every build |

## Kept on purpose

- The ~290 other Rust dead-code findings on Linux: their callers are Windows-only, or they are used by tests or other code paths.
- Design tools that are not dead code: `caption-showcase.html`, `pebble-showcase.html`, `caption-tuner.html`, `public/gallery.html` (opened by hand when tuning visuals).
- Large model files: every one is loaded at runtime or bundled (`silero_vad_v6.onnx` is loaded by `@ricky0123/vad-web` by name; `scripts/hey_jarvis_v0.1.onnx` by `compare_jarvis_vs_nexus.py`).

## Verification

The laptop was too low on memory for a local Rust build, so CI on the PR is the check: three `cargo check` feature sets, the dead-code ratchet (baselines shrunk by exactly the removed items), lib + integration tests, frontend `tsc`/build/tests, the IPC contract gate, and the `.deb` build-and-install job.
