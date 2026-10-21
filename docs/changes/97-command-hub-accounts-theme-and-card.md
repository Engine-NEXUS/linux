# Command Hub Accounts — Opaque Themes + Carbon-Copy Card (Phases A+B, 2026-10-07)

**Plan:** `docs/features/94-command-hub-accounts-carbon-copy-plan.md` (A theme + B accounts executed; C–F queued).
**Directives honored:** transparency out (opaque light/dark only); phone-width card; no mobile topbar; truth-only scoping.

## What changed
- **A. Theme foundation:** `theme_mode` (`dark` default / `light`) in `NexusSettings` (persistence-safe); `sidebar/theme.ts` (resolve/apply/event, dark-fallback) + tests; `UnifiedSidebar` applies on mount + follows live changes; `unified.css` opaque `--hub-*` token system (iOS grouped values) + `.hub-phone-col` (380px); Change Theme segmented row in AuthTab.
- **B. Accounts carbon copy:** profile header card (primary/first account 56px photo, name, email + Add Account link-button); per-account iOS rows (40px photo, name, email, chevron, Primary badge; tap row promotes, Remove stays explicit); initial-letter fallback tiles (never broken images); custom-creds drawer untouched below; GitHub/API-keys sections untouched for C–F.
- Verified pre-existing assets reused, not rebuilt: Google `picture` URLs (real, from userinfo), connect/primary/disconnect commands, vault keychain.

## Verify
- tsc 0; vitest **191/191** (2 new theme tests); Rust **1005/1005** serial; release **91.5 MB**; zero new warnings (23 pre-existing).
- One JSX imbalance from the restructure (dropped section opener) caught by tsc, fixed.
- Uncommitted. Live script (`nexus start` → Command Hub → Accounts): card reads phone-width in both themes; add second Google account → 2 rows; tap row → primary flips; Remove → row gone; theme toggle repaints instantly; zero transparent surfaces in this view.
