# 134 — Edge first, local always ready, switch back only when the connection is proven; fast conversation, patient tasks (PLAN, nothing built)

Date: 2026-10-17. Owner policy (binding):
1. **Conversation** (talking to NEXUS) must be fast, no perceptible delay.
2. **Tasks** may think and take time; a delay is understandable.
3. **Edge TTS is the first priority.** Local voice is used **only** when there is no internet. When the internet returns, **keep local preserved** until the connection is proven good, then move back to online. If the internet suddenly drops, local is **already ready** and takes over **without delay**.
This replaces the "Kokoro everywhere" direction of docs 127-129 and the voice part of 131/132. Identical-voice-cloud-and-local is no longer a requirement; the design below keeps the online (Edge) voice for as many offline lines as possible so most offline speech still sounds identical anyway (section 4).

---

## 1. What exists today (verified in code) and what is missing
| Piece | Today | Gap against the policy |
|---|---|---|
| Edge first, local second | Yes (`tts.rs`, `tts_network.rs`) | OK |
| Network monitor | polls every **60 s online / 30 s offline**, 3 s probe (`tts_network.rs`) | Detects a drop up to 60 s late; relies on the next failed request |
| First failing line | waits **4 s** for Edge (`EDGE_FIRST_AUDIO_TIMEOUT_SECS`), then local | Up to 4 s of dead air on the first line after a sudden drop |
| Local voice when online | **not loaded** (lazy); unloaded after 10 min idle | Sudden drop -> cold local, so today the line is spoken by the Windows voice while Kokoro loads (my 2026-10-15 safety net) |
| Switch back | `set_network_up()` after a successful probe; local kept loaded 10 min (`LOCAL_GRACE_PERIOD`) | One good 3 s probe is enough to move back; no stability proof, can flap |
| Offline STT (Groq unreachable) | Python Moonshine fallback, `STT_KEEP_ALIVE = false` (killed after 60 s idle) | **Cold start 10-15 s** (comment in `lazy_stt.rs`). With no internet, speech recognition, not the voice, is the biggest delay |
| Offline command path | deterministic grammar + in-process NLU (no cloud) | Works offline already |
Important fact: **with no internet there is also no cloud LLM and no Groq STT.** Offline "conversation" = local commands + local STT + a short spoken acknowledgement. So the offline speech workload is small and mostly repeatable (acks, confirmations), which makes a pre-rendered bank very effective.

## 2. Two latency lanes
| Lane | What | Budget | How it is protected |
|---|---|---|---|
| **Conversation** | greetings, answers, acks, follow-ups, questions | first sound <= 0.3 s for acks/stock lines; first audio of a spoken reply p50 <= 1.8 s, p95 <= 2.5 s (today ~2.6 s, measured from your 15:55 log: STT 0.32 + parse 0.29 + cloud 0.61 + Edge 1.28) | see section 3 |
| **Task** | Ghost actions, multi-step, PR work, long research | thinking time allowed; **never dead air**: an immediate stock ack (<= 0.3 s) + an earcon, then one completion line when verified | acks come from the bank; GPT-sol's receipt wording is read, not edited |

## 3. Making the conversation lane faster (online, all speed-neutral or faster)
| # | Change | Expected gain | Evidence |
|---|---|---|---|
| C1 | "Working on it sir." etc. become **stock lines** (today it is synthesized live, takes 0.84 s, then **discarded** because the answer arrives first: log "stop requested during synthesis, skipping playback") | the ack is actually heard at ~0.3 s instead of never | M (log) |
| C2 | **Pre-warm at the wake word**: load NLU (removes 0.29 s first-turn load), open the Edge WebSocket/TLS connection | -0.3 s NLU first turn, -0.3..0.5 s connection | NLU M; connection E |
| C3 | **Stream the cloud answer into speech sentence by sentence** (speak sentence 1 while the rest is still generating) | -0.5 s typical | E |
| C4 | **Short first chunk** (<= 6 words) and persist the Edge WebSocket across chunks | -0.2..0.4 s | E |
| C5 | Hedge/adaptive timeout instead of a flat 4 s Edge wait (section 5) | caps the bad case | E |
Target after C1-C5: reply first-audio p50 ~1.4-1.8 s (E; measure before claiming).

