# NEXUS Research Index

All research documents organized by topic.

## Landscape & Audit
- [Vision-Grounded Gmail Engine & Proactive Watch Architecture](vision-grounded-gmail-engine-and-proactive-watch-architecture-2026-09-29.md) — 3-tier hybrid perception (browser URL hash + VLM fallback), native Gmail Engine watch state machine, and proactive deadline diffing
- [Voice & Ghost Mode Root Cause Analysis & Flow Hardening](voice-and-ghost-mode-root-cause-analysis-and-flow-hardening-2026-09-29.md) — Deep audit of ghost waves, strict motion invariants, browser search & typing (Ctrl+L/dictation), app-open TTS latency (<5ms), and complete GitHub intent isolation
- [AI Assistant Landscape Audit](ai-assistant-landscape-audit-2026-09-26.md) — 15 GitHub projects + YouTube landscape + claim verification vs academic papers

## Competitive Analysis
- [Wake Word Academic Analysis](wakeword-academic-analysis.md) — Wake word systems vs academic papers (arXiv:2304.03416, arXiv:2011.01460, arXiv:2307.14743)
- [Ghost Mode Competitive Analysis](ghost-mode-competitive-analysis.md) — AI desktop control comparison (Open Interpreter, AI Desktop, Everywhere, PyGPT)
- [NLU Competitive Analysis](nlu-competitive-analysis.md) — NLU systems comparison (Leon, Home Assistant, Almond, Rhasspy, Mycroft)
- [MCP Ecosystem Analysis](mcp-ecosystem-analysis.md) — MCP ecosystem study (arXiv:2607.25635, ACM DOI:10.1145/3796519)
- [Voice Assistant Comparison](voice-assistant-comparison.md) — Full feature-by-feature comparison table

## Planning
- [Improvement Plan](improvement-plan.md) — 19 prioritized improvements across 4 phases (A-D)

## Existing Research (from AGENTS.md)
- [Wake Word Research](features/71-personalized-and-neural-augmented-wakeword-training.md)
- [NLU Phonetic Alias Map](features/69-nlu-phonetic-alias-map-and-missed-intent-logging.md)
- [STT Hallucination Fix](changes/50-stt-hallucination-verify-ring-prefix-pad-and-ghost-mode-vocabulary.md)
- [Ghost Mode Complete](features/70-ghost-mode-complete.md)
- [Ghost Architecture Reference](features/ghost-mode-architecture-reference.md)

## Claim Verification Summary
| Claim | Verdict | Source |
|-------|---------|--------|
| Whisper too slow for real-time | Exaggerated | arXiv:2307.14743 (Whisper-Streaming) |
| Local models 3-7x slower | Misleading | arXiv:2601.09527 (GPU benchmarks) |
| 0% false positive rate | Incorrect | openWakeWord docs, arXiv:2304.03416 |
| 104MB idle RAM | Optimistic | Tauri #5889, Microsoft WebView2 docs |
| Streaming TTS ~500ms | Conservative | arXiv:2509.15969 (VoXtream), arXiv:2111.09052 |
| Agent protocols reduce dev time | Unverified | arXiv:2607.25635, ACM DOI:10.1145/3796519 |

## Key Findings
- NEXUS is best-in-class in wake word (9.5/10), Ghost Mode safety (9.5/10), MCP (9/10), OTA updates (10/10)
- Critical gap: No persistent memory (2/10) — Leon has 5-layer memory system
- High-ROI improvements: persistent memory, streaming TTS, speaker verification wiring, Ghost Mode vision
- 19 improvements prioritized across 4 phases (A-D), ~6 months total
