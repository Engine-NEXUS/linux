# MCP Program — Overview and Status (2026-09-18)

Family builds have no brain (`admin-brain` compiled out, `is_admin()` always
false). Family pipeline: deterministic regex → BERT-Mini NLU (52 intents,
zero MCP) → Unknown → Worker. Everything below is scoped to that reality.

## Per-command status on family builds

| Command family | Deterministic | NLU fallback | Executes? |
|---|---|---|---|
| open/close app, URLs, media, greetings | exact prefixes | 35–120 rows | local — works |
| search, analyse, check_branch | yes | thin (10–23 rows) | via Worker — mostly works |
| GitHub 24 ops | regex | 3 complex ops deterministic-only | via octocrab — works if phrased right |
| GitHub 4 dead ops (delete_release, branch protection, outside-collab ×2) | none | maps to None | **dead by voice** |
| Live type/press/stop/new-tab | yes | yes | works |
| Live navigate/search, whatsapp_open/search, focus_app | none | NLU-only | fragile — single path |
| order_food, search_product, send_whatsapp | exact prefixes only | 0 rows | parses sometimes, **never executes** |
| Compounds ("X then Y") | if every step parses | no brain rescue | works only when textbook-phrased |
| Reworded commands ("craving sushi", "tell mom…") | no | no/Worker | fails |

## Per-MCP status

| MCP | Server in code | Tools wired | Auth/bridge | Result today |
|---|---|---|---|---|
| Swiggy Food/Im/Dineout | URLs registered | 1 of ~48 (`search_restaurants`) | OAuth never wired (`auth_token=None`) | reachable, 401 always (`nexus mcp check` confirms) |
| WhatsApp (`127.0.0.1:8765`) | yes | `send_message` only | no bridge running | "unavailable" always — now with circuit breaker (3 fails → 60s cool), `mcp_audit.jsonl` trail, actionable bridge guidance (names mcp-whatsapp + QR) |
| Amazon (`127.0.0.1:8766`) | yes | `amazon_search` only | no bridge running | same hardening as WhatsApp; output capped 4000 chars + sanitized before speech |
| Gmail | none (Worker read-only) | none | `gmail.readonly` scope | read works; send/draft impossible |
| Calendar | none (Worker reads today; `create_event` opens browser URL) | none | scope exists, unused for write | read works; voice-create fakes it |
| Contacts/People | none (contacts.json = local WhatsApp names) | none | no scope | missing |
| Maps / Drive / Sheets | none (URL-open only) | none | `drive.readonly` unused | missing |
| LinkedIn / Spotify / YouTube / Meet / Vercel / Render | none | none | none | missing — see `02-server-catalog.md` |

## Scorecard (family, today)

- Local control: ~90%. GitHub voice: ~75%. MCP execution: ~0%.
- Google beyond read/open: 0%. Reworded commands: ~40%.

## Docs in this folder

- `01-auth-vault-one-login.md` — vault design + one-login method.
- `02-server-catalog.md` — all 15 servers: bridge pick, tools, risks.
- `03-scribe-mode-and-confirmations.md` — Scribe mode, contacts, cards.
- `04-voice-call-control.md` — SIM/modem/cloud-number options + budget.
- `05-build-order-and-acceptance.md` — phases + acceptance bars.
- `06-research-sources.md` — papers, datasets, projects, bridges.
