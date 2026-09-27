# MCP OAuth Spec — What NEXUS Must Implement (2026-09-20)

Source: `modelcontextprotocol.io/specification/2025-11-25` and
`2026-07-28`, `basic/authorization/*`. This is the protocol all
OAuth-class servers (Swiggy and any future hosted MCP) must follow, and
the checklist our Swiggy phase will be graded against.

---

## 1. The normative sequence (client role — that's us)

```
C->M: MCP request without token
M->C: HTTP 401 + WWW-Authenticate: Bearer resource_metadata="<url>"   (RFC 9728)
C->M: GET resource_metadata URI  (fallback: /.well-known/oauth-protected-resource[/path])
M->C: { authorization_servers: [...] }
C->A: GET authorization-server metadata (RFC 8414, try oauth + OIDC endpoints in order)
A->C: metadata (MUST validate issuer == the URL used, RFC 8414 §3.3)
C:    register — priority: (1) pre-registered, (2) Client ID Metadata Document,
     (3) Dynamic Client Registration (deprecated fallback), (4) prompt user
C:    PKCE params + resource indicator (RFC 8707) + scope selection + record issuer
C->B: open browser: authorize URL + code_challenge + resource + state
B->A: user authorizes
A->B: redirect to callback with code + iss
B->C: callback (validate iss vs recorded, RFC 9207)
C->A: token request + code_verifier + resource
A->C: access (+ refresh) token
C->M: retry original MCP request with Bearer token
```

## 2. Rules that constrain our design

- **Servers MUST serve Protected Resource Metadata; clients MUST use
  it.** Our Swiggy client must parse `WWW-Authenticate` first, well-known
  probing second — never hardcoded auth URLs as the only path.
- **Client ID Metadata Documents SHOULD be supported; DCR is
  deprecated.** Our Worker can host the metadata document at an HTTPS URL
  (we control `nexus-worker.*.workers.dev`) — this avoids per-user
  dynamic registration entirely. DCR only as fallback.
- **Scope selection:** use `scope` from the 401 challenge when present,
  else minimal `scopes_supported`. Least privilege at first auth.
- **Step-up authorization (runtime `insufficient_scope`):** re-authorize
  with the UNION of old + new scopes (never drop granted scopes), retry
  bounded ("no more than a few times"), track attempts per
  resource+operation to avoid loops.
- **Registration state is per authorization server** (keyed by issuer);
  never reuse credentials across changed metadata — surface an error and
  re-register.
- **PKCE + `resource` parameter + `iss` validation are MUSTs**, not
  hardening extras. Our `setup/oauth.ts:37-52` already generates
  verifier/challenge correctly; add `resource` and `iss` checks in the
  Swiggy pass.

## 3. Fit against our existing OAuth (`setup/oauth.ts:37-201`)

| Spec step | Our status |
|---|---|
| 401 + metadata discovery | MISSING — add for Swiggy |
| Metadata-document registration | MISSING — host doc on Worker, preferred path |
| DCR fallback | MISSING — add only as fallback |
| PKCE + browser + deep-link callback + polling | PRESENT and proven (Google/GitHub) |
| `resource` indicator + `iss` validation | MISSING — add |
| Vault store + silent refresh | PRESENT for Google; extend to Swiggy |
| Bounded retry / no scope loss on step-up | PARTIAL (401-retry-once exists; add scope-union rule) |

The Swiggy phase is therefore: discovery + registration + two validation
fields + refresh extension — on top of a flow that already works. It is
not a new auth system.
