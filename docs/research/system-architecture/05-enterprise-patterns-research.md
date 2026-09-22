# Enterprise AI Patterns — Research for NEXUS/ULTRON

> Research date: 2026-09-03
> Purpose: Map enterprise-grade AI infrastructure patterns to NEXUS/ULTRON's architecture
> to identify what we already do well, what we're missing, and what to adopt next.

---

## 1. RAG (Retrieval-Augmented Generation)

### What enterprises do

Enterprise RAG is **not** "embed documents → vector search → stuff into prompt."
That's the tutorial version. Production RAG is a **6-stage pipeline**:

```
Query Understanding → Hybrid Retrieval → Reranking → Context Filtering → Generation → Evaluation
```

#### The 7 production RAG patterns (converged industry standard)

| Pattern | When to use | Latency | Complexity |
|---------|-------------|---------|------------|
| **Naive RAG** | Demos only | Low | Low |
| **RAG with reranking** | General baseline | +50-150ms | Medium |
| **Hierarchical RAG** | Multi-level corpora | Medium | Medium |
| **GraphRAG** | Cross-document reasoning | High | High |
| **Agentic RAG** | Multi-step queries | Variable | High |
| **Adaptive RAG** | Mixed query types | Variable | Medium |
| **Self-RAG** | Quality-critical | High | High |

#### Hybrid retrieval (the production baseline)

No single retrieval method works alone:
- **Dense vector search** — matches meaning, paraphrases, synonyms. Misses exact terms.
- **BM25 sparse search** — matches exact terms, IDs, acronyms. Misses paraphrases.
- **Solution**: Run both in parallel, fuse with **Reciprocal Rank Fusion (RRF)**:
  ```
  score(d) = Σ 1 / (k + rank_i(d))   where k=60
  ```
  RRF is scale-invariant (operates on ranks, not scores), needs zero calibration.

#### Reranking (two-stage)

1. **Retrieve** 50-100 candidates cheaply (BM25 + vector)
2. **Rerank** to top-5 with a cross-encoder (e.g., `ms-marco-MiniLM-L-6-v2`)
3. Adds ~150ms but improves precision by 15-25%
4. Cuts retrieval failures by **67%** vs dense-only

#### Latency budget (enterprise target: 1.5s total)

```
Retrieval (parallel BM25 + ANN + ACL filter):  250ms
Rerank:                                        150ms
Context assembly:                              100ms
LLM TTFT (time-to-first-token):                800ms
Citation post-process:                         100ms
Network:                                       100ms
                                               ──────
Total:                                       1,500ms
```

### What NEXUS/ULTRON does today

- **Worker cascade** (`server/worker/src/`) — routes queries through Gemini → Groq fallback chain
- **Wikipedia/Wikidata research** (`research.ts`) — ad-free search with citations
- **KV/D1 caching** (`cache.ts`) — edge cache for search results
- **Per-user quotas** (`quota.ts`) — daily limits on requests, neurons, deep analyses

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Hybrid search (BM25 + vector)** — currently LLM-only, no retrieval | High | Medium |
| **Reranking** — no cross-encoder precision step | Medium | Low |
| **Query rewriting** — vague queries not reformulated before retrieval | Medium | Low |
| **RAGAS evaluation** — no automated retrieval quality measurement | Medium | Medium |
| **ACL-aware retrieval** — no per-user document filtering at query time | Low (single-user focus) | High |

---

## 2. Chunking Strategies

### What enterprises do

Chunking is the **highest-leverage index-time decision**. Wrong chunk size degrades
retrieval more than any other factor.

#### The 4 strategies (matched to document type)

| Strategy | Best for | How it works |
|----------|----------|--------------|
| **Fixed-size** | Homogeneous prose | Split at N tokens with overlap. Simple, breaks structure. |
| **Sentence-boundary** | Articles, docs | Split at sentence boundaries. Preserves semantic completeness. |
| **Semantic** | Mixed corpora | Split where embedding similarity drops. Variable size. |
| **Parent-child** | General baseline | Small chunks for retrieval precision, return parent for context depth. |

#### Parent-child (the strongest general baseline)

```
INDEX TIME:
  Parent 1 (512 tokens)
    ├── Child 1 (128 tokens)
    ├── Child 2 (128 tokens)  ← matched by query
    └── Child 3 (128 tokens)
  Parent 2 (512 tokens)
    ├── Child 4 (128 tokens)
    ├── Child 5 (128 tokens)
    └── Child 6 (128 tokens)

QUERY TIME:
  User query → Child 2 matches → return Parent 1 (contains Child 2)
  → LLM gets precision at retrieval, depth at generation
```

