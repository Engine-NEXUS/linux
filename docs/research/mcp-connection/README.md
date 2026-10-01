# MCP Connection UX — Research Index (2026-09-18/20)

**Question:** when the user asks for something needing a disconnected MCP
(send a WhatsApp message, order food, search Amazon…), NEXUS today only
*speaks* a fix-it sentence. Industry products *show* status and put the
fix action in the same surface. This folder holds the full research and
the build verdicts, organized by feature department and tech stack.

## Map

| File | Department / stack | What it answers |
|---|---|---|
| `01-industry-patterns.md` | industry survey | How Composio, Zapier, Claude, Cursor, VS Code, ChatGPT handle disconnected/auth-expired connectors |
| `02-oauth-spec.md` | protocol (applies to all OAuth servers) | The MCP Authorization spec sequence NEXUS must implement for Swiggy-class servers |
| `03-whatsapp.md` | department: WhatsApp pairing | Sealjay bridge mechanics (`/pair`, `pairing_status`, 20-day rotation) + approach comparison + verdict |
| `04-swiggy.md` | department: Swiggy commerce OAuth | Spec-OAuth plan reusing our Google/GitHub flow, refresh, Builders-Club caveat |
| `05-amazon-and-long-tail.md` | departments: Amazon bridge, Spotify/Vercel/Render | Bridge sign-in pattern + token-page buttons |
| `06-shared-infrastructure.md` | tech stack: Rust + frontend + Worker | State machine, Connect card, per-server Recheck/toggle/logs, resume-on-complete, audit hygiene, tests |
| `07-comparison-verdict.md` | decision | Option matrices per server, best approach vs our codebase, phased build order |
| `08-gap-analysis-and-upgrades.md` | deep audit round 2 | Live-source audit (Anthropic trackers, Novu, AutoGPT, Qwen, spec security) → 5 gaps found + fixed + verified ×2 |

## Master verdict (detail in `07-comparison-verdict.md`)

| Server | Best approach | Why it wins here |
|---|---|---|
| WhatsApp | Poll the bridge's own `pairing_status` tool, render its QR payload in our card; `/pair` URL as fallback | First-party data, no scraping; matches WhatsApp's native Linked-Devices UX |
| Swiggy ×3 | Spec-sequence OAuth (metadata → metadata-document registration → PKCE → vault + refresh), reusing `setup/oauth.ts` + Worker endpoints | Same shape as our proven Google/GitHub flow; spec-compliant; self-healing refresh |
| Amazon | Managed launcher + sign-in trigger + session polling | Same card system as WhatsApp, different pairing step |
| Spotify/Vercel/Render | Keep vault paste; add "Get token" deep buttons + numbered steps | OAuth unjustified until usage demands it |
| All | Connect card opens on first failure per session + auto-resume of the failed call on completion | Composio-validated pattern; our pending-request machinery already supports it |

## Codebase anchor points (verified 2026-09-20)

- Failure path: `src-tauri/src/orchestrator.rs:1131-1202` (Mcp arm),
  `1427-1587` (`dispatch_to_mcp`), `2192-2309` (`orchestrator_mcp_confirm`)
- Guidance strings: `orchestrator.rs:1607-1648` (tests `:2742-2769`)
- Status probe: `src-tauri/src/mcp_client.rs:574-649` (`mcp_status`)
- Audit log (token-free): `mcp_client.rs:261-265`
- Vault: `src-tauri/src/auth_vault.rs` (services `:24`, refresh `:206-229`)
- Connections tab: `frontend/src/settings-sidebar/SettingsSidebarApp.tsx:641-848`
- OAuth precedent: `frontend/src/setup/oauth.ts:37-201` (PKCE, deep-link + polling)
