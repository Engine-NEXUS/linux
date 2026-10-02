# NEXUS Research System — Documentation Index

> **Two research series live in this folder.**
> - **01–10:** the multi-source ad-free research system in the Worker.
> - **10–19:** the 2026-10 competitive & platform audit (see below).

## Series 2 — 2026-10 Competitive & Platform Audit

**Start here:** [`../62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)

Ten parallel research agents scoped to one angle each, run 2026-10-02. Each was
required to separate **shipping now** from **announced but unusable**, with source
URLs, dates and measured numbers. Vendor self-claims are labelled as such.

The companion compendium (canonical home) lives in
[`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS).

| # | File | What it covers |
|---|------|----------------|
| 10 | [10-competitive-audit-executive-summary.md](10-competitive-audit-executive-summary.md) | Synthesis, per-area verdicts, priority order, scope changes |
| 11 | [11-platform-constraints-wayland-tauri.md](11-platform-constraints-wayland-tauri.md) | Tauri v2 / Linux Wayland capability matrix, the unfixed `tao` panic, window-architecture alternatives |
| 12 | [12-ambient-ui-design-language-2026.md](12-ambient-ui-design-language-2026.md) | Google/Microsoft/Apple presence patterns, collapse rules, M3 Expressive motion specs, documented redesign failures |
| 13 | [13-voice-ux-conversation-latency.md](13-voice-ux-conversation-latency.md) | Turn-taking (+208 ms floor), latency ladder, state signalling, error recovery, confirmation, why not to build visemes |
| 14 | [14-voice-stack-sota-wakeword-vad-stt-tts.md](14-voice-stack-sota-wakeword-vad-stt-tts.md) | Wake word / VAD / STT / TTS state of the art with concrete upgrade paths |
| 15 | [15-nlu-intent-decision-models.md](15-nlu-intent-decision-models.md) | Jev + Laya verdict with independent benchmarks; the fine-tune path; local model landscape |
| 16 | [16-computer-use-linux-gap-perception.md](16-computer-use-linux-gap-perception.md) | The Linux moat (verified from primary vendor docs), OCR/GUI-grounding SOTA, AT-SPI, pointer patterns |
| 17 | [17-security-oauth-prompt-injection.md](17-security-oauth-prompt-injection.md) | The unauthenticated token endpoints, MCP spec violations, secure Linux storage, prompt injection |
| 18 | [18-distribution-linux-packaging.md](18-distribution-linux-packaging.md) | Flathub's Generative AI policy, Tauri bundle reality, the microphone-portal gap |
| 19 | [19-devtool-ui-design-systems.md](19-devtool-ui-design-systems.md) | Token architecture, dark-mode convention, and the four AI-slop tells already in our CSS |

**Three findings that change the plan:**

1. **The orb architecture cannot work on Linux Wayland.** `always_on_top` is an
   empty function in GTK3; `set_position` is impossible by design; click-through
   is broken on Mutter; `set_ignore_cursor_events` panics in current stable `tao`.
2. **The 2026 design consensus is the opposite of a full-size animated orb.**
   Google, Microsoft and Apple converged on small / monochrome / collapsed.
   Microsoft removed its assistant's colour *deliberately*.
3. **Endpointing is a bigger bug than the visuals.** SRI: a fixed 500 ms gate causes
   ~100% premature cut-off; **100 ms with pre-pausal acoustics drops it to 20.3%** —
   and that is our documented "speech onset decapitation" bug.

**Standing scope decision (2026-10-02):** multilingual (Hindi/Telugu) is **out of
scope**. This removes the largest unmeasured risk in the stack — no verified Telugu
benchmark exists for any sub-6B model — and unlocks the English-only intent
classification literature and the SiFT footprint optimisation.

---

## Series 1 — Worker Research System

This folder documents the complete multi-source ad-free research system
built into the NEXUS Worker. The system retrieves factual information
from 9+ independent sources, then synthesizes answers using a 3-tier LLM
cascade — all on free tiers, designed for 5–10 users at $5/month total
cost.

