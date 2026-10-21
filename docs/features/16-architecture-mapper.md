# 16 — Architecture Mapper

**Feature:** Voice-triggered, two-phase repository architecture analysis with real dependency graphing, impact analysis, and an agentic chat interface — built into NEXUS.

---

## Problem Statement

> *"Turn an unfamiliar repository into an explorable map of architecture and dependencies — then show what a change could break, and why."*
>
> Core objective: Transform an unfamiliar codebase into an understandable map of architecture and relationships, so developers can explore impact before they change anything.

The key constraints:

> *"A folder tree is not architecture."*
> *"Impact analysis must be explainable — show dependency paths, not just a list of affected files."*
> *"LLMs must add value beyond summarizing source."*

---

## How NEXUS Solves It

NEXUS uses a **three-phase progressive architecture** that gives the user immediate value, then deepens it silently in the background, then lets them interrogate the codebase by voice or text — like Cursor, but voice-first and visually grounded.

```
User: on github.com/vercel/next.js → says "NEXUS, analyze this repo"

Phase 1 (~8s)    → Fast visual map appears immediately
Phase 2 (~60s)   → Real import graph enriches the diagram in the background
Phase 3 (always) → Voice + text agent to explore impact and ask questions
```

---

## Architecture Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                      NEXUS Desktop (Tauri)                        │
│                                                                   │
│  Wake: "NEXUS, analyze this repo"                                 │
│       ↓                                                           │
│  Rust: GetForegroundWindow() → parse window title                 │
│        "vercel/next.js: The React Framework · GitHub – Chrome"   │
│       ↓                                                           │
│  Extracted: owner=vercel  repo=next.js                            │
│                                                                   │
│  ┌─────────── Phase 1 (fast path, ~8s) ──────────────┐           │
│  │  GitHub API (parallel, ~5 calls):                  │           │
│  │    GET /repos/{owner}/{repo}          → meta        │           │
│  │    GET /git/trees/HEAD?recursive=1    → full tree   │           │
│  │    GET /contents/package.json         → deps        │           │
│  │    GET /contents/README.md            → intent      │           │
│  │       ↓                                             │           │
│  │  Cloudflare Worker → LLM (Mistral 24B):             │           │
│  │    "Cluster this file tree into architectural       │           │
│  │     layers. Return typed ReactFlow JSON."           │           │
│  │       ↓                                             │           │
│  │  architect window opens — high-level diagram        │           │
│  │  NEXUS: "Got it. Loading real graph in background…" │           │
│  └─────────────────────────────────────────────────────┘           │
│       ↓  (concurrent with Phase 1, starts on URL detection)       │
│  ┌─────────── Phase 2 (deep path, background) ───────┐            │
│  │  git clone --depth=1 owner/repo                    │            │
│  │    → %APPDATA%\nexus\repos\owner-repo\             │            │
│  │       ↓                                            │            │
│  │  Rust background thread (rayon parallel):          │            │
│  │    Walk files (skip: node_modules, dist, .git)     │            │
│  │    Per file: detect language → extract imports     │            │
│  │    Build adjacency list: file → [imports]          │            │
│  │    Invert:  file → [files that import IT]          │            │
│  │    Compute: in-degree, out-degree, centrality      │            │
│  │    Detect:  circular deps (DFS), hub nodes         │            │
│  │       ↓                                            │            │
│  │  Graph JSON → Worker → LLM enriches diagram        │            │
│  │  Tauri event → ReactFlow upgrades in-place:        │            │
│  │    Real edges appear, hotspots turn red/orange     │            │
│  │  NEXUS: "Deep scan done. API client is a critical  │            │
│  │          hub — 34 files depend on it."             │            │
│  └────────────────────────────────────────────────────┘            │
│       ↓                                                            │
│  ┌─────────── Phase 3 (agent chat, always-on) ───────┐            │
│  │  User clicks node OR says:                         │            │
│  │    "What breaks if I change api/client.ts?"        │            │
│  │       ↓                                            │            │
│  │  If Phase 2 done:                                  │            │
│  │    Rust reverse BFS → affected files + paths       │            │
│  │    LLM explains path: A → B → C (root)             │            │
│  │    ReactFlow animates blast radius                 │            │
│  │    NEXUS speaks: "34 files depend on this.         │            │
│  │     Highest risk: Dashboard → client → App root"   │            │
│  │                                                    │            │
│  │  If Phase 2 still running:                         │            │
│  │    LLM answers from Phase 1 context (graceful)     │            │
│  │                                                    │            │
│  │  Text chat sidebar (Cursor-style):                 │            │
│  │    @mention files, typed follow-up questions       │            │
│  └────────────────────────────────────────────────────┘            │
└──────────────────────────────────────────────────────────────────┘
```

---

## Phase 1 — Fast Visual Map

### Goal
Give the user an oriented architecture overview in under 10 seconds. First impression, no waiting.

### URL Detection

NEXUS reads the OS foreground window title when the user wakes it:

- **Windows:** `GetForegroundWindow()` + `GetWindowTextW()`
  - GitHub titles follow: `"<owner>/<repo>: <description> · GitHub – <browser>"`
  - Regex: `/([a-zA-Z0-9_.-]+)\/([a-zA-Z0-9_.-]+).*·\s*GitHub/`
- **macOS:** `NSWorkspace` + Accessibility API for active tab URL
- **Linux:** `xdotool getactivewindow getwindowname`

If the window is not a GitHub page, NEXUS asks: *"Which repo should I analyze?"*

### GitHub API Calls

| Call | Endpoint | Purpose |
|---|---|---|
| Repo metadata | `GET /repos/{owner}/{repo}` | Stars, language, description, default branch |
| Full file tree | `GET /git/trees/HEAD?recursive=1` | Every file path — single call |
| Key files | `GET /contents/{path}` × 3–5 | `package.json`, `README.md`, `go.mod`, `Cargo.toml`, `tsconfig.json` |

**Total: 5–8 API calls. Time: ~2–4 seconds.**

GitHub API rate limit: 5,000 req/hour with auth. Token already stored in Cloudflare D1 from existing GitHub OAuth flow. Works for public and private repos.

### LLM Input (Phase 1)

The Worker sends structured data, not raw file contents:

```
Given this repository:
  Name:        {owner}/{repo}
  Language:    {primary_language}
  Description: {description}
  File tree (top paths by depth/significance): {tree}
  Key config:  {package.json or go.mod or Cargo.toml contents}

