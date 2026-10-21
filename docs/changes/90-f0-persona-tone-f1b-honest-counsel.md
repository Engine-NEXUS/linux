# F0 Persona Tone + F1b Honest Counsel (2026-10-06)

**Plan:** `docs/features/93-friendship-humor-personality-plan.md` (F0 + F1/F1b executed; F2-lightness is prompt-level, F3/F4 queued). No joke system anywhere, per directive.
**Research grounding baked in:** memory-↑-sycophancy counter, explicit truthfulness demand (Anthropic), direct-even-if-critical fragment (ELEPHANT), assess-don't-comply structure (SAA), 5-metric probe rubric + rebuttal stability.

## What changed
- **F0 tone mode:** `persona_mode` (`butler` default / `friend`) in `NexusSettings` + `read/set_persona_mode` (read-modify-write, typo-safe) + `persona.rs` (`is_friend`, `address`, `greet_address`, `restyle_greeting`) + `PersonaFriend/PersonaButler` intents + Command Hub Personality section (Butler/Friend select). Greetings migrate via one orchestrator hook (parser stays pure) — friend mode addresses by name or drops the address.
- **F1b honest counsel:** `ShareConcern{story}` intent (opener arming with 120s deadline expiry + direct-story forms with counsel anchors); `COUNSEL_ARMED` + normal-pipeline intercept (control intents bypass; ghost turns skip by design); runners (`run_share_concern`, `run_counsel_turn`, `run_persona_switch`) in both pipelines; `COUNSEL_CONTRACT` travels via dialog_context.memory (9Router, zero signature changes) + explicit `task.intent="counsel"` (Worker `handleCounsel` + dispatch branch); turns recorded as high-relevance `share_concern` (conversation.rs relevance 95); `route_intent` → WorkerBackend, `center_for` → GreetingCenter.
- **Worker:** `counsel.ts` (contract + builder) + `handleCounsel` + dispatch branch.

## Alignment fixes after audit (same day)
- Contracts were missing the framework's clarifying-question step — added "(2) if underspecified, ask ONE clarifying question instead of verdicting" to both Rust `COUNSEL_CONTRACT` and Worker `COUNSEL_SYSTEM` (+ fragment tests both sides).
- Friend tone never reached LLM replies (only greetings) — closed via `FRIEND_TONE` memory injection (9Router) + explicit `task.persona` → `generalSystem()` variant (Worker); butler path byte-identical. Added `share_concern` relevance-95 test.
- Uncommitted.

## Verify
- Rust targeted (persona ×3, counsel parser) + full **1001/1002** serial — 1 failure is the pre-existing `turn_detect::model_loads_and_runs` timing flake (2739ms under suite load vs budget; passes isolated in 0.73s; untouched module). Post-fix full run: **1005/1005**.
- Worker **103/103** (6 new contract tests); vitest **189/189**; tsc 0; release **91.5 MB**; zero new warnings.
- Uncommitted. Live matrix (`nexus start`, friend mode): greeting uses name → "I want to share something" invites → story counsels (acknowledge→verdict→step) → "was I right…" runs immediately → "tell me a joke" deflects, never jokes → "be formal" restores sir.
