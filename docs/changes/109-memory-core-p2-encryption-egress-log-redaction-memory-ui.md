# 109 — Memory Core P2: encryption at rest, cloud-send log, name redaction, "what I remember"

Plan: `docs/features/100-memory-core-plan.md` (phase P2). Follows change 108 (P1).

## What was built
- **Encryption at rest** (`memcore/crypto.rs`, `memcore/store.rs`). The whole database (records, FTS index, audit, egress log) lives in memory and is persisted as one sealed blob `memcore.enc` = `NXM1 | nonce | AES-256-GCM ciphertext+tag` (`ring`, already in the dependency tree). Whole-file sealing is deliberate: an FTS index over plaintext columns would leak the text the columns hide.
  - The 256-bit key lives in Windows Credential Manager. It is stored, then read back through a fresh handle: the in-memory mock backend fails that check, so a non-persistent key store can never own your data. If the key store is unusable the database stays a plain file and the Memory page says so.
  - A plaintext `memcore.db` from P1 is migrated into the sealed file, and only after the sealed copy is on disk is the old file (and `-wal`/`-shm`) zero-overwritten and deleted.
  - A wrong key, tampering or truncation is refused and **never** overwrites the snapshot; an encrypted store with no key available is refused rather than forked into a second plaintext file.
  - `wipe` clears records, index, audit and egress log, then **rotates the data key** (crypto-erase), so old ciphertext left in free disk blocks becomes unreadable.
  - Writes are batched during the legacy import (one seal instead of hundreds).
  - Honest scope: protects against disk theft, other Windows users and casual file reads. It does **not** protect against malware running as you, which can ask the same API for the key. Windows Recall-grade (TPM/enclave) protection is not available to this app.
- **Cloud-send log.** Every cloud turn records, encrypted and for 7 days, what you said (PII-redacted) and the exact memory text that went with it. `memcore_egress_log` IPC and the Memory page show it.
- **Name redaction** (`memcore/names.rs`, setting `memcoreRedactNames`, default off). Locally known people (contacts + profile) are swapped for `Person A/B/…` in the transcript and memory pack that leave the device, and swapped back in the reply and in what is stored locally. Whole-word, case-insensitive, full name before first name, stable labels over the sorted roster, round-trip tested. Only names NEXUS already knows are redacted.
- **"What do you remember".** Saying it (existing `MemoryAudit` intent) now also opens a sidebar card: *You told me* / *I picked up* / *Recent conversations*, each with where it came from and how old it is, plus the encryption state and the number of cloud sends. Markdown in stored values is escaped. The spoken answer stays short.
- **Command Hub → Memory page** (`hub/MemoryPage.tsx`, pure helpers + tests in `memoryModel.ts`): summary and encryption pill, name-redaction switch, per-item Forget with confirm, and the cloud-send log. IPC: `memcore_status`, `memcore_list`, `memcore_egress_log` (forget reuses `memory_forget`).

## Verify
- `cargo test --lib -- --test-threads=1`: 1072 passed, 0 failed (6 ignored dev helpers, plus 2 new ignored live keychain probes). +22 tests: crypto (round trip, fresh nonce, no plaintext in ciphertext, wrong key/tamper/truncation, hex, shred), store (encrypted round trip with no plaintext on disk, wrong key refused and snapshot untouched, plaintext→sealed migration + shred, rekey after wipe, batch persist, egress prune, provenance listing), names (7), memcore (status, wipe + rotation stays usable, redaction in the pack, card markdown).
- `cargo check --features custom-protocol,admin-brain`: clean, 0 warnings.
- Frontend: `tsc --noEmit` clean, vitest 204/204 (+4), `npm run build` ok.
- **Live keychain probe (run by hand, two separate processes):** the entry appeared in `cmdkey /list` and a second process read it back, then deleted it. This also closes the "keychain persistence" gap left open in change 107.

## Not verified / limits
- The Memory page and the sidebar card have not been looked at in the real window (no visual check).
- First launch on your real data: the P1 plaintext `memcore.db` → `memcore.enc` migration and shred have only been exercised on test directories.
- A Worker/9Router round trip with name redaction on has not been run live.
- Names redaction does not cover a stranger's name typed into a sentence, and the per-turn log stores the redacted transcript, not the original.
- Encrypted mode serializes the whole database on each write (fine at this size, a few hundred KB; revisit if memory grows by orders of magnitude).
