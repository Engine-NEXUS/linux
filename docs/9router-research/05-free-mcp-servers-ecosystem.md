# Free MCP Servers Research — 100% Free Control of WhatsApp, Social Media, Email, GitHub, and More

**Date:** 2026-09-15
**Status:** Research complete
**Researcher:** Devin (GLM-5.2 High)
**Constraint:** 100% free. No paid API keys. No paid services. No credit cards. Self-hosted, open-source, keyless where possible.

---

## 1. What is MCP and Why It Matters for NEXUS

**Model Context Protocol (MCP)** is an open standard by Anthropic. It lets AI
models connect to external tools, APIs, databases, and services through one
standardized protocol — "a universal USB port for AI."

For NEXUS, this is huge. Instead of building custom integrations for WhatsApp,
YouTube, Gmail, GitHub, etc., NEXUS's Router Brain can call **MCP servers** that
already exist. The brain just picks the right MCP tool and calls it.

```
User: "hey nexus, send a message to mom on whatsapp"
  |
Router Brain (0.5B)
  |
  +-- Function call: whatsapp.send_message(to: "mom", text: "...")
  |
  v
WhatsApp MCP Server (local, free)
  |
  v
WhatsApp Web (your account, QR login)
```

**The key finding:** There are **8,000+ MCP servers** now (2026). Many are
**100% free, open-source, self-hosted, and need no API keys.** This document
catalogs the ones NEXUS can use for free.

---

## 2. The "100% Free" Test

I classified every MCP server into three tiers:

| Tier | Meaning | NEXUS uses? |
|------|---------|-------------|
| **A — Fully Free** | Open source, self-hosted, no API key, no account | ✅ Yes |
| **B — Free with Auth** | Open source, self-hosted, but needs your own OAuth/login (Gmail, GitHub, Spotify — your account, no cost) | ✅ Yes |
| **C — Paid/Freemium** | Requires a paid API key or has a tight freemium limit | ❌ No |

**The user wants 100% free.** So NEXUS uses Tier A and Tier B only. Tier B is
still 100% free — you're using your own accounts (Gmail, WhatsApp, GitHub),
not paying anyone. You just log in once.

---

## 3. The Big FindMCP / Awesome Lists

Three registries track the MCP ecosystem:

| Registry | Servers indexed | URL |
|----------|----------------|-----|
| **FindMCP** | 8,000+ | https://findmcp.dev |
| **awesome-mcp-servers** | 7,493 unique | https://github.com/alycz/awesome-mcp-servers |
| **MCP Directory** | 2,500+ verified | https://mcpdirectory.app |
| **100-free-mcp-servers** | 100 curated free | https://github.com/MustafaYamin/100-free-mcp-servers |
| **best-of-mcp-servers** | 400 ranked, 34 categories | https://github.com/tolkonepiu/best-of-mcp-servers |

**Key categories from awesome-mcp-servers:**
- Communication & Messaging: 327 servers
- Browser Automation & Web Scraping: 275 servers
- Cloud Platforms & Services: 371 servers
- Databases: 290 servers
- Developer Productivity: 393 servers
- Knowledge & Memory: 18 servers
- Social Media: 2+ servers (more in other lists)

---

## 4. WhatsApp — Free MCP Servers

WhatsApp is the user's primary messaging app. Three solid free options exist.

### 4.1 Sealjay/mcp-whatsapp (Tier A — Fully Free)

- **Repo:** https://github.com/sealjay/mcp-whatsapp
- **Language:** Go (single binary, ~4 MB)
- **License:** MIT
- **Protocol:** whatsmeow (WhatsApp Web protocol, unofficial)
- **Tools:** 42 tools
- **Auth:** QR code (scan once with your phone)
- **RAM:** Very low (Go binary, SQLite cache)
- **URL:** `127.0.0.1:8765`

**Tools include:**
- Messaging: send, reply, edit, revoke, react, forward
- Groups: create, manage members, admin controls, invite links, polls
- Media: send audio, files, images, view-once
- Privacy: block list, presence, privacy settings
- Status: post, view
- Contact cards, typing indicators, mark-read

**Risk:** Unofficial — violates WhatsApp ToS. Use a burner number. NEXUS
already has WhatsApp deep-link support (`whatsapp_chat` in `intent_parser.rs`),
so this MCP server is for full automation, not the existing simple flow.

### 4.2 vaibhavpandeyvpz/wappmcp (Tier A — Fully Free)

- **Repo:** https://github.com/vaibhavpandeyvpz/wappmcp
- **Language:** Node.js
- **License:** MIT
- **Protocol:** whatsapp-web.js (browser automation of WhatsApp Web)
- **Tools:** 20 tools
- **Auth:** QR code
- **Storage:** `~/.wappmcp/`