Identify architectural layers (e.g. frontend, backend, database, infra, shared).
For each layer, list which directories belong and what it does.
Return strict JSON matching the ReactFlow schema.
Do NOT invent dependencies — only cluster what the file tree shows.
```

LLM groups files into layers based on naming conventions, extensions, and directory structure. It does not guess import relationships at Phase 1 — that is Phase 2's job.

### Phase 1 Output Schema

```json
{
  "summary": "Brief plain-English summary of what the repo is.",
  "layers": [
    {
      "id": "client",
      "label": "Client Layer",
      "type": "frontend",
      "dirs": ["packages/next/src/client/"],
      "techStack": "React 19, TypeScript"
    },
    {
      "id": "server",
      "label": "Server Layer",
      "type": "service",
      "dirs": ["packages/next/src/server/"],
      "techStack": "Node.js, Edge Runtime"
    }
  ],
  "edges": [
    { "source": "client", "target": "shared", "label": "imports" },
    { "source": "server", "target": "shared", "label": "imports" }
  ],
  "entryPoints": ["packages/next/src/server/next.ts"]
}
```

---

## Phase 2 — Real Dependency Graph (Background)

### Goal
Produce a real import-level dependency graph from actual file contents. Every edge is grounded in a parsed `import`, `require`, `use`, or `from` statement — not inferred by an LLM.

### Clone Strategy

```bash
git clone --depth=1 --single-branch \
  https://<token>@github.com/{owner}/{repo} \
  %APPDATA%\nexus\repos\{owner}-{repo}\
