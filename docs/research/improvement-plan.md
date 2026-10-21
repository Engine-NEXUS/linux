# NEXUS Improvement Plan — Prioritized Roadmap

## Phase A — Quick Wins (1-2 days each)

### A1. Wire Speaker Verification into Wake Pipeline
**Status**: voice_profile.rs exists, enrollment works, verification is `#[allow(dead_code)]`
**Effort**: Low
**Impact**: High — eliminates false wakes from other speakers
**What to do**:
1. Add 500ms audio ring buffer to wakeword_oww.rs
2. On wake detection, extract embedding from ring buffer
3. Compare against enrolled voice_profile embeddings
4. Reject if cosine similarity < 0.45 threshold
5. Update wakeword_oww.rs detect_chunk() to call verification

**Evidence**: voice_profile.rs is dead code but enrollment works. Academic papers (arXiv:2304.03416) confirm speaker verification reduces false accepts.

### A2. Move API Keys to OS Keychain
**Status**: API keys in settings.json (plaintext JSON). settings.json is in %APPDATA%.
**Effort**: Low
**Impact**: High — security concern
**What to do**:
1. Use Windows Credential Manager for API keys
2. settings.json stores only non-secret preferences
3. auth_vault.rs already has credential store abstraction
4. Migrate groqApiKey, geminiApiKey, cerebrasApiKey to keychain

**Evidence**: LocalAI uses OS credential store. Jan uses Keycloak. All major projects avoid plaintext secrets.

### A3. Add Health Dashboard
**Status**: Diagnostics are startup-only (ASCII box on boot). No runtime monitoring.
**Effort**: Low
**Impact**: Medium — runtime visibility
**What to do**:
1. Build health dashboard frontend component
2. Expose runtime metrics via nexus_diagnostics Tauri command
3. Track: STT latency, NLU confidence, MCP status, memory usage, wake word stats
4. Add health dashboard to sidebar or settings

**Evidence**: LocalAI has distributed tracing (OpenTelemetry, Jaeger, Grafana). Jan Server has monitoring.

### A4. Add PII Filtering Middleware
**Status**: All transcripts sent to cloud (Groq, Worker). No privacy filtering.
**Effort**: Low
**Impact**: Medium — privacy compliance
**What to do**:
1. Add PII detection middleware before cloud API calls
2. Redact/redact PII in transcripts before sending to Groq/Worker
3. Configurable PII sensitivity levels
4. Local processing for PII detection (no cloud dependency)

**Evidence**: LocalAI v4.9 has PII filtering middleware. LocalAI 4.9 has "auth deny-by-default."

### A5. Add Settings Export/Import
**Status**: No settings backup/restore capability.
**Effort**: Low
**Impact**: Low — convenience
**What to do**:
1. Export settings.json + associated data as ZIP
2. Import from ZIP with validation
3. Include NLU models, voice profiles, OAuth tokens (encrypted)

**Evidence**: Jan has settings export/import.

---

## Phase B — High-Impact Features (3-7 days each)

### B1. 3-Tier Persistent Memory System
**Status**: No persistent memory whatsoever. 6-turn in-memory context only.
**Effort**: Medium
**Impact**: Very High — the #1 missing capability
**What to do**:
1. **Core facts** (persistent): User name, preferences, frequent contacts — stored in %APPDATA%/com.nexus.assistant/memory.json
2. **Episodic** (30-day rolling): Past conversations with timestamps — stored in SQLite at %APPDATA%/com.nexus.assistant/memory/episodes.db
3. **Semantic** (searchable): Key-value facts extracted from conversations — stored in SQLite with FTS
4. Memory retrieval: On each turn, search episodic/semantic memory and inject relevant context into LLM prompts
5. Memory consolidation: Daily summarization of episodic memories into semantic facts

**Evidence**: Leon's 5-layer memory (FTS + RAG + vector + SQLite + markdown). Jarvis's Memory Vault. Academic: 3-tier memory (core facts / daily summaries / raw) is proposed as fix for "task managers feel like homework" (HN comment).

### B2. Streaming TTS
**Status**: Full utterance synthesized then played. ~200ms latency + playback delay.
**Effort**: Medium
**Impact**: High — reduces perceived latency from ~500ms to ~200ms
**What to do**:
1. Implement edge-tts WebSocket connection (or chunked synthesis)
2. Play first audio chunk while synthesizing rest
3. Buffer management for chunk alignment
4. Fallback to batch TTS on WebSocket failure

