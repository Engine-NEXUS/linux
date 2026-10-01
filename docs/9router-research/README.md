# 9Router Free-Model Manager — Research Index

**Date:** 2026-09-15
**Status:** Research phase complete — implementation plan pending

This folder contains the research for building a **9Router Free-Model
Manager** into NEXUS — a system that discovers, configures, and maintains
free AI model providers through 9Router, controlled by a conversational
"thinking brain."

## Documents

| # | Document | Purpose |
|---|----------|---------|
| 01 | [9Router Free-Model Manager Research](./01-9router-free-model-manager-research.md) | 9Router capabilities, free-tier landscape, gap analysis, self-healing key lifecycle |
| 02 | [Brain Research — Low-RAM Thinking Models](./02-brain-research-low-ram-thinking-model.md) | Which small LLM can actually "understand and think" (not just pattern-match) — recommends Qwen3-4B-Thinking (2.8 GB) |
| 03 | [Conversational Agent Flow Design](./03-conversational-agent-flow-design.md) | How the assistant interacts with the user about free tiers, keys, reminders |
| 04 | [Router Brain — 500 MB RAM with Cloud Delegation](./04-router-brain-500mb-cloud-delegation.md) | **Revised brain architecture** — 0.5B router delegates to cloud free models (fits 500 MB constraint) |
| 05 | [Free MCP Servers Ecosystem](./05-free-mcp-servers-ecosystem.md) | **725+ free tools** across WhatsApp, YouTube, email, GitHub, social media, browser, calendar, Telegram, Spotify, memory — all 100% free, self-hosted, open-source |
| 06 | [Full Life Control — Indian Apps, Multi-User, $5 Plan](./06-full-life-control-mcp-indian-apps-multiuser.md) | **MCPJungle + Amazon + Swiggy + Zomato + Zepto + Blinkit + BookMyShow + District + 5 users on $5 plan — 90% possible today** |
| 07 | [Worker AI Usage Plan — Local vs Cloud vs Compulsory](./07-worker-ai-usage-plan-local-vs-cloud.md) | **Classifies every feature: what's fully local (0 cost), what needs Worker (0 neurons), what needs Workers AI (neurons). With 9Router, 5 users cost $5/mo total.** |
| 08 | [Latency-Optimized Routing — Speed First](./08-latency-optimized-routing-speed-first.md) | **Proves the 9Router plan is 3-7x FASTER than the current Worker approach. Keeps Workers AI only where it's genuinely better (GLM code review).** |
| 09 | [Multi-User Brain Tiers — Admin vs Family](./09-multi-user-brain-tiers-admin-vs-family.md) | **Admin gets Qwen 0.5B (500 MB). Family gets BERT-Mini NLU only (80 MB, no LLM). Cloud does the thinking for everyone. 5 users = 2,050 neurons/day.** |
| 10 | [Self-Improving Brain — Continuous Training](./10-self-improving-brain-continuous-training.md) | **Admin retrains BERT-Mini weekly. Corrections propagate to family via Worker R2. System gets smarter every week, for everyone, at $0 cost.** |
| 11 | [Best Pre-Trained STT Model Research](./11-best-stt-model-research.md) | **Evaluates every major English STT model (Sept 2026): Cohere Transcribe, Qwen3-ASR, Whisper, Moonshine, Parakeet. Recommends keeping Groq + upgrading Moonshine to Medium v2. English-only focus, 100% accuracy target.** |
| 12 | [Command Center Architecture — n8n-Style](./12-command-center-architecture-n8n-style.md) | **Main command center divides tasks to 5 sub-centers (Local, Cloud AI, Commerce, Social, System Control). Unified task state machine, multi-step planning, confirmation gates, error handling. n8n orchestrator-worker pattern.** |
| 13 | [Implementation Plan — Phased Roadmap](./13-implementation-plan-phased-roadmap.md) | **6 phases: 9Router (3-7x faster) → Moonshine v2 (-15% WER) → MCP servers (new features) → Command center (multi-step) → Self-improvement (continuous training) → Brain (Qwen 0.5B). Start with Phase 1+2.** |