#### Code-specific chunking (critical for NEXUS)

**Generic text chunking fails catastrophically on source code:**
- Functions cut in half → signature in one chunk, body in another
- Imports orphaned at top → high similarity to every file, zero information
- Docstrings separated from implementations → the natural-language part
  that embeddings can actually use is disconnected from the code
- Boundaries move on every edit → every chunk ID changes, whole file re-embedded

**AST-aware chunking (the production approach):**
1. Parse source with **tree-sitter** (40+ languages, error-tolerant, IDE-grade)
2. Extract complete syntactic units: functions, classes, methods, types
3. Attach decorators, docstrings, leading comments to their definitions
4. Enrich with context header: file path, enclosing scope chain, relevant imports
5. Each chunk = one definition + its context header

**Tools doing this in production:**
- **CodeRadius** — blast radius + MCP server for AI agents
- **Axon Pro** — Neo4j + tree-sitter, Laravel/PHP/Python/JS
- **Adapts** — continuous codebase indexing, COBOL to Rust
- **Bito AI Architect** — knowledge graph from repos + Jira
- **GitGrapher** — Rust + tree-sitter + Rayon, incremental indexing
- **glyphtrail** — local-first, MCP, cross-repo blast radius
- **Intuit Infigraph** — 62 languages, Cypher queries, BM25 + vector hybrid

### What NEXUS/ULTRON does today

- **Phase 1**: `cluster_files_into_layers` — heuristic file-path clustering into architectural layers
- **Phase 2**: `analyze_repo_deep` — dependency graph, hotspots, circular deps via `ignore::WalkBuilder`
- **File reading**: GitHub API `get_content` → base64 decode → send to Worker LLM
- **No AST parsing** — purely path-based heuristics, no tree-sitter
- **No chunking** — entire key files sent to LLM (truncated at 50KB)

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **AST-aware chunking** (tree-sitter) — functions/classes as chunks, not whole files | High | Medium |
| **Context enrichment** — file path + scope chain + imports per chunk | High | Low |
| **Incremental indexing** — only re-parse changed files (sha-diff) | High | Medium |
| **Persistent graph** — store the code graph, don't rebuild every analysis | Medium | Medium |
| **Embedding-based retrieval** — vector search over code chunks | Medium | High |
| **Parent-child chunking** — retrieve function, return enclosing class/module | Low | Medium |

---

## 3. Streaming / Bits-by-Bits Delivery

### What enterprises do

#### Transport: SSE is the standard

All major providers (OpenAI, Anthropic, Google) chose **Server-Sent Events over HTTP**,
not WebSockets, for token streaming:

| Property | SSE | WebSocket |
|----------|-----|-----------|
| Direction | Server → client (one-way) | Bidirectional |
| Transport | Plain HTTP | HTTP upgrade |
| Proxy/CDN | Works everywhere | Often breaks |
| Scaling | Stateless, horizontal | Sticky sessions needed |
| Reconnection | Built-in (`Last-Event-ID`) | Manual |
| Cancellation | Close socket | Close frame |

**Why SSE wins for LLM streaming:**
- LLM output is inherently one-way (server → client)
- SSE passes through every proxy, CDN, corporate firewall
- No sticky sessions → standard load balancers work
- Browser `EventSource` handles reconnection automatically

**Critical implementation detail**: Browser `EventSource` only supports GET with no
headers. LLM APIs use POST + auth headers. Production clients use `fetch()` +
`ReadableStream` and manually parse SSE frames.

#### The streaming relay pattern

```
Browser → Your Backend (relay) → LLM Provider
         ↑ holds the API key
         ↑ enforces rate limits
         ↑ meters cost
         ↑ handles abort (closed tab = cancel upstream)
         ↑ forwards bytes as they arrive (no buffering)
```

**Never let the browser talk to the provider directly.** The key ships public within
hours, and every call bypasses your rate limits, logging, and spend caps.

#### Backpressure handling

- **SSE/fetch**: Web Streams API propagates backpressure automatically via `ReadableStream`
- **WebSocket**: Implement token bucket with max buffer size before pausing reads
- If tokens arrive faster than UI can paint: **batch to one paint per animation frame**

#### Partial structured output

- Do NOT `JSON.parse` mid-stream
- Accumulate buffer, attempt tolerant parse for preview, validate against schema on stream close
- Use partial JSON detection for structured output streaming

#### Error recovery mid-stream

- Send an error SSE event with the partial completion + retry token
- Client resumes from last received chunk
- A stream that just stops is indistinguishable from a stall — always say why

