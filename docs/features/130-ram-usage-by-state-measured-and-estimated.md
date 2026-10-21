# 130 — NEXUS RAM usage in every state (measured today + recorded + estimated)

Date: 2026-10-16. Machine: HP laptop, i7-1355U (10 cores / 12 threads), 15.7 GB RAM. Build: release `nexus.exe` built 2026-10-16 18:18, running since 21:24 (PID 12820), idle for hours.
Legend — **M** = measured today on this PC, **R** = recorded in AGENTS.md by an earlier session (older architecture), **E** = estimate (not measured; stated basis).

## 0. Which "RAM" number? (read first)
Windows has three numbers and they differ a lot:
* **Commit / Private Bytes** = memory the process has reserved for itself. Best number for "will this fit?".
* **Working Set (WS)** = what is physically in RAM right now, including shared DLL pages. This is what Task Manager's "Memory" column roughly shows for a busy app.
* **Private Working Set** = resident AND not shared. Task Manager's default column. NEXUS runs in Windows EcoQoS + WebView2 "low memory" mode, so after hours idle Windows has **trimmed** it: nexus.exe shows only **16.5 MB** private WS while its commit is **69.6 MB** and its peak WS was **165.6 MB**. Right after activity the resident number climbs back.
All totals below use **Commit** (safe) and **WS** (resident); Task Manager will show less when idle.

## 1. Measured today — the idle process tree (state: stage overlay up, wake word listening, nothing happening)
| Process (all children of nexus.exe) | Commit MB | WS MB | Private WS MB | Src |
|---|---|---|---|---|
| nexus.exe (Rust core: wake engine, audio, STT/TTS clients, stock-line cache, orchestrator) | 69.6 | 94.1 | 16.5 | M |
| WebView2 browser process | 39.2 | 51.0 | 11.3 | M |
| WebView2 renderer (the stage page: orb + captions) | 51.6 | 46.2 | 26.9 | M |
| WebView2 GPU process (WebGL orb) | 82.7 | 12.4 | 4.5 | M |
| WebView2 utility x2 | 20.1 | 25.1 | 4.5 | M |
| WebView2 crashpad | 3.4 | 3.7 | 0.7 | M |
| conhost.exe | ~1–2 | 3.1 | 0.4 | M |
| **TOTAL idle** | **268.9** | **235.6** | **64.8** | M |
Peak seen by nexus.exe since start: WS 165.6 MB, commit 81.2 MB (M).

## 2. Measured today — Kokoro (offline voice) on this laptop
Test: `dump_wavs_for_asr` (loads the int8 `model_quantized.onnx`, 88.1 MB file, 5 voices x 5 sentences), sampled every 200 ms, while NEXUS and other apps were running. **Debug build** (so speed is pessimistic).
| Measurement | Value | Src |
|---|---|---|
| Load: first second | WS 160 MB / commit 128 MB | M (matches the recorded "+128 MB at load") |
| First synthesis done (8.7 s in) | WS 358 MB / commit 356 MB | M |
| Steady after many sentences / voice swaps | WS 360–376 MB / commit 357 MB | M |
| **Peak** | **WS 395.6 MB / commit 358.8 MB** (includes ~15–20 MB test-harness overhead) | M |
| Speed (debug build, CPU contention): wall / audio | **2.6x slower than real time** (af_heart 37.3 s for 14.3 s of audio; bm_george 42.4 s for 16.2 s) | M — **not** the final number; the release figure recorded earlier was RTF 0.7–1.7 (R). Re-measure in release before deciding. |
Takeaway: **Kokoro adds ~340–360 MB commit / ~360–400 MB resident** and nothing else is needed to run it (no Python, no espeak).

