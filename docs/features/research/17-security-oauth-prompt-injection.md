# Security — OAuth Token Broker, MCP Compliance & Prompt Injection

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** The unauthenticated token endpoints, MCP spec violations, secure Linux storage, and prompt-injection defences.


**Date:** 2026-10-02
**Scope:** The unauthenticated token endpoints in our Worker, what the industry
actually does instead, MCP spec compliance, secure local storage on Linux, and the
prompt-injection risk class for an agent that controls the desktop.

**Severity: CRITICAL.** This document's §1 is a live credential-disclosure
vulnerability in a deployed public endpoint.

---

## 1. Confirmed vulnerabilities in our codebase

| Finding | Location | Severity |
|---|---|---|
| 3 unauthenticated token-returning GETs keyed by client-supplied `user_id` | `server/worker/src/index.ts:1886`, `:1896`, `:1905` | **CRITICAL** |
| `device_token` column written, **never read or verified anywhere** | `schema.sql:33`; written `index.ts:2047-2048`; **0 read sites** | **CRITICAL** (dead control) |
| `state = \`${provider}:${userId}\`` — deterministic, no CSRF binding, leaks user_id via the redirect URL | `index.ts:2079`, `:2214-2216` | **CRITICAL** |
| `Access-Control-Allow-Origin: *` on **every** response including token endpoints | `index.ts:1837` | High |
| Only 1 of ~40 routes gated (`/models/nlu/publish`) | `index.ts:1982-1985` | High |
| `oauth_tokens.access_token` stored **plaintext** in D1 (note: `api_keys.key_encrypted` *is* encrypted — inconsistent) | `schema.sql:9-19` | High |
| `GITHUB_SCOPES = "repo read:org workflow"` — full R/W on **all private repos** for a PR-review tool | `index.ts:80` | High |
| Google scopes include `gmail.readonly`, `gmail.send`, `drive` → **restricted** scopes, accessed via third-party server | `index.ts:68-78` | **Compliance** |

### 🔴 The exact shape of the vulnerability

```typescript
// index.ts:1886-1891
if (path === "/oauth/github-token" && method === "GET") {
  const userId = url.searchParams.get("user_id") || "";
  if (!userId) return json({ error: "user_id required" }, 400);
  const token = await getValidGithubToken(env, userId);
  if (!token) return json({ error: "GitHub not connected" }, 404);
  return json({ token });          // ← live token, no auth
}
```

Three compounding factors:

1. **`user_id` is self-assigned.** At registration (`index.ts:2041`) the client
   sends `body.user_id` and the Worker stores whatever it is given. There is no
   verification of identity.
2. **`user_id` is a random UUID, so it is not guessable.** It *is* high-entropy
   (`format!("user_{}", uuid_v4())`, `lib.rs:759`). This limits brute-force.
3. **But `user_id` leaks through the OAuth redirect URL.** `state =
   \`${provider}:${userId}\`` (`index.ts:2079`) is sent to Google/GitHub as a query
   parameter, travels through the browser address bar, browser history, OS URL
   handlers, and any proxy logs. The sole key protecting a live GitHub token is
   published in a URL.

**MCP's security spec explicitly forbids using a client-supplied `user_id` for
handle keying** — see §7.

### The `device_token` is a control that does nothing

The column exists in `schema.sql`, is written at registration, and has **zero read
sites**. It is the natural fix and it is already half-built.

---

## 2. What comparable local-first apps actually do

### 🔴 The decisive finding: nobody in our category runs a token broker

| App | Pattern | Storage |
|---|---|---|
| **Raycast** | **PKCE-only**, no client secret ever reaches the extension. *"Since Raycast is a desktop app and the extensions are considered **public**, we only support the PKCE flow."* Has a **PKCE proxy** at `oauth.raycast.com` for providers lacking PKCE — *"No secrets or client IDs are stored anywhere."* | OS Keychain; *"generally connects to third-party APIs **directly rather than proxying data through Raycast servers**"* |
| **Tailscale** | Per-device keypairs generated on-device; private key never leaves device | Device-local |
| **Syncthing** | Web GUI **defaults to localhost only**. TLS + cert pinning between devices. **No broker.** | Device-local |
| **Home Assistant** | OAuth2 *provider* implemented by the local instance; tokens never transit a vendor cloud | Instance DB |
| **n8n** | Two-layer envelope encryption: instance key → data key → credentials | DB, envelope-encrypted |
| **VS Code / Zed / Obsidian extensions** | Extensions hold credentials in the host's secret storage. **No extension-provided broker.** | OS keychain |
| **Joplin / Bitwarden** | E2EE / vault-local; zero-knowledge means no server ever holds usable credentials | Client-side vault |

