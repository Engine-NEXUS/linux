# Admin-Gated Worker Identity — Phase-by-Phase Implementation Checklist

**Date:** 2026-10-03
**Status:** READY TO EXECUTE (awaiting admin go)
**Companion docs:**
- Architecture: [`01-admin-gated-worker-identity-and-byok-architecture-2026-10-03.md`](./01-admin-gated-worker-identity-and-byok-architecture-2026-10-03.md)
- Feature spec: [`../../features/88-admin-gated-worker-identity-and-byok.md`](../../features/88-admin-gated-worker-identity-and-byok.md)

**Rule:** every phase = implement → gate ×2 (Rust serial, worker vitest ×2, frontend vitest ×2, tsc, build) → only then next phase.

---

## Verified current-state anchors (from the 2× cross-check, 2026-10-03)

| Anchor | Location |
|--------|----------|
| Client first-run identity gen | `src-tauri/src/lib.rs:795-826` |
| uuid_v4 | `src-tauri/src/network.rs:83-109` |
| Session struct + auto-open | `network.rs:72-167` |
| Transcript POST payload (`requester.{id,device_id}`) | `network.rs:169-296` |
| Worker fetch-handler route table | `server/worker/src/index.ts:~1833-1971` |
| All 7 `env.AI.run` sites | `index.ts:153, 259, 1403, 1581, 3081, 3233`; `external_llm.ts:134` |
| Transcript handler entry | `index.ts:1967-1969` → `handleTranscript` |
| D1 schema | `server/worker/schema.sql` (`api_keys` BYOK violation; `user_devices` unused) |
| Quota | `server/worker/src/quota.ts` (`usage_log` per user_id/day) |
| Admin-token precedent | `Env.NEXUS_ADMIN_TOKEN`, `/models/nlu/publish` gate `index.ts:1943` |
| Local keyring vault | `src-tauri/src/auth_vault.rs` (`com.nexus.assistant`, keyring crate) |
| Setup wizard (claim hook point) | `frontend/src/setup/SetupApp.tsx` (steps 0-3; Accounts=3) |
| Settings sidebar (identity card target) | `frontend/src/settings-sidebar/SettingsSidebarApp.tsx` |
| Protocol versioning | `server/worker/src/protocol.ts` (`PROTOCOL_VERSION="1"`, additive-only) |

---

## PHASE W1 — Worker: schema + identity endpoints (mode=off, zero behavior change)

### W1.1 Schema migration

- [ ] Append to `server/worker/schema.sql`: `profiles`, `devices`, `entitlements`, `profile_events`, `identity_migration` (exact DDL in architecture doc §5).
- [ ] Run: `npx wrangler d1 execute nexus-db --file=schema.sql --remote`.
- [ ] `DROP TABLE user_devices` (unused — verified zero read/write call-sites).

### W1.2 New module `server/worker/src/identity.ts`

- [ ] `newId(prefix)` — crypto.randomUUID → base64url-trim 22 chars; `prof_`/`dev_`.
- [ ] `sha256Hex(s)` — WebCrypto.
- [ ] `claimProfile(env, body)` — idempotency rules (arch §6.1): same provisional hints + no creds → return existing; `reinstall:true` → new; pending-cap 50; IP rate-limit via KV/`cache_entries` counter.
- [ ] `resolveProfile(env, requester, authHeader)` → `EntitlementResult` (decision table arch §7).
- [ ] `recordEvent(env, profileId, deviceId, event, detail)` — append-only.
- [ ] Token issue: 32-byte random → base64url; store SHA-256 hash only.

### W1.3 Routes (fetch handler)

- [ ] `POST /v1/profiles/claim` (no auth needed — this is the bootstrap).
- [ ] `GET /v1/profiles/me` (Bearer device token).
- [ ] `POST /v1/devices/rotate-token`.
- [ ] `DELETE /v1/devices/current`.
- [ ] `/health` additive fields: `"identity":"v1","entitlement":true` (protocol additive rule respected; bump nothing).

### W1.4 Env additions

- [ ] `wrangler.toml`/docs: `MIGRATION_MODE` secret (`off` default).
- [ ] New secret: `NEXUS_ADMIN_IDENTITY_TOKEN` (distinct from `NEXUS_ADMIN_TOKEN`).

### W1.5 Worker tests (vitest, `server/worker/src/__tests__/identity.test.ts`)