**Tools:**
- Read: `whatsapp_list_chats`, `whatsapp_get_chat`, `whatsapp_get_chat_messages`, `whatsapp_search_messages`
- Send: `whatsapp_send_message`, `whatsapp_send_media_from_base64`, `whatsapp_send_media_from_path`, `whatsapp_reply_to_message`
- Manage: `whatsapp_react_to_message`, `whatsapp_edit_message`, `whatsapp_delete_message`, `whatsapp_forward_message`, `whatsapp_send_typing`
- Contacts: `whatsapp_list_contacts`, `whatsapp_get_contact`, `whatsapp_search_contacts`, `whatsapp_lookup_number`
- Status: `whatsapp_get_me`, `whatsapp_get_status`

**Notifications:** Can emit incoming message events over an optional MCP
notification channel — so NEXUS could react to incoming WhatsApp messages
proactively.

### 4.3 kahflane/whatsapp-mcp (Tier A — Fully Free)

- **Repo:** https://github.com/kahflane/whatsapp-mcp
- **Language:** Node.js or Bun
- **License:** MIT
- **Tools:** 87 tools
- **Storage:** Local SQLite
- **Safety:** Anti-ban send pacing, daily cap, kill-switch for all sends

**Extra features over the others:**
- Scheduled messages
- Reusable templates
- Keyword auto-replies
- Buttons, lists, commerce messages
- Status / Story (rich text, image, video, gif, voice)

**Warning (from their README):** "Use a dedicated/burner number. This is
unofficial automation and violates WhatsApp's ToS — your account can be
banned. Never connect your primary line."

### 4.4 FredShred7/whatsapp-mcp-server (Tier B — Free, Official API)

- **Repo:** https://github.com/FredShred7/whatsapp-mcp-server
- **License:** (check repo)
- **Protocol:** Official Meta WhatsApp Cloud API
- **Account type:** WhatsApp Business account (free to create)
- **Risk:** None — Meta-approved, no ban risk

**Trade-off:** Official and safe, but requires a WhatsApp Business account
and Meta Developer setup. Better for production; the unofficial ones are
better for personal use.

### 4.5 NEXUS Recommendation for WhatsApp

| Use case | Server |
|----------|--------|
| Personal account, full features | `kahflane/whatsapp-mcp` (87 tools) |
| Lightweight, single binary | `sealjay/mcp-whatsapp` (42 tools, Go) |
| Incoming message notifications | `vaibhavpandeyvpz/wappmcp` |
| Production / Business / No ban risk | `FredShred7/whatsapp-mcp-server` (official) |

**For NEXUS:** Start with `sealjay/mcp-whatsapp` (single binary, low RAM,
42 tools). It's the lightest and most production-ready. If you need incoming
message events, switch to `wappmcp`.

---

## 5. YouTube — Free MCP Servers

### 5.1 Anarcyst/youtube-mcp-server (Tier A — Fully Free, No API Key)

- **Repo:** https://github.com/anarcyst/youtube-mcp-server
- **Language:** Python
- **License:** MIT
- **API key needed:** NO — uses YouTube's internal API
- **Install:** `uvx yt-mcp-server` or `pip install yt-mcp-server`

**Tools (8):**
- `search_videos` — Search YouTube
- `get_video_info` — Title, description, stats, chapters
- `get_channel_info` — Subscribers, description, video count
- `get_channel_videos` — List videos from a channel
- `get_comments` — Comments sorted by relevance
- `get_transcript` — Full transcript with timestamps
- `search_transcript` — Search within a video's transcript
- `search_channel_transcripts` — **Search across ALL videos of a channel** — find what any creator said about any topic

**This is the best free YouTube MCP.** No API key, no quota, no cost. The
transcript search across an entire channel is unique and powerful.

### 5.2 granitebps/youtube-mcp (Tier A — Fully Free, No API Key)

- **Repo:** https://github.com/granitebps/youtube-mcp
- **License:** MIT
- **API key needed:** NO — uses `youtubei.js` (InnerTube API) + `youtube-transcript-plus`

**Tools (5):**
- `search_youtube` — Search with filters (date, popularity, duration)
- `get_video_info` — Metadata, views, likes, duration, tags
- `get_video_comments` — Comment threads with replies
- `get_video_transcript` — Transcripts with timestamps
- `get_transcript_languages` — Available caption languages

**Transports:** stdio (local) and Streamable HTTP (remote)

### 5.3 vaporif/mcp-server-youtube (Tier A — Fully Free, Rust)

- **Repo:** https://github.com/vaporif/mcp-server-youtube
- **Language:** Rust (single binary)
- **License:** GPL-3.0
- **Transcripts:** Uses `rustypipe` (InnerTube API) — no API key quota consumed

**Advantage:** Rust binary, zero runtime dependencies, starts instantly.
Good if NEXUS wants a lightweight YouTube server.

