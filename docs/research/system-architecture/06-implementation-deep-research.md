# Deep Implementation Research — Enterprise Patterns for NEXUS/ULTRON

> Research date: 2026-09-03
> Purpose: Concrete implementation paths for adopting enterprise patterns in
> NEXUS/ULTRON's Tauri 2 + Rust + React + TypeScript stack.
> Companion to: `05-enterprise-patterns-research.md`

---

## 1. Tree-Sitter Integration in Rust (for NEXUS)

### Current state

NEXUS's Phase 2 (`architect.rs:1862-1939`) uses **line-by-line regex/string matching**
to extract imports:
- TypeScript: `import ... from "path"` and `require("path")`
- Python: `import X` and `from X import Y`
- Rust: `use crate::...` and `mod ...;`
- Go: `import "path"`

This misses:
- Multi-line imports
- Re-exports (`export ... from`)
- Dynamic imports inside expressions
- Type-only imports (`import type`)
- Namespace imports (`import * as`)
- Conditional imports
- Call edges (function A calls function B)
- Class/method relationships
- Type definitions

### What tree-sitter gives us

Tree-sitter parses source code into an **AST** (Abstract Syntax Tree) — the same
library used by Neovim, GitHub, and Helix for syntax highlighting.

**For each file, we can extract:**
- Functions (name, params, return type, line range, visibility)
- Classes/structs/enums (name, fields, methods, line range)
- Imports (exact module paths, not guessed)
- Call sites (which function calls which)
- Type definitions
- Docstrings/comments attached to their definitions

### Cargo.toml dependencies needed

```toml
[dependencies]
tree-sitter = "0.25"
tree-sitter-typescript = "0.23"   # covers .ts and .tsx
tree-sitter-javascript = "0.25"
tree-sitter-python = "0.25"
tree-sitter-rust = "0.24"
tree-sitter-go = "0.25"
tree-sitter-java = "0.23"
tree-sitter-c = "0.24"
tree-sitter-cpp = "0.23"
# Or use the all-in-one pack:
# tree-sitter-language-pack = "1.16"  # 371 languages, on-demand download
```

**Trade-off**: Individual crates = larger binary but no network dependency.
Language pack = smaller binary but downloads grammars on first use.

**Recommendation for NEXUS**: Use individual crates for the 8 most common languages
(TS, JS, Python, Rust, Go, Java, C, C++). This covers ~95% of GitHub repos.
Binary size impact: ~2-3 MB total (tree-sitter grammars are small C libraries).

### Implementation pattern (from production codebases)

Based on `codedash`, `tracedecay`, `revet-core`, and `codegraph-rs`:

```rust
use tree_sitter::{Parser, Node};

pub struct CodeSymbol {
    pub name: String,
    pub kind: SymbolKind,    // Function, Class, Method, Type, etc.
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub params: Vec<String>,
    pub return_type: Option<String>,
    pub visibility: Visibility,
    pub docstring: Option<String>,
}

pub enum SymbolKind {
    Function, Method, Class, Struct, Enum, Trait,
    Interface, TypeAlias, Const, Module,
}

pub fn parse_file(file_path: &str, content: &str) -> Vec<CodeSymbol> {
    let mut parser = Parser::new();
    let language = match extension(file_path) {
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "js" | "jsx" => tree_sitter_javascript::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "rs" => tree_sitter_rust::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return vec![],
    };
    parser.set_language(&language.into()).ok()?;

    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut symbols = vec![];
    visit_node(root, content, file_path, &mut symbols);
    symbols
}

fn visit_node(node: Node, source: &str, file: &str, symbols: &mut Vec<CodeSymbol>) {
    match node.kind() {
        "function_item" | "function_declaration" | "method_definition" => {
            if let Some(sym) = extract_function(node, source, file) {
                symbols.push(sym);
            }
        }
        "struct_item" | "class_declaration" | "class_definition" => {
            if let Some(sym) = extract_class(node, source, file) {
                symbols.push(sym);
            }
        }
        // ... enum, trait, interface, type_alias, etc.
        _ => {}
    }
    // Recurse into children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            visit_node(child, source, file, symbols);
        }
    }
}
```