```

`--depth=1` skips all git history. Performance by repo size:

| Repo size | Full clone | Shallow clone |
|---|---|---|
| Small (~1K files) | 8–15s | 1–3s |
| Medium (~10K files) | 30–90s | 5–15s |
| Large (~50K files) | 5+ min | 20–45s |

The clone starts **immediately when the URL is detected**, concurrent with Phase 1 — not after. By the time Phase 1 diagram is shown to the user, the clone may already be 50% done.

**Caching:** On subsequent analysis of the same repo, check `pushed_at` from Phase 1 API against cached timestamp. If unchanged, skip clone and use cached graph. Sub-second response.

### Import Extraction (Rust, `rayon` parallel)

| Language | Extensions | Pattern |
|---|---|---|
| TypeScript / JavaScript | `.ts`, `.tsx`, `.js`, `.jsx` | `import X from './path'`, `require('./path')` |
| Python | `.py` | `import X`, `from X import Y` |
| Rust | `.rs` | `use crate::module`, `mod submodule` |
| Go | `.go` | `import "pkg/path"` |
| Java / Kotlin | `.java`, `.kt` | `import com.package.Class` |
| PHP | `.php` | `use Namespace\Class`, `require_once` |

**Skipped:** `*.test.*`, `*.spec.*`, `*.d.ts`, `node_modules/`, `dist/`, `build/`, `vendor/`, `__pycache__/`, `.git/`

### Smart Sampling for Large Repos

For repos with >500 source files, prioritize:

1. Entry points (`main.*`, `index.*`, `app.*`, `server.*`)
2. Files reachable within 3 BFS hops from entry points
3. High-signal files: config, utils, shared, services
4. Skip: test files, generated files, migration files

This covers 80–90% of meaningful architectural relationships in any real-world repo.

### Graph Algorithms (`petgraph`)

| Algorithm | Output |
|---|---|
| Adjacency list | `file → [files it imports]` |
| Inverted graph | `file → [files that import IT]` |
| DFS cycle detection | Circular dependency chains |
| In-degree | Hub nodes (high = high coupling risk) |
| Out-degree | Leaf nodes (high = low reuse) |
| Topological sort | Layer ordering |
| BFS from entry points | Dead code detection (unreachable files) |

### Phase 2 Output Schema

```json
{
  "graph": {
    "src/api/client.ts": {
      "imports": ["src/utils/http.ts", "src/config/env.ts"],
      "imported_by": ["src/pages/Dashboard.tsx", "src/pages/Home.tsx"]
    }
  },
  "circular_deps": [
    {
      "chain": ["src/services/auth.ts", "src/store/user.ts", "src/services/auth.ts"],
      "risk": "Cannot be independently tested or tree-shaken"
    }
  ],
  "hotspots": [
    { "file": "src/api/client.ts", "in_degree": 34, "risk": "critical" },
    { "file": "src/store/index.ts", "in_degree": 28, "risk": "high" }
  ],
  "isolated": ["src/utils/legacy.ts"],
  "entry_points": ["src/main.tsx"],
  "total_files": 847,
  "files_analyzed": 312
}
```

### Diagram Enrichment

The ReactFlow diagram receives a Tauri `architect:graph-ready` event. The diagram upgrades in-place (no reload):

- Real import edges appear inside the layer clusters
- Hotspot nodes change colour: orange (high), red (critical)
- Circular dep nodes get a warning ring
- Isolated/dead files get a greyed-out indicator
- Edge labels: `imports`, `re-exports`, `type-only`

---

## Phase 3 — Agentic Chat (Impact Analysis)

### Goal
Let the developer ask consequence questions in natural language — by voice or text — and get explainable answers grounded in the real graph. This is the **consequence engine** the problem statement asks for.

### Trigger Methods

| Method | How |
|---|---|
| Click a node | Node becomes active context → NEXUS: "What do you want to know?" |
| Voice: "What breaks if I change X?" | Intent extracted, file matched via fuzzy graph search |
| Typed chat input | Cursor-style sidebar below the diagram |
| Hover + hotkey | Quick impact shortcut for the hovered node |

### Impact Analysis Algorithm

When user asks about a file/module:

1. **Rust reverse BFS** on the inverted graph from the target node
2. Returns: direct dependents (depth 1), transitive dependents (depth 2+), test files affected
3. **Path reconstruction:** shortest path from each affected file back to the origin node
4. **LLM narrates:** given the paths, explains in plain English why each one matters

```
Example query: "What breaks if I change src/api/client.ts?"

