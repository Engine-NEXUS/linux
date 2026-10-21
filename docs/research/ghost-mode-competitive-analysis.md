# Ghost Mode — Competitive Analysis & Architecture Deep-Dive

## NEXUS Ghost Mode Architecture

```
User says "ghost mode" → EnterGhostControl intent
  → Session state machine: Idle → Active → Yielded
  → Stage overlay window (fullscreen transparent)
  → Ring rides cursor (AI-commanded motion only)
  → Esc panic button (dynamic, never global)
  → Takeover detector (task-abort on grab, session stays)
  → AI drives REAL cursor via enigo
```

**Core innovation**: Mouse use NEVER ends the session. Takeover requires a commanded target (in-flight suppresses; settled deviation = task abort; no commanded target → motion never yields).

---

## Competitive Landscape: AI Desktop Control

| System | Mechanism | Vision | Safety | Platform |
|--------|-----------|--------|--------|----------|
| **NEXUS Ghost Mode** | Stage overlay + ring + takeover | UIA-first | Esc panic, blackout, kill-switch | Windows |
| **Open Interpreter OS Mode** | GPT-4o vision + code exec | GPT-4o vision | Sandboxing | Cross-platform |
| **AI Desktop (FareedKhan)** | OmniParser icon detection + VLM | VLM | Permission system | Cross-platform |
| **Everywhere (Sylinko)** | Screen context + MCP tools | Screen awareness | Per-action permissions | Cross-platform |
| **PyGPT** | Computer Use mode | Vision + agent loop | Confirmation gates | Cross-platform |
| **NVIDIA G-Assist** | Local SLM + system APIs | GPU-accelerated | Hardware optimization | NVIDIA only |
| **Guidy** | Real-time step-by-step | Screen parsing | Context-aware | Commercial |

---

## What NEXUS Does Better

1. **Pure takeover detector**: Unlike Open Interpreter or AI Desktop, NEXUS doesn't use a vision loop. The takeover detector is pure — it only judges deviation from a commanded target. No screenshot → VLM → action loop means lower latency and no hallucination from vision.

2. **Session vs task abort distinction**: NEXUS is the only system that distinguishes between "AI grabbed the cursor mid-glide" (task abort, session stays) and "user explicitly wants to stop" (session end). Open Interpreter and AI Desktop have no such distinction.

3. **Dynamic Esc**: Esc is registered only during the session and released on exit. It never steals Escape globally. Other systems either have static global shortcuts or no escape mechanism.

4. **Blackout watchdog**: 2s cadence monitoring, 8s cold-boot grace, 3 failed fixes → stay hidden. No other system has this level of self-recovery monitoring.

5. **Kill-switch Ctrl+Alt+X**: Destroy stage + disable session in one action. No other system has a dedicated kill-switch.

6. **Hitbox click-through**: 30ms cursor poll toggling ignore_cursor_events around frontend-sent rects. Per-pixel-alpha hit-testing exists in neither Tauri nor Electron — this is the documented workaround.

---

## What NEXUS is Missing (Vision)

**The biggest gap**: No vision/screen parsing. All competitors that do desktop control use vision:
- Open Interpreter OS Mode: GPT-4o vision
- AI Desktop: OmniParser icon detection + VLM
- Everywhere: Screen context + MCP tools
- PyGPT: Computer Use mode with vision

**Recommendation**: Add screenshot → VLM pipeline. Use Groq's vision API (already has API key) or local VLM. This would unlock:
- Clicking on visual elements UIA can't see (custom buttons, games, legacy apps)
- Reading screen content (OCR for verification)
- Verifying visual state before/after actions

---

## Architecture Comparison: Stage System

| System | Overlay | Transparency | Click-through | Monitoring |
|--------|---------|-------------|---------------|------------|
| **NEXUS stage** | Fullscreen transparent | WS_EX_TOOLWINDOW | 30ms cursor poll | Blackout watchdog |
| **Open Interpreter** | Browser overlay | CSS | N/A | None |
| **AI Desktop** | Screen capture | VLM-based | Permission gates | Permission system |
| **Home Assistant** | None | N/A | N/A | None |

NEXUS's stage system is unique: a fullscreen transparent overlay that doesn't pause video underneath (WS_EX_TOOLWINDOW), with click-through hitboxes and a blackout watchdog monitoring its own existence.

---

## Safety Systems (Unique to NEXUS)

1. **Takeover detection with in-flight suppression**: AI's own glide is never judged as human takeover.
2. **Task-abort vs session-end**: Grab mid-glide aborts the task, not the session.
3. **Dynamic Esc**: Never steals Escape globally.
4. **Blackout watchdog**: Self-monitors its own existence.
5. **Kill-switch**: Ctrl+Alt+X destroys stage instantly.
6. **Silent stand-down**: Stage hide/kill → session disarm → no ghost behavior.
7. **Follow-up queue**: Cap/drop-oldest with stop-word intercept.
8. **Stop-word abort**: Raw match for bare "cancel" parsed as task-abort, not session-end.

---

## References
- Open Interpreter OS Mode (openinterpreter/openinterpreter)
- AI Desktop (FareedKhan)
- Everywhere (Sylinko)
- PyGPT (oyamex/PyGPT)
- NVIDIA G-Assist
- Home Assistant voice pipeline
- docs/features/64-ghost-mode-plan.md, docs/features/70-ghost-mode-complete.md