### 5.4 NEXUS Recommendation for YouTube

**`Anarcyst/youtube-mcp-server`** — it has the unique `search_channel_transcripts`
tool (search what a creator said about a topic across all their videos) and
needs no API key. Perfect for "hey nexus, find me a video where Linus talks
about RAM timing."

---

## 6. Email — Free MCP Servers

### 6.1 Txtus/mail-mcp (Tier B — Free, Your Account)

- **Repo:** https://github.com/Txtus/mail-mcp
- **License:** MIT
- **Protocol:** IMAP/SMTP (works with any provider)
- **Auth:** App password, or OAuth2 (Gmail/Office 365)
- **Tools:** 17 tools
- **Multi-account:** Yes (work, personal, freelance in parallel)
- **Config UI:** Browser-based at `http://localhost:4321`

**Tools:**
- `list_emails`, `get_email`, `search_emails`
- `send_email`, `reply_email`, `forward_email`
- `mark_read`, `mark_emails_read`
- `move_email`, `move_emails`, `delete_emails`
- `list_folders`, `create_folder`
- `list_attachments`, `download_attachment`
- `list_accounts`

**Auto-discovery:** Detects IMAP/SMTP settings for 20+ providers automatically.

### 6.2 JosueM1109/email-mcp (Tier B — Free, Your Account)

- **Repo:** https://github.com/JosueM1109/email-mcp
- **License:** MIT
- **Install:** `npx @marlinjai/email-mcp`
- **Providers:** Gmail (REST API), Outlook (Microsoft Graph), iCloud (IMAP), generic IMAP/SMTP
- **Tools:** 24 tools
- **Security:** AES-256-GCM encrypted credential storage

**Setup wizard:** Walks you through provider selection and OAuth. Can set up
Gmail, Outlook, and iCloud in one go.

### 6.3 UseJunior/email-agent-mcp (Tier B — Free, Security-First)

- **Repo:** https://github.com/usejunior/email-agent-mcp
- **License:** Apache 2.0
- **Providers:** Microsoft 365 / Outlook and Gmail
- **Security-first:** Agents cannot send email until you configure an allowlist

**Tools:**
- Read: `list_emails`, `read_email`, `search_emails`, `get_thread`, `list_attachments`, `download_attachment`
- Write (allowlist-gated): `send_email`, `reply_to_email`, `create_draft`, `update_draft`, `send_draft`
- Manage: `label_email`, `mark_read`, `move_to_folder`

**Best for NEXUS:** The send allowlist is exactly what NEXUS needs — the brain
can read freely, but sending requires explicit user approval.

### 6.4 NEXUS Recommendation for Email

**`UseJunior/email-agent-mcp`** — the send allowlist matches NEXUS's
confirmation-gate pattern. The brain can read and search freely, but
sending email requires the user to approve the recipient first.

---

## 7. GitHub — Free MCP Server (Official)

### 7.1 @modelcontextprotocol/server-github (Tier B — Free, Your Token)

- **Repo:** https://github.com/modelcontextprotocol/servers
- **Install:** `npx -y @modelcontextprotocol/server-github`
- **License:** MIT
- **Auth:** GitHub Personal Access Token (free, create at github.com/settings/tokens)
- **Maintained by:** Anthropic (now moved to github.com/github/github-mcp-server)

**Tools:**
- Repository: `create_repo`, `get_file`, `create_or_update_file`, `list_commits`
- Issues: `create_issue`, `list_issues`, `get_issue`, `update_issue`, `add_issue_comment`
- Pull requests: `create_pull_request`, `list_pull_requests`, `get_pull_request`, `merge_pull_request`, `get_pull_request_files`, `get_pull_request_status`, `create_pull_request_review`
- Search: `search_code`, `search_issues`, `search_users`, `list_commits`
- Branches: `create_branch`, `list_branches`, `get_branch`

**This is the official GitHub MCP server.** Free, maintained, comprehensive.
NEXUS already uses GitHub for its own repo, so this is a natural fit.

---

## 8. Social Media — Twitter/X, Instagram, LinkedIn, Reddit, TikTok

### 8.1 Twitter/X

**tharuxpert/x-mcp (Tier B — Free with X API)**
- **Repo:** https://github.com/tharuxpert/x-mcp
- **License:** MIT
- **Tools:** `post_tweet`, `quote_tweet`, `delete_tweet`, `get_tweet`, `search_tweets`, `get_timeline`, `get_mentions`, `get_user`, `get_followers`, `get_following`, `retweet`, `upload_media`, `get_metrics`, `reply_to_tweet`
- **Transport:** Streamable HTTP (modern) + SSE (legacy)
- **Note:** X API free tier allows posting and reading. Some tools require
  Basic+ tier (bookmarks, likes). Reply is restricted.

