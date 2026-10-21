# 107 — Memory Core P0: keychain backend, forget/wipe coverage, auto-learning wired

Plan: `docs/features/100-memory-core-plan.md` (phase P0). Research: `docs/research/memory/01-...`.

## Problems found (verified, not assumed)
1. **`keyring` had no Windows backend.** `Cargo.toml` declared `keyring = "3"` with no platform feature. In keyring 3.x the Windows store only compiles with `windows-native`; otherwise it falls back to an in-memory mock. Evidence: `cargo tree -e features -i keyring` showed only `default`; Credential Manager (`cmdkey /list`) held no NEXUS entries; `settings.json` still held the Groq and Gemini keys. Keys and OAuth tokens that the docs describe as being in the OS keychain were therefore not persisting across restarts.
2. **`memory::log_episode` had no production callers** (tests only), so M2 auto-learning ("I work at X", "call me Y") never ran.
3. **`forget` only touched `core.json`**, so a learned fact in `facts.json` could not be forgotten.
4. **`wipe_memory` left `conversation.jsonl`, `conversation_brief.json`, `diary.jsonl` and `mail_watches.json`**, while the spoken confirmation claimed conversations were erased.
5. Memory files were written with plain `fs::write` (a crash or a racing writer could leave a half-written file).

## Changes
- `Cargo.toml`: `keyring = { version = "3", features = ["windows-native"] }`.
- `memory.rs`: new `atomic_write` (temp file + rename), used by `write_json`, episode pruning, and the conversation/brief writers. `forget` now removes the key from `core.json` and `facts.json`. `wipe_memory` also removes `mail_watches.json`, the conversation thread, its brief and the diary.
- `orchestrator.rs`: `record_worker_turn` calls `log_episode` for Completed turns from a recognised owner (Rejected audio, failed and cancelled turns never teach anything). Wipe warning now says what is erased.
- `conversation.rs`, `diary.rs`: file-name constants made `pub(crate)`.

## Behaviour change to know about
With a real keychain backend, the existing keychain-first read/migration code can now actually move API keys out of `settings.json` into Windows Credential Manager on next start. That is the documented design. It is not yet checked on a real restart, so after the first launch confirm that your Groq and Gemini keys still work and appear under `cmdkey /list`.

## Verify
- `cargo test --lib -- --test-threads=1`: 1034 passed, 0 failed (6 ignored dev helpers). +3 new memory tests: learn-then-forget, wipe coverage, atomic write.
- `cargo check --features custom-protocol,admin-brain`: clean.
- **Keychain persistence: verified in change 109** (separate processes read back a written Credential Manager entry).
- **Not verified:** local-command turns still do not call `log_episode` (only Worker-routed turns do).