### Performance benchmarks (from production tools)

| Tool | Fixture | Cold parse | Incremental | Peak RSS |
|------|---------|-----------|-------------|----------|
| GitGrapher | 50K TS functions / 500 files | 4.54s | 0.48s (1 file) | 395 MB |
| codegraph-rs | 139 TS files | 190ms | N/A | ~5 MB binary |
| tree-sitter (raw) | Single 500-line file | <1ms | <1ms | N/A |

**For NEXUS**: A typical GitHub repo (300 files) would parse in ~200-500ms with
Rayon parallelism (already a dependency). This is **faster** than the current
line-by-line approach because tree-sitter is C-native and error-tolerant.

### What this replaces in NEXUS

| Current (architect.rs) | With tree-sitter |
|------------------------|------------------|
| `extract_imports_from_source` — line-by-line regex | AST `import_statement` nodes — exact, complete |
| `cluster_files_into_layers` — path heuristics | AST symbol extraction → graph clustering |
| No call edges | `call_expression` nodes → call graph edges |
| No symbol-level analysis | Per-function/class nodes in the graph |
| File-level only | Symbol-level + file-level graph |

---

## 2. Persistent Code Graph with SQLite

### Current state

NEXUS stores the Phase 2 graph in memory (`CACHED_GRAPH` at `architect.rs:153`):
```rust
static CACHED_GRAPH: once_cell::sync::Lazy<
    parking_lot::Mutex<Option<Arc<CachedGraphState>>>
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(None));
```

This means:
- **Rebuilt from scratch every analysis** — even if the repo hasn't changed
- **Lost when the app restarts** — no persistence
- **No incremental updates** — full rebuild every time
- **One repo at a time** — previous repo's graph is overwritten

### SQLite schema (from production codebases)

Based on `codegraph-rs`, `deagle-core`, `rinne-graph`, and `mimir-graph`:

```sql
-- File metadata (for incremental indexing)
CREATE TABLE files (
    id          INTEGER PRIMARY KEY,
    repo_hash   TEXT NOT NULL,          -- hash of owner/repo
    rel_path    TEXT NOT NULL,
    language    TEXT,
    content_hash TEXT,                   -- SHA-256 of file content
    mtime       INTEGER,
    line_count  INTEGER,
    UNIQUE(repo_hash, rel_path)
);

-- Symbols (functions, classes, etc.)
CREATE TABLE symbols (
    id          INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL,           -- function, class, method, etc.
    qualified_name TEXT,                 -- e.g., "MyClass.myMethod"
    start_line  INTEGER,
    end_line    INTEGER,
    params      TEXT,                    -- JSON array
    return_type TEXT,
    visibility  TEXT,
    docstring   TEXT
);

-- Edges (calls, imports, contains, implements, extends)
CREATE TABLE edges (
    source_id   INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    target_id   INTEGER REFERENCES symbols(id),  -- nullable for external
    edge_kind   TEXT NOT NULL,           -- calls, imports, contains, etc.
    PRIMARY KEY(source_id, target_id, edge_kind)
);

-- FTS5 for full-text symbol search (BM25)
CREATE VIRTUAL TABLE symbols_fts USING fts5(
    name, qualified_name, docstring,
    content='symbols', content_rowid='id'
);
```

### Cargo.toml dependencies needed

```toml
[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }  # SQLite compiled in
sha2 = "0.10"    # SHA-256 for content hashing
```

`rusqlite` with `bundled` feature compiles SQLite statically — no system SQLite
needed. Adds ~1.5 MB to binary.

### Incremental indexing algorithm (from production codebases)

Based on `gollum`, `codemem-engine`, `axil-indexer`, and `codecache-rs`:

```
1. Discover all source files (ignore::WalkBuilder — already used)
2. For each file:
   a. Compute SHA-256 hash of content
   b. Compare with stored hash in `files` table
   c. If hash matches → SKIP (file unchanged)
   d. If hash differs or file is new:
      - Parse with tree-sitter
      - Delete old symbols/edges for this file
      - Insert new symbols/edges
      - Update `files.content_hash`
3. Detect deleted files (in DB but not on disk)
   - Delete their symbols/edges
   - Remove from `files` table
4. Re-resolve cross-file edges (only for changed files)
5. Commit in one transaction
```