- [ ] Claim: fresh → pending + one-time token; re-claim idempotent; reinstall flag → new profile; pending-cap 429; rate-limit 429.
- [ ] `/me`: valid token → full body; bad token → 401; revoked device → 403 `revoked`.
- [ ] Rotate: old token invalid after rotation; event logged.
- [ ] Hash: `device_token_hash` ≠ plaintext anywhere in response/DB rows.

**Phase gate ×2:** `cd server/worker && npm test` (×2) + deploy to dev Workers URL + `curl /health` shows additive fields. Zero client impact verified by sending a legacy transcript POST (mode=off ⇒ behaves exactly as today).

---

## PHASE W2 — Worker: entitlement gate in transcript path + admin plane

### W2.1 Entitlement gate

- [ ] New `server/worker/src/entitlement.ts`: `gateAI(request, env)` per arch §7 (single choke).
- [ ] `handleTranscript` (index.ts:~2530 area): resolve → gate → **deny path**: 403 `{error:"worker_ai_not_enabled", status}` + `profile_events{event:"denied"}` + **NO** `env.AI.run`, **NO** external LLM call, **NO** `incrementUsage`.
- [ ] Type-enforcement: `classifyIntent`, `summarize`, `external_llm` functions require an `EntitlementContext` parameter (compile error if called without a successful gate result).
- [ ] Grep gate script `server/worker/scripts/check-ai-gate.mjs` (pattern after `scripts/check-turn-ends.mjs`): fails CI if `env.AI.run` appears outside `entitlement.ts`-guarded modules; negative test for the gate itself.
- [ ] `MIGRATION_MODE` semantics implemented exactly per arch §12.2 (off/strict/revoke-legacy) at the `identity_migration` lookup.

### W2.2 Admin plane

- [ ] `GET /v1/admin/profiles/pending|list`, `POST /v1/admin/profiles/approve|suspend|revoke`, `POST /v1/admin/devices/revoke`, `POST /v1/admin/entitlements/set`, `GET /v1/admin/profiles/:id/events`.
- [ ] Timing-safe bearer compare; never log the header (mirror `audit_line` discipline from Phase-A MCP work).
- [ ] `revoke` cascade: devices → status revoked; `oauth_tokens` rows for profile deleted; entitlements zeroed; all logged.

### W2.3 Worker tests

- [ ] Decision table §7 — every row (incl. `expires_at` boundary, D1-error fail-closed).
- [ ] Deny ⇒ spy asserts zero AI calls, zero usage increments, no transcript persistence.
- [ ] Admin: 401 on bad/missing token; approve/suspend/revoke state transitions + events; revoke cascade deletes oauth rows.
- [ ] Migration: mapped legacy id works in off+strict; unmapped legacy 403 in strict; off-mode unmapped works.
- [ ] Isolation: profile A token cannot read B `/me`; A's revoke doesn't touch B's counters.

**Phase gate ×2:** worker vitest ×2 + the grep gate + live drill against dev deployment (claim → pending → approve → transcript 200 → revoke → 403).

---

## PHASE C1 — Client: claim integration (Rust)

### C1.1 Identity state

- [ ] `nexus-config.json` gains additive fields: `"identity": "provisional" | "canonical"`, `"profileId"`, `"legacyUserId"` (existing `userId`/`deviceId` remain readable for the migration window).
- [ ] `lib.rs:795-826`: keep provisional UUID generation but stamp `identity:"provisional"`. **Never regenerate on retry** (arch §9.4).

### C1.2 Claim command

- [ ] `commands.rs`: `claim_profile(app)` — reads config, POSTs claim, writes canonical IDs, stores device token via `auth_vault` keyring entry `nexus:device_token`.
- [ ] `commands.rs`: `get_identity_status(app)` → `{state: provisional|pending|approved|denied, profile_id, device_name, reason}` (state cached in memory + refreshed on poll).
- [ ] `network.rs`: `send_transcript` adds `requester.profile_id` (canonical clients) and `X-Nexus-Device-Token` header; legacy clients unchanged.
- [ ] Retry scheduler: claim/`/me` poll every 10 min while pending (tokio task, cancellable); no repeated UUID minting.

### C1.3 Rust tests

- [ ] Claim success writes config + keyring (temp-dir harness, keyring stubbed trait).
- [ ] Network fail → IDs unchanged, retry queued.
- [ ] 403 mapping → denied state enum correct for each `status` string.
- [ ] Legacy config (no new fields) boots unchanged.

