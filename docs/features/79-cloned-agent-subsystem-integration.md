# Feature 79: Cloned Agent-Subsystem Integration (Clone → Extract → Delete)

## Provenance (clones deleted after Phase 4 — SHAs recorded here)

| Repo | SHA (depth-1, 2026-09-30) | Verdict |
|---|---|---|
| `lastmile-ai/mcp-agent` | `f62d849350816588b1c6294e7914bbe4d8b84072` | EXTRACT per §2A |
| `sl4m3/agent-memory` (LedgerMind 4.1.0) | `cd68dfd80185fc189ae0d8b38f494dd8b29012a3` | **DROPPED — see §2B** |
| `Memento-Teams/Memento` | `42fbbcac63dd58ed6856c0761357345a58e4f032` | EXTRACT per §2C |
| `tainguyen07/agent-workflow-mcp` | `f8eaffa8f14fcfc4db50566d1a3fb97c871fb7c5` | EXTRACT per §2D |

Staging: `.staging/` (gitignored). Orb UI untouched throughout (owner's
separate plan).

## Corrected source map (verified against the actual trees — the draft
plan's paths were partly wrong)

### 2A. mcp-agent → `server/agent/`
- `workflows/orchestrator/orchestrator.py` ✓ exists
- `workflows/router/router_base.py` + `router_llm.py` (NO single
  `router.py`; skip `router_embedding*` / provider variants)
- `workflows/evaluator_optimizer/evaluator_optimizer.py` ✓ exists
- Evaluate in Phase 2: `orchestrator_prompts.py`, `orchestrator_models.py`
  (prompts may belong in 2D instead), `deep_orchestrator/` (likely skip —
  overlaps `command_center.rs`)

### 2B. LedgerMind → DROPPED, do not extract
The repo contains NO Python package (docs + benchmarks + `skills/`
installer flow only) and targets **Linux x86_64/aarch64 exclusively** —
unusable on Windows. The encrypted-vault need is met inside the tree
instead: `auth_vault.rs` (Windows Credential Manager via keyring) is the
correct secret store; no custom AES vault, no new dependency.

### 2C. Memento → `server/memory/cbr.py`
- `memory/np_memory.py` ✓ exists (case-bank read/write/query)

### 2D. agent-workflow-mcp → prompts + retry
- Prompts live in `agents/prompts.py` (NOT `.md` files) — extract constants
  into `server/agent/prompts.py`
- `src/agent_workflow_mcp/retry.py` ✓ exists → rewrite to `server/agent/retry.py`
  (~40 lines, no dep); planner/executor/critic agents stay THEIR code
  (we keep `command_center.rs` as executor — no parallel agent runtime)

### 2E. fastmcp → pip dependency only (DEFERRED, needs user approval)
`requirements.txt += fastmcp>=2.0`; `server/github_mcp_server.py` written
fresh (~60 lines). NOT STARTED — bundled with the deferred Rust-side
spawn + real LLM adapter in Phase 3's record below.

## Phase record (all complete — this section is the final account)

- **Phase 2**: extract allowlist ONLY (§2A/2C/2D as corrected above).

**Status (done, verified ×2):** 10 source files — `server/agent/`
(prompts, orchestrator_prompts, orchestrator_models, evaluation,
retry rewritten stdlib-only, runner wiring, requirements) +
`server/memory/` (cbr with torch-free scorer) + 5 unittest files.
27 agent + 4 memory tests green twice.
Corrections applied during extraction: `Step.description` default
(upstream default_factory threw), retry `fatal` tuple replacing the
foreign exception coupling. Deliberately NOT copied: mcp-agent
orchestrator/router/evaluator runtimes (mcp_agent-internal deps +
direct overlap with `command_center.rs`), Memento torch retriever,
LedgerMind (dropped §2B), provider-specific router variants.
- **Phase 3 (done, verified ×2)**: dedupe audit clean — grep over
  `server/` found NO existing retry/backoff implementation (only NLU
  training phrases containing the word "retry") and no planner/executor/
  critic prompts; `command_center.rs`/`memory.rs`/`mcp_client.rs` operate
  in Rust, the extracted subsystem in Python — no duplicates.
  Wiring: `server/agent/runner.py` — the single caller tying every
  extracted module together: planner (PLANNER_SYSTEM + optional
  case-bank few-shot seeds via memory.cbr) → bounded execute/critic loop
  (max_steps) → merged PlanResult; tolerant JSON parser (raw/fenced/
  balanced-brace); retry with backoff on transient errors only; critic
  rejection aborts the loop, unvalidatable critique fails OPEN (accepted).
  LLM is injected (`llm_call(system, user) -> str`) — zero network code,
  stdlib + pydantic only, no new sidecar/port yet. 27 agent + 4 memory
  tests green twice. Reserved (documented, not dead): mcp-agent
  FULL_PLAN/ITERATIVE_PLAN/SYNTHESIZE templates stay verbatim for the
  future iterative re-planning loop.
  Deferred (needs user approval): Rust-side spawn of an agent server +
  real LLM adapter (Groq/Cerebras) + fastmcp GitHub MCP server.
- **Phase 4 (done, verified ×2)**: gates — no-dup grep clean; all
  extracted modules reachable through runner.py (no orphans); import
  closure verified (`agent.*` + `memory.cbr` resolve from `server/`);
  py_compile clean; 31 tests ×2 green; smoke = fake-LLM full-loop runner
  tests (no network). Scope check: only `server/agent/`, `server/memory/`
   and this doc added; no orb/UI/Rust files touched.
- **Phase 5 (done)**: `.staging/` deleted (all 4 SHAs recorded above —
  any file can be re-cloned at `git checkout <sha>`). This doc is the
  provenance record. The `.staging/` gitignore entry stays — re-staging
  for the deferred Rust-side integration can reuse it.
