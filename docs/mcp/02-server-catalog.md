# MCP Server Catalog (2026-09-18)

All picks are free, family-compatible, vault-compatible. Write tools are
confirm-gated; destructive tools get the irreversible-action warning
(existing `requires_confirmation` / `is_destructive` split).

## Registered today (expand in place)

| Server | Endpoint | Bridge to run | Tools to wire (read → write) |
|---|---|---|---|
| SwiggyFood | `https://mcp.swiggy.com/food` | hosted by Swiggy | search_restaurants → cart → coupon → place_food_order (destructive) |
| SwiggyInstamart | `https://mcp.swiggy.com/im` | hosted | search → cart → place_im_order (destructive) |
| SwiggyDineout | `https://mcp.swiggy.com/dineout` | hosted | search → book_table (destructive) / cancel_booking |
| WhatsApp | `http://127.0.0.1:8765/mcp` | `Sealjay/mcp-whatsapp` — single Go binary, serves exactly `:8765`, 42 tools, SQLite cache, VCF import | list/search chats+contacts, read messages (passive, never `mark_read` in agent flows), send_message (gated), send_media, revoke (undo) |
| Amazon | `http://127.0.0.1:8766/mcp` | Playwright scraper bridge | amazon_search → details → reviews (read-only) |

## New — Google group (one login, see `01-auth-vault-one-login.md`)

| Server | Bridge pattern | Tools |
|---|---|---|
| Gmail | draft-first server + `send_draft(draftId)` sender | search/read → create_draft → send_draft (gated) → delete_draft |
| Calendar | Calendar API via vault | read day → insert (gated) → update/delete (gated) |
| Meet | `workos-gmeet-mcp-server` (14 tools) — 0 extra logins | list → create with link (gated) → recordings/transcripts/participants |
| Contacts/People | People API via vault | search → photo cards → alias learning (see `03-…md`) |
| Drive/Sheets | Drive API via vault | search/read rows first; writes gated later |
| Maps | Places/directions links + API key (free quota) | read-only first (links, ETAs) |

## New — social / media

| Server | Recommended bridge | Auth | Tools | Risks |
|---|---|---|---|---|
| LinkedIn | `JohannsenLum/linkedin-api-mcp` (12 read + 2 write, session cookie, paced, 68 tests) | session cookie (0 logins) | search people/jobs/companies/posts → send_message, connect (both gated) | unofficial API — small ban risk, human volume only |
| Spotify | `AlexanderMolano-spec/spotify-mcp-server` (streamable HTTP, auto-refresh) | 1 OAuth (official API) | search/queue/playlists/devices → play/pause/skip/volume, playlist CRUD | playback needs Premium + active device; extends local media intents |
| YouTube | `Anarcyst` zero-config (search+transcripts, no key) + Data API v3 key for channel ops | 0 / 1 key | search, transcripts, channel videos, comments → upload (gated, quota-guarded) | 10K units/day free (search=100) — client-side quota guard, fail soft |

## New — dev platforms

| Server | Recommended bridge | Auth | Tools | Safety |
|---|---|---|---|---|
| Vercel | `helbertparanhos/vercel-mcp-pro` (70 tools, `VERCEL_READONLY` mode) | 1 token | list projects/deployments/logs (default) → redeploy, env set (gated) | default readonly; official `mcp.vercel.com` OAuth is approved-clients-only |
| Render | official `render-oss/render-mcp-server` | 1 OAuth (preferred — keys are broadly scoped) | services, deploys, logs, metrics, DB query → redeploy, env update (gated) | no scaling controls via MCP |

## Safety tiers (all servers)

- Tier 1 read-only default (Vercel readonly, YouTube keyless, Render list/logs).
- Tier 2 confirm-gated (messages, deploys, uploads, bookings).
- Tier 3 destructive double-confirm (delete deployment, place order, revoke).
- Draft/preview + execute + undo is required before any channel is
  "send-capable" (Gmail draft→send, WhatsApp revoke, Vercel rollback).
