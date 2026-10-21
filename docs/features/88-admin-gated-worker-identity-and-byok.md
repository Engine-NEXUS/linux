# Feature 88 — Admin-Gated Worker Identity & Per-Laptop BYOK

**Date:** 2026-10-03
**Status:** SPEC (implementation not started)
**Owner:** Admin (Chitkul Lakshya) — sole approver of cloud AI access
**Architecture detail:** [`docs/research/worker-identity/01-admin-gated-worker-identity-and-byok-architecture-2026-10-03.md`](../research/worker-identity/01-admin-gated-worker-identity-and-byok-architecture-2026-10-03.md)

---

## 1. Problem Statement

Today any device that knows the Worker URL can consume Workers AI inference and the Worker's LLM relays with zero registration, zero approval, and no identity binding. The admin has **no control** over who uses the cloud tier. Meanwhile the D1 database stores user provider API keys centrally (`api_keys` table), which the product direction explicitly forbids: **each laptop is sovereign** — its own profile, its own chat history (local), its own API keys (local).

## 2. The Model (user-confirmed directives)

1. The Worker (with D1) creates a canonical profile when a new laptop installs and claims.
2. The profile lands **pending**. Only the admin can flip it to **approved**.
3. D1 stores **only** identity (`profile_id`, `device_id`), entitlement state, and operational counters.
4. Chat history, transcripts, memory, diary — **local only**, never in D1.
5. Gemini/Groq/Cerebras keys — **local only** (OS keyring), never uploaded. Existing central `api_keys` storage is deprecated and deleted.
6. Unapproved ⇒ no Workers AI, no Worker LLM relay, no quota burn — fail closed.

## 3. User-Visible Behavior

### 3.1 First install (new laptop)

1. Setup wizard runs as today (Persona → Permissions → Preferences → Accounts).
2. On Accounts step, NEXUS claims a profile with the Worker.
3. Setup shows: **"NEXUS Cloud — awaiting admin approval"** (amber banner + spoken line once).
4. Everything local works during pending: wake word, hotkey, local STT, offline commands, local memory/diary.

### 3.2 After admin approval

- Client discovers approval on next poll (10 min) or next transcript attempt.
- Banner flips green: "Cloud connected". Full cloud tier active (Workers AI, LLM relay, PR analysis, research synthesis).

### 3.3 Denied / suspended / revoked / expired

- Distinct spoken line: *"Cloud access isn't enabled for this device yet, sir."* (or reason-specific variant).
- Settings → Identity card shows exact state + admin-contact hint.
- Cloud features off; local features unaffected.

### 3.4 Admin experience (Phase 1: CLI)

```bash
curl -s -H "Authorization: Bearer $NEXUS_ADMIN_IDENTITY_TOKEN" \
  "$WORKER/v1/admin/profiles/pending"        # see who wants in
curl -s -X POST ... /v1/admin/profiles/approve -d '{"profile_id":"prof_..."}'
curl -s -X POST ... /v1/admin/profiles/revoke -d '{"profile_id":"prof_..."}'
```

Approval grants Workers AI + LLM relay scopes with optional quota tier + expiry. Revocation is instant and cascades (device tokens invalidated, OAuth tokens deleted).

## 4. What Changes / What Doesn't

| Area | Change |
|------|--------|
| D1 schema | +`profiles`, +`devices`, +`entitlements`, +`profile_events`, +`identity_migration`; −`api_keys` (deleted), −`user_devices` (dropped) |
| Worker routes | +claim, +me, +rotate-token, +self-revoke, +admin suite; transcript endpoint gated; `/apikeys/*` → 410 |
| Client startup | Provisional IDs → claim → canonical IDs; device token in keyring |
| Setup wizard | Accounts step: claim + status banner |
| Settings sidebar | Identity card (state, device name, revoke-self) |
| Chat/history | NO change — stays local (already the case) |
| STT/TTS/wake/orb/ghost | NO change |

## 5. Explicit Non-Goals

- No cloud chat history (ever, without a new signed directive).
- No central provider-key storage (ever).
- No self-service signup into AI access.
- No admin UI in Phase 1 (CLI is the interface).
- No multi-user account linking in Phase 1.

## 6. Security Properties Delivered

1. **Default-deny cloud AI** — the Worker refuses inference to anyone the admin hasn't approved.
2. **Cloned-configuration resistance** — identity without the device token is useless; tokens rotate and revoke.
3. **No server-side user secrets** — a D1 compromise exposes no provider keys.
4. **Auditability** — every register/approve/deny/revoke event is append-only logged.
5. **Fail-closed** — infrastructure errors deny rather than allow.

## 7. Acceptance Criteria

- [ ] Fresh install claims; status `pending`; zero AI calls consumed while pending (Worker test proves `env.AI.run` never invoked).
- [ ] Admin approves → next poll unlocks cloud; quota counters increment under `profile_id`.
- [ ] Revoke → instant 403 on next request; oauth rows deleted; device token dead.
- [ ] Two laptops never share quota rows, oauth rows, or entitlements (isolation test).
- [ ] Legacy `user_id` installs keep working in `MIGRATION_MODE=off`.
- [ ] `/apikeys/*` return 410; stored rows deleted at migration.
- [ ] Transcript request payloads provably contain no provider-key strings.
- [ ] All repo gates pass ×2 (cargo lib serial, vitest, worker vitest, tsc, build, release binary).

## 8. Phases

| Phase | Content | Depends on |
|-------|---------|------------|
| P1 | Worker: schema + claim + me + rotate + admin endpoints + entitlement gate (mode=off) | — |
| P2 | Client: claim integration, keyring token, pending/denied UX, identity card | P1 |
| P3 | Migration backfill + BYOK deprecation/deletion + admin review of existing users | P1+P2 |
| P4 | `strict` → `revoke-legacy` mode flips; recover flow (optional) | P3 + admin go |

## 9. Risks

| Risk | Mitigation |
|------|-----------|
| Gate accidentally blocks legitimate users at deploy | `MIGRATION_MODE=off` default; rollout is staged |
| Pending spam floods admin list | Rate limit + pending cap + cleanup |
| User confusion in pending state | Spoken line + setup banner + docs |
| Admin token loss | wrangler secret re-set; tokens never cached client-side |