Rust result:
  Depth 1 (direct):   Dashboard.tsx, Home.tsx, Profile.tsx (34 total)
  Depth 2 (indirect): App.tsx (via Dashboard.tsx), Router.tsx (via Home.tsx)
  Tests affected:     Dashboard.test.tsx, api.test.ts

LLM prompt:
  "Given these BFS paths from api/client.ts to its dependents,
   explain in plain English what the developer should be careful about.
   Focus on production risk, not file count. Be specific about paths."

NEXUS speaks:
  "Changing api/client.ts affects 34 files directly.
   The highest risk path: api/client.ts → Dashboard.tsx → App.tsx,
   because App.tsx is the root — any error crashes the whole app.
   Auth also depends on it through authService.ts, which could
   lock out all users. 2 test files exist but only test the happy path."
```

**Impact queries run in sub-10ms after Phase 2 is cached.** No API call needed — pure local graph lookup.

### Blast Radius Visualisation

When a query runs:
- Affected nodes pulse with a highlight colour
- The dependency PATH is animated as a flowing edge in ReactFlow
- A badge on the selected node shows affected count
- The detail panel lists all paths with depth labels

### Context-Aware Follow-ups

The agent maintains the selected node as context between questions:

```
User:  "What depends on the auth service?"
NEXUS: [answers with blast radius]

User:  "Are any of those in the critical path?"
NEXUS: [filters to critical path only — no re-specification needed]

User:  "Is there a circular dependency in that area?"
NEXUS: [checks graph, finds auth → session → auth cycle]

