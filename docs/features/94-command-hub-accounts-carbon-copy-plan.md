# Command Hub Accounts Redesign — Carbon-Copy Plan (2026-10-06)

**Reference:** mobile Profile Settings (light + dark). **Directives:** forget transparency (opaque light/dark only); phone-width card in the sidebar; no mobile topbar; Switch Account → Add Account + per-account rows (photo + username); Account Security → API Keys (add/delete/replace); Insights = API-key count + MCP count + API request count; Log out; Delete account. M3 deferred.

## Truth table (verified against code — no guessing)

### ✅ Possible (all dependencies exist)
- **Opaque light/dark themes:** CSS-only inside the existing window (card goes opaque; window chrome untouched). New `themeMode` setting (`light`/`dark`, default dark). Light: iOS grouped `#F2F2F7` bg + `#FFFFFF` cards + `#000` text; dark: `#000000` bg + `#1C1C1E` cards + `#FFF` text; accent `#0A84FF`/`#007AFF`.
- **Phone-width column:** sidebar keeps its window; Accounts view renders a centered `max-width: 380px` rounded (18px) card — "this size" without resizing the window.
- **Account rows:** photo + name + email per Google account — `picture` is REAL (Google userinfo `picture` URL, fetched at connect, `oauth.rs:211`). Existing commands cover add (`google_connect_account`), primary (`google_set_primary_account`), disconnect/remove (`google_disconnect_account` → vault purge).
- **API Keys add/delete/replace:** `set_api_key`/`clear_api_key` exist (keychain, values never leave it). Needs ONE new command: key-presence list (names + has-key booleans, never values).
- **MCP count:** `mcp_connect_state()` returns per-server cards with state — ready-count is a filter. Exists.
- **Log out / Delete:** disconnect per account exists; global Log out = disconnect-all + Worker session clear (scoped in phase E). Delete = remove + keychain purge + confirm gate.
- **No topbar:** trivially yes — never build phone chrome in a desktop sidebar.

### ⚠️ Partial (scoped honestly)
- **iOS pixel-copy:** system fonts + inline SVG icons approximate SF Symbols; no exact San Francisco rendering on Windows. Alignment/spacing/radii carbon-copied; glyphs closest-match.
- **Non-Google photos:** GitHub avatar only if connected (API fetch); else initial-letter tile. Never broken-image icons.
- **Request count:** NEXUS-routed counts only — needs a NEW Worker route (`GET /usage` over existing `getUsage`, quota.ts:46) + local lifetime counters as offline fallback. Provider-dashboard totals (Groq/Gemini consoles) are NOT programmatically accessible — UI labels it "via NEXUS".
- **Key registry split:** keys live in TWO stores today (settings.json plaintext groq/gemini/cerebras + keychain vault). Count = union of both; migration to keychain-only is noted, not in scope.
- **Service rows:** Google rows first-class; GitHub/Spotify/Vercel as simplified rows (name + connected badge + disconnect) via existing oauth-status/token checks.

### ❌ Not doing
- iOS-native components/haptics/exact fonts; real-time provider totals; displaying key VALUES anywhere (invariant — counts + presence only); any transparency in this view (directive).

## Phases
**A. Theme foundation** — `themeMode` setting + opaque light/dark CSS system + Change Theme row (Light/Dark segmented). Tests: computed-style assertions per theme.
**B. Accounts card** — profile header (first/primary account photo+name+email), per-account rows (photo, name, email, primary badge, set-primary, disconnect), Add Account button + flow, empty state. Reuses existing commands; no new Rust except where noted.
**C. API Keys section** — new `vault_key_presence` command (names + booleans, zero values) + add/update/delete commands + UI (masked rows, add form, replace = update, delete with confirm). Registry definition documented: settings keys + vault services.
**D. Insights trio** — key count (registry union), MCP ready count (`mcp_connect_state` filter), request count (new Worker `GET /usage` + local counters fallback, "via NEXUS" label). Worker route + test; Rust counter + test.
**E. Log out + Delete + layout** — per-account disconnect, global Log out (disconnect-all + session clear + confirm), Delete account (purge + confirm), phone-width column, 18px card, alignment pass, no topbar — verified absent.
**F. Gates** — tsc, vitest (new suites per phase), cargo (new commands), build. Live script: both themes, add/delete account, add/replace/delete key (value never shown), counts correct, log out, delete.

## Acceptance
Side-by-side with the reference: same information hierarchy, same row order (profile → add → insights → rows → theme → additional → delete → log out), aligned spacing, both themes, zero transparent surfaces in this view, zero key values on screen, all counts truthful with "via NEXUS" where scoped.
