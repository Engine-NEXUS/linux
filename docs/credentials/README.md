# NEXUS Credentials & Security Documentation

This folder details the authentication architecture, token management, encryption at rest, and OAuth 2.1 integrations for NEXUS.

---

## 🔐 Credentials Document Catalog

| # | Document | Scope & Focus |
|---|----------|---------------|
| 01 | **[01-credential-architecture.md](01-credential-architecture.md)** | Master credential architecture: 3 token types (OAuth tokens, API keys, device tokens), storage locations, request-time resolution, and threat boundaries. |
| 02 | **[02-oauth-flow.md](02-oauth-flow.md)** | OAuth 2.0 / 2.1 PKCE authorization code flow step-by-step for Google and GitHub, token exchange, and automatic silent refresh. |
| 03 | **[03-api-keys.md](03-api-keys.md)** | Management of user-supplied LLM API keys (Gemini, Groq, Cerebras), encryption at rest with Fernet/AES-GCM, and injection headers. |
| 04 | **[04-google-integrations.md](04-google-integrations.md)** | Google Workspace APIs (Contacts, Gmail, Calendar), OAuth scope union, quota management, and user permissions. |
| 05 | **[05-github-integration.md](05-github-integration.md)** | GitHub Personal Access Tokens and OAuth App integration, requested scopes (`repo`, `read:org`, `workflow`), and Octocrab integration. |
| 06 | **[06-device-registration.md](06-device-registration.md)** | Edge device registration, machine ID derivation, Cloudflare D1 pairing records, and multi-user device tokens. |
| 07 | **[07-security-best-practices.md](07-security-best-practices.md)** | Secret hygiene, audit logging, zero plain-text storage rules, text-only protocol sandboxing, and incident response playbook. |
| 08 | **[08-setup-page-guide.md](08-setup-page-guide.md)** | User-facing walkthrough of the setup wizard: connection tabs, OAuth authorizations, and API key configurations. |