## Key Findings

### 9Router
- 9Router is a real, MIT-licensed local gateway (localhost:20128) with 60+ providers
- It handles routing, fallback, quota tracking, OAuth, multi-account — but NOT discovery or self-healing
- NEXUS needs to build the discovery + classification + key-lifecycle layer on top of it
- Free-tier landscape: 9 permanent free providers (Groq, Gemini, Cerebras, etc.), 8 trial-credit providers
- Key pooling multiplies throughput (rate limits are per-key, not per-account)

### Brain (revised — see doc 04)
- **Original recommendation (doc 02):** Qwen3-4B-Thinking-2507, 2.8 GB RAM — too much for the 500 MB constraint
- **Revised recommendation (doc 04):** Keep the existing Qwen2.5-0.5B brain (398 MB model file), upgrade it to a **Router Brain**
- The 0.5B brain uses **function calling** (Qwen2.5 supports this natively via hermes parser) to route requests to the best cloud free model
- Heavy reasoning happens in the cloud (Groq Llama 3.3 70B, Gemini Flash, Cerebras) — not locally
- Multi-model orchestration: best model for each task (coding -> DeepSeek, long context -> Gemini, speed -> Groq)
- Self-improving: corrections accumulate (FreePalp pattern) so the 0.5B gets smarter over time
- **RAM: ~1.5 GB total (existing brain, no new model)** — fits the constraint since no additional RAM is needed
- Offline: limited (routing + simple answers only; complex reasoning needs cloud)

### Conversational Flow
- The assistant should be proactive (find providers, propose them), stateful (reminders), and conversational
- Clipboard integration for API key capture (user copies key, says "I have it", brain reads clipboard)
- JSON state file for multi-turn memory (no LLM-token cost for retrieval)
- 6 conversational flows designed: discovery, quota exhaustion, trial depletion, quarantine, status, manual add

## Architecture Summary

```
                    User speaks
                        |
                   Wake word + STT
                        |
                   Transcript
                        |
              +---------+---------+
              |                   |
        Deterministic         BERT-Mini NLU
        parser (regex)       (58 intents, ONNX)
              |                   |
         [matches]           [matches]
              |                   |
              +----+----+----+----+
                   |    |
              [no match] [no match]
                   |
                   v
              Router Brain (Qwen 0.5B, port 39219)
              Function calling: routes to best model or tool
                   |
         +---------+---------+---------+---------+
         |         |         |         |         |
         v         v         v         v         v
    Answer    9Router    Cloud     Clipboard  Browser
    locally   API        Free      (read     (signup
    (simple)  (manage    Models    API keys)  pages)
              providers) via 9Router
                         |
                    +----+----+----+----+
                    |    |    |    |
                    v    v    v    v
                  Groq Gemini Cerebras OpenRouter
                  (fast)(long (fast  (many
                        ctx)  tokens) models)
                         |
                         v
                    Cloud model does the thinking
                         |
                         v
                    Result returns to Router Brain
                         |
                         v
                    Router Brain formats for TTS
```

**Key insight:** The 0.5B brain doesn't think — it **routes**. The thinking
happens in cloud free models (70B+ parameters). This fits the 500 MB RAM
constraint because no large model runs locally.

## Next Steps (Implementation)

1. **Verify 9Router API** — Confirm `/api/providers`, `/api/combos` endpoints work programmatically
2. **Upgrade existing brain server** — Add function-calling + routing + cloud delegation to the existing Qwen 0.5B brain (port 39219). No new model, no new server, no additional RAM.
3. **Implement conversation state** — JSON state file with reminders and context
4. **Wire 9Router client** — Rust module for 9Router API calls (add/remove/validate providers)
5. **Implement clipboard reader** — Using existing `arboard` dependency (already in Cargo.toml)
6. **Build discovery scheduler** — Periodic scan of free-provider lists
7. **Implement multi-model routing** — Brain uses function calling to pick best cloud model per task
8. **Test end-to-end flow** — Discovery -> proposal -> user authenticates -> key added -> health check -> cloud delegation