## Files in this folder

| # | File | What it covers |
|---|------|----------------|
| 01 | [01-research-sources.md](01-research-sources.md) | All 9 research sources, their APIs, free tiers, limits, and code |
| 02 | [02-llm-cascade.md](02-llm-cascade.md) | Gemini → Groq → Cloudflare LLM cascade with model names and fallback logic |
| 03 | [03-api-keys-and-secrets.md](03-api-keys-and-secrets.md) | Every API key, where to get it, free tier details, Cloudflare secret setup |
| 04 | [04-cascade-architecture.md](04-cascade-architecture.md) | The full retrieval + synthesis flow, code paths, and decision logic |
| 05 | [05-capacity-analysis-10-users.md](05-capacity-analysis-10-users.md) | Whether each free tier survives 10 active users, with real math |
| 06 | [06-latency-benchmarks.md](06-latency-benchmarks.md) | Measured end-to-end latency for every command type, from live tests |
| 07 | [07-deployment-guide.md](07-deployment-guide.md) | Step-by-step deploy: secrets, D1 schema, KV namespace, wrangler config |
| 08 | [08-testing-results.md](08-testing-results.md) | Live test results with actual queries, response times, and provider routing |
| 09 | [09-intent-routing-fixes.md](09-intent-routing-fixes.md) | Bug fixes: "research" keyword, isSearchQuestion, isMathQuery, isAcademicQuery |
| 10 | [10-future-improvements.md](10-future-improvements.md) | Pending keys, potential upgrades, and scaling beyond 10 users |

## Quick summary

**What changed:**

1. **9 research sources** added to `server/worker/src/research.ts` —
   Wikipedia, Wikidata, DuckDuckGo, knowledgelib.io, SearchX, Tavily,
   Google Custom Search, Serper.dev, Wolfram Alpha, Semantic Scholar.

2. **3-tier LLM cascade** in `server/worker/src/external_llm.ts` —
   Gemini Flash Lite (1,500/day) → Groq Qwen 3.8 27B (14,400/day) →
   Cloudflare llama-3.2-3b (~200/day).

3. **Intent routing fixes** in `server/worker/src/index.ts` —
   "research", "look up", "explain", "define" now route to search
   without needing an LLM intent classifier call.

4. **6 Cloudflare secrets** set via `wrangler secret put`.

5. **D1 schema + KV namespace** created and deployed.

6. **28 tests** passing (4 new test suites for math/academic detection).

7. **Worker deployed** to `https://nexus-worker.chitkullakshya.workers.dev`.

**Total cost: $5/month** (Cloudflare Workers Paid — all other APIs are free tier).

**10-user capacity: 153x headroom** on LLM, 200x on search.
| 20 | [Is Pixels the Right Abstraction? Rethinking Linux Computer Control](20-linux-computer-control-architecture-rethink.md) | **libei/portal, AT-SPI `DoAction`, CDP AX-tree actuation, stale-coordinate attacks, false completion. Why screenshot+vision is the wrong model for an assistant on a machine you own** |
| 21 | [Every Way to Control a Linux Laptop, Compared](21-linux-computer-control-full-comparison.md) | **Full enumeration of perception × actuation channels, scored on Wayland support / consent / privilege / latency / verifiability. Desktop coverage matrix, whole-system design comparison, and what would falsify the recommendation** |
| 22 | [Appendix: Linux Perception & Actuation Codebase Audit](22-linux-perception-actuation-codebase-audit.md) | **Evidence base for docs 20–21. Every claim in the computer-control redesign carries a `file:line` citation: what actuates on Linux today, what no-ops, the dead `live_*` voice path, the inert privacy gate, and a severity-ranked defect list** |
| 23 | [Coverage and Guardrails: What NEXUS Can Actually Reach](23-coverage-and-guardrails.md) | **Honest per-app-class reachability matrix, proof that nothing is hardcoded to specific apps, and the policy layer that gates every semantic operation on OBSERVED window identity rather than model-supplied text — plus the gap this work found in itself** |