**Key insight from production**: Content hash is the primary change signal, NOT
mtime. Touching a file without altering content leaves it classified as
`Unchanged`. This prevents unnecessary re-parsing.

### Performance with incremental indexing

| Scenario | Current (full rebuild) | With incremental |
|----------|----------------------|------------------|
| First analysis (300 files) | ~3-5s | ~3-5s (same, cold) |
| Re-analyze unchanged repo | ~3-5s (full rebuild) | <100ms (all skipped) |
| 1 file changed | ~3-5s (full rebuild) | ~50ms (1 file re-parse) |
| 10 files changed | ~3-5s (full rebuild) | ~200ms (10 files re-parse) |

### Storage location

```
%APPDATA%/com.nexus.assistant/codegraph/<owner>__<repo>.db
```

Per-repo SQLite databases. Each is independent, no cross-contamination.
A 300-file repo produces ~500KB-2MB of graph data.

---

## 3. Tauri Channel API for Progressive Results

### Current state

NEXUS uses Tauri **events** (`app.emit()`) for all Rust→frontend communication:
- `architect:progress` — stage updates ("scanning", "graph", etc.)
- `architect:phase1-ready` — complete Phase 1 data
- `sidebar:backdrop` — backdrop image
- `architect:set-repo` — repo identity

Events are fire-and-forget, JSON-serialized, broadcast to all listeners.
The frontend `invoke()`s a command and waits for the single return value.

### Tauri's three IPC patterns

| Pattern | Direction | Use case | NEXUS uses? |
|---------|-----------|----------|-------------|
| **Command** (`invoke`) | Frontend → Rust, request-response | One-shot calls | Yes (all commands) |
| **Event** (`emit`/`listen`) | Rust → Frontend, broadcast | Lifecycle, small payloads | Yes (progress, backdrop) |
| **Channel** (`Channel<T>`) | Rust → Frontend, streaming | Progress, high-throughput | **No** |

### Why Channel is better for progressive results

From Tauri docs and production usage:

1. **Ordered delivery** — Channel uses an index system to guarantee message order
2. **Scoped to invocation** — only the caller receives messages (not broadcast)
3. **Higher throughput** — optimized for streaming, not fire-and-forget
4. **Type-safe** — generic over message type, both Rust and TS agree on shape

### Implementation pattern for progressive architecture display

**Rust side:**
```rust
use tauri::ipc::Channel;
use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(tag = "stage", content = "data")]
pub enum AnalysisProgress {
    DetectingRepo { window_title: String },
    Phase1Layer { index: usize, total: usize, layer: LayerData },
    Phase1Complete { layers: Vec<LayerData>, summary: String },
    Enriching { message: String },
    Enriched { layer_updates: Vec<LayerUpdate> },
    Phase2Scanning { files_scanned: usize, total_files: usize },
    Phase2Hotspot { hotspot: HotspotData },
    Phase2Cycle { cycle: CycleData },
    Phase2Complete { hotspots: Vec<HotspotData>, cycles: Vec<CycleData> },
    Done,
    Error { message: String },
}

#[tauri::command]
pub async fn open_architect_progressive<R: Runtime>(
    app: AppHandle<R>,
    on_progress: Channel<AnalysisProgress>,
) -> Result<i32, String> {
    // Step 1: Detect repo
    on_progress.send(AnalysisProgress::DetectingRepo { ... }).ok();

    // Step 2: Phase 1 — emit each layer as it's clustered
    for (i, layer) in layers.iter().enumerate() {
        on_progress.send(AnalysisProgress::Phase1Layer {
            index: i,
            total: layers.len(),
            layer: layer.clone(),
        }).ok();
    }
    on_progress.send(AnalysisProgress::Phase1Complete { ... }).ok();

    // Step 3: AI enrichment — emit as each layer is rewritten
    on_progress.send(AnalysisProgress::Enriching { ... }).ok();
    on_progress.send(AnalysisProgress::Enriched { ... }).ok();

    // Step 4: Phase 2 — emit hotspots/cycles as found
    on_progress.send(AnalysisProgress::Phase2Hotspot { ... }).ok();
    on_progress.send(AnalysisProgress::Phase2Cycle { ... }).ok();
    on_progress.send(AnalysisProgress::Phase2Complete { ... }).ok();

    on_progress.send(AnalysisProgress::Done).ok();
    Ok(1)
}
```