## 3. Component costs (what each feature adds on top of idle)
| Component | Adds (commit MB) | Adds (WS MB) | Src / basis |
|---|---|---|---|
| NLU in-process model (files 16.8 + 16.8 MB; loads on the first parse miss, then stays) | +60–100 | +50–90 | E (file sizes + typical ONNX overhead) |
| Capturing speech (16 kHz f32 = 0.64 MB per 10 s) + orb animating + upload buffers | +10–30 | +15–30 | E |
| Thinking (cloud call, HTTP buffers) | +5–10 | +5–10 | E |
| Speaking via Edge (decoded 24 kHz f32 = 96 KB per second of speech; 30 s reply = 2.9 MB) + 29 stock lines already in RAM (~4–5 MB, inside idle) | +5–15 | +5–15 | E (computed) |
| **Speaking via Kokoro (model resident)** | **+340–360** | **+360–400** | **M** |
| Hand mode (extra WebView2 window + MediaPipe 7.5 MB model + 9.1 MB wasm + camera frames) | +150–300 | +150–300 | E (not measured; camera/MediaPipe not started by me) |
| Ghost mode (UI Automation caches, hot-mic loop, ring overlay; no new process) | +10–30 | +10–30 | E |
| One extra WebView2 window (Command Hub / Settings / Sidebar / Architecture mapper) | +100–250 | +100–250 | R (about 250 MB per window recorded 2026-09-01 before low-memory mode) / E |
| Agent-watch Node sidecar (node + node-pty) | +40–120 | +30–50 | E (analog: node processes here use 32–49 MB WS, 21–114 MB commit) |
| Local STT fallback (Python + Moonshine, only when Groq is unreachable) | +130 loaded / +340 active (medium model ~400) | same | R (AGENTS.md, 2026-09-01) |
| Smart-turn / Silero VAD | 0 today (disabled) | 0 | log: "disabled" |

## 4. State totals — CURRENT build (Edge cloud voice is primary; Kokoro only offline)
"Warm" = after the first spoken turn, when the NLU model is resident (+60–100).
| State | Commit MB | Resident (WS) MB | % of an 8 GB PC (commit) | Src |
|---|---|---|---|---|
| 1. Idle, cold (measured) | **269** | **236** | 3.3 % | M |
| 2. Idle, warm (NLU loaded) | 330–370 | 285–325 | 4–4.5 % | M + E |
| 3. Listening / capturing | 345–400 | 300–355 | 4.3–4.9 % | E |
| 4. Thinking (cloud) | 335–380 | 290–335 | 4.1–4.6 % | E |
| 5. Speaking via Edge | 335–385 | 290–340 | 4.1–4.7 % | E |
| 6. Ghost mode active | 340–400 | 295–355 | 4.2–4.9 % | E |
| 7. Hand mode on | 480–670 | 435–625 | 5.9–8.2 % | E |
| 8. Command Hub / Settings window open | 430–620 | 385–575 | 5.3–7.6 % | E/R |
| 9. Offline: Kokoro loaded and speaking | **670–730** | **650–730** | 8.2–8.9 % | M + E |
| 10. Offline: Kokoro + Python STT fallback active | 1010–1070 | 990–1070 | 12.3–13 % | M + R |
| 11. "Everything on" (warm + Kokoro + hand + Hub open + agent watch) | **1.1–1.5 GB** | 1.1–1.5 GB | 13–18 % | E |

## 5. State totals — PLANNED design (docs/features/129: Kokoro is the ONE voice, no Edge)
| State | Commit MB | Resident MB | Note |
|---|---|---|---|
| Idle, warm, Kokoro unloaded (after the idle timer) | 330–370 | 285–325 | same as current warm idle; the voice loads on demand (~1–2 s first line, hidden by the stock-line clip bank) |
| Idle, warm, Kokoro resident | **670–730** | 650–720 | what a user sees for the first minutes after speaking |
| Speaking (Kokoro) | 680–750 | 660–740 | + 1–2 MB per spoken sentence of buffers |
| Hand mode + Kokoro resident | 830–1030 | 800–1020 | |
| Everything on | 1.4–1.8 GB | 1.4–1.8 GB | |
Reduction levers (T1 in doc 129): unload Kokoro after N idle minutes; fewer ORT threads (smaller arenas); ORT memory-arena settings; keep only the active voice file; close the Hub/Settings window (destroys its WebView2, already the app's policy); reuse the stage WebView2 for hand mode instead of a second window (E: saves 100–200 MB).

## 6. What is NOT measured (be careful with these)
Rows marked E are reasoned estimates: NLU resident size, hand mode, ghost mode, the extra-window cost under the current WebView2 low-memory setting, agent-watch sidecar. The two big unknowns for the product decision are **hand mode** and **extra windows**; both can be measured by opening them while this sampler runs (script: `scratchpad/measure_kokoro.ps1` pattern; I did not start the camera, the mouse control or extra windows on your PC without you).
The Kokoro speed number is a **debug-build** figure under load; do not use it as the final latency.
The 8 GB % column assumes the same numbers on an 8 GB machine; Windows itself plus a browser typically use 3–4 GB of it.
Older recorded numbers (idle 104–232 MB; Kokoro +350 MB) come from the pre-stage-overlay architecture and are superseded by section 1.
