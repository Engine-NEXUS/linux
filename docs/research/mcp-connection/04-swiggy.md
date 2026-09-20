# Department: Swiggy Commerce OAuth (2026-09-20)

**Goal:** "order food" with no/expired Swiggy credential → request login,
open the auth flow, store + refresh silently. Applies identically to
Food/Instamart/Dineout (they share the `swiggy` vault service and the
same failure strings).

---

## 1. Current state (verified in tree)

- URLs registered, 1 tool wired (`search_restaurants`); `auth_token=None`
  on every call until vault phases land.
- Vault `swiggy` service exists with paste-token Save/Delete + badges;
  card hint admits *"OAuth login — coming in the Swiggy phase."*
- Failure strings ready: `Swiggy — reconnect it in Settings, Connections
  tab…` + expired variant; 401-clear-and-retry-once; `mcp_status` reports
  `reachable — login required` on 401.
- Refresh: only `google` auto-refreshes (`auth_vault.rs:206-229`); Swiggy
  returns `None` ("manual until their OAuth phases land").
- No Swiggy OAuth URL/PKCE anywhere in Rust or frontend.

## 2. Plan (spec-sequence, §02, on proven rails)

1. **Worker endpoints** (mirror Google's): `/oauth/swiggy-auth-url`
   (metadata discovery → registration → PKCE challenge → authorize URL)
   and token exchange + `/oauth/swiggy-token` (refresh). Host our Client
   ID Metadata Document on the Worker (preferred registration; DCR
   fallback).
2. **Frontend `connectSwiggy`** beside `connectOAuth`
   (`setup/oauth.ts:80-201`): same dual-channel completion (deep-link
   `nexus://oauth/` + 1.5s status poll + 5-min timeout), same
   `shell.open` browser handoff.
3. **Vault:** store access + refresh under `swiggy`; extend
   `refresh_service_token` to Swiggy (Google pattern — the fix for the
   Claude-family "silent mid-session drop" failure class).
4. **Card + voice:** auth-required → *"Swiggy needs a fresh login, sir —
   the card has the login button."* Card button runs `connectSwiggy`;
   completion auto-retries the failed call (shared resume, §06).
5. **Caveat on the card** (already documented): hosted Swiggy MCP needs
   Builders-Club approval for production; localhost dev is free — so a
   401 lists "dev token vs approval" as the two causes, not a mystery.

## 3. Tests to pin

- 401 → discovery → auth URL generated (mock Worker).
- Callback → vault holds refreshable token; `token_status` flips
  missing → live.
- Expiry → silent refresh, no user-visible failure (regression test for
  the exact Claude-issue failure mode).
- Step-up: scope challenge → union re-auth, bounded retries.
