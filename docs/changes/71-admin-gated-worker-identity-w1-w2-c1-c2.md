# 71 — Feature 88 W1+W2+C1+C2: Admin-Gated Worker Identity & Per-Laptop BYOK

**Date:** 2026-10-03
**Status:** Implemented, all gates ×2 green.
**Plan docs:** `docs/research/worker-identity/01-admin-gated-worker-identity-and-byok-architecture-2026-10-03.md` + `02-implementation-checklist-admin-gated-identity.md` + `docs/features/88-admin-gated-worker-identity-and-byok.md`

## What shipped (verified)

### W1 — Worker identity registry
- `server/worker/schema.sql`: added `profiles`, `devices`, `entitlements`, `profile_events`, `identity_migration`; dropped unused `user_devices`.
- `server/worker/src/identity.ts`: claim handshake (idempotent; provisional-hint retry returns status WITHOUT token), one-time device token (SHA-256 stored only), IP rate-limit (5/h) + pending cap (50), rotation, self-revoke, legacy migration modes (off/strict/revoke-legacy).
- Routes: `POST /v1/profiles/claim`, `GET /v1/profiles/me`, `POST /v1/devices/rotate-token`, `DELETE /v1/devices/current`. `/health` additively advertises `identity:"1"` + `entitlement_gate:"1"`.

### W2 — Entitlement gate + admin plane
- `server/worker/src/entitlement.ts`: `gateTranscript` — deny path does zero `env.AI.run` calls, zero external-LLM calls, zero usage increments; fail-closed on lookup errors.
- `handleTranscript` gates right after `requester.id` validation, before intent classification. Denials return `403 {error:"worker_ai_not_enabled", code, status}` + audit event (metadata only).
- Quota + OAuth credential lookups re-keyed to `credentialKey` (= canonical profile_id for new clients; legacy user_id unchanged).
- `server/worker/src/admin.ts` + routes under `/v1/admin/*` (pending/list/approve/suspend/revoke/devices-revoke/entitlements-set/events), timing-safe token compare, revoke cascade deletes the profile's `oauth_tokens` rows.
- `server/worker/scripts/check-ai-gate.mjs`: CI gate — pins gate-before-AI ordering and the 7-site `env.AI.run` inventory.

### C1 — Rust client identity
- `src-tauri/src/identity_state.rs` (new): identity config block in `nexus-config.json` (`identity`, `profileId`, `lastKnownStatus`, `deviceName` — additive, legacy fields preserved), claim + status refresh + disconnect HTTP flows, bounded 10-min pending poll (~3h cap), first-run auto-config stamped `identity:"provisional"`.
- IPC: `claim_profile`, `get_identity_status`, `refresh_identity_status`, `disconnect_device` (all registered in `lib.rs`).
- `orchestrator.rs`: `build_worker_payload_ident` adds `requester.profile_id`; device token sent as `Authorization: Bearer` from keyring (never plaintext config).
- `network.rs::send_transcript`: same payload/header wiring.

### C2 — Frontend UX
- `frontend/src/setup/identityBanner.ts` (pure, tested): banner derivation for pending/approved/denied/provisional states.
- `SetupApp.tsx`: Accounts step claims on entry; amber "Awaiting admin approval" / green "Cloud connected" / red denied banners; offline → dim retry banner.
- `SettingsSidebarApp.tsx` Connections tab: Identity card (state pill, device name, profile tail, Disconnect button → self-revoke → config reset to provisional).
- Distinct denial spoken lines are Rust-side (`denial_spoken_line`, pinned test); the frontend error handler speaks them verbatim — no frontend change needed.

## Incidents during implementation (recorded)

1. **Set-Content UTF-8 corruption of `index.ts`**: a PowerShell `Get-Content | Set-Content` bulk edit mangled all non-ASCII (em-dashes → mojibake) and destroyed the uncommitted working-tree state beyond HEAD recovery. Recovered the intact pre-edit file from opencode's snapshot repo (`~/.local/share/opencode/snapshot` — shadow git repos keyed by session snapshot hashes in the SQLite DB), reversed the CP437 double-encoding byte-exactly, re-applied the lost edits with the edit tool. **Rule: never bulk-edit files via PowerShell content cmdlets — use the edit tool.**
2. `worker/worker-identity-migration.sql` was created and deleted twice due to writing errors; final version deferred to schema.sql as the single source (no drift risk).
3. Mini-D1 mock needed prepare-level (unbound) `.first()` support and deferred execution (a prepare-time exec polluted tables with empty-arg rows).

## Verify (each ×2)

- Worker: `cd server/worker && npm test` → **97/97** (identity 36, entitlement e2e 7, github-routing 15, cache 9, protocol 4, quota 6, research 20); `node scripts/check-ai-gate.mjs` → OK 7/7.
- Rust: `cargo test --lib -- --test-threads=1` → **848 passed, 1 ignored**.
- Frontend: `npx vitest run` → **154/154** (21 files); `npx tsc --noEmit` → clean; `npm run build` → clean.
- Release: `cargo build --release --features custom-protocol,admin-brain` → `nexus.exe` 51.4 MB.

## Pending (admin-run, M1/M2)

1. Apply schema: `npx wrangler d1 execute nexus-db --file=schema.sql --remote`
2. Set secrets: `npx wrangler secret put NEXUS_ADMIN_IDENTITY_TOKEN` (+ optional `MIGRATION_MODE`)
3. `npx wrangler deploy`
4. Live drill: fresh install → claim → pending → approve via curl → transcript 200 → revoke → 403 (runbook in doc 02 §Final acceptance drill)
5. BYOK deprecation (410 on /apikeys/*, delete stored rows) — M1 checklist