**isteamhq/twitter-mcp (Tier B — Free with X API)**
- **Repo:** https://github.com/isteamhq/twitter-mcp
- **Tools:** `search_tweets`, `post_tweet`, `reply`, `like`, `retweet`, `follow`

### 8.2 Instagram

**mcpware/ig-mcp (Tier B — Free with Meta Developer Account)**
- **Repo:** https://github.com/mcpware/ig-mcp
- **Install:** `npx @mcpware/instagram-mcp`
- **License:** MIT
- **Tools:** 23 tools
- **Requirements:** Instagram Business account + Facebook Page + Meta Developer account (all free)
- **Tools:** Profile info, media posts, media insights, publish media, account pages, conversations (DMs — requires Advanced Access), send DM (requires Advanced Access)

**adelaidasofia/instagram-mcp (Tier B — Free, Official Graph API)**
- **Repo:** https://github.com/adelaidasofia/instagram-mcp
- **Tools:** 29 tools — multi-account, read, publish, comment, analytics, review-gated DMs
- **ToS-safe:** Uses official Graph API, no private/reverse-engineered API

### 8.3 Multi-Platform Social (The Best Option)

**woosal1337/media-mcp (Tier A — Fully Free, Local Whisper)**
- **Repo:** https://github.com/woosal1337/media-mcp
- **License:** MIT
- **Tools:** 31 tools across Twitter/X, YouTube, Instagram, and video processing
- **Transcription:** Local Whisper — no audio leaves your machine
- **Instagram:** Downloads posts, reels, carousels via self-hosted Cobalt
- **No API keys for most features** (Twitter uses TwitterAPI.io REST API)

**XPOZpublic/xpoz-mcp (Tier B — Free Trial, Then Paid)**
- **Repo:** https://github.com/xpozpublic/xpoz-mcp
- **Tools:** 14 Twitter tools, plus Instagram, Reddit, TikTok
- **Free trial:** 5 days, limited results
- **Not 100% free** — skip for NEXUS

### 8.4 LinkedIn, Facebook, Pinterest, Threads, Bluesky, Reddit

**Upload-Post MCP (Tier C — Freemium)**
- **Repo:** https://github.com/upload-post/upload-post-mcp
- **Platforms:** TikTok, Instagram, YouTube, LinkedIn, Facebook, X, Threads, Pinterest, Reddit, Bluesky, Google Business, Discord, Telegram
- **Free API key:** "No credit card required" — but this is a freemium service, not 100% free
- **Skip for NEXUS** unless the free tier is sufficient

**isteamhq/mcp-servers (Tier B — Free with Platform Auth)**
- **Repo:** https://github.com/isteamhq/mcp-servers
- **Servers:** Twitter, Bluesky, LinkedIn, Google Ads, Hacker News
- **License:** MIT
- **Install:** `npx` — zero config
- **Tools:** Search, post, reply, like, follow (varies by platform)

### 8.5 NEXUS Recommendation for Social Media

| Platform | Server | Tier |
|----------|--------|------|
| Twitter/X | `tharuxpert/x-mcp` | B (free X API) |
| Instagram | `mcpware/ig-mcp` | B (free Meta dev account) |
| Multi (Twitter+YouTube+IG) | `woosal1337/media-mcp` | A (free, local Whisper) |
| LinkedIn, Bluesky, HN | `isteamhq/mcp-servers` | B (free with auth) |

---

## 9. Browser Automation — Free MCP Servers

### 9.1 microsoft/playwright-mcp (Tier A — Fully Free)

- **Repo:** https://github.com/microsoft/playwright-mcp
- **License:** MIT
- **Maintained by:** Microsoft
- **Approach:** Accessibility tree (not screenshots) — LLM-friendly, no vision models needed

**Tools:**
- `browser_navigate` — Go to a URL
- `browser_click` — Click an element
- `browser_fill_form` — Fill multiple form fields
- `browser_snapshot` — Capture accessibility snapshot (better than screenshot)
- `browser_take_screenshot` — Take a screenshot
- `browser_select_option`, `browser_hover`, `browser_press_key`
- `browser_list_tabs`, `browser_select_tab`, `browser_close_tab`

**This is the official Microsoft Playwright MCP.** Free, maintained,
production-ready. NEXUS can use this to open signup pages, fill forms,
scrape pages — anything a browser can do.

### 9.2 Mhrnqaruni/mcp-playwright-browser (Tier A — Fully Free, Advanced)

- **Repo:** https://github.com/Mhrnqaruni/mcp-playwright-browser
- **License:** MIT
- **Tools:** 71 tools
- **Features:** Multi-tab, form audit, A11y tree, session persistence, cookie export/import
- **Token-optimized:** Capture profiles (light/balanced/full) + 280KB hard payload ceiling

**Best for:** Complex multi-tab workflows like job applications, form filling,
multi-step logins.

### 9.3 NEXUS Recommendation for Browser

