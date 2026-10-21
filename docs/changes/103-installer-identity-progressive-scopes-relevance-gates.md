# Installer Identity Learn + Progressive Contact Scopes + Relevance Gates (2026-10-07)

**Plan:** `docs/features/96-installer-connect-and-relevance-plan.md` (B-verify → A → B → C → D, all executed).
**Rule honored:** plan items only; Claude's C–F lanes untouched; no token bridge, ever.

## B-verify verdict (no-bridge decision)
Setup OAuth is Worker-mediated (tokens in D1, device never sees them); settings OAuth is native loopback (tokens in OS keychain). Bridging tokens device-side is the wrong security direction — decided: NO token bridge (documented in code). Setup learns identity via token→provider-API (never stored) + `memory_seed_setup_identity` (user.json gaps only).

## What changed
- **A. Progressive scopes:** `base_scope_list(false)` keeps install consent minimal (scope-tier test locks it); `connect_extended_profile()` re-consents with `user.phonenumbers.read` + `user.addresses.read`, People API fetch (None on any failure — absence is normal), re-consent merge preserves primary/added_at/old data. `GoogleAccountProfile.phone/address` (`#[serde(default)]` — legacy profiles parse). `google_connect_extended` command + settings per-row button + display.
- **B. Installer card:** hardcoded "nexus-assistant@google.com"/"GitHub User" replaced with real identity; powers-labels per row; "What NEXUS learned" panel (avatar, source tags, audit pointer); `fetchSetupIdentity` (google userinfo + github api/user) + seed invoke; new setup.css panel styles.
- **C. 50% mapping:** `refresh_user_profile` consumes vault primary (email/phone/address); `UserProfile.phone/address/avatar`; audit speaks contact surface when present, silent when absent.
- **D. Relevance gates:** `save_learned_facts` update-only-write (identical re-learn = byte-identical file, no timestamp churn) + test; contradiction = UPDATE via same path; forget = M0 path.
- **Honest deviations:** locale→city hint dropped (low-value inference); D1 formalized as pattern-allowlist (only 5 families can write — no separate fn needed).

## Verify (triple cross-check)
- Pass 1 (per-edit): targeted suites as each piece landed (memory 24, google 42, oauth scope test, seed/upsert tests).
- Pass 2 (full gates): tsc 0; vitest **200/200**; cargo **1025/1025** serial; release **91.6 MB**; zero new warnings.
- Pass 3 (plan-vs-code): every plan bullet reconciled above; deviations recorded, not hidden.
- Uncommitted. Live script (`nexus start` + fresh setup): connect Google → real name/email/photo + learned panel → audit lists them; settings "Add phone & address" → re-consent → phone shows (or button reappears if absent); duplicate episode → facts.json byte-identical; "I live in Mumbai now" revises Hyderabad.
