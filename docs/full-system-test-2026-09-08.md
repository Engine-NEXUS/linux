# NEXUS Full System Test Report — 2026-09-08

## Test Environment
- **Worker:** https://nexus-worker.chitkullakshya.workers.dev (Version ID: d0bca5b4)
- **User ID:** user_f9b8689fdffc444e81ee275c602eb1a3
- **Platform:** Windows 11, Intel i7-1355U
- **Rust:** cargo with features `custom-protocol,admin-brain`

---

## 1. Build Verification

### Rust Check
```
cargo check --features custom-protocol,admin-brain
→ Finished, 0 warnings, 0 errors
```
**Status: PASS**

### Frontend Build
```
npm run build (vite + tsc)
→ 817 modules transformed, built in 13.89s
→ 0 errors, 0 type errors
→ Bundle warnings: sidebar chunk >500KB (known, acceptable)
```
**Status: PASS**

---

## 2. Unit Tests

### Rust Tests (cargo test)
```
Library tests:       300 passed, 0 failed
Intent parser tests: 111 passed, 0 failed (subset of library)
Offline commands:     10 passed, 0 failed
User commands:         7 passed, 0 failed
Phase2 integration:   8 ignored (require network/API keys)
Doc tests:             0 tests
Total:               317 passed, 0 failed
```
**Status: PASS**

### Worker Tests (vitest)
```
github-routing.test.ts:  15 tests passed (new regression tests)
research.test.ts:        20 tests passed (6 new extractSearchEntity tests)
quota.test.ts:            5 tests passed
cache.test.ts:            9 tests passed
Total:                   49 tests passed
```
**Status: PASS**

---

## 3. Worker Backend Endpoints

### Health Check
```
GET /health → 200 OK
{"ok":true,"service":"NEXUS Worker","protocol":"text-only","serverless":true}
```
**Status: PASS**

### Config Check
```
GET /config/check → 200 OK
google: configured=true, scopes=[gmail, calendar, drive, openid, email, profile]
github: configured=true, scopes=[repo, read:org, workflow]
redirect_uri: nexus://oauth/callback
```
**Status: PASS**

### OAuth Status (Bug 5 fix verified)
```
GET /oauth/status?user_id=user_f9b8689fdffc444e81ee275c602eb1a3
→ github: connected=true, expired=false, scopes=[repo, read:org, workflow]
```
**Status: PASS** — `expired: false` (was `true` before fix)

---

## 4. Intent Classification (Worker)

### Greetings & Conversational
| Request | Intent | Response | Status |
|---------|--------|----------|--------|
| `hello nexus` | general | "Hello! How can I help you today?" | PASS |
| `thank you nexus` | general | "You're very welcome!" | PASS |
| `who are you?` | general | "I'm NEXUS, your helpful personal assistant." | PASS |
| `what can you do` | general | "I can help you manage your schedule..." | PASS |
| `good morning` | general | "Good morning! How can I help?" | PASS |

**New fixes applied:**
- "thank you nexus" was being routed to `search` (Wikipedia "Climate Nexus") — now correctly `general`
- "who are you" was being routed to `search` (Wikipedia "You" pronoun) — now correctly `general`
- Added `isGreetingOrThanks()` check before LLM classifier and before `isSearchQuestion` override
- Fixed `pull request` regex to `pull requests?` (plural) in keyword fallback

### Search / Research
| Request | Intent | Wikipedia Result | Status |
|---------|--------|------------------|--------|
| `what is Rust programming language` | search | "Rust (programming language)" | PASS |
| `who is Albert Einstein` | search | "Albert Einstein" (was "Hans Albert Einstein") | PASS |
| `what is the capital of France?` | search | "List of capitals of France" (was "Closed-ended question") | PASS |
| `what is quantum computing` | search | "Quantum computing" with citations | PASS |

**Bug 4 fix verified:** Entity extraction now strips question prefixes before Wikipedia search.

### GitHub
| Request | Intent | Response | Status |
|---------|--------|----------|--------|
| `analyse repo chitkullakshya/ServX` | fast_analyse | Analyzed ServX (645 files, React, MongoDB) | PASS |
| `list PRs in chitkullakshya/ServX` | github | PR #68 from ServX (was PR #254 from Zync) | PASS |
| `show pull requests in chitkullakshya/ServX` | github | PR #68 from ServX | PASS |
| `check latest PR in chitkullakshya/ServX` | github | PR #68 fetched and summarized | PASS |
| `check PR 68 in chitkullakshya/ServX` | github_analyse | Full PR analysis with verdict | PASS |
| `analyse PR 68 in chitkullakshya/ServX` | github_analyse | Full PR analysis with stats | PASS |
| `check issue 1 in chitkullakshya/ServX` | github | Issue summary | PASS |
| `deep analyse chitkullakshya/ServX` | deep_analyse | Architecture mapper triggered | PASS |