User:  "What's the safest file to refactor first?"
NEXUS: [finds highest-impact isolated module — most value, fewest reverse deps]
```

### Chat Sidebar (Cursor-Style Text Input)

Below the ReactFlow canvas, a persistent text chat:

- Markdown-rendered responses
- Code blocks with syntax highlighting
- `@filename` references hyperlink to graph nodes
- Conversation history preserved per-repo session
- Exportable as markdown

---

## Cloudflare Worker Changes

The existing `server/worker/src/index.ts` handles GitHub OAuth, intent classification, PR analysis, Gmail, and Calendar. Add one new intent branch.

### New Intent Detection

Added to `keywordFallback()`:

```typescript
// Detect: "analyze this repo", "map the codebase", "understand the architecture"
if (/\b(analy[sz]e|map|understand|explore|scan)\b/.test(t) &&
    /\b(repo|repository|codebase|project|code|architecture)\b/.test(t)) {
  return "analyze_repo";
}
```

### New Handler: `handleAnalyzeRepo()`

Three sub-modes dispatched by `phase` field in the request:

| Phase | Input | Output |
|---|---|---|
| `1` | `owner`, `repo` (GitHub API token from D1) | ReactFlow JSON (layer-level) |
| `2` | `owner`, `repo`, `graph` (Rust-generated JSON) | Enriched ReactFlow JSON |
| `3` | `question`, `graph`, `selected_file` | Explanation + affected paths |

Uses existing Worker infrastructure:
- `getValidGithubToken()` — GitHub token from D1
- `SUMMARY_MODEL` — Mistral 24B (Phase 1 + 3)
- `ANALYSIS_MODEL` — GLM-4.7-Flash (Phase 2, better reasoning)
- `extractText()` helper

---

## New Tauri Window: `architect`

> **Superseded (2026-10-02):** the standalone `architect` window / `architect.html` page were removed — Architect now renders as a view inside the unified `sidebar` window (see `docs/changes/66-implementation-patterns-quick-reference.md` Pattern 2). The table below is the historical record.

Added to `tauri.conf.json` and `vite.config.ts`:

| Property | Value |
|---|---|
| Label | `architect` |
| HTML | `architect.html` |
| Size | 1400 × 900 (resizable) |
| Decorations | `true` |
| Skip taskbar | `false` |
| Always on top | `false` |

**Layout:**
- Top bar: repo name, phase status badge, refresh button
- Main canvas: ReactFlow (full width/height)
- Right panel: node detail + chat sidebar
- Bottom bar: phase progress ("Fetching tree…", "Cloning…", "Analyzing imports…", "Ready")

### ReactFlow Component (ported from Zync)

Source: `Zync/src/components/zlam/ArchitectureMap.tsx`

Changes:
- Firebase calls removed → `listen()` on `architect:update` Tauri event
- Fetch calls removed → `invoke()` Tauri commands
- New node types added: `module`, `entrypoint`, `hotspot`, `circular`, `isolated`
- Blast radius animation: edges along impact path pulse on Phase 3 query
- Node click → emits `architect:node-selected` → triggers Phase 3

---

## New Rust Commands

### `get_active_repo_url`

```rust
/// Reads OS foreground window title → extracts GitHub owner/repo.
/// Returns None if the active window is not a GitHub repo page.
#[tauri::command]
pub fn get_active_repo_url() -> Option<(String, String)> // (owner, repo)
```

Platform implementations:
- **Windows:** `GetForegroundWindow()` + `GetWindowTextW()` → regex parse
- **macOS:** `NSWorkspace.shared.frontmostApplication` → window title parse
- **Linux:** `xdotool getactivewindow getwindowname`

### `analyze_repo_deep`

```rust
/// Background thread: clone → walk → extract imports → build graph → emit event.
#[tauri::command]
pub async fn analyze_repo_deep<R: Runtime>(
    app: AppHandle<R>,
    owner: String,
    repo: String,
    github_token: Option<String>,
) -> Result<(), String>
// Emits: "architect:graph-ready" with the full graph JSON
// Emits: "architect:progress" with status strings during processing
```

### `query_impact`

```rust
/// Reverse BFS on cached graph. Sub-10ms. No API call.
#[tauri::command]
pub fn query_impact(
    file_path: String,
    max_depth: Option<usize>,  // default: 5
) -> Result<ImpactResult, String>

pub struct ImpactResult {
    pub affected: Vec<String>,             // all affected files
    pub paths: Vec<Vec<String>>,           // path from origin to each affected file
    pub depth: usize,                      // max depth reached
    pub direct_count: usize,
    pub transitive_count: usize,
    pub test_files: Vec<String>,
}
```

---

## New Rust Crates Required

```toml
# src-tauri/Cargo.toml additions
# NOTE (2026-10-02): `walkdir` was removed — dead dep, file walking uses
# `ignore::WalkBuilder` (respects .gitignore during walk).
ignore   = "0.4"   # respects .gitignore during walk
rayon    = "1.10"  # parallel per-file processing
petgraph = "0.6"   # DFS, BFS, cycle detection, centrality
```

`rayon` may already be present as a transitive dependency. `petgraph` is the only genuinely new heavy dependency.

---

## Demo Script

```
[Judge observes]

1. NEXUS is running. User opens Chrome, navigates to
   github.com/vercel/next.js

2. User says: "NEXUS, analyze this repo"

3. t=0.5s  NEXUS: "Got it. Analyzing vercel/next.js…"
   t=8s    ReactFlow window opens — 4 layered nodes, high-level edges.
           NEXUS: "Next.js has 4 layers — client, server, build, and shared.
                   Loading the real dependency graph in the background."