**`microsoft/playwright-mcp`** — official, maintained, uses accessibility tree
(not pixels), so it works with the 0.5B brain without needing vision. For
complex workflows, add `mcp-playwright-browser` (71 tools).

---

## 10. Calendar — Free MCP Servers

### 10.1 zavora-ai/mcp-calendar (Tier B — Free, Your Account)

- **Repo:** https://github.com/zavora-ai/mcp-calendar
- **Language:** Rust (single binary)
- **License:** MIT
- **Providers:** Google Calendar + Microsoft Outlook
- **Auth:** OAuth (one-time browser auth, auto-refresh)
- **Tools:** 12 tools

**Tools:**
- `list_calendars`, `list_events`, `get_event`, `search_events`
- `create_event`, `update_event`, `delete_event`
- `quick_add` (natural language), `get_today_events`
- `list_attendees`, `get_free_busy`, `rsvp`

**Advantage:** Single Rust binary, no Node.js/Python, OAuth auto-refresh,
enterprise governance with risk classification.

### 10.2 rauf543/calendar-mcp (Tier B — Free, Multi-Provider)

- **Repo:** https://github.com/rauf543/calendar-mcp
- **Providers:** Google Calendar, Microsoft 365, Exchange On-Premises
- **Tools:** 12 tools + 4 resources + 4 prompts
- **Multi-account:** Multiple accounts per provider

### 10.3 NEXUS Recommendation for Calendar

**`zavora-ai/mcp-calendar`** — Rust binary, low RAM, OAuth auto-refresh,
12 tools. Perfect for "hey nexus, what's on my calendar today?"

---

## 11. Messaging — Telegram, Discord, Slack

### 11.1 santoshakil/nexus (Tier B — Free, Multi-Platform, Rust)

- **Repo:** https://github.com/santoshakil/nexus
- **Language:** Pure Rust (single binary, ~4 MB)
- **License:** MIT
- **Platforms:** Telegram, Gmail, WhatsApp, Slack, Discord
- **Tools:** 48 tools across all five platforms

**This is the most efficient option for NEXUS** — one Rust binary, five
platforms, 48 tools. Matches NEXUS's Rust architecture.

**Tools include:**
- Universal: `list_platforms`, `get_profile`, `list_channels`, `read_messages`, `send_message`, `search`
- Telegram (13 tools): search chat, get message, forward, pin, react, edit, delete, download media
- Gmail: archive, label, star, draft, send with attachments
- Slack: create channels, set topics, manage reactions
- Discord: list guilds, create threads, pin messages

### 11.2 mcp-telegram/mcp-telegram (Tier B — Free, 181 Tools)

- **Repo:** https://github.com/mcp-telegram/mcp-telegram
- **License:** MIT
- **Protocol:** MTProto (userbot — operates as your personal account)
- **Tools:** 181 tools
- **Auth:** QR code login or phone number

**Tools:** Messages, reactions, polls, scheduled messages, stickers, media,
contacts, forum topics, stories, discussion groups, QR login, session
persistence, human-in-the-loop confirmation.

**Most full-featured Telegram MCP server available.** But 181 tools is a lot
— NEXUS's brain would need a curated subset.

### 11.3 korotovsky/slack-mcp-server (Tier B — Free, 1,755 stars)

- **Repo:** https://github.com/korotovsky/slack-mcp-server
- **License:** MIT
- **Stars:** 1,755
- **Features:** Stealth mode (no permissions needed), OAuth, DMs, Group DMs,
  smart history fetch, unread messages, search, safe message posting
  (disabled by default)

### 11.4 iflow-mcp/goul4rt-mcp-discord (Tier B — Free, 80+ Tools)

- **Repo:** https://github.com/iflow-mcp/goul4rt-mcp-discord
- **License:** MIT
- **Tools:** 80+ tools across 8 categories
- **Dual-mode:** Standalone or plugin into existing discord.js bot
- **Transports:** stdio or HTTP with Bearer auth

### 11.5 NEXUS Recommendation for Messaging

**`santoshakil/nexus`** — one Rust binary covers Telegram, Gmail, WhatsApp,
Slack, Discord with 48 tools. Lowest RAM, matches NEXUS's architecture. If
you need deeper Telegram features (stories, polls, stickers), add
`mcp-telegram/mcp-telegram` alongside it.

---

## 12. Music — Spotify Free MCP Servers

### 12.1 darrenjaworski/spotify-mcp (Tier B — Free with Spotify Account)

- **Repo:** https://github.com/darrenjaworski/spotify-mcp
- **License:** MIT
- **Tools:** 32 tools
- **Auth:** OAuth 2.0
- **Note:** Spotify Premium required for playback control (free account can search and manage playlists)