## 4. The bank: why most offline lines can still be the Edge voice
* **Learned bank**: every line Edge speaks is stored on disk as the **original MP3 + word boundaries** (about 6 KB per second of speech; 1 000 lines of ~4 s = ~24 MB), keyed by `persona voice + prosody + text`. LRU, capped (default 200 MB).
* **Pre-render while online** (paced, 400 ms between requests, stop at the first 429 - lessons from 2026-10-15): all stock lines, Ghost receipt phrases (read from GPT-sol's outcome strings by a test), power/boot/shutdown lines, and "Opening {app}, sir." for the apps you actually use (usage-ranked).
* Offline, a banked line plays in **~10-40 ms in the exact Edge voice**. Only a **never-spoken-before sentence** needs the local Kokoro voice.
* Measure the coverage (share of offline lines served from the bank) from real use; target >= 80 % for local-command acks.

## 5. Connectivity state machine (the part that makes switching safe)
States: `ONLINE_STABLE` -> `ONLINE_DEGRADED` -> `OFFLINE` -> `RECOVERING` -> `ONLINE_STABLE`.
Signals, combined (any one can move to a worse state; recovery needs all):
1. **OS events**: Windows network-connectivity-change notifications (instant on cable/Wi-Fi drop).
2. **Request outcomes**: every Edge / Groq / 9Router call reports success, error, latency (free, zero extra traffic).
3. **Lightweight probe**: every 10-15 s while not STABLE, 60 s while STABLE (HTTPS to the Edge speech host, the same host that matters).
Rules:
* **Drop -> OFFLINE** on: OS says disconnected, OR 2 failures within 20 s, OR a first-audio wait beyond the adaptive limit with the probe failing. Local becomes active **for the next utterance** (never mid-utterance).
* **Hedge (the "sudden" case)**: if Edge has not delivered first audio by `max(2.0 s, 1.5 x recent p95)` and local is warm, start the local render and speak it for **this one line**. Worst case on the very first line after an upstream outage with no OS event: ~2 s + local time, once; every later line is immediate.
* **Recovery -> RECOVERING** when the OS reports connected. **Local stays the active voice** and stays loaded. Move back to `ONLINE_STABLE` only after ALL of: >= 3 consecutive successful probes spaced >= 10 s apart, one **canary synthesis** (1 word through Edge) succeeds with first-audio < 1.5 s, and >= 60 s have passed since reconnect. Switch happens at the **next utterance boundary**.
* **Anti-flap**: minimum dwell 30 s in OFFLINE and in RECOVERING; after 2 relapses in 10 min the recovery window doubles (up to 10 min).
* **After switching back**: local stays loaded for a **relapse window of 10 min** (as the code does today), then follows the standby mode below.
* Everything logged: `[NET] state=RECOVERING reason=... probes=2/3 canary=pending`.

## 6. "Local is already ready" - what that costs, honestly
Ready means three things must be warm, not just the voice:
| Piece | Resident when ready | Today when cold | Src |
|---|---|---|---|
| Local voice (Kokoro, model + dictionary) | ~380 MB now; ~270 MB after the compact dictionary | 1-2 s load, and 2.4x-slower-than-real-time synthesis on this laptop on battery (so novel sentences take ~4 s; banked ones are instant) | M / E |
| Offline STT (Moonshine) | ~130 MB loaded (older record) or ~100 MB with an in-process engine (estimate) | **10-15 s cold start** | R / E |
| NLU (offline command parsing) | +42 MB | 0.7 s | M |
**Standby modes (setting "Instant offline fallback"):**
| Mode | Idle RAM while online | Sudden internet loss | Who it suits |
|---|---|---|---|
| **Always ready** (recommended on >= 12 GB PCs, matches your wish) | ~280 + 270 + 100..130 + 42 = **~690-720 MB** | no extra delay except the one hedged line | default |
| **Smart** (recommended on 8 GB PCs) | ~280 typical; goes to ~690 when the link looks unstable, for 10 min after any recovery, and for the first 2 min after start | immediate when predicted (OS event, degraded signals); otherwise one 1-3 s hiccup | low-RAM PCs |
| **Save RAM** | ~280 | 2-4 s voice + **10-15 s STT** (today's behaviour) | not recommended |
Trade-off to decide: **instant offline readiness and a 600-700 MB ceiling cannot both hold with everything else on.** With Always ready the idle total is already ~700 MB; with hand mode, Hub window, agent-watch and ghost on it reaches ~1 GB. The memory manager (doc 132) evicts in this order under pressure: NLU, hub/hand extras, GPU context, **then** the standby engines last.

## 7. Plan, gates and ownership
| Step | Work | Days | Gate |
|---|---|---|---|
| P0 | Add per-turn `[LAT]` stage timing (speech end -> transcript -> parse -> LLM -> first audio) and `[NET]` logging; 20-turn baseline | 0.5 | baseline table (today ~2.6 s) |
| P1 | C1-C2: stock acks, wake-time pre-warm (NLU + Edge connection) | 1 | ack heard <= 0.3 s; first-turn parse 0.29 s -> ~0 |
| P2 | Connectivity state machine + hedge + canary + hysteresis (`connectivity.rs`, hooks in `tts_network.rs`/`tts.rs`; fake-clock tests + fault injection: drop mid-synthesis, flapping, captive portal, DNS up/HTTPS down, slow link) | 2-3 | chaos test: 30 min of random drops, zero mid-utterance switches, zero flaps, first-line worst case <= hedge + local time |
| P3 | Learned bank + paced background pre-render + coverage metric | 1.5 | offline replay of banked line <= 50 ms, Edge voice; coverage report |
| P4 | Local standby: keep Kokoro ready (mode setting), **remove the Windows voice from the chain** except "Kokoro not installed yet", offline STT readiness (in-process engine or warm-on-signal; measure RAM and WER) | 2-3 | switch to local for a novel line <= 150 ms when ready; offline STT first transcript <= 1.5 s |
| P5 | C3-C4: stream answers into speech, short first chunk, persistent connection (touches `orchestrator.rs`/`router.rs`: shared, narrow anchored hooks) | 2 | reply first-audio p50 <= 1.8 s with no extra mid-sentence gaps |
| P6 | Settings: "Instant offline fallback" mode, memory-manager hooks, `[RAM]` | 1 | all modes selectable, no Settings clobber (doc 128 S1) |
Ownership: ours = `tts*.rs`, `tts_network.rs`, `lazy_stt.rs` (re-read first: someone changed `STT_KEEP_ALIVE`), new `connectivity.rs`, bank files. Shared = `orchestrator.rs`, `lib.rs`, `commands.rs`: one-line hooks only. GPT-sol's Ghost files are not touched; their outcome phrases are read through a test.

## 8. Decisions needed
1. **Always ready** as the default on this PC (idle ~700 MB while online), accepting that "everything on" then exceeds 700 MB? Or Smart?
2. Is a one-time hedged delay (~2 s) acceptable only for the very first line after a silent upstream outage?
3. Recovery proof of ~60-90 s (3 probes + canary) before returning online: OK?
4. Allow the Edge voice to be saved on disk as MP3 (the bank, up to 200 MB, deletable in Settings, never leaves the PC)?
5. Should the offline speech recognizer be replaced by an in-process engine (removes the 10-15 s cold start and the Python install)? Needs one measurement first.

## 9. Not verified
The 2.6 s baseline is from one turn of your log. C2-C5 gains are estimates until P0 timing exists. Kokoro speed was measured on battery power (14 %, Balanced plan); on AC it may be 2-3x faster, which would shorten novel offline sentences. The in-process STT footprint and accuracy are unmeasured. Windows connectivity-event behaviour (Wi-Fi vs upstream outage) must be tested on the real network. Nothing here is built.