### What NEXUS/ULTRON does today

- **Tauri IPC** — command-based, not streaming. Frontend `invoke()` → wait → response
- **No SSE/WebSocket** between frontend and Rust backend
- **Worker HTTP** — request/response, not streamed
- **Loading indicator** — corner animation while waiting (good UX, but not streaming)
- **TTS** — Kokoro speaks "On it sir" immediately (good acknowledgement pattern)

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Streaming architecture analysis** — show layers as they're detected, not all at once | High | Medium |
| **Streaming AI enrichment** — show layer labels as the LLM rewrites them | Medium | High |
| **Progressive Phase 2** — show hotspots/cycles as they're found, not after full scan | Medium | Medium |
| **SSE from Worker** — stream LLM tokens to frontend instead of blocking | Low (Tauri IPC is fine for desktop) | High |
| **Abort/cancel** — no way to cancel an in-progress analysis | Medium | Low |

---

## 4. Progressive UX / Loading States

### What enterprises do

#### The state machine (not just "loading → done")

```
idle → submitted → thinking → streaming → complete
                                          ↗ stopped
                                          ↘ error
```

- **submitted**: Request sent, echo user's prompt immediately
- **thinking**: Model working, no output yet (200ms-4s TTFT). Show skeleton, not spinner.
- **streaming**: Tokens arriving. Render incrementally.
- **complete/stopped/error**: Terminal states with different affordances.

#### Skeleton loaders (not spinners)

- Show 3-5 lines of grey shimmer at decreasing widths
- Mimics the natural variation of text line lengths
- Reserves space → no layout shift when content arrives
- Psychologically reassuring even when text isn't there

#### Optimistic UI

- **Acknowledge immediately**: The moment the request leaves, show something
- **Placeholder**: Reserve the space where the answer will appear
- **"On it sir"** is exactly this pattern — NEXUS already does it well

#### Progressive disclosure for long-running tasks

- Start with high-signal status, add results as they arrive
- **Thin-to-thick updates**: begin with lightweight status, enrich over time
- **Granular result streaming**: deliver intermediate results in meaningful chunks
- Status line: "Processing 3 of 7 data sources... Results may change."
- When complete: "Complete · 7 data sources processed · Last updated 2 minutes ago."

#### Streaming visualizations

- Show loading skeleton of chart/table immediately
- Chart fills in with early segments, table rows stream in batch by batch
- Filters become active as soon as first rows arrive
- User can interact with partial results while more arrive

### What NEXUS/ULTRON does today

- **"On it sir"** TTS acknowledgement — excellent, matches optimistic UI pattern
- **Corner loading animation** — visible during background processing
- **Architecture window opens only when complete** — all-or-nothing, not progressive
- **No intermediate states** — user sees nothing about analysis progress until done

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Progressive architecture map** — show Phase 1 layers as detected, not after enrichment | High | Medium |
| **Status line during analysis** — "Analyzing layer 3 of 5..." | High | Low |
| **Skeleton map** — placeholder nodes before real data arrives | Medium | Low |
| **Partial hotspot/cycle display** — show as found, not after full Phase 2 | Medium | Medium |
| **Cancel button** — let user abort long analyses | Medium | Low |
| **Error recovery** — partial results on failure, not blank window | Low | Low |

---

## 5. Model Routing / Cascading

### What enterprises do

#### Model Router vs Cascading

| Dimension | Model Router | Cascading |
|-----------|-------------|-----------|
| Decision point | Before generation (classify input) | After generation (evaluate output) |
| Model calls | Exactly one (plus classifier) | One to N (depends on escalation) |
| Latency (best case) | Low — single call | Low — cheap model answers fast |
| Latency (worst case) | Low — single call | High — escalates through N models |
| Implementation | Need a classifier | Need a quality evaluator |
| Cost savings | 40-70% | 40-70% (but escalation is a live cost variable) |

#### The cascade pattern (cheap-first, escalate-on-failure)

```
Query → Cheap model (Gemini Flash)
         ↓ quality OK? → return
         ↓ quality low?
         → Mid model (Groq Llama)
            ↓ quality OK? → return
            ↓ quality low?
            → Frontier model (GPT-4)
               → return
```

**Critical risk**: A drifting quality verifier silently escalates everything to the
expensive tier. Monitor escalation rate as a live cost variable.

#### Semantic router (2026 standard)

- Classify intent: Fast / Standard / Deep
- Fast lane: cache/static answers (near-zero cost)
- Standard lane: hybrid retrieval + LLM
- Deep lane: tool/agent path for complex queries
- Router adds negligible overhead vs LLM inference time

