# NEXUS Cloudflare Worker — Fully Serverless

No server needed. No sidecar. No n8n. No Ollama. Just the NEXUS laptop +
this Worker + Cloudflare D1.

## Architecture

```
NEXUS Laptop (5 users)              Cloudflare Edge (free)
┌──────────────────────┐           ┌──────────────────────────┐
│ User speaks          │  HTTP     │  Worker                  │
│ STT (local)          │ ────────→ │  1. Classify intent      │
│ Sends transcript     │  POST     │  2. Get token from D1    │
│ + user_id            │           │  3. Refresh if expired   │
│                      │ ←──────── │  4. Call GitHub/Google   │
│ TTS speaks answer    │  JSON     │  5. Summarize            │
│ Sidebar shows result │           │  6. Return text          │
└──────────────────────┘           │                          │
                                   │  D1 Database (free)      │
                                   │  - OAuth tokens          │
                                   │  - User/device registry  │
                                   │  - API keys              │
                                   └──────────────────────────┘
```

## What the Worker handles (replaces sidecar + n8n + Ollama)

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | POST | Process transcript (classify → API call → summarize → return text) |
| `/health` | GET | Health check |
| `/api/register` | POST | Register new user + device (called by installer) |
| `/oauth/auth-url` | GET | Get OAuth authorization URL |
| `/oauth/exchange` | POST | Exchange OAuth code for tokens (stores in D1) |
| `/oauth/status` | GET | Check which providers are connected |
| `/oauth/disconnect` | DELETE | Remove a provider's tokens |
| `/apikeys/add` | POST | Store an API key |
| `/apikeys/remove` | DELETE | Remove an API key |
| `/apikeys/list` | GET | List stored API key providers |
| `/config/check` | GET | Check which OAuth providers are configured |

## Setup (10 minutes)

### 1. Install dependencies

```bash
cd server/worker
npm install
```

### 2. Login to Cloudflare

```bash
npx wrangler login
```

### 3. Create the D1 database

```bash
npx wrangler d1 create nexus-db
```

This prints a `database_id`. Copy it and paste it into `wrangler.toml`:

```toml
[[d1_databases]]
binding = "DB"
database_name = "nexus-db"
database_id = "PASTE_THE_ID_HERE"
```

### 4. Create the database tables

```bash
npx wrangler d1 execute nexus-db --file=schema.sql --remote
```

### 5. Set OAuth secrets

```bash
npx wrangler secret put GOOGLE_CLIENT_ID
# Paste your Google OAuth client ID

npx wrangler secret put GOOGLE_CLIENT_SECRET
# Paste your Google OAuth client secret

npx wrangler secret put GITHUB_CLIENT_ID
# Paste your GitHub OAuth client ID

npx wrangler secret put GITHUB_CLIENT_SECRET
# Paste your GitHub OAuth client secret
```

Get these from:
- Google: https://console.cloud.google.com/apis/credentials
- GitHub: https://github.com/settings/apps → New GitHub App

**Important:** You must set the redirect URI in both Google Cloud Console and GitHub Developer Settings to your Cloudflare Worker URL, NOT the desktop deep link.
Format: `https://nexus-worker.<your-subdomain>.workers.dev/oauth/callback`

### 6. Deploy

```bash
npx wrangler deploy
```

Output:
```
Published nexus-worker
  https://nexus-worker.<your-subdomain>.workers.dev
```

### 7. Bake the Worker URL into the installer

```powershell
$env:NEXUS_SERVER_URL = "https://nexus-worker.your-subdomain.workers.dev"
pwsh ./scripts/build.ps1
```

All 5 users install this same `.exe`. It automatically connects to the
Worker. No manual configuration needed.

## Cost

| Resource | Free tier | Your usage (5 users) | Cost |
|----------|-----------|---------------------|------|
| Worker requests | 100K/day | ~100-500/day | $0 |
| Workers AI neurons | 10K/day | ~1K-3K/day | $0 |
| D1 storage | 5GB | <1MB | $0 |
| D1 reads | 5M/day | ~500/day | $0 |
| D1 writes | 100K/day | ~10/day | $0 |
| **Total** | | | **$0/month** |

## Performance

| Metric | Old (n8n + sidecar) | New (Worker + D1) |
|--------|---------------------|-------------------|
| Cold start | 2,300ms | <5ms |
| Intent classification | 500ms (Ollama) | <50ms (Workers AI) |
| Token retrieval | 5ms (SQLite) | ~10ms (D1) |
| Total overhead | 3,500ms | <100ms |
| Server maintenance | Yes (VPS, Docker) | None |
| Monthly cost | $30 (VPS) | $0 |

## Local development

```bash
npx wrangler dev
```

Runs locally at `http://localhost:8785`. For local D1:

```bash
npx wrangler d1 execute nexus-db --file=schema.sql --local
```

## Security

- **Deny-by-default device gate** — every route except the public allowlist
  (`/health`, `/v1/profiles/claim`, `GET /oauth/callback`, `/models/nlu/*`,
  `/v1/admin/*` which carries its own admin token) requires a valid device
  bearer. The owner of a request is the device's canonical `profile_id`; a
  `user_id` in the query or body is ignored. See
  [`docs/changes/linux-port-j1-worker-deny-by-default-device-gate.md`](../../docs/changes/linux-port-j1-worker-deny-by-default-device-gate.md).
- OAuth tokens stored in D1 (encrypted at rest by Cloudflare)
- OAuth client secrets stored as Worker secrets (never in code)
- Worker is stateless (V8 isolate) — no data persists between requests
- HTTPS end-to-end (Cloudflare terminates TLS)
- D1 access only via the Worker binding (no external DB access)
- CORS reflects an allowlisted origin (never `*`) and varies on `Origin`; the
  device credential — not the origin — is what authorises a request

## Adding new intents

1. Add the intent name to the classifier prompt in `src/index.ts`
2. Write a handler: `async function handleX(req, env, token): Promise<string>`
3. Add a `case` in `handleTranscript`
4. Add credential retrieval if needed (D1 query)