**Evidence**: Streaming TTS achieves 75-250ms TTFB (arXiv:2509.15969 — VoXtream, arXiv:2111.09052 — Google Tacotron streaming). ElevenLabs Flash v2.5 achieves ~75ms inference. Home Assistant has streaming TTS since 2025.3.

### B3. Emotion/Style TTS Control
**Status**: Fixed voices via edge-tts. No emotion/style control.
**Effort**: Medium
**Impact**: Medium — more natural voice interaction
**What to do**:
1. Integrate OpenVoice v2 (1-5s voice cloning, emotion/style control)
2. Add emotion tags to TTS prompts (cheerful, calm, urgent, whisper)
3. Voice cloning option for user's own voice
4. Fallback to edge-tts if OpenVoice fails

**Evidence**: OpenVoice v2 (myshell-ai/OpenVoice) decouples voice timbre from speaking style. Same speaker can deliver lines cheerfully, angrily, whispering. MIT license — commercially safe.

### B4. Parallel Command Execution
**Status**: Sequential-only command center. "A then B" works; "A and B in parallel" doesn't.
**Effort**: Medium
**Impact**: High — faster multi-step operations
**What to do**:
1. Detect independent steps (no data dependency between steps)
2. Run independent MCP calls concurrently using tokio::join!
3. Merge results after all parallel steps complete
4. Keep dependent steps sequential

**Evidence**: Semantic Kernel supports Concurrent, Sequential, Handoff, Group Chat, Magentic orchestration. tokio::join! already exists in architect.rs for parallelized GitHub API calls.

### B5. Durable Execution (Checkpointing)
**Status**: Command center is in-memory only. Crash loses all state.
**Effort**: Medium
**Impact**: Medium — crash recovery
**What to do**:
1. Serialize TaskPlan state to disk on each step
2. On startup, check for unfinished plans
3. Resume from last checkpoint
4. Clean up completed plans

**Evidence**: LangGraph has durable execution with checkpointing. Agents persist through failures and resume.

---

## Phase C — Strategic Features (1-2 weeks each)

### C1. Vision/Screen Parsing for Ghost Mode
**Status**: UIA-first resolver only. Can't see screen content.
**Effort**: High
**Impact**: Very High — unlocks Ghost Mode capabilities
**What to do**:
1. Add screenshot capture (Windows DWM)
2. Send screenshot to Groq vision API (already has API key)
3. Parse screenshot with VLM to identify visual elements
4. Integrate with existing UIA resolver (VLM for visual, UIA for accessibility tree)
5. Before/after visual verification

**Evidence**: AI Desktop (FareedKhan) uses OmniParser icon detection + VLM. Open Interpreter OS Mode uses GPT-4o vision. Everywhere uses screen context + MCP tools.

### C2. Proactive Behavior (Pulse + Diary)
**Status**: Purely reactive. Never initiates action.
**Effort**: High
**Impact**: High — transitions from assistant to proactive agent
**What to do**:
1. **Pulse system**: Monitor context for actionable opportunities (e.g., "you have a meeting in 5 minutes")
2. **Diary**: Log notable events and decisions for self-reflection
3. **Self-model**: Track behavioral patterns and adapt
4. **Proactive notifications**: Suggest actions based on context

**Evidence**: Leon has proactive pulse and private diary with self-model that evolves over time.

### C3. Agent Protocol (ACP or Custom)
**Status**: Custom protocol between frontend and Rust backend.
**Effort**: Medium
**Impact**: Medium — interoperability with other agent ecosystems
**What to do**:
1. Evaluate MCP (Model Context Protocol) as standard
2. Or implement ACP (Agent Client Protocol) for editor integration
3. Standardize event formats for cross-tool compatibility

**Evidence**: ACP (Agent Client Protocol) jointly governed by Zed Industries + JetBrains. Reuses MCP JSON shapes. MCP ecosystem study (arXiv:2607.25635) shows 1,723 MCP apps with ecosystem convergence.

### C4. Voice Cloning (OpenVoice Integration)
**Status**: Fixed TTS voices. No voice cloning.
**Effort**: Medium
**Impact**: Medium — personalization
**What to do**:
1. Integrate OpenVoice v2 (1-5s cloning)
2. Allow users to record a short sample for voice cloning
3. Use cloned voice for TTS responses
4. Maintain edge-tts as fallback

**Evidence**: OpenVoice v2 achieves 1-5s voice cloning with emotion/style control. MIT license.

