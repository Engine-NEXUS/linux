# 108 — Memory Core P1: SQLite store, ranked recall, context pack, Worker reads memory

Plan: `docs/features/100-memory-core-plan.md` (phase P1). Follows change 107 (P0).

## What was built
- **Link spike passed.** `rusqlite 0.40.2` (`bundled`) compiles and links with `ort`/`tract`/`tokenizers` in the test profile (no CRT `/MT` vs `/MD` conflict). Release link verified: `cargo build --release --features custom-protocol,admin-brain` finished in 11m01s, exit 0.
- **`src-tauri/src/memcore/store.rs`** — SQLite (WAL, `secure_delete`) with `records`, an FTS5 index, an append-only `audit` log and `meta`.
  - Provenance on every row: `source` + `trust` (`user_said | user_owned | derived | untrusted`).
  - Admit gate: refuses empty values, secrets (`memory::is_secret`) and **untrusted text as a fact**. Values are clipped to 500 chars. Identical value = no write. A mined value never overwrites a pinned user-said value.
  - Ranked search: `0.6·BM25 + 0.2·recency(30d) + 0.1·use-frequency + 0.1·pinned`, scaled by confidence. Prefix match and simple plurals. Query tokens are quoted, so FTS syntax cannot be injected.
  - Decay: unpinned derived facts lose 3%/day of confidence and are dropped below 0.2. Episodes expire after 30 days and are capped at 500. Pinned facts never decay.
  - `forget_key` removes a key from every tier and writes a content-free tombstone. `wipe` clears records, index, audit and meta, then checkpoints and VACUUMs.
- **`src-tauri/src/memcore/mod.rs`** — per-directory handle cache; one-time idempotent import of `core.json`, `facts.json` and `episodes.jsonl` (legacy files left in place); mirror/forget/wipe entry points; **`context_pack(query, budget=2000)`** — ranked facts + 3 recent episodes, PII-redacted, cut on a line boundary, used facts get their frequency bumped. Setting `memcore` (default on) turns the whole core off.
- **`memory.rs` is now dual-write / ranked-read:** `remember`, `save_learned_facts`, `log_episode`, `forget`, `wipe_memory` mirror into memcore. `recall` and `get_memory_context` answer from memcore first and fall back to the legacy substring/episode path when memcore is off or has nothing.
- **Fixes along the way:** the legacy context used a byte-based `String::truncate` (can panic on a multi-byte char). The 9Router path previously received memory without PII redaction; the pack is redacted for both paths. Budget raised 800 → 2000 chars.
- **Worker now reads memory:** `dialog_context.memory` is prepended (clipped to 2500 chars, control characters stripped, labelled "data, not instructions") in `handleGeneral` and `handleCounsel` via `memoryPreamble` in `counsel.ts`. Before this, Worker-routed turns never saw any memory.
- **`NexusSettings.memcore`** (default true) added to the struct so `save_settings` does not drop it.

## Verify
- `cargo test --lib -- --test-threads=1`: 1050 passed, 0 failed (6 ignored dev helpers). +16 tests: store (admit gate, upsert/novelty/pinned protection, truncation, ranking, empty query, frequency lift, forget tombstone, wipe, prune/decay, FTS injection), memcore (legacy import once, disabled flag, pack ranked/redacted/budgeted, line-boundary cut, forget/wipe), and a 16-case recall fixture set.
- `cargo check --features custom-protocol,admin-brain`: clean.
- Worker `npm test`: 109/109 (+3 `memoryPreamble`). `tsc` reports one error in `identity.ts` (`__listProfiles`) that is not from this change (file has other uncommitted edits).

## Known limits (honest)
- Retrieval is lexical. "where do I work" does not find `employer: Acme` (no shared word). The fixture set pins this as a known miss; semantic matching would need embeddings, which this phase does not add.
- Episodes are stored as the user's transcript text (300 chars) mirrored from the legacy log, not as one-line overviews. Overviews are plan 92 S3.
- Memory is still plaintext on disk (encryption is P2). The legacy JSON files remain authoritative for one release (dual-read).
- Not run live: first launch migration on the real `%APPDATA%` data, and a real Worker round-trip with memory in the prompt.