**Frontend side:**
```typescript
import { invoke, Channel } from "@tauri-apps/api/core";

const channel = new Channel<AnalysisProgress>();
channel.onmessage = (msg) => {
    switch (msg.stage) {
        case "detecting_repo":
            setStatus("Detecting repository...");
            break;
        case "phase1_layer":
            // Show each layer as it's found — progressive!
            addLayer(msg.data.layer);
            setStatus(`Layer ${msg.data.index + 1} of ${msg.data.total}`);
            break;
        case "phase1_complete":
            setStatus("Phase 1 complete, AI enriching...");
            break;
        case "enriching":
            setStatus(msg.data.message);
            break;
        case "enriched":
            // Update layer labels with AI-rewritten names
            updateLayerLabels(msg.data.layer_updates);
            break;
        case "phase2_hotspot":
            // Show each hotspot as it's found
            addHotspot(msg.data.hotspot);
            break;
        case "phase2_cycle":
            // Show each cycle as it's found
            addCycle(msg.data.cycle);
            break;
        case "done":
            setStatus("Analysis complete");
            setLoading(false);
            break;
        case "error":
            setError(msg.data.message);
            break;
    }
};

await invoke("open_architect_progressive", { onProgress: channel });
```

### What this enables

Instead of the current flow:
```
User says "open architecture mapper"
  → "On it sir" + loading animation (3-5s of nothing)
  → Window opens with everything at once
```

The new flow:
```
User says "open architecture mapper"
  → "On it sir" + loading animation
  → Window opens immediately with skeleton
  → Layer 1 appears ("Frontend") → Layer 2 ("Backend") → ... (progressive)
  → Status: "AI enriching layer 3 of 5..."
  → Layer labels update one by one (enriched names appear)
  → Hotspots appear one by one in the analytics column
  → Cycles appear one by one
  → Status: "Analysis complete"
```

**Perceived latency drops from 3-5s to <500ms** (time to first layer).

---

## 4. Blast Radius Algorithm (BFS over code graph)

### Current state

NEXUS already has a BFS blast radius implementation (`architect.rs:2036-2100`):
```rust
// Sub-10ms reverse BFS impact analysis on the cached petgraph
let mut visited: HashSet<NodeIndex> = HashSet::new();
let mut queue: VecDeque<(NodeIndex, usize, Vec<String>)> = VecDeque::new();
```

This works at the **file level** — nodes are files, edges are import relationships.

### What tree-sitter + SQLite enables

With symbol-level graph, blast radius becomes much more precise:

```
Current (file-level):
  "If I change auth.py, what files import it?"
  → auth.py ← middleware.py ← main.py ← server.py

With symbol-level:
  "If I change validate_user(), what calls it?"
  → validate_user() ← handle_login() ← auth_route() ← server.start()
  → validate_user() ← check_session() ← middleware()
```

### BFS algorithm (from production codebases)

Based on `banso-graph`, `hank`, `unfault`, and `mati-core`:

```rust
pub fn blast_radius(
    graph: &DiGraph<SymbolNode, EdgeKind>,
    seed: &str,  // symbol name or file path
    direction: Dir,  // Callers (impact) or Callees (dependencies)
    max_depth: usize,
) -> BlastRadiusResult {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut reached = Vec::new();

    // Seed: find all nodes matching the seed
    for node in graph.nodes() {
        if node.matches(seed) {
            queue.push_back((node, 0));
            visited.insert(node);
        }
    }

    while let Some((node, depth)) = queue.pop_front() {
        if depth >= max_depth { continue; }

        // Walk edges in the specified direction
        let neighbors = graph.neighbors_directed(node, match direction {
            Dir::Callers => Direction::Incoming,  // who calls me
            Dir::Callees => Direction::Outgoing,  // who I call
        });

        for neighbor in neighbors {
            if !visited.contains(&neighbor) {
                visited.insert(neighbor);
                reached.push(Reached {
                    name: neighbor.name.clone(),
                    file: neighbor.file.clone(),
                    distance: depth + 1,
                    via: direction.via(),
                });
                queue.push_back((neighbor, depth + 1));
            }
        }
    }

    BlastRadiusResult {
        direct: reached.iter().filter(|r| r.distance == 1).count(),
        transitive: reached.iter().filter(|r| r.distance > 1).count(),
        score: direct as f32 + transitive as f32 * 0.3,  // weighted
        tier: classify_risk(direct, transitive),
        reached,
    }
}
```

### Risk classification (from `mati-core`)

```rust
pub enum BlastTier {
    Low,      // 0-2 direct dependents
    Medium,   // 3-8 direct dependents
    High,     // 9-20 direct dependents
    Critical, // 20+ direct dependents
}
```

---

## 5. Semantic Caching Implementation

### Current state

NEXUS's Worker cache (`cache.ts`) uses exact key matching:
```typescript
const key = searchKey("en", query);  // `search:en:${query.toLowerCase()}`
```

"What is X" and "explain X" are different cache keys → cache miss → full LLM call.

### Implementation pattern

```typescript
// On query arrival:
// 1. Embed the query (using a small embedding model)
// 2. Search cache for similar embeddings (cosine similarity > threshold)
// 3. If match → return cached response (no LLM call)
// 4. If no match → call LLM, store query embedding + response

interface CacheEntry {
    queryEmbedding: Float32Array;  // 384-dim (MiniLM) or 768-dim
    query: string;
    response: string;
    timestamp: number;
    model: string;
    tokensUsed: number;
}

// Cloudflare Workers AI has a built-in embedding model:
// @cf/baai/bge-base-en-v1.5 (768-dim, free tier)
// Or use a smaller model: @cf/baai/bge-small-en-v1.5 (384-dim)

const SIMILARITY_THRESHOLD = 0.92;  // strict for code/technical queries

async function semanticCacheLookup(query: string): Promise<string | null> {
    const queryEmbedding = await embedQuery(query);

    // Search KV/D1 for similar embeddings
    // Cloudflare Vectorize (if available) or brute-force in D1
    const candidates = await searchSimilarEmbeddings(queryEmbedding, 5);

    for (const candidate of candidates) {
        const similarity = cosineSimilarity(queryEmbedding, candidate.embedding);
        if (similarity > SIMILARITY_THRESHOLD) {
            // Check TTL
            if (Date.now() - candidate.timestamp > CACHE_TTL) continue;
            // Check model match
            if (candidate.model !== currentModel) continue;
            return candidate.response;
        }
    }
    return null;
}
```

### Cloudflare-specific options

1. **Cloudflare Vectorize** — managed vector database, works with Workers
2. **D1 + brute-force** — store embeddings as JSON, compute similarity in JS
   (works for <10K entries, which covers 5-10 users × daily queries)
3. **KV + embedding hash** — bucket queries by embedding hash prefix, then
   brute-force within each bucket

### Expected impact

Based on production data:
- 30-40% of queries are semantically similar to previous ones
- 60-80% cache hit rate with loose threshold (0.15)
- 30-45% hit rate with strict threshold (0.10)
- Each cache hit = 0 LLM tokens = $0 cost
- Response time: <50ms (cache hit) vs 2-8s (LLM call)

---

## 6. Implementation Priority and Dependency Graph