### C5. Webhook Triggers
**Status**: No external event triggers. Only voice-initiated actions.
**Effort**: Medium
**Impact**: Medium — event-driven agent activation
**What to do**:
1. Add webhook endpoint for external triggers
2. Process webhook events through NLU pipeline
3. Execute actions based on webhook input
4. Security: authenticate webhook sources

**Evidence**: AutoGPT Forge supports webhook-triggered agents.

---

## Phase D — Ecosystem (2+ weeks each)

### D1. Composable Backends (gRPC)
**Status**: Monolithic STT/TTS/NLU. Can't swap components.
**Effort**: High
**Impact**: Medium — flexibility
**What to do**:
1. Define gRPC interfaces for STT, TTS, NLU
2. Implement default backends
3. Allow swapping components without recompiling
4. Similar to LocalAI's composable architecture (60+ gRPC backends)

**Evidence**: LocalAI has composable core + 60+ gRPC backends. OCI images pulled on demand.

### D2. Declarative Agent Specs (YAML)
**Status**: Intent definitions are Rust code. Can't be user-defined.
**Effort**: Medium
**Impact**: Medium — user-defined intents
**What to do**:
1. Define YAML schema for agent specs
2. Allow users to define custom intents in YAML
3. Parse YAML at runtime and register with intent parser
4. Similar to Semantic Kernel's declarative YAML agent specs

**Evidence**: Semantic Kernel uses declarative YAML agent specs. Microsoft Agent Framework successor.

### D3. Self-Building Improvement
**Status**: NLU training is manual (nexus train). No autonomous improvement.
**Effort**: High
**Impact**: High — autonomous model improvement
**What to do**:
1. Implement BabyAGI-style functionz framework
2. Agent evaluates its own performance and identifies edge cases
3. Automatically generates training data for weak areas
4. Triggers retraining when performance degrades

**Evidence**: BabyAGI's self-building agents write new functions from user descriptions. Brain monitor already has continuous learning loop.

### D4. On-Device Wake Word (ESP32)
**Status**: Host-only. No microcontroller option.
**Effort**: High
**Impact**: Low — niche use case
**What to do**:
1. Port openWakeWord to ESP32-S3 (like Home Assistant microWakeWord)
2. TinyML model for ESP32-S3
3. Wake word detection on microcontroller
4. Wake trigger sent to host via WebSocket

**Evidence**: Home Assistant uses microWakeWord on ESP32. ESP32-S3 builds (YouTube) use WakeNet wake word.

---

## Implementation Timeline

| Phase | Items | Estimated Duration |
|-------|-------|-------------------|
| A | 5 quick wins | 1-2 weeks |
| B | 5 high-impact features | 3-5 weeks |
| C | 5 strategic features | 5-10 weeks |
| D | 4 ecosystem features | 10-16 weeks |
| **Total** | 19 items | **~6 months** |

## Priority Matrix

| Impact \ Effort | Low | Medium | High |
|-----------------|-----|--------|------|
| **Very High** | | B1 (Memory) | C1 (Vision) |
| **High** | A1 (Speaker verify) | B2 (Streaming TTS), B4 (Parallel) | C2 (Proactive) |
| **Medium** | A3 (Dashboard), A4 (PII) | B3 (Emotion TTS), B5 (Durable), C3-C5 | D1 (Composable) |
| **Low** | A5 (Export), G19-G24 | | D3-D4 |

## References
- Leon (leon-ai/leon) — 5-layer memory, proactive pulse
- OpenVoice v2 (myshell-ai/OpenVoice) — voice cloning, emotion control
- Semantic Kernel (microsoft/semantic-kernel) — YAML agent specs, multi-agent
- LangGraph (langchain-ai/langgraph) — checkpointing, durable execution
- BabyAGI (significant-gravitas/autogpt) — self-building agents
- LocalAI (mudler/LocalAI) — composable backends, PII filtering
- Open Interpreter (openinterpreter/openinterpreter) — ACP, vision
- arXiv:2607.25635 — MCP ecosystem study
- arXiv:2111.09052 — Google Tacotron streaming
- arXiv:2509.15969 — VoXtream
- arXiv:2603.05413 — Enterprise voice agents
- arXiv:2304.03416 — Successive Refinement
- ACM DOI:10.1145/3796519 — MCP security study
- docs/features/64-ghost-mode-plan.md
- docs/features/70-ghost-mode-complete.md
- docs/research/ai-assistant-landscape-audit-2026-09-26.md
