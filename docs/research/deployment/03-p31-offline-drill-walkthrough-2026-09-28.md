# P3.1 Offline Drill — Local-Only Walkthrough (2026-09-28)

Deployment plan 02, P3.1. Manual drill + static walkthrough of the
full offline path (Worker down, Groq down, network down). Goal: no
silent hangs, every degradation spoken. Run the drill on-device
before each rollout ring; this file records the walkthrough and the
current gap list.

## 1. How to run the drill

```powershell
# Simulate down: disconnect Wi-Fi (or firewall-block
# nexus-worker.chitkullakshya.workers.dev + api.groq.com).
nexus start
# Then by voice, in order:
#   "open chrome"      (local — must work)
#   "close chrome"     (local)
#   "play" / "next song" (media)
#   "write this down" → dictation → "send it" skip (Ghostwriter local)
#   "remember that my wifi password is falcon" → "what is my wifi password"
#   "what's the capital of France"  (cloud — must speak the offline line)
#   "ghost mode" → "open whatsapp" → Esc   (drills are local)
#   "open settings"       (sidebar local)
# Reconnect → "what's the capital of France" must answer normally.
```

## 2. Stage-by-stage offline behavior (audited in code)

| Stage | Offline behavior | Verdict |
|---|---|---|
| Wake word | cpal + ONNX, fully local | works |
| STT | Groq fails → local Moonshine fallback (`stt.rs:67/115/162`); `localSttOnly` users never touch Groq | works (slower) |
| Deterministic parse | pure Rust, <1ms | works |
| ML fallback on miss | offline branch runs `parse_with_ml` (brain → BERT-Mini sidecar) (`orchestrator.rs:520-530`) | works (cold NLU ~10-15s first call) |
| Local commands (open/close/media/greeting/memory/specs) | no network anywhere (`execute_command`, memory.rs) | works |
| Ghostwriter dictation | local STT + local TTS | works |
| Ghost drills | local Win-search/UIA/registry + local TTS; confirm gates local | works |
| General questions / analysis / MCP / GitHub | Worker unreachable → `dispatch_to_worker` Err → **P3.1 fix: speaks the offline line** ("Network's down, sir — I've gone local…") instead of raw error JSON | **fixed this pass** |
| 9Router | offline → all providers fail fast (20s timeout) → falls to Worker → offline line | works, wasteful (see gap 1) |
| TTS | edge-tts fails → Piper local fallback (`tts_network.rs`) | works |
| Loading indicator | Rust hides it on the Err arm (`hide_loading` before Error emit) | works |

## 3. Gap list (from the walkthrough)

1. **9Router offline waste (P3.2 candidate).** When the network is
   down, `can_route` still fires a 20s-timeout Groq call before the
   Worker error. Fix candidate: check `tts_network::is_network_up()`
   before the 9Router attempt and skip straight to the offline line.
   Small latency win, no behavior change.
2. **NLU cold-start on first offline miss.** `parse_with_ml` →
   `lazy_nlu` spawns the Python sidecar on first use; offline-first
   turn pays a 10-15s model load, silently. Fix candidate: reuse the
   existing lazy-start logs + one spoken "one moment" if the first
   offline ML call exceeds 3s. Cosmetic; defer to a ring finding.
3. **No offline banner.** The Connections tab health row shows
   Worker Offline, but nothing during a session tells the user
   *before* they ask a cloud question. The spoken offline line now
   covers the "after" case; a proactive `vault:changed`-style event
   could cover the "before". Defer to P4 rings (network monitor
   already exists — `tts_network::start_network_monitor`).
4. **Ghost drill while fully offline.** Registry + Win-search paths
   verified local; WhatsApp bridge (:8765) is a LOCAL service —
   reachable offline. No gap found; drills actually *prefer* offline.
5. **Webhook triggers** (localhost :39220) work offline by design —
   no gap.

## 4. Acceptance (plan 02 P3.1 gate)

Offline run completes without wedges: every command either executes
locally or speaks the attributed offline line within
`120s timeout + 15s connect` worst case (typically <20s via the
9Router cascade timeouts). Loading indicator never survives the
error (hide-before-emit ordering, `orchestrator.rs` Err arm).