**Tools:** `spotify_play`, `spotify_pause`, `spotify_next`, `spotify_previous`,
`spotify_set_volume`, `spotify_get_playback_state`, `spotify_get_devices`,
`spotify_transfer_playback`, `spotify_shuffle`, `spotify_repeat`,
`spotify_search`, `spotify_get_user_playlists`, etc.

### 12.2 pikaro/spotify-mcp (Tier B — 100+ Tools)

- **Repo:** https://github.com/pikaro/spotify-mcp
- **License:** MIT
- **Tools:** 100+ tools
- **Unique features:** Smart shuffle (6 strategies), vibe engine (mood
  analysis), natural language song search, artist network mapping, taste
  evolution tracking, library index

### 12.3 NEXUS Recommendation for Music

**`darrenjaworski/spotify-mcp`** — 32 tools cover the basics. If NEXUS wants
"play music that matches my mood," upgrade to `pikaro/spotify-mcp` (100+
tools with vibe analysis).

---

## 13. Knowledge & Memory — Free MCP Servers

### 13.1 andylow92/file-system-brain-mcp (Tier A — Fully Free)

- **Repo:** https://github.com/andylow92/file-system-brain-mcp
- **License:** MIT
- **Tools:** 27 tools
- **Storage:** Plain `.md` files, local, offline, no API key

**Features:**
- GitHub-style file tree + Notion-style editing
- Semantic & hybrid search, RAG, cited answers
- Self-improving: learns your writing voice from edits
- Human-in-the-loop review queue
- Wikilinks, backlinks, knowledge graph

**Best for:** NEXUS's long-term memory. The brain can store what it learned
about the user (preferences, reminders, provider history) as markdown files.

### 13.2 NeveuGregor/mcp-obsidian (Tier A — Fully Free)

- **Repo:** https://github.com/NeveuGregor/mcp-obsidian
- **License:** Other
- **Tools:** 6 tools
- **No Obsidian plugin required** — operates on the vault directory directly

**Tools:** `obsidian_read`, `obsidian_search`, `obsidian_write`,
`obsidian_append`, `obsidian_patch_frontmatter`, `obsidian_list`

**Best for:** If the user already uses Obsidian, NEXUS can read and write
notes directly to the vault.

### 13.3 aka-kika/kika-obsidian-mcp (Tier A — Fully Free)

- **Repo:** https://github.com/aka-kika/kika-obsidian-mcp
- **License:** MIT
- **No plugins, no API keys, no cloud, no Obsidian running**
- Supports Obsidian Bases (`.base` files) with schema validation

### 13.4 NEXUS Recommendation for Knowledge

**`andylow92/file-system-brain-mcp`** — 27 tools, self-improving, RAG,
local markdown. This is NEXUS's long-term memory layer. The brain stores
what it learns about the user and retrieves it with semantic search.

---

## 14. The "Keyless" All-in-One Servers

Two MCP servers are specifically designed to be **100% free with zero API keys**:

### 14.1 elyerinfox/lodestone-mcp (Tier A — Fully Free, 475 Tools)

- **Repo:** https://github.com/elyerinfox/lodestone-mcp
- **Language:** Rust (single binary)
- **License:** MIT
- **Tools:** ~475 tools in ~100 skill families
- **Keyless by default:** Zero accounts or keys. A few sources can
  *optionally* use a credential, but none is required.

**Capabilities:**
- Search and retrieve the open web (scrapes search engines, reads keyless endpoints)
- Operate the machine (Docker, Kubernetes, files, shell, git, databases, serial/printers)
- Compute over real data (math, geo, finance, units, dates, JSON/YAML/regex, NASA/space, markets)
- Safety: Destructive actions never fire unguarded (confirm-token handshake)
- Dangerous families off by default

**This is the most comprehensive free MCP server.** 475 tools, single Rust
binary, no keys. NEXUS could use this as the "everything else" server —
math, dates, web search, file operations, git, all in one.

### 14.2 highercomve/mcptools (Tier A — Fully Free, 39 Tools)

- **Repo:** https://github.com/highercomve/mcptools
- **License:** MIT
- **Tools:** 39 tools in 10 groups
- **No cloud keys required** — search is Startpage (+ Google headless fallback),
  weather is open-meteo, embeddings are local LM Studio

**Tools:**
- Web: `web_search`, `fetch_url`, `extract_links`, `get_page_metadata`, `http_request`
- Compute: `calculator`, `current_datetime`
- System: `read_file`, `write_file`, `list_directory`, `run_command`, `execute_code`, `notify`
- Files: `search_files`, `find_files`, `edit_file`, `edit_lines`, `pdf_to_json`, `download_file`
- Data: `sqlite_query`, `json_query`
- Knowledge: `wikipedia_lookup`, `weather`
- Feeds: `read_rss`, `hackernews`, `arxiv_search`, `youtube_transcript`
- Memory: `memory_save`, `memory_recall`, `memory_list`, `memory_delete` (persistent SQLite)
- Tasks: `todo_add`, `todo_list`, `todo_done`
- RAG: `rag_index`, `rag_search`, `rag_list`, `rag_delete` (embeddings via LM Studio)