### What NEXUS/ULTRON does today

- **Worker cascade** (`models.ts`) — Gemini → Groq fallback chain
- **Search question detection** (`isSearchQuestion()`) — routes factual questions to Wikipedia
- **Deterministic parser** (`intent_parser.rs`) — handles known commands without LLM
- **NLU fallback** — BERT-Mini ONNX classifier for unknown commands

### What NEXUS/ULTRON does well

- **Already has a cascade** — cheap-first is the right pattern
- **Already has routing** — deterministic parser → NLU → LLM is a 3-tier router
- **Already has caching** — KV/D1 edge cache

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Quality verifier** — no automatic check if cheap model output is good enough | Medium | Medium |
| **Escalation rate monitoring** — no tracking of how often cascade escalates | Medium | Low |
| **Semantic cache** — no embedding-based cache (only exact key match) | Medium | Medium |
| **Per-query model selection** — all queries use same cascade regardless of complexity | Low | Medium |

---

## 6. Semantic Caching

### What enterprises do

- **30-40% of LLM requests are semantically similar** to previous ones
- Users ask the same 200 questions phrased 15,000 different ways
- Semantic cache uses **embedding similarity** to match queries, not exact strings
- "What is your return policy?" = "How do I return a product?" = "Can I get a refund?"

#### Implementation

```
Query arrives → Embed query → Search cache for similar embeddings
               ↓ match found (similarity > threshold) → return cached response
               ↓ no match → forward to LLM → store query embedding + response
```

#### Similarity threshold tuning

| Threshold | Hit rate | False positive rate | Use case |
|-----------|----------|---------------------|----------|
| 0.05 (strict) | 15-25% | <1% | Legal, medical, financial |
| 0.10 (moderate) | 30-45% | 2-5% | General customer support |
| 0.15 (loose) | 45-65% | 5-10% | FAQ, documentation |
| 0.25 (very loose) | 60-80% | 15-25% | Internal tools, low-risk |

#### Production results

- GPTCache: 2-10x speed improvement on cache hit
- GPT Semantic Cache: 68.8% API call reduction, 97%+ positive hit accuracy
- Real-world: $23K/month → $8.6K/month (63% cost reduction) with semantic cache + model routing + prompt caching

#### Best practices

- **Namespace caches** by domain/endpoint/product (not one global cache)
- **Add metadata filters**: model name, temperature, app version
- **Set TTL** — cached responses expire
- **Combine with provider prompt caching** (OpenAI/Anthropic cache shared prefixes at 90% discount)
- **Monitor false positives** — wrong cached answers erode trust faster than slow answers

### What NEXUS/ULTRON does today

- **KV/D1 cache** (`cache.ts`) — exact key match (`searchKey("en", query)`)
- **No semantic similarity** — "what is X" and "explain X" are different cache keys
- **No embedding-based cache** — no vector similarity lookup

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Semantic cache** — embedding-based query matching | Medium | Medium |
| **Cache namespacing** — per-user or per-domain caches | Low | Low |
| **Cache TTL** — current cache has 24h TTL, no per-type variation | Low | Low |
| **False positive monitoring** — no tracking of wrong cached answers | Low | Low |

---

## 7. Code Intelligence Graph (GraphRAG for Code)

### What enterprises do

The 2026 standard for code intelligence is a **persistent knowledge graph** built from
static analysis, not LLM-guessed architecture.

#### The pipeline

```
Repository → Tree-sitter parse → Extract symbols → Build graph edges → Persist
                                                              ↓
                    Query (CLI / MCP / API / Cypher)
                    ├── "Who calls validate_user?"
                    ├── "Blast radius of this change?"
                    ├── "Find authentication logic"
                    └── "Show architecture map"
```

#### Graph schema

```
Nodes: File, Module, Class, Function, Method, Type, Interface
Edges: CONTAINS, DEFINES, CALLS, IMPORTS, IMPLEMENTS, EXTENDS
```

#### Key capabilities

