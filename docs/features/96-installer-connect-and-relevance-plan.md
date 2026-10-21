# Installer Connect UX + 50% Profile Bootstrap + Relevance Engine Plan (2026-10-07)

**Ask:** (1) installer Google/GitHub connect must be neat, clear, and teach the user what NEXUS learns — bootstrapping ~50% user knowledge with zero friction; (2) can Google connect fetch phone + address? (3) how memory decides relevant vs irrelevant.
**Rule:** plan only. Truth-first throughout (what's possible vs not is stated, not promised).

## Part A — What accounts can actually teach (verified)

**Google (direct loopback OAuth, oauth.rs):** base scopes today = email + profile + photo (`userinfo.email/profile`, real `picture` URL). Name/email/photo: guaranteed. **Phone/address: NOT guaranteed** — they need 2 extra sensitive scopes (`user.phonenumbers.read`, `user.addresses.read`) AND the data must exist in the Google Account (usually absent) AND consent-screen friction rises (sensitive scopes risk unverified-app warnings). Decision: base scopes stay; phone/address as an OPTIONAL progressive button ("Add phone & address" → re-consent, graceful absence, never blocks).
**GitHub (Worker OAuth):** login, name, email (often null when private), avatar_url, bio, company, location. Never a phone. Company/location/bio are good profile signals when present.

## Part B — Installer card redesign (neat & clear)

- Keep: brand icon rows, opening/waiting/done phases, checkmarks, identity banner, skip affordance (connect must NEVER block install).
- Fix: connected subtitle shows the REAL account (name + email + photo avatar) — today it hardcodes "nexus-assistant@google.com" (`SetupApp.tsx` installer list).
- Add per-row plain-language permission lines ("Sees your name, email, photo — stored on this device, revokable anytime") + a "What NEXUS learned" panel after connect (name/email/photo ✓, phone/address status or "not shared", GitHub login/bio/location) with a "Review in audit" link. Every learned item source-tagged and visible in "what do you remember".
- Verify-and-bridge (planned verification first): whether setup-connected Google lands in the vault registry (`google_get_accounts`) — if not, settings shows zero accounts after setup; bridge it or route setup through the same flow.

## Part C — 50% bootstrap mapping (connect → profile, all source-tagged)

Google: name→profile.name, email→primary_email, picture→avatar, locale→weak city/language hint (low-confidence, labeled). GitHub: login→github_user, name/bio/company/location→facts (`source: github`). Phone/address (if granted+present)→facts (`source: google`). Extend M1 `refresh_user_profile` inputs; audit gains a "learned from Google/GitHub on <date>" section. Realistic scope statement for UI copy: identity + contact surface at connect; routines/preferences accrue over days (S4) — "50%" means bootstrap, not omniscience.

## Part D — Relevance engine (relevant vs irrelevant memory)

Current: turn_relevance tiers + substring recall + secrets denylist. Missing decisions, specified here (mem0's ADD/UPDATE/DELETE adapted deterministically, no LLM):
- **D1 type gate (write_policy, pure + tested):** reminder/commitment/preference/fact/people/routine → durable stores; greeting/ack/small-talk → overview-only, never durable.
- **D2 novelty gate:** same key exists → update if value differs (log UPDATE event), skip if identical. Kills duplicates at write time.
- **D3 contradiction handling:** correction ("no, Mumbai now") → UPDATE + fresh timestamp; "forget X" → DELETE (M0 already deletes — wire the same path for mined facts).
- **D4 irrelevance decay:** explicit facts never decay; mined facts 45d half-life (M3 plan); user "that's not important" → immediate forget.
- **D5 retrieval relevance:** M3 scoring (separate plan) — referenced, not duplicated here.

## Acceptance & ordering
B-verify (registry bridge check) → A scopes decision → B card + C mapping → D gates. Each: unit tests (parsers, gates, mapping, decay) + gates + live script (connect → learned panel truthful incl. absent phone; audit shows sources; duplicate fact collapses to one UPDATE; contradiction revises; chit-chat never lands in facts).