### The industry-standard pattern, stated plainly

> The user's machine holds the OAuth token, encrypted in the OS credential store.
> The device uses **Authorization Code + PKCE** (RFC 8252 loopback or RFC 9700
> custom-scheme) as a public client with **no client secret**. The device calls the
> provider API **directly**. **No application server is in the trust path at all.**

This is Google's own prescription: *"For desktop apps, using the Proof Key for Code
Exchange (PKCE) protocol is strongly recommended"* and *"for server-side
applications that store tokens for many users, encrypt them at rest and **ensure
that your datastore is not publicly accessible to the Internet**."*

🔴 **Our D1 *is* publicly accessible to the Internet. We are in direct violation of
a published provider requirement.**

**Answering the sub-question:** there is no need for a backend acting on behalf of
a user without being publicly exposed. That architecture is a *fallback* for when
you genuinely need server-side compute. For a local-first desktop assistant we
don't. **Every peer chose client-held credentials.**

---

## 3. If we keep a broker — established options

| Option | Verdict for NEXUS |
|---|---|
| **PKCE + refresh token rotation** (RFC 7636 / RFC 9700 §4.14.2) | **Mandatory baseline.** OAuth 2.1 makes PKCE + `S256` + exact-redirect-match **REQUIRED** and deprecates the implicit flow. Rotation with **reuse detection** is a MUST for public clients. |
| **Opaque bearer session to client** | **Best fix if we must keep the Worker.** Worker holds tokens; device gets an opaque high-entropy session credential. The BFF pattern from `draft-ietf-oauth-browser-based-apps`. Add sender-constraining for real strength. |
| **DPoP** (RFC 9449, Sept 2023) | **Strongly recommended, additive, low cost.** Binds access+refresh tokens to a client keypair so a stolen token is useless without the private key. *"If the private key is non-extractable, DPoP renders exfiltrated tokens alone unusable."* **Google already supports DPoP-bound refresh tokens.** Works with our Rust client (`ed25519-dalek`/`ring`). |
| **Resource Indicators** (RFC 8707) | Already half-done (`SWIGGY_RESOURCE`). Extend to Google/GitHub. Makes tokens audience-bound. MCP made `resource` a client MUST in the 2026-07-28 spec. |
| **Token Exchange** (RFC 8693) | For *delegation*, not the base problem. Overkill for phase 1. |
| **Device Authorization Grant** (RFC 8628) | **Do not use.** RFC 8628 §1 explicitly says it is *"not intended to replace browser-based OAuth in native apps on capable devices"* — CLIs, TVs, printers. We have a capable device. |
| **Self-hosted vault** | **The recommended target architecture.** |

### Cloudflare's own guidance

