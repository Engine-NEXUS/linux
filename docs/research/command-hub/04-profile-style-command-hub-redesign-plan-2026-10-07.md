# Command Hub redesign — "Profile Settings" style, Light/Dark, 400 px (plan, 2026-10-07)

Status: **plan only — nothing implemented.** Reference: the user's two-phone "Profile Settings" mock (light + dark).

## 1. User decisions (final)
| # | Decision |
|---|---|
| Log out | **No Log Out option** (user: "forget the logout option" — interpreted as *should not exist*; confirm if it was meant to stay) |
| Accounts | Show every added account (Google **and GitHub**), each removable by the user |
| MCP insight | number of **connected** MCP servers |
| Requests insight | **today** and **all-time** (both shown, today emphasised) |
| Window | compact card ≈ 400 × 780, right-docked, **blunt (small-radius) corners** |
| Theme | Light and Dark only, opaque; applies to **everything including the wake-up orb**; in Light the orb particles go **white → black** |

## 2. Reading of the reference image
Header: "Close" left, centred title (status bar, time/signal/wifi/battery, home bar, phone frame, gradient backdrop and the light phone's ~3° tilt are mock-up dressing and are dropped). Card 1: round avatar, name, email, hairline divider, centred blue action (**"Switch Account ⊙" → "+ Add Account"**). Card 2: chart icon, "Your Profile Insights", chevron; three value-over-label columns. Grouped rows (icon tile, title, grey subtitle, right chevron) under small grey section labels. Light: `#fafafa` rows on white; dark: rows ≈ `#262626` on ≈ `#1f1f1f`–`#2d2d2d` with hairline borders. **Colour values are estimates** (1024×768 source, tilted phone) — final values need an eyedropper pass against the original.

## 3. Target screen
Header `×` | "Command Hub".
1. **Accounts card** — one row per account: avatar, name/username, email or handle, primary ✓, remove control; then blue **+ Add Account**.
2. **Insights card** — **API keys** (count) · **MCP connected** (count) · **Requests** (today, with all-time beneath).
3. **Set up NEXUS** — API Keys · Audio · Display · Connections (each opens an in-window page with "‹ Back").
4. **Adjust the theme** — Change Theme (Light / Dark).
5. **Additional settings** — Remove Account (per-account control lives on the card; a single row here opens the same list). No Log Out.

Existing `DisplayTab`/`AudioTab`/`AuthTab`/`ConnectionsTab` (`SettingsSidebarApp.tsx`, 1726 lines, built for 740 px) are reused as pages but must be reflowed for ~360 px content width.

## 4. What is possible — the truth
| Item | Verdict | Detail |
|---|---|---|
| Drop status bar / home bar / time & wifi | **Yes** | trivial |
| Width ≈ 400, compact height | **Yes** | `commands.rs::sidebar_geometry` "settings" arm is 740 × (monitor−40); change + update the geometry tests |
| Light/Dark opaque themes | **Yes** | `sidebar/theme.ts` (`data-theme`, localStorage, `nexus:theme-change`) and `themeMode` already exist; Command Hub CSS needs token rewrite |
| Theme for *everything* | **Yes, with work** | each window is a separate WebView with its own `document`; they share origin so the `storage` event / a Tauri event can broadcast; every window's CSS (assistant sidebar, architect, PR list, companion HUD, stage captions/callouts) must adopt the tokens |
| Soft translucent grey rim + shadow, blunt corners | **Yes** | CSS border + shadow on a solid card inside a small transparent window margin |
| Real blur of the desktop behind the rim | **No** | DWM blur on a non-activating window renders opaque black (the 2026-10-01 blackout); not attempted |
| Account rows with photo + username | **Yes** | Google: `GoogleAccountProfile {name, email, picture}`; CSP already allows `img-src https:` so Google avatars load |
| GitHub as an account row | **Yes, with one fetch** | NEXUS caches no GitHub login/avatar today (`vault` holds only the token); read `GET /user` once with the token (login + `avatar_url`), cache it. Without a token there is no row |
| Add Account | **Yes** | Google loopback OAuth exists; GitHub OAuth exists (Worker flow) |
| Remove account (Google or GitHub) | **Yes** | `auth_vault::remove_google_account(email)`; GitHub via `vault_clear_token("github")` + clear the cached identity. Removes it **from NEXUS only**; it does not delete the Google/GitHub account itself and the UI will say so |
| API Keys page (add / replace / delete) | **Yes** | `auth_vault::{set_api_key, get_api_key, clear_api_key}` exist; need 3 new IPC commands (list masked, set, delete). Full keys are never displayed back |
| Insight: API key count | **Yes** | count of non-empty keychain keys (groq, gemini, cerebras + tokens that are keys) |
| Insight: connected MCP count | **Yes** | `mcp_connect_state` probes the 5 servers in parallel (≈5 s worst case): show "–" immediately, fill in when the probe returns; count = servers in the `ready` state |
| Insight: requests today / all-time | **No today; Yes after building a counter** | the only counter is `vision_usage.json` (Gemini/Groq vision, tour, ghost). 9Router, STT, Worker and MCP calls are not counted. A new `usage_counter.rs` (per-day buckets + all-time total, persisted) incremented at each outbound call site. **Counts start at zero on the day it ships — no back-fill** |
| Log Out | **Removed by decision** | NEXUS has no login of its own anyway |
| Pixel-identical to the image | **No — close** | layout, spacing, hierarchy, radius and light/dark tokens matched by measurement; exact colours/fonts are estimates (looks like Inter; the app would need Inter bundled to match) |

## 5. Orb in Light theme (the risky part — read carefully)
Today the orb renders **white/colour particles on black**. `voice-orb.js` line ~410: `gl.blendFunc(gl.ONE, gl.ONE)` — **additive blending**. Additive blending of *black* adds nothing, so simply "turning the particles black" makes them **invisible**. Making the Light orb real requires:
1. a theme uniform in the shader and a **normal (premultiplied alpha) blend** path in Light (`ONE, ONE_MINUS_SRC_ALPHA`) instead of additive;
2. a **dark palette** for each state — idle/listening (amber `userTint` → darker amber), thinking (electric purple + white nucleus → deep purple + black nucleus), speaking — because bright additive colours wash out on white; density/size/alpha retuned since additive glow stacking disappears;
3. the orb sits in the (since 2026-10-06) **obsidian capsule** (`#000000 !important`); in Light that capsule becomes light, so the capsule colour is theme-driven too;
4. 1:1 parity edits in `_paint2D` (CPU fallback);
5. **Dark must stay pixel-identical to today** (regression gate: snapshot the current dark frames per state before touching the shader).
Verdict: **possible, medium risk, needs live eyes on every state** (idle, listening, thinking, speaking, text-morph). I cannot judge the look from code; expect 1–2 tuning rounds.

## 6. Work breakdown (order)
1. **Tokens + shell**: `theme.css` tokens (light/dark), Command Hub shell 400 px, header, cards, rows, blunt corners, rim; geometry change + tests.
2. **Accounts**: unified `list_accounts` (Google + GitHub identity fetch/cache), add/remove commands, avatar fallback (initials), tests.
3. **API Keys page**: `list_api_keys` (masked last-4), `set_api_key`, `delete_api_key`; validation per provider (`keyFormat.ts` exists); tests that a full key never leaves Rust.
4. **Pages**: reflow Audio / Display / Connections into pages with Back navigation.
5. **Insights**: key count, connected-MCP probe (async), `usage_counter.rs` + call-site increments + today/all-time, tests (day rollover, persistence).
6. **Global theme**: broadcast + adopt tokens in assistant sidebar, architect, PR list, companion HUD, stage (captions, tour callouts/ring).
7. **Orb light theme** (section 5).
8. Docs (`docs/changes/92-…`, AGENTS.md entry) and live checklist.

## 7. Verification plan
Rust unit tests: sidebar geometry, key masking/validation, counter (rollover, concurrency, persistence), account list assembly, MCP-connected count from probe results. Vitest: theme resolver/broadcast, accounts list rendering states (0/1/many, GitHub-only), API-key add/replace/delete flows, insights formatting. Live: light/dark side-by-side against the reference image (measure padding/radius/row height), every orb state in both themes, remove-account on both providers, key replace round-trip, requests counter increments after real commands, 125/150 % scaling.

## 8. Risks / honest limits
* Light orb may need multiple tuning passes; additive→alpha blending changes the character of every state.
* "Everything" themed = touching ~6 windows' CSS; other concurrent sessions edit the orb/captions files — coordinate to avoid collisions.
* Request counts cannot be reconstructed for the past.
* GitHub avatar/handle needs the token and one network call; offline shows a generic icon.
* Settings struct rule: any new setting must be added to `NexusSettings` or it is erased on save (learned in change 91).