### 14.3 sweetcornna/free-search-mcp (Tier A — Fully Free, 10 Tools)

- **Repo:** https://github.com/sweetcornna/free-search-mcp
- **License:** MIT
- **Install:** `uvx free-search-mcp`
- **No API key** — uses DuckDuckGo, Mojeek, Startpage (parallel multi-engine)

**Tools:**
- `search` — Parallel multi-engine search, RRF-merged, deduped
- `research` — One-shot: search + fetch top N + return Markdown brief
- `compare` — Concurrent fetch of 2-5 URLs, side-by-side excerpts
- `fetch` — Reader-mode Markdown for pages, parsed text for documents
- `fetch_batch` — Concurrent multi-URL fetch (max 20)
- `read_doc` — Parse PDF/DOCX/XLSX/PPTX/EPUB/CSV/code/zip-tar/HTML/TXT/MD
- `extract_structured` — Pull JSON-LD/OpenGraph/Twitter cards/microdata
- `cache_search` — FTS5 search across previously fetched pages
- `engines` — List available engines

---

## 15. The Complete NEXUS Free MCP Stack

Here's the recommended stack — all 100% free:

| Category | Server | Tools | Tier | Language |
|----------|--------|-------|------|----------|
| **WhatsApp** | `sealjay/mcp-whatsapp` | 42 | A | Go |
| **YouTube** | `anarcyst/youtube-mcp-server` | 8 | A | Python |
| **Email** | `usejunior/email-agent-mcp` | 14 | B | TypeScript |
| **GitHub** | `@modelcontextprotocol/server-github` | 20+ | B | TypeScript |
| **Twitter/X** | `tharuxpert/x-mcp` | 14 | B | TypeScript |
| **Instagram** | `mcpware/ig-mcp` | 23 | B | Python |
| **Browser** | `microsoft/playwright-mcp` | 15+ | A | TypeScript |
| **Calendar** | `zavora-ai/mcp-calendar` | 12 | B | Rust |
| **Telegram+Slack+Discord** | `santoshakil/nexus` | 48 | B | Rust |
| **Spotify** | `darrenjaworski/spotify-mcp` | 32 | B | TypeScript |
| **Knowledge/Memory** | `andylow92/file-system-brain-mcp` | 27 | A | TypeScript |
| **Everything else** | `elyerinfox/lodestone-mcp` | 475 | A | Rust |
| **Web search** | `sweetcornna/free-search-mcp` | 10 | A | Python |

**Total: ~725 tools across 13 MCP servers, all 100% free.**

---

## 16. How NEXUS's Router Brain Uses MCP Servers

The 0.5B brain doesn't talk to MCP servers directly. Instead, NEXUS's Rust
backend runs the MCP servers as subprocesses and exposes them to the brain
as function-calling tools:

```
User: "hey nexus, send mom a whatsapp message saying I'll be late"
  |
Router Brain (0.5B, port 39219)
  |
  +-- Function call: whatsapp_send_message(to: "mom", text: "I'll be late")
  |
NEXUS Rust backend
  |
  +-- Calls WhatsApp MCP server (sealjay/mcp-whatsapp, port 8765)
  |
  v
WhatsApp Web -> message sent
  |
  v
Brain: "Done, sir. Message sent to mom."
```

The brain sees a simplified tool list. The Rust backend handles the MCP
protocol, subprocess management, and security.

### 16.1 Tool Schema (What the Brain Sees)

The brain doesn't see all 725 tools at once (that would overwhelm a 0.5B
model). Instead, NEXUS loads tools **on demand** based on context:

```
When user mentions WhatsApp:
  Load: whatsapp_send_message, whatsapp_list_chats, whatsapp_get_chat_messages

When user mentions email:
  Load: email_list_emails, email_send_email, email_search_emails

When user mentions YouTube:
  Load: youtube_search_videos, youtube_get_transcript

When user mentions GitHub:
  Load: github_list_issues, github_create_pr, github_search_code
```

This keeps the brain's context small and its decisions accurate.

### 16.2 Security: Confirmation Gates

Some MCP tools require explicit user confirmation before execution:

| Tool type | Confirmation required? |
|-----------|----------------------|
| Read (list emails, search YouTube, get calendar) | No |
| Send (WhatsApp message, email, tweet) | **Yes** |
| Delete (email, file, GitHub issue) | **Yes** |
| Browser navigation | No (but user is told) |
| Browser form fill (login, payment) | **Yes** |
| Social media post | **Yes** |
| File write | **Yes** |