**Bug 1, 2, 3 fixes verified.**

### Architecture
| Request | Intent | Response | Status |
|---------|--------|----------|--------|
| `analyze this repo` | analyze_repo | Architecture mapper opening | PASS |
| `what breaks if I change the auth module` | analyze_repo | Architecture mapper opening | PASS |

---

## 5. Rust Intent Parser (Local)

### Offline Commands (tested via cargo test)
| Test | Status |
|------|--------|
| `close chrome` → CloseApp | PASS |
| `close whatsapp` → CloseApp (not nexus) | PASS |
| `close notepad` → CloseApp | PASS |
| `whatsapp chat with lakshya` → WhatsappChat | PASS |
| `whatsapp message lakshya` → WhatsappChat | PASS |
| `whatsapp send message to lakshya` → WhatsappChat | PASS |
| `whatsapp open my chat with lakshya` → WhatsappChat | PASS |
| `whatsapp chat with mom` → WhatsappChat | PASS |

**Status: PASS** — All 10 offline command tests pass.

### Intent Parser Coverage
- 111 intent parser tests covering:
  - Greetings (hello, hi, hey, thanks, goodbye, etc.)
  - App open/close (chrome, youtube, spotify, whatsapp, etc.)
  - Media controls (pause, next, play)
  - GitHub commands (PR, issue, branch, merge, approve, close, list)
  - Search queries (google search, url direct)
  - Architecture/analyse commands
  - STT mishearing fuzzy matching (cervix→servx, zinc→zync, etc.)
  - Filler word stripping
  - Case insensitivity

**Status: PASS** — All 111 tests pass.

---

## 6. Architecture Notes

### Request Flow
```
User speaks → STT (Groq/Moonshine) → Rust intent_parser
  → LocalCommand (open/close app, media, greeting) → <5ms, no network
  → GithubCommand (merge/approve/close PR) → Rust github_cmd → GitHub API
  → WorkerBackend (search, analyse, PR analysis) → Cloudflare Worker
    → classifyIntent (keywordFallback → isGreetingOrThanks → LLM → isSearchQuestion)
    → handleSearch / handleGitHub / handleFastAnalyse / handleGitHubAnalyse
    → Reply text + intent
```

### Offline vs Worker Routing
- `close chrome`, `open youtube`, `pause`, `whatsapp chat with X` → handled locally by Rust
- `analyse repo`, `list PRs`, `check latest PR`, `what is X` → sent to Worker
- The Worker correctly handles search, GitHub, and analysis intents
- The Worker does NOT handle offline commands (those are intercepted by Rust)

---

## 7. Bugs Found & Fixed During This Test Session

### Bug 6 (new): "show pull requests" misclassified as general
- **Root cause:** Keyword fallback regex used `pull request` (singular) — `\b` word boundary didn't match "pull requests" (plural)
- **Fix:** Changed to `pull requests?` in `keywordFallback`

### Bug 7 (new): "thank you nexus" routed to search
- **Root cause:** "nexus" triggered LLM classifier to return "search", which then fetched Wikipedia "Climate Nexus"
- **Fix:** Added greeting/thanks keyword detection in `keywordFallback` before the search check

### Bug 8 (new): "who are you" routed to search
- **Root cause:** `isSearchQuestion("who are you")` returned `true` (matched `who are` pattern), overriding the `general` intent from `classifyIntent`
- **Fix:** Added `isGreetingOrThanks()` check in `classifyIntent` (before LLM) and in the main router (before `isSearchQuestion` override)

---

## 8. Summary

| Component | Tests | Status |
|-----------|-------|--------|
| Rust build (cargo check) | 0 warnings | PASS |
| Frontend build (vite) | 0 errors | PASS |
| Rust unit tests | 317/317 | PASS |
| Worker unit tests | 49/49 | PASS |
| Worker health | 200 OK | PASS |
| Worker OAuth status | expired=false | PASS |
| Worker config | all configured | PASS |
| Greetings (5 tests) | 5/5 | PASS |
| Search (4 tests) | 4/4 | PASS |
| GitHub (8 tests) | 8/8 | PASS |
| Architecture (2 tests) | 2/2 | PASS |
| Offline commands (10 tests) | 10/10 | PASS |
| Intent parser (111 tests) | 111/111 | PASS |

**Overall: ALL TESTS PASS** — NEXUS is working correctly across all components.

### Bugs fixed this session
1. `analyse repo owner/repo` regex (Bug 1)
2. `list PRs in owner/repo` wrong repo (Bug 2)
3. `check latest PR` not classified (Bug 3)
4. Wikipedia irrelevant articles (Bug 4)
5. OAuth expired flag for classic tokens (Bug 5)
6. `show pull requests` plural mismatch (Bug 6, new)
7. `thank you nexus` misclassified as search (Bug 7, new)
8. `who are you` misclassified as search (Bug 8, new)