**Phase gate ×2:** `cargo test --lib -- --test-threads=1` ×2 + `npx tsc --noEmit`.

---

## PHASE C2 — Client: frontend UX

### C2.1 Setup wizard

- [ ] `SetupApp.tsx` Accounts step: after connects, call `claim_profile`; render three-state banner (pending amber / approved green / denied red + reason).
- [ ] Pending does NOT block "Finish" — local features work; banner persists.

### C2.2 Orchestrator surface

- [ ] `net/orchestrator.ts`: map `worker_ai_not_enabled` (403) to distinct spoken line *"Cloud access isn't enabled for this device yet, sir."* — pinned test.
- [ ] Suppressed state (pending): silent local-only operation + one-time inform; NOT nagging on every miss.

### C2.3 Settings sidebar

- [ ] Identity card: state pill (provisional/pending/approved/denied), device name, profile_id tail (last 6 chars), "Disconnect this device" (self-revoke → confirm dialog).
- [ ] Accounts tab copy: "Your API keys stay on this laptop — NEXUS cloud never stores them."

### C2.4 Frontend tests

- [ ] Banner state rendering per identity status.
- [ ] Denied spoken-line pin (extends `orchestrator.test.ts` denial test family).
- [ ] Self-revoke confirm + config cleanup.

**Phase gate ×2:** frontend vitest ×2 + tsc + `npm run build`.

---

## PHASE M1 — Migration + BYOK deprecation

- [ ] Backfill script (admin-run, via wrangler d1 execute or a small node script): for each distinct legacy `user_id` in `usage_log`/`oauth_tokens` → create profile (`provision_hint` = legacy id, status=pending) + `identity_migration` row.
- [ ] Admin reviews + approves real devices (live runbook in arch §8.3).
- [ ] `api_keys`: migration job deletes all rows; endpoints switch to 410 `{error:"apikeys_deprecated"}`; log count via `profile_events`.
- [ ] Frontend: remove any add-key-to-cloud affordances (verified: Connections tab hints updated).
- [ ] Payload self-check: Rust unit test asserting transcript payloads contain no provider-key patterns (list known key prefixes from settings fallback reads).

**Phase gate ×2:** full worker suite ×2 + frontend ×2 + Rust lib ×2.

---

## PHASE M2 — Mode flips (admin live operations, reversible)

- [ ] Set `MIGRATION_MODE=strict` → legacy unmapped = 403. Live regression: your own laptop (mapped + approved) unaffected.
- [ ] Announce window (family users) → then `MIGRATION_MODE=revoke-legacy`.
- [ ] Final audit: `api_keys` empty; `user_devices` dropped; no transcripts/history columns exist anywhere in schema.
- [ ] Rollback path documented + rehearsed: flip mode back to `off`, redeploy (no data destruction until the final delete step).

---

## Final acceptance drill (live, admin-run)

1. Fresh VM/second laptop → install → setup → claim → pending banner. **curl admin pending list shows it.**
2. Approve → within 10 min (or on demand) cloud works; PR analysis consumes neurons under new profile.
3. Say a wake command → transcript → approved flow (existing 9Router/Worker behavior intact).
4. Revoke → next voice request speaks the distinct denial line; local features still work.
5. Two-laptop isolation: laptop B's revoke does not affect laptop A (both approved).
6. Old legacy install (pre-identity binary) → strict mode → 403 with legacy notice (expected).
7. BYOK audit: install on clean laptop, add Groq key locally, run cloud query → `wrangler d1 execute "SELECT * FROM api_keys"` returns empty; `oauth_tokens` only where user opted into Google/GitHub.

---

## Definition of Done (repo-standard gates, each ×2)

```
cd server/worker && npm test              # ×2
node server/worker/scripts/check-ai-gate.mjs
cargo test --lib -- --test-threads=1      # ×2
npx vitest run (frontend)                 # ×2
npx tsc --noEmit
npm run build
cargo build --release --features custom-protocol,admin-brain
```

Docs to add at completion: `docs/changes/71-admin-gated-worker-identity-and-byok.md` (implementation record with deviations) + AGENTS.md entry. Per repo rule, docs+code pushed in lockstep to `Engine-NEXUS/WINDOWS` on explicit request.