NEXUS's Rust backend enforces this. The brain can call any tool, but
destructive tools are intercepted and the user is asked "Should I send
this message to mom?" before execution.

---

## 17. RAM Impact on NEXUS

MCP servers run as **separate processes**, not inside the brain. So they
don't count against the brain's 500 MB RAM budget.

| MCP server | RAM (estimated) | Runs when? |
|-----------|----------------|------------|
| sealjay/mcp-whatsapp (Go) | ~20 MB | Always (if WhatsApp enabled) |
| youtube-mcp-server (Python) | ~30 MB | On demand |
| email-agent-mcp (Node.js) | ~40 MB | On demand |
| server-github (Node.js) | ~40 MB | On demand |
| playwright-mcp (Node.js) | ~50 MB + browser | On demand |
| mcp-calendar (Rust) | ~10 MB | Always |
| santoshakil/nexus (Rust) | ~15 MB | Always |
| lodestone-mcp (Rust) | ~15 MB | Always |
| file-system-brain-mcp | ~40 MB | Always |

**Always-on total:** ~100 MB (WhatsApp + Calendar + Nexus + Lodestone + fsbrain)
**On-demand:** Loaded when needed, killed after idle timeout

**The brain stays at ~500 MB. MCP servers add ~100 MB always-on + on-demand.**
Total NEXUS RAM with full MCP stack: ~600 MB always-on, ~800 MB when a
cloud MCP server is active.

---

## 18. The Honest Limitations

### 18.1 WhatsApp ToS Risk

The unofficial WhatsApp MCP servers (`sealjay`, `wappmcp`, `kahflane`) use
the WhatsApp Web protocol (whatsmeow, whatsapp-web.js). This violates
WhatsApp's Terms of Service. Your account **can be banned**.

**Mitigations:**
- Use a burner number
- Warm up slowly (20 messages/day for the first week)
- Don't spam
- Use the official Meta Cloud API server (`FredShred7`) for production

### 18.2 Instagram DMs Require Meta App Review

Instagram DMs via the Graph API require Advanced Access (Meta App Review).
This is free but takes time to get approved. Standard Access (immediate)
covers posts, comments, and insights only.

### 18.3 X/Twitter Free Tier Limits

X API free tier allows posting and reading. Some tools (bookmarks, likes)
require Basic+ tier ($100/month). Reply is restricted on free tier.

### 18.4 Spotify Premium for Playback

Spotify playback control requires Premium. Free accounts can search and
manage playlists but not control playback.

### 18.5 MCP Server Maturity Varies

Some MCP servers are production-ready (Microsoft Playwright, official
GitHub, Slack with 1,755 stars). Others are new with few stars. NEXUS
should prefer servers with high stars, active maintenance, and MIT license.

---

## 19. Summary

| Question | Answer |
|----------|--------|
| Can NEXUS control WhatsApp for free? | **Yes** — `sealjay/mcp-whatsapp` (42 tools, Go, MIT) |
| Can NEXUS control YouTube for free? | **Yes** — `anarcyst/youtube-mcp-server` (8 tools, no API key) |
| Can NEXUS control email for free? | **Yes** — `usejunior/email-agent-mcp` (14 tools, send allowlist) |
| Can NEXUS control GitHub for free? | **Yes** — official `@modelcontextprotocol/server-github` (20+ tools) |
| Can NEXUS control social media for free? | **Yes** — Twitter, Instagram, LinkedIn, Bluesky, Reddit all have free MCP servers |
| Can NEXUS control a browser for free? | **Yes** — `microsoft/playwright-mcp` (official, Microsoft-maintained) |
| Can NEXUS control calendar for free? | **Yes** — `zavora-ai/mcp-calendar` (12 tools, Rust, Google + Outlook) |
| Can NEXUS control Telegram/Discord/Slack for free? | **Yes** — `santoshakil/nexus` (48 tools, 5 platforms, Rust) |
| Can NEXUS control Spotify for free? | **Yes** — `darrenjaworski/spotify-mcp` (32 tools, OAuth) |
| Can NEXUS have long-term memory for free? | **Yes** — `andylow92/file-system-brain-mcp` (27 tools, self-improving) |
| Can NEXUS do web search for free? | **Yes** — `sweetcornna/free-search-mcp` (10 tools, no API key) |
| Can NEXUS do "everything else" for free? | **Yes** — `elyerinfox/lodestone-mcp` (475 tools, Rust, keyless) |
| Total tools available for free? | **~725 tools across 13 MCP servers** |
| Total cost? | **$0** |
| Brain RAM impact? | **~100 MB always-on** (MCP servers run as separate processes) |

**The MCP ecosystem is the missing piece.** NEXUS's Router Brain doesn't
need to build integrations — it just calls free MCP servers that already
exist. The brain routes, the MCP servers execute, the user confirms
destructive actions. All free, all open-source, all self-hosted.
