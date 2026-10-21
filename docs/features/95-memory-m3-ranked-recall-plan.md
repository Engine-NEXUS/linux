# Memory M3 — Ranked Recall & Full-Path Injection Plan (2026-10-07)

**Scope:** ranked recall (field weights + coverage + recency + frequency), budget 800→2000 with section caps, injection verified/extended on every conversational path. M4/M5 queued.
**Non-goals:** semantic embeddings (deterministic scoring only), cloud summarization, brain changes (Qwen is a classifier — no conversational output, nothing to inject).

## Research (verified in code)
- `recall()`: substring, tokens len>2, core (file order) then facts (file order), cap 10, **no scoring, no stats, no timestamps on core**.
- `MAX_CONTEXT_CHARS = 800` (memory.rs:14); facts take 8, episodes last 3.
- **9Router: covered** — `build_prompt` renders dialog_context.memory as "Context:" (router.rs:388-390).
- **Worker general: NOT covered** — `handleGeneral` uses only `req.task.request`; dialog_context arrives in task but is ignored. Needs the explicit change.
- **Brain: N/A** — classifier outputs intent JSON; its turns flow into dispatch paths that already inject.

## Phases
**M3.1 — Scored recall: BM25-lite, not hand-tuned weights (researched 2026-10-07).**
Evidence: personal-memory systems reach keyword R@5 ~90% with BM25 + entity legs (production FTS5 system, 14-month corpus); BM25≈bi-encoder on small corpora; defaults k1=1.2/b=0.75 are robust, and our corpus (~tens of facts) is too small for full IDF to matter — so we hand-roll ~30 lines, no new dependency:
`score = Σ IDF(t) · TFsat(t) · field_boost + recency + frequency`, with TF saturation `tf·(k1+1)/(tf+k1)`, length-norm vs fixed avg length, IDF smoothed for tiny corpora. Field boosts mirror mem0's ENTITY_BOOST (name/contact matches boost — our equivalent of their entity leg). Recency: exponential half-life — **core facts never decay** (explicit "remember" is durable by user intent), learned facts half-life 45d. Unit tests on tiers + ordering + dedupe preservation.
**M3.2 — Hit stats (`fact_stats.json {key: {hits, last_hit_ms}}`) with anti-feedback rules.** Updated in `get_memory_context` (not raw `recall` — keeps recall pure); frequency term is `ln(1+hits)` **capped and multiplied by the same recency decay**, so stale popular facts fade instead of self-perpetuating (rich-get-richer is the documented failure mode). Prune keys absent from core+facts. Tests: increment, prune, decay-coupling (old popular fact loses to fresh relevant one).

**M3.3 — Budget 800→2000.** Per-section caps (facts ≤1200, episodes ≤800); truncation by score order (never mid-fact). Test: caps + total bound.

**M3.4 — Worker general memory.** `handleGeneral` prepends dialog_context.memory as "Context:" (mirrors 9Router); Rust sanitizes the memory string once at merge (covers both paths — verify current merge sanitization first). Worker vitest: memory inclusion + absent-memory byte-identical.

**M3.5 — Recall observability.** Debug-log top recalled keys per turn (log-completeness: "why did it remember X" becomes greppable).

**M3.6 — Recall fixture eval (regression harness).** Canned `recall_fixtures.json` (~25 query→expected-top-fact pairs, incl. recency duels, name-entity queries, and negative "must-not-surface" cases) run as a Rust test asserting recall_any@3. Makes ranking tunable with proof — every future tweak re-runs the fixture instead of vibes. (LongMemEval-style, at our scale.)

## Acceptance & ordering
M3.1 → M3.2 → M3.3 → M3.4 → M3.5. Each: unit tests + gates. Live script: seed 15+ facts across core/learned → ask overlapping question → most relevant surfaces first (not file order); Worker-path reply references a fact 9Router would also see; audit shows hit counts.