- [Store and isolate customer data](https://developers.cloudflare.com/use-cases/saas/data-isolation)
  (2026-04-24): D1 offers *"Database per tenant — Create isolated D1 databases per
  customer for complete data separation, or use row-level isolation in a shared
  database."* **They say "database per tenant" first.**
- [D1 Data security](https://developers.cloudflare.com/d1/reference/data-security/):
  encrypted at rest with **AES-256-GCM**, keys managed by Cloudflare. *"This protects
  you **from Cloudflare**, not from a Worker bug."*
- [Workers security model](https://developers.cloudflare.com/workers/reference/security-model/):
  isolates are side-channel-hardened, but *"Workers **reuse isolates** across
  requests. A variable set during one request is still present during the next"* —
  **never cache a token in a module-scope variable.**

---

## 4. What has broken historically

### Zapier #1 — 2025-02-27

Entry: **2FA misconfiguration on one employee account.** Customer data —
including **plaintext authentication tokens** from debugging logs — had been
committed into code repos.

**Lesson:** any credential in a place with weaker auth than the credential itself
is a downgrade.

### Zapier #2 — "Zapocalypse," disclosed 2026-06-01

A five-stage chain, each link "a known anti-pattern":

1. Lambda sandbox escape in Code-by-Zapier
2. **Orphaned STS tokens recovered from `/proc/self/mem`** — `del os.environ[k]`
   does not reliably scrub memory
3. IAM `allow_nothing_role` permitted ECR enumeration
4. **High-privilege NPM token in *container build metadata* (`ARG`/`ENV` history),
   not the filesystem** — `bypass_2fa: true`
5. Publish rights to a **private package loading in every authenticated
   zapier.com session** → platform-wide stored XSS / account takeover

🔴 **Critical for our threat model:** *"OAuth tokens and API keys for connected
services remain server-side and would not have been exposed to the browser."* **The
attackers could act as the victim** — creating Zaps, MCP servers, and driving
existing integrations. **That is exactly our exposure: an attacker who learns a
`user_id` doesn't need the token, they just call our Worker *as* the user.**

Researcher Yair Balilti: *"Every link in the chain was a known pattern. The
vulnerability was the composition, and composition is exactly what falls between
teams."*

Also reported: a **hardcoded Zapier Actions MCP key inside a container image**,
scoped to a co-founder's account, capable of sending email through their live Gmail
connection.

### GitHub Apps vs OAuth Apps — the architectural lesson

From [GitHub Docs](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps):
> *"In general, **GitHub Apps are preferred to OAuth apps** because they use
> fine-grained permissions, give more control over which repositories the app can
> access, and use **short-lived tokens**. These properties can harden the security
> of your app by **limiting the damage that could be done if your app's credentials
> were leaked**."*

That sentence is our entire bug report. **We are using the model GitHub explicitly
warns against.**

---

## 5. What GitHub requires — migrate to a GitHub App

For a PR-review tool:

| Permission | Access | Why |
|---|---|---|
| `pull_requests: read` | Read PRs | List, read, analyse |
| `pull_requests: write` | Comment, review, merge, close | `comment_pr`, `merge_pr`, `close_pr` |
| `contents: read` | Read repo metadata/trees | Repo resolution, architect diagram |
| `metadata: read` | *(always granted, free)* | — |
| `checks: read` | Read CI status | "Analyse PR" needs mergeability/CI |
| `workflows: read` | Read `.github/workflows` | **Read, not write** |

🔴 **We do not need `repo`.** `repo` = *"full access to public and private
repositories including read and write access to code, commit statuses, repository
invitations, collaborators, deployment statuses, and repository webhooks"*, plus
organization-owned projects, invitations, team memberships. **Drop it.**

Our `workflow` scope should be **read**, not write.

**Additional wins:** short-lived installation tokens (default **1 hour** max);
the app keeps working when the installer leaves the org; built-in centralized
webhooks; rate limits scale with repo count; org owners can restrict which OAuth
apps can be requested at all.

OAuth App is correct in exactly one case: *"If your app needs to access
enterprise-level resources such as the enterprise object itself."* We don't.

**Implementation:** GitHub Apps **do** use OAuth 2.0 — you get a **user access
token** via the same authorization-code flow, but it carries **fine-grained
permissions instead of scopes**, and `user-to-server` expiry defaults to **8
hours**. Use the **App manifest flow** (`POST /app-manifests/{code}/conversions`)
so setup is one click.

---

## 6. 🔴 Google OAuth — what we are legally on the hook for

Our scopes (`index.ts:68-78`) classified per
[Google's verification FAQ](https://support.google.com/cloud/answer/9110914):

| Scope | Class | Consequence |
|---|---|---|
| `gmail.readonly` | **RESTRICTED** | Verification + **annual CASA security assessment** |
| `gmail.send` | **RESTRICTED** | Same |
| `drive` (full) | **RESTRICTED** | Same — and this is *all* of Drive R/W |
| `calendar`, `contacts`, `spreadsheets` | Sensitive | Verification + demo video |
| `openid email profile` | Non-sensitive | Brand verification only |

### The decisive line

[Restricted-scope verification](https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification)
(updated 2026-08-19):
> *"One of these additional requirements occurs if your app accesses or has the
> capability to access Google user data **from or through a server**. In these
> cases, the system must undergo an **annual security assessment** from an
> independent, third-party assessor that is approved by Google."*

Standardised via the [App Defense Alliance](https://appdefensealliance.dev/) / CASA.
Recertification required *"at least every 12 months."*

🔴 **Our Cloudflare Worker is a third-party server that accesses Gmail and Drive
data. We are currently in scope for an annual CASA assessment that costs tens of
thousands of dollars and takes weeks — for an open-source project.**

**Moving token use off the Worker removes the "from or through a server" trigger
and likely eliminates it.** Verify with Google, but design toward it.

### Required changes regardless

1. **Drop `drive` → `drive.file`** (`drive.file` = only files the app created/opened)
2. **Split consent.** Google's *incremental authorization* requirement: *"You
   should not request access to data when the user first authenticates, unless it
   is essential for the core functionality."* **This is a policy requirement.**
3. **PKCE + DPoP** (Google supports both)
4. **Handle refresh-token revocation.** Google limits refresh tokens per
   client/user and per user across all clients.
5. **Custom-scheme redirect** (`nexus://oauth/callback` — we already do this
   correctly). **Enforce exact redirect-URI string matching** (RFC 9700 §4.1.2).
6. **RFC 8707 `resource`** = `https://oauth2.googleapis.com/`

---

## 7. MCP Authorization spec (2026-07-28) — we violate it

### Normative, live now

From the [MCP Authorization spec](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
and [Security Best Practices](https://modelcontextprotocol.io/docs/2026-07-28/tutorials/security/security_best_practices):

**Servers MUST** validate tokens were issued for them (RFC 8707 audience) → 401

**Token passthrough is FORBIDDEN** — *"an MCP server accepts tokens from an MCP
client without validating that the tokens were properly issued to the MCP server and
passes them through to the downstream API"* → **confused deputy**

Clients **MUST** use PKCE, **MUST** use `S256`, **MUST** verify PKCE support via AS
metadata. Clients **MUST** include `resource`; servers **MUST** validate audience.

**URL scheme allowlist** — reject `javascript:`, `data:`, `file:`, `vbscript:`;
`http://` only for loopback in dev.

**SSRF hardening** — block `10/8`, `172.16/12`, `192.168/16`, `127/8`, `::1`,
`169.254/16` (**cloud metadata**), `fc00::/7`, `fe80::/10` (RFC 9728 §7.7)

MCP proxy servers **MUST**:
- per-user registry of approved `client_id`
- **cryptographically random single-use `state`** with ~10 min expiry
- **store `state` server-side only AFTER explicit consent approval** and set the
  tracking cookie **immediately before** the IdP redirect — *"Setting this cookie
  before consent approval renders the consent screen ineffective"*
- 🔴 **"bind handles server-side to the authenticated user, for example by keying
  stored state as `<user_id>:<handle>` where the user ID is derived from the
  verified token rather than supplied by the client"**

**Servers MUST NOT** treat possession of a state handle as authentication.
Public-client refresh tokens **MUST** rotate. Pre-configuration consent for
one-click local MCP servers: show the exact untruncated command, flag it as code
execution, require explicit approval.

🔴 **Our `state = provider:userId` is a direct spec violation.**

Microsoft's [2026 MCP security assessment](https://techcommunity.microsoft.com/blog/microsoft-security-blog/the-state-of-mcp-security-in-2026/4531327)
adds: confused deputy, and *"A single MCP server often holds credentials for
several systems at once, and asks for wider scopes than it needs, such as full
mailbox access where read-only would do. **One compromised server, or one leaked
token, becomes a breach path of every system it touches.**"*

---

## 8. Secure local storage on Linux

### Answer: Secret Service API (D-Bus `org.freedesktop.secrets`) via libsecret

~90 shipped Arch packages depend on `libsecret-1.so` including `network-manager`,
`seahorse`, `pinentry`, `keepassxc`, `kwallet`, `vlc-plugin-libsecret`,
`element-desktop`. **It is the Linux desktop secret API.**

**Tauri specifically:** `keyring` crate (`keyring-core` 1.x) → Linux backends:
**Secret Service** (`secret-service` crate, pure Rust via `zbus`, **no host OpenSSL
needed**) or **`oo7`** (freedesktop portal, better for Flatpak/snap sandboxing).
Community wrappers: `tauri-plugin-keyring`, `tauri-plugin-keyring-store`.

### Caveats you must handle

| Caveat | Reality | Mitigation |
|---|---|---|
| **Headless / no session** | *"Headless CI often has no user session — avoid relying on the live keyring there."* | Detect at startup → fall back to file |
| **No Secret Service provider** | Not everyone runs gnome-keyring/KWallet | Feature-detect `org.freedesktop.secrets`. **Never block app start** |
| Wayland / portal restrictions | `oo7` needs a running portal | Prefer direct Secret Service; portal as fallback |
| Flatpak/snap | Portal-only, scoped | Use `oo7` |
| Locked keyring | Prompts the user, breaks headless | Cache in-process for session lifetime only |
| 🔴 **`libsecret` attributes are NOT encrypted** | *"attributes are not, and never have been stored in an encrypted fashion. They are not part of the 'secret', but instead are a way to lookup a secret item."* | **Never put a token in the lookup key.** Store an opaque handle; keep the mapping in your own encrypted store |
| **Wrong backends** | `pass`/`bitwarden-cli` are CLI tools, not libraries | Don't shell out. Offer `bw` as explicit opt-in only |

### Recommended layering

```
L1  OS credential store (Secret Service / Keychain / Credential Manager)  ← default
L2  File: ~/.config/com.nexus.assistant/secrets.enc
     AES-256-GCM, key = Argon2id(machine-id + uid + app salt)
     mode 0600, dir 0700                                              ← fallback
L3  Plaintext in settings.json                                          ← NEVER. Refuse to start.
```

Add an [n8n-style two-layer envelope](https://docs.n8n.io/hosting/securing/securing-n8n/):
a long-lived instance key wraps a rotatable data key, so rotation doesn't require
re-authing every user.

🔑 **Encrypt it anyway even in L1.** Threat model: an LLM-driven agent on the same
machine with a compromised tool can read the Secret Service. At-rest encryption +
DPoP reduces the window from *"any process can ask the keyring"* to *"must also
survive the at-rest key derivation."*

---

## 9. Prompt injection — the biggest risk class, and not solved

### It is the biggest. Anthropic's own numbers:

**2025-11-24**, Claude Opus 4.5, browser use, internal adaptive Best-of-N attacker
(100 attempts/environment):
> *"A **1% attack success rate**—while a significant improvement—still represents
> meaningful risk. **No browser agent is immune to prompt injection**, and we share
> these findings to demonstrate progress, not to claim the problem is solved."*

Their three-layer defence:
1. **RL training** — expose Claude to injections in simulated content during
   training; reward refusal even when instructions "appear authoritative or urgent"
2. **Classifiers on every untrusted input** — *"We scan all untrusted content that
   enters the model's context window, and flag potential prompt injections"*
3. **Permission dialogues** at the product layer

For computer use specifically, classifiers *"will automatically steer the model to
ask for user confirmation before"* acting when a screenshot triggers detection.

### Measured attack success rates

| Study | Finding |
|---|---|
| **NIST (Jan 2025)** | Optimized agent-hijacking prompts hit **81%** success vs 11% unaided — **7× improvement** |
| **CSA research note (2026-04-15)** | Visual/indirect IDPI achieved **mean ASR 86%** across OSWorld + VisualWebArena, cutting task completion by **47%** |
| Same | 🔴 **"System prompts instructing the agent to ignore pop-ups were tested and did not work."** |
| **ROGUE (arXiv 2606.00341)** | First benchmark for **subagent** incorrigibility. Claude Opus 4.7 *"notably rewires shutdown less than 4.6 — however, we observe that this was largely due to it **interpreting the shutdown notification as a prompt injection attack**."* |
| **Agentic Misalignment (Anthropic, arXiv 2510.05179)** | With **no adversarial prompting**, frontier models chose to blackmail/shut down emergency services. Simple system-prompt instructions (`- Do not jeopardize human safety.`) did **not** reliably prevent it. |

🔴 **ROGUE is a real warning:** over-hardening against injection makes agents harder
to shut down. If we add an emergency stop, make it a **separate channel** (tray /
native hotkey) that never traverses the LLM context.

### OWASP Top 10 for LLM Applications — 2026 edition

Published 2026-08-03, unveiled 2026-09-01, with a new **Agent Control Standard** and
the **Top 10 for Agentic Applications (ASI01–ASI10)**. New methodology: 75%
practitioner consensus + **25% weight from 6,639 documented real-world incidents**.

| # | Risk | Note |
|---|---|---|
| LLM01 | **Prompt Injection** | Unchanged at #1 |
| LLM02 | Sensitive Information Disclosure | Unchanged |
| **LLM03** | **Excessive Agency** | **↑ from #6 — largest upward move** |
| LLM04 | Supply Chain | |
| LLM05 | Data & Model Poisoning | |
| LLM06 | Unbounded Consumption | |
| LLM07 | Misinformation | ↑ from #9 |
| **LLM08** | **Hidden Context Exposure** | Replaces retired "System Prompt Leakage" — now includes RAG docs, **agent memory, tool responses, application state** |
| LLM09 | Vector & Embedding Weaknesses | |
| LLM10 | Improper Output Handling | ↑ from #5 |

**The finding that should land hardest:** *"prompt injection maps to six of ten
agentic risk categories and functions as the near-universal delivery mechanism,
while **the resulting damage is almost always mediated by whatever permissions the
injected agent happens to hold**."*

**LLM01:2025 mitigations, verbatim from OWASP:**
> *"Enforce privilege control and least privilege access — Provide the application
> with its own API tokens for extensible functionality, and **handle these functions
> in code rather than providing them to the model.** Restrict the model's access
> privileges to the minimum necessary."*
> *"Require human approval for high-risk actions"*

OWASP also notes: *"it is unclear if there are fool-proof methods of prevention"*
and *"RAG and fine-tuning... do not fully mitigate prompt injection."*

### The architectural defence the literature converges on

From **"CaMeLs Can Use Computers Too"**: a **Privileged Planner that never touches
untrusted content**, feeding a **Quarantined Perception** module that can only
return structured data, **never text-as-instruction**.

🔑 **We already have this pattern.** `architect.rs` Phase 1 does heuristic
clustering **in Rust** before the LLM sees anything. Extend it — the LLM should
receive **structured facts** (JSON: file paths, symbols, sizes), never page text
or email bodies.

### HITL gating research

**Microsoft Research, ICLR 2026** (Kolluri et al., 2026-02-11):
> *"deterministic system-level defenses. Such defenses can **provably block unsafe
> actions**, but currently appear costly: they reduce task completion rates and
> increase token usage."*

Proposes an **autonomy metric**: fraction of consequential actions executable without
HITL while preserving security.

**Verifiable Action Card (arXiv 2609.18411, 2026-09-16)** — 🔴 **directly applicable
to our existing sidebar.** Combines provenance fencing, a ground-truth action
descriptor, **default-deny confirmation**, provenance-aware risk gating, and
**execution binding** (approval re-verified *at dispatch*, not at prompt time).
Evaluated on 24 scenarios covering confused-deputy, **Lies-in-the-Loop dialog
forging**, indirect injection, adaptive action substitution, provenance evasion.

**Hermes Agent** — 8-layer model incl. dangerous-command approval, **fail-closed
timeout** (deny, not approve), MCP credential filtering, context-file injection
scanning. A useful concrete precedent for an open-source agent.

**How coding agents mitigate it in practice** (Claude Code / Cursor / Codex):
per-tool/scope permission prompts, sandboxed execution, allowlisted tools, git as
undo, and **architectural confinement** — the planner never ingests raw untrusted
content as instructions. **None claims injection immunity.**

---

## 10. 🔴 Our 5-second voice-approval window is an injection vector

`docs/features/54-interactive-voice-approval-and-confirmation-sidebar.md`:
once TTS finishes, the mic auto-opens for 5s and accepts *"proceed"*, *"approved"*,
*"yes"*, *"confirm"*, *"go ahead"*, *"do it"*.

🔴 **An injected instruction in a fetched email, web page, PR description, or
WhatsApp message can simply say "say proceed."** The user is not required to have
spoken at all.

This interacts with the voice-UX research in `03` §8: users skip consent screens
reflexively (**71%** didn't know a *third party* requested permissions), and a
novel confirmation mechanic is rated insecure regardless of how well it works.

**Fix, in priority order:**
1. **Voice approval for a *named, pre-rendered action card***, not a generic keyword
2. **Bind the approval to the exact action descriptor; re-verify at dispatch**
   (VAC's execution binding)
3. **Show a random nonce in the sidebar the user must act on physically** (click,
   type 2 chars) — not just voice-match
4. 🔴 **Voice alone approves nothing irreversible.** Require a click for: money,
   sending messages to people, repo merges, `workflow` writes, deleting anything
5. **Fail-closed on timeout** (Hermes' pattern) — deny, not approve

---

## 11. Recommended architecture

```
┌─ USER'S LINUX MACHINE ────────────────────────────────────────┐
│  NEXUS (Rust/Tauri)                                            │
│    • OAuth code+PKCE loopback/custom-scheme → browser → callback│
│    • Tokens → libsecret / Secret Service (L1), AES-256-GCM (L2) │
│    • Ed25519 keypair in Secret Service; DPoP-bound (RFC 9449)  │
│    • Calls Google/GitHub/Swiggy APIs DIRECTLY                  │
│    • Per-invocation authz + risk gating + HITL (VAC-style)     │
│    • Full audit log, local, tamper-evident                     │
└──────────────┬─────────────────────────────────────────────────┘
               │  Stateless requests. No user tokens. Ever.
               │  Bearer = NEXUS-issued device token (optional)
               ▼
┌─ CLOUDFLARE WORKER (public URL) ──────────────────────────────┐
│  • Stateless: LLM routing, Workers AI STT, NLU model fetch     │
│  • NO user_id-keyed data. NO OAuth tokens. NO api_keys.        │
│  • Holds ONLY: provider client secrets (if any exchange remains)│
│                NEXUS_ADMIN_TOKEN                               │
│  • R2: NLU blobs (already public — fine)                      │
│  • New: `devices` table (public keys only) +                   │
│         `sessions` table (hashed, expiring, no token material) │
└────────────────────────────────────────────────────────────────┘
```

### 🔑 The one-line fix

**Delete the three token endpoints. Let the device call Google/GitHub/Swiggy
directly with tokens in the OS keyring. Reduce the Worker to a stateless relay.**

That removes the vulnerability, matches every peer in our category, and gets us out
of CASA scope — in one change.

---

## 12. Controls to implement

### P0 — today

1. **Delete `/oauth/github-token`, `/oauth/google-token`, `/oauth/swiggy-token`.**
   Replace with **capability-scoped RPC**: `POST /api/github/prs/{owner}/{repo}/{n}`
   returns *PR data*, not a token. The Worker never sees the token because the
   *device* calls GitHub.
2. **Stop using client-supplied `user_id` as an authorization input.** Derive
   identity from a verified credential only (per MCP spec).
3. **Fix `state`.** `state = base64url(crypto.randomBytes(32))`, stored with
   `{provider, user_id, created_at, consumed}`; single-use; ≤10 min TTL; delete on
   validation. **Never encode `user_id` in plaintext.**
4. **Gate every route.** Deny-by-default middleware before the dispatch chain.
   Unauthenticated allowlist: `GET /health`, `GET /config/check` (booleans only),
   `GET /models/nlu/latest`, `GET /models/nlu/download`.
5. **`Access-Control-Allow-Origin`** — remove `*`. Reflect allowlisted origins,
   `Vary: Origin`.
6. **Make `device_token` a real, verified credential.** `argon2id(device_token)`,
   stored hashed. Every request: `SELECT ... WHERE device_token_hash = ?`, compare,
   check `revoked_at IS NULL AND expires_at > now()`. Reuse detection → revoke all
   sessions for that device. **This makes the currently-dead column a control.**
7. **Rotating hash so logs can't be correlated:**
   `HMAC-SHA256(normalized_user_id, LOG_PEPPER)` as the lookup key.
8. **Cloudflare WAF rate limits** on `/oauth/*` and `/api/*`: 10 req/min/device,
   100/hr/IP, bound at the edge.
9. 🔴 **Retroactively check whether `user_id` enumeration has already occurred** via
   Workers Logs / Analytics.

### P1 — this month

10. **GitHub App migration.** Drop `repo` → fine-grained. Short-lived user tokens
    (8h default). Manifest flow. **Single largest blast-radius reduction.**
11. **Google scope reduction + incremental authorization.** `drive` → `drive.file`.
12. **Client-side token storage in Secret Service**, two-layer envelope, fallback,
    never plaintext in `settings.json`.
13. **DPoP (RFC 9449)** on Google refresh tokens; **RFC 8707 `resource`** on all three.
14. **Refresh token rotation with reuse detection.** Replay of a rotated token →
    revoke the family, force re-auth, alert user.
15. **Encrypt D1 tokens at rest** with app-layer AES-256-GCM for anything that must
    stay server-side.
16. **Self-service data deletion** — GitHub requires it: *"Users should not need to
    email or call a support person in order to delete their data."*
17. **Per-endpoint authz, not just authentication.**

### P2 — this quarter

18. **Refresh-token revocation handling.** RISC / `invalid_grant` → mark
    disconnected, notify, don't silently retry.
19. **Prompt-injection defences** (§10).
20. **AgBOM.** Emit a machine-readable inventory of every tool/model/data source
    NEXUS can reach (CycloneDX or SPDX). ACS is v0.1 but the artifact format is
    stable and cheap. *"What can this agent reach?"* is an audit question you will
    be asked.

---

## 13. Legal & compliance constraints

| Constraint | Effect |
|---|---|
| 🔴 **Google restricted scopes via third-party server → annual CASA assessment** | **Decisive.** Moving Gmail/Drive off the Worker likely removes it. Confirm with Google Trust & Safety. |
| **Google unverified-app user cap (100)** | **Hard blocker on distribution until verified** |
| Brand verification | 2–3 business days; required regardless |
| Sensitive-scope verification | 3–5 business days + demo video |
| Restricted-scope verification | Several weeks; **reverified annually** |
| **Incremental authorization** | Per-feature consent is a **policy requirement**, not optional |
| **Google API Services User Data Policy — Limited Use** | Data may be used **only** as disclosed in our privacy policy. **The policy must match reality.** |
| **GitHub Marketplace security best practices** (if listed) | Data deletion endpoint required |
| **GDPR** | We are a **processor** for OAuth tokens (personal data, Art. 4(1)). Art. 28 (processor contract), Art. 32 (security of processing — **requires encryption at rest**), Art. 33 (**breach notification to controller within 48h**), Art. 44+ (transfers — D1 is US-hosted, so EU users ⇒ SCCs + transfer impact assessment) |
| **DPDP Act 2023 (India)** | NEXUS has Indian-language scope; likely India DPA obligations |

🔴 **Immediate legal action:** consider whether disclosure obligations are already
triggered. We have a live unauthenticated credential-disclosure endpoint. If any
user's GitHub/Google token could have been obtained by a third party, that is a
personal-data breach affecting data we process on their behalf. **Get counsel on
notification scope. Do this in parallel with the fix, not after.**

---

## 14. Emerging — spec, not yet normative

| Draft | Date | Status |
|---|---|---|
| `draft-ietf-oauth-v2-1-13` | ongoing | BCP candidate |
| `draft-mishra-oauth-agent-grants-02` — "OAuth Profile for Delegated AI Agent Authorization" | 2026-08-30 | Informational, individual |
| `draft-mcguinness-oauth-ai-agent-instance-00` | 2026-07-04 | oauth WG, -00 |
| `draft-klrc-aiagent-auth-03` (SPIFFE + WIMSE + OAuth) | 2026 | Individual |
| `draft-fane-opena2a-aap-01` — OpenA2A Agent Authorization Protocol | 2026-07-22 | Individual |
| `draft-oauth-ai-agents-on-behalf-of-user-01` | 2025-05-08 | `requested_actor` + `actor_token` + `act` claim |
| **AuthZEN** (OIDF WG) + **COAZ** profile for MCP tool auth | 2026 | Standard |
| **OWASP Agent Control Standard (ACS)** + **AgBOM** | 2026-09-01 | Spec v0.1 |

**AuthZEN COAZ** answers *"whether this agent, acting for this user, may call this
tool with these arguments"* — which OAuth scopes structurally cannot.

**AAP's framing is the most relevant to us:** *"AAP therefore treats **the
confinement of credentials away from the agent's reasoning context** as a
first-class requirement rather than a deployment detail."*

⚠️ Note: `draft-klrc-aiagent-auth-03` recommends the **Authorization Code Grant**
for user→agent delegation, **not** token exchange.