```
Phase 1: Fix existing code (from optimization analysis)
  ├── Replace base64_decode with base64 crate
  ├── Switch PENDING_ARCHITECT_REPO to parking_lot::Mutex
  ├── Reuse reqwest::Client in STT
  ├── Wrap get_active_repo_url in spawn_blocking
  ├── Switch initial backdrop to JPEG
  ├── Fix tauri.conf.json beforeBuildCommand
  ├── Delete dead ArchitectSidebar.tsx + unused store fields + CSS
  ├── Memoize ArchitectureMap + node components
  ├── Extract handleOpenArchitect helper in recorder.ts
  ├── Fix captureInProgress release
  └── Fix ensure_stt_running race condition

Phase 2: Tauri Channel API for progressive results
  ├── Add Channel<AnalysisProgress> to open_architect_with_auto_detect
  ├── Frontend: skeleton map + progressive layer rendering
  └── Frontend: progressive hotspot/cycle display

Phase 3: Tree-sitter AST parsing
  ├── Add tree-sitter + language crates to Cargo.toml
  ├── Implement parse_file() for TS/JS/Python/Rust/Go
  ├── Replace extract_imports_from_source with AST-based extraction
  └── Add symbol extraction (functions, classes, methods)

Phase 4: Persistent SQLite code graph
  ├── Add rusqlite + sha2 to Cargo.toml
  ├── Create schema (files, symbols, edges, symbols_fts)
  ├── Implement incremental indexing (SHA-256 hash check)
  ├── Store graph per-repo in %APPDATA%/com.nexus.assistant/codegraph/
  └── Replace CACHED_GRAPH with SQLite-backed graph

Phase 5: Symbol-level blast radius
  ├── BFS over symbol graph (not just file graph)
  ├── Risk classification (Low/Medium/High/Critical)
  └── Frontend: show call chain, not just file chain

Phase 6: Semantic caching (Worker)
  ├── Add embedding model to Worker
  ├── Implement semantic cache lookup
  ├── Store embeddings in D1/KV
  └── Monitor hit rate and false positives
```

### Dependency order

```
Phase 1 (fixes) — no dependencies, do first
     ↓
Phase 2 (Channel API) — depends on Phase 1 (clean codebase)
     ↓
Phase 3 (tree-sitter) — independent of Phase 2, can parallel
     ↓
Phase 4 (SQLite graph) — depends on Phase 3 (needs AST data)
     ↓
Phase 5 (symbol blast radius) — depends on Phase 4 (needs graph)
     ↓
Phase 6 (semantic cache) — independent, can do anytime
```

### Estimated binary size impact

| Addition | Binary size increase |
|----------|---------------------|
| tree-sitter + 8 language grammars | ~2-3 MB |
| rusqlite (bundled SQLite) | ~1.5 MB |
| sha2 | ~50 KB |
| **Total** | **~4-5 MB** (current binary is ~15-20 MB) |

### Estimated RAM impact

| Addition | RAM increase |
|----------|-------------|
| tree-sitter parsers (loaded on demand) | ~5-10 MB |
| SQLite connection + cache | ~5-15 MB |
| Per-repo graph database (300 files) | ~500KB-2MB on disk, ~5MB in RAM |
| **Total** | **~15-25 MB** (current idle is ~104-232 MB) |

---

## 7. What NOT to Do (Anti-patterns from research)

1. **Don't use WebSockets for Tauri IPC** — Tauri's Channel API is purpose-built
   for this, works through the WebView2 bridge, and is type-safe.

2. **Don't use Neo4j for a desktop app** — SQLite is sufficient for single-user
   code graphs up to 100K symbols. Neo4j requires a server process and adds
   ~200MB+ RAM overhead.

3. **Don't embed entire files for RAG** — Use AST-aware chunking (one function =
   one chunk with context header). Whole files waste tokens and lose precision.

4. **Don't use mtime for change detection** — Use SHA-256 content hash. Touching
   a file without changing content triggers unnecessary re-parsing with mtime.

5. **Don't build a separate MCP server (yet)** — NEXUS is a desktop app, not an
   agent platform. The code graph is consumed internally by the architecture
   mapper. MCP exposure can come later if users want to connect external agents.

6. **Don't use a loose semantic cache threshold for code queries** — Code is
   exact. "How does auth work" and "How does login work" may have different
   correct answers in the same codebase. Use 0.92+ similarity threshold.

7. **Don't replace the current event system entirely** — Events are still right
   for broadcast notifications (backdrop, loading state). Channels are right for
   per-invocation streaming. Use both.
