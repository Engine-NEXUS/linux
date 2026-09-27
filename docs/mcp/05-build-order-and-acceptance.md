# Build Order + Acceptance Bars (2026-09-18)

Rule: every phase tested twice before the next begins.

## Order (₹0, family-first)

1. **Vault + Google one-login** — keychain store, 401 auto-refresh,
   diagnostics status per MCP. (Meet rides free.)
2. **HEAD-restore + 55-intent retrain** + deterministic fallbacks for the 5
   NLU-only live intents + regexes for 4 dead GitHub ops.
3. **WhatsApp + LinkedIn** — bridges up, shared confirm cards, passive-read
   rule enforced in `dispatch_to_mcp` (hard block `mark_read` in agent flows).
4. **Gmail draft→send + Calendar/Meet create + Contacts photos** — scribe
   flows plug in here.
5. **Spotify + YouTube** — highest daily use; quota guard for YouTube.
6. **Vercel (readonly) + Render (readonly)** → gated deploys last.
7. **Swiggy order chain + Amazon scraper + Drive/Sheets read + Maps links.**
8. **Voice-call module** (simulated audio now; number rental later).

## New intents (deterministic + NLU families each)

`play_music_on_spotify`, `search_youtube` / `summarize_video`,
`create_meet`, `linkedin_search` / `linkedin_message` / `linkedin_post`,
`vercel_deploy_status` / `redeploy`, `render_service_status` / `redeploy`,
`open_meeting_from_chat`, scribe allowlist (`send it`, `scratch that`,
`new line`, `read it back`, `discard it`, `polish it`, `change X to Y`).

## Acceptance bar per MCP

- Parse: deterministic + NLU both hit ≥0.85 on 20 test phrasings.
- Execute: real-account round-trip succeeds; 401 self-heals via refresh.
- Safety: every write shows a confirm card (who/what/where); undo path
  exists (Gmail undo window, WhatsApp revoke, draft-first, Vercel rollback).
- Failure mode: clear "bridge down, reconnect?" — never silent, never fake
  (cf. `set_timer` lesson in `command_executor.rs:503-519`).