- **Blast radius**: Seed from a symbol/file/diff, walk transitive dependents
- **Call graph**: Direct + transitive callers/callees
- **Cross-repo**: Federate graphs across repositories
- **Incremental**: SHA-diff files, only re-parse what changed
- **Community detection**: Louvain clustering finds coupled modules
- **Hybrid search**: BM25 + vector over symbols (find "auth" even if function isn't named "auth")

#### Performance benchmarks (GitGrapher)

| Fixture | Cold index | No changes | One-file incremental | Peak RSS |
|---------|-----------|------------|---------------------|----------|
| 50K TS functions / 500 files | 4.54s | 0.23s | 0.48s | 395 MB |

#### MCP integration

All modern code intelligence tools expose an **MCP server** so AI agents (Claude Code,
Cursor, Windsurf, Copilot) can query the graph directly instead of reading files:
- "What's the blast radius of changing this function?" → 1ms graph traversal
- "Show me the call chain from this route to the database" → graph path query
- 8.2x average context reduction vs raw file reads

### What NEXUS/ULTRON does today

- **Phase 1**: Heuristic file-path clustering → 5 architectural layers
- **Phase 2**: `ignore::WalkBuilder` + Rayon → dependency graph, hotspots, cycles
- **No persistent graph** — rebuilt from scratch every analysis
- **No tree-sitter** — no AST parsing, no symbol extraction
- **No MCP server** — no way for AI agents to query the graph
- **No incremental indexing** — full rebuild every time
- **No cross-repo** — single repo at a time

### What NEXUS/ULTRON is missing

| Gap | Priority | Effort |
|-----|----------|--------|
| **Tree-sitter AST parsing** — extract functions/classes/types as first-class entities | High | Medium |
| **Persistent code graph** — store in SQLite, don't rebuild every time | High | Medium |
| **Incremental re-indexing** — SHA-diff, only re-parse changed files | High | Medium |
| **MCP server** — expose graph to AI agents | Medium | Medium |
| **Cross-repo federation** — trace dependencies across repositories | Low | High |
| **Community detection** — Louvain clustering for coupling analysis | Low | Low |
| **Hybrid symbol search** — BM25 + vector over symbols | Low | High |

---

## 8. Inference Optimization (for reference)

### Prefill/Decode Disaggregation

Separate the prefill (process prompt) and decode (generate tokens) stages onto
different GPU pools:
- **Prefill-heavy** (RAG with short answers): more prefill GPUs
- **Decode-heavy** (chat with long answers): more decode GPUs
- **Cost reduction**: 25-40% on chat/RAG traffic
- **KV cache transfer** is the bottleneck (FlowKV reduces it by 96%)

### Speculative Decoding

- Draft model generates candidate tokens → target model verifies in parallel
- **QuantSpec**: 4-bit quantized KV cache, >90% acceptance rate, 2.5x speedup
- Best for long-context inference where KV cache is the bottleneck

### Provider prompt caching

- OpenAI/Anthropic cache shared prompt prefixes (system prompts, context)
- 90% discount on cached tokens
- Complementary to semantic caching (prefix caching = per-call, semantic = per-query)

**Relevance to NEXUS**: We use Cloudflare Workers AI + external LLM APIs, so inference
optimization is handled by the provider. Our leverage is in **routing, caching, and
reducing calls** — not in GPU-level optimization.

---

## Summary: What NEXUS/ULTRON should adopt, prioritized

### Tier 1 — Highest impact, aligns with core "what breaks" mission

1. **Tree-sitter AST parsing** for code analysis (replaces heuristic clustering)
2. **Persistent code graph** (SQLite-backed, incremental updates)
3. **Progressive architecture display** (show layers as detected, not all at once)
4. **Status line during analysis** ("Analyzing layer 3 of 5...")
5. **Cancel/abort for long analyses**

### Tier 2 — Significant improvement, moderate effort

6. **Semantic caching** (embedding-based query matching for Worker)
7. **Hybrid search** (BM25 + vector for code symbol retrieval)
8. **Incremental re-indexing** (SHA-diff, only re-parse changed files)
9. **AST-aware chunking** (functions/classes as chunks, not whole files)
10. **MCP server** (expose code graph to AI agents)

### Tier 3 — Enterprise-grade, higher effort

11. **Cross-repo federation** (trace dependencies across repositories)
12. **Reranking** (cross-encoder for retrieval precision)
13. **RAGAS evaluation** (automated retrieval quality measurement)
14. **Community detection** (Louvain clustering for coupling)
15. **Quality verifier in cascade** (auto-escalate when cheap model output is poor)

### What NEXUS/ULTRON already does well

- **"On it sir" TTS** — textbook optimistic UI acknowledgement
- **Corner loading animation** — visible progress during background work
- **Model cascade** — cheap-first (Gemini → Groq) is the right pattern
- **Deterministic parser → NLU → LLM** — 3-tier routing is enterprise-grade
- **Per-user quotas** — cost control with daily limits
- **KV/D1 edge caching** — Cloudflare edge for low latency
- **Lazy window creation** — RAM-efficient (only orb at idle)
- **Lazy STT/TTS** — sidecars start on demand, not at boot