4. t=45s   Diagram enriches live — file nodes appear, one turns red.
           NEXUS: "Deep scan complete. The config module is critical —
                   47 files depend on next.config.js. 2 circular dependencies found."

5. User clicks the red config node.
   NEXUS: "next.config.js feeds the webpack build layer,
            which bundles all 12 page routes.
            Changing it will affect every page in production."
   [Blast radius animates outward from the node in the graph]

6. User (typed): "Are there any circular dependencies?"
   NEXUS: "Yes, two. The most dangerous:
            server/config.ts → lib/load-custom-routes.ts → server/config.ts.
            These modules cannot be independently tested or tree-shaken."
   [Both nodes pulse red, cycle edge highlighted]

7. User: "What's the safest starting point for an onboarding engineer?"
   NEXUS: "The shared utilities layer — lowest coupling, no circular deps,
            31% test coverage. 8 files. Good place to read first."
```

---

## Criteria Coverage Matrix

| Evaluation Criterion | What NEXUS Delivers | Phase |
|---|---|---|
| **Understanding** | File structure + real import relationships + architectural layers | 1 + 2 |
| **Relationships** | Actual import graph from parsed source, not LLM inference | 2 |
| **Architecture** | Layered ReactFlow enriched with real dependency edges | 1 → 2 |
| **Impact** | Reverse BFS blast radius with path reconstruction | 3 |
| **Explainability** | LLM narrates path A→B→C and why each step matters | 3 |
| **Actionability** | Click any node → instant impact query; voice or text | 3 |
| **Technical** | Rust graph engine + Cloudflare Worker + ReactFlow | All |
| **AI value** | LLM explains graph structure and consequences — not guessing deps | 3 |
| **Efficiency** | Impact queries: <10ms offline. Phase 1: <10s. | All |
| **Scale** | Smart sampling, graph cached to disk, incremental re-analysis | 2 |
| **Innovation** | Voice-triggered from active browser tab; live diagram enrichment | All |

---

## Hard Constraints Check

| Constraint (verbatim) | How we satisfy it |
|---|---|
| "A folder tree is not architecture" | Phase 2 builds a real import graph. Every edge is a parsed statement — not a directory listing. |
| "LLMs must add value beyond summarizing source" | LLM receives pre-computed graph JSON. It explains and narrates — the graph algorithm discovers, the LLM communicates. |
| "Impact analysis must be explainable — show dependency paths" | We show the PATH (A→B→C→root), animated in the graph and spoken by NEXUS. |
| "Consider scale" | Smart file sampling for large repos, petgraph-based O(V+E) algorithms, disk-cached graph. |
| "Static analysis is not the only approach" | LLM semantic layer adds understanding of intent and risk beyond what static imports show. |

---

## Honest Scope Limitations

- **Function/method level:** File-level graph only. Function-level requires tree-sitter AST — V2.
- **Cross-repo microservices:** Single repo per session. Multi-repo requires API mesh data — V2.
- **Runtime dependencies:** HTTP calls between services are not captured by static import analysis.
- **Database schema relationships:** ORM models appear as regular files; schema-level analysis not implemented.
- **Repos >500 source files:** Smart sampling covers the architectural core. Fringe files may be missed.

---

## Cross-References

- [AGENTS.md](../../AGENTS.md) — Build system, Cloudflare Worker architecture, local ports
- [07-app-registry.md](./07-app-registry.md) — Background thread pattern (reused for graph analysis)
- [13-response-sidebar.md](./13-response-sidebar.md) — Sidebar window system (pattern reused for chat sidebar)
- `server/worker/src/index.ts` — Worker where `handleAnalyzeRepo()` is added
- `src-tauri/src/commands.rs` — Where new Tauri commands are registered
- `src-tauri/src/network.rs` — Existing HTTP bridge reused for Phase 1 and Phase 3
