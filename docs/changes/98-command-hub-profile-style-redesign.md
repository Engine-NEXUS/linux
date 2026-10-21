# Change 98 — Command Hub: profile-style redesign, API Keys, insights, global Light/Dark + light orb (2026-10-07)

**Plan:** `docs/research/command-hub/04-profile-style-command-hub-redesign-plan-2026-10-07.md` · builds on change 97 / feature 94 (theme foundation + Google account rows, done by another session — reused, not redone).

## User decisions applied
No Log Out · accounts removable (Google **and** GitHub) · "MCP" = connected servers · requests = today **and** all-time · compact card with small ("blunt") corners · Light/Dark applies to everything **including the wake-up orb, whose particles go white → black in Light**.

## What was built
**Rust**
| Piece | Detail |
|---|---|
| `usage_counter.rs` (new) | persisted `usage_stats.json`; today + all-time, per kind (`vision`, `llm`, `stt`, `worker`, `mcp`); pure `Counter` (date injected → roll-over tested); `get_usage_stats` IPC; `bump()` at the call sites: `router.rs` (2 LLM POSTs), `stt_groq.rs` (3), `network.rs` Worker POST, `mcp_client.rs` call, `vision::record_use`. **Counts start the day this ships — no back-fill; TTS (Edge) and the Gemini key tests are not counted.** |
| `api_keys.rs` (new) | `api_keys_list` (masked tail only), `api_key_set` (structural validation, keychain + settings.json mirror), `api_key_delete` (keychain **and** settings.json, incl. legacy snake_case), `api_keys_count`. Keys for `groq`, `gemini`, `cerebras`. |
| `save_settings` | an EMPTY key in the payload no longer wipes the disk copy and a stale form can't resurrect a deleted key (`api_keys::merge_key`); non-empty keys from the setup wizard still win. `read_groq_api_key` now reads the keychain first (it read only settings.json before). |
| `github_profile.rs` (new) | `GET /user` with the Worker-held token → login / name / avatar, cached in `github_profile.json`; `github_profile_clear`. |
| Window | Command Hub = **400 × min(780, monitor−40)**, right-docked (was 740 × full height); geometry tests updated. |

**Frontend** (`settings-sidebar/hub/`)
* `HubHome` — Accounts card (one row per account: photo or initial tile, name, handle, Primary badge, provider tag, ✕ → inline "Keep / Remove"; **+ Add Google account**, **+ Add GitHub** when not connected), Insights card (**API keys · MCP connected · Requests today** with all-time under it), grouped rows (API Keys, Audio, Display, Connections, Advanced), **Change Theme** segmented Light/Dark. No Log Out. Removal text: it only disconnects from NEXUS.
* `ApiKeysPage` — add / replace / delete per provider, masked tail, delete confirmation, errors inline.
* `useHubData` — accounts (Google + GitHub) and insights hooks; MCP probe (~5 s) runs async and shows "–" until it returns.
* `hubModel.ts` — pure helpers (account assembly, ready-MCP count, `45.1K` formatting) — 9 tests.
* `SettingsSidebarApp` — tab bar and glass container replaced by the hub shell (`‹ Back` navigation); Audio / Display / Connections / Advanced are the existing pages rendered inside the new shell. Form state no longer holds API keys.
* Theme: `theme.ts` `initThemeSync()` (storage event across same-origin windows) wired into the stage and the companion HUD; `useThemeMode` hook.
* **Orb (light):** `voice-orb.js` gets a `theme` attribute → `ink` uniform: in Light the fragment shader emits black with the additive-white intensity as coverage and the GL blend switches from additive (`ONE, ONE`) to normal alpha (`ONE, ONE_MINUS_SRC_ALPHA`); `_paint2D` fallback switches to `source-over` + black fill. Dark path is byte-identical in behaviour. The capsule turns `#fafafa` in Light (`styles.css`); the tour callout gets a light card (`tour.css`).

## Verification
* Rust `cargo test --lib -- --test-threads=1`: **1018 passed, 0 failed** (6 ignored dev helpers): new `usage_counter` (5), `api_keys` (5), `github_profile` (3), updated geometry tests.
* Frontend: `tsc --noEmit` clean; vitest **200/200** (+9 `hubModel`).

## Browser-preview check (2026-10-07, Vite + the in-app browser, temporary harness files since deleted)
* **Home screen** rendered at 400 px in **Dark and Light** with mock data (3 accounts incl. GitHub, insights 3 / 2 / 128 + "45.1K all-time"): card/row structure, spacing, hairline borders, blunt corners and the blue add-link match the reference's hierarchy. Not pixel-measured.
* **API Keys page**: Added / Not set pills, masked tail, Delete / Replace / Add key buttons render correctly.
* **Orb**: all four states (idle, listening, thinking, speaking) rendered in **Light (black particles on `#fafafa`)** and **Dark (white on black)** side by side — black particles are clearly visible in every state, and Dark looks unchanged.
* **Legacy pages at 400 px**: Audio, Display, Connections show **no horizontal overflow** (measured). Advanced could not be rendered in the mock (the mock returned an array where `vision_quota` returns an object) — not a product bug, but Advanced is **unverified**.
* Light legacy pages: the interim invert filter reads fine; colour swatches initially inverted to wrong colours and were fixed (re-inverted).
* Preview ran with a **mocked Tauri backend**: real account data, keychain writes, the MCP probe and counters were NOT exercised in the UI.

## Honest limits / not verified
1. **Real Command Hub window not seen.** The preview was a browser tab, not the Tauri window: window size/dock position, the real rim, and real data are unchecked. Colours remain estimates against the reference.
2. **Light orb**: looks right in the preview; the *real* stage (orb inside the capsule over the desktop, animated levels) still needs a look. Dark was not changed.
3. **Audio / Display / Connections / Advanced pages are not redesigned.** In Light they are rendered with an interim colour-inversion filter (`.hx-legacy`) because their colours are hard-coded for dark; they are also still laid out for a wider window and may need reflow at 400 px. Advanced still contains the old Google/GitHub/theme controls (duplicates of the new card).
4. The "soft translucent grey rim" is an **inner** 1 px rim; an outer glow/blur outside the window edge is not possible (no transparent margin; DWM blur on this window renders black).
5. Requests counter has no history; MCP count depends on each server's live probe; GitHub row needs the token and one network call (offline shows the cache or a generic row).
6. Captions (white text with dark shadow over the desktop) were left unchanged in Light.
7. No release build was made.
