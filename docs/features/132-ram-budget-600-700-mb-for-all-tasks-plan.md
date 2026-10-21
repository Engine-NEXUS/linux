# 132 — Keep NEXUS at 600-700 MB resident with EVERYTHING on, without removing a feature (PLAN, nothing built)

Date: 2026-10-16. Requirement (owner): *total RAM must stay 700 -> 600 MB or below for all tasks, keep every feature.*
Builds on `130` (RAM per state) and `131` (Kokoro breakdown). New measurement today: **the NLU model costs only +42 MB** (not the 60-100 MB I had estimated).

---

## 0. Definitions (so the target is testable)
* **Scope**: the whole NEXUS tree = `nexus.exe` + its WebView2 processes + any sidecar NEXUS starts (Node agent-watch, optional Python STT).
* **Metric**: **resident working set (WS)** of that tree. Commit/private bytes run ~35 MB higher at idle (WS 236 vs commit 269 MB measured). Task Manager's default column (private working set) is lower still (65 MB idle) because Windows trims an idle process.
* **Targets**: **hard cap 700 MB** with every feature active at once; **soft target 600-650 MB**. Typical day (idle/voice/ghost): **<= 350 MB**.

## 1. Where we are (M = measured today, E = estimate)
| Block | Resident MB today | Src |
|---|---|---|
| nexus.exe (Rust core) | 94 idle (peak seen 166) | M |
| Stage WebView2 tree (6 processes) | 138 | M |
| conhost (dev launch only) | 3 | M |
| **Always-on base** | **236 idle / ~260 warm** | M |
| NLU (tract, BERT-mini) | **+42** (load 662 ms once; dropping it returns only ~23) | M |
| Voice (Kokoro int8) speaking | **+368** (default) / +280 (ONNX arena+pattern off) | M |
| Hand mode (extra window + MediaPipe) | +150-300 | E |
| Hub / Settings / Sidebar window | +100-250 | E |
| Agent-watch Node sidecar | +30-50 | E |
| Ghost mode | +10-30 | E |
| Offline STT (Python Moonshine) | +130 loaded / +340 active | R (older) |
| **Everything on, today** | **~1.2-1.6 GB** | M + E |
So to meet the cap with everything on, we must cut roughly **500-900 MB** and make sure it can never grow back.

## 2. Component budget (the contract)
| Component | Cap (resident MB) | How it gets there | Confidence |
|---|---|---|---|
| nexus.exe Rust core | 120 | profile the heap (dhat), stock-line cache f32 -> i16, trim registries; EcoQoS stays | E, low risk |
| Stage WebView2 tree | 125 | release the orb's WebGL context when hidden > 30 s (recreate on wake word), try `--in-process-gpu` and a smaller JS heap flag **only if measured stable** | E, needs measurement |
| NLU | 40 -> **0 when idle** | unload after ~5 min idle (model 42 MB; reload 0.7 s on the next miss); int8 quantise to cut the file to ~17 MB | **M** (42 MB) |
| Voice (Kokoro) | **175 speaking / 0 idle** | R1 ONNX arena+pattern off (-88, **M**), R2 compact pronunciation dictionary (-100..-115, **M** size), idle unload 60-120 s (returns ~300, **M**); stretch R3 ORT-format model (-40..-90, E) -> ~110-135 | R1/R2 measured pieces; R3 estimate |
| Hand mode | 100 | host MediaPipe in the existing WebView2 environment / a worker of the stage page instead of a second heavy renderer; stop the camera pipeline when disarmed for > 60 s (the 1-min idle exit already exists) | E, **must measure** |
| Hub / Settings / Sidebar window | 90 | already destroyed on close; add WebView2 `TrySuspend()` + low-memory target when the window loses focus > 60 s; lazy-load heavy pages | E, **must measure** |
| Agent-watch sidecar | 45 | lazy start (exists) + kill on idle (exists); cap Node heap `--max-old-space-size=48` | E |
| Ghost mode | 20 | none needed | E |
| Offline STT | 100 (replaces the 130-340 Python one) | in-process sherpa-onnx Moonshine-tiny / Whisper-tiny.en (27 M / 39 M parameters, Q8), loaded only when Groq is unreachable, unloaded after 60 s | E, **must measure** (no public RAM figure found) |

### Worst case arithmetic (everything at once)
| Scenario | Sum of caps | Fits 700? |
|---|---|---|
| Base 260 + voice speaking 175 + hand 100 + hub 90 + agent 45 + ghost 20 | **690** | yes, barely (NLU evicted) |
| + NLU resident 40 | 730 | **no** -> the governor evicts NLU first |
| Offline: base 260 + STT 100 (voice and STT alternate; the mic is muted while speaking) + hand 100 + hub 90 + agent 45 + ghost 20 | 615 (STT) / 690 (voice turn) | yes |
| Same with stretch items (voice 120, base 230) | ~600-630 | meets the 600-650 soft target |
**Honest verdict:** 700 MB with everything on is **reachable but tight**; 600 needs the stretch items (R3, base trim, hand/hub caps). The two components I have not measured (hand, hub) decide it, so measuring them is step one.

## 3. The guarantee: a RAM governor (so the cap holds even when features pile up)
New module `ram_budget.rs` (Rust):
* **Sampler** every 5 s: resident WS of the NEXUS tree (self via `GetProcessMemoryInfo`; children via a cheap child-PID scan). Logs `[RAM] total=… core=… webview=… voice=… nlu=… hand=… hub=…`.
* **Registry**: each heavy component registers `{name, resident_estimate, priority, last_used, in_use(), evict()}`.
* **Soft limit 600 / hard limit 700** with hysteresis. Above soft: evict the cheapest-to-reload idle component first. Above hard: also suspend windows and stop non-essential background work. Eviction order: (1) NLU, (2) voice engine if not speaking, (3) offline STT if not capturing, (4) orb GPU context, (5) hidden/unfocused windows (suspend), (6) hand-mode camera if disarmed, (7) agent-watch sidecar if finished, (8) stock-line cache trim.
* **Admission control**: before loading something heavy, check the budget and evict others first (never fails a user action; it only adds a reload delay of 0.7-2 s).
* **User control**: Command Hub "Memory" card with a live meter and a profile: *Low (<= 450)*, *Balanced (<= 650, default)*, *Generous (<= 1000)*. Features are never switched off; only residency changes.
* **No OS hard limits** (a job-object memory limit would kill the app instead of degrading).

## 4. Plan and gates (each step ends with a measurement, no estimates left behind)
| Step | Work | Days | Gate |
|---|---|---|---|
| P0 | **Measure every remaining row** while you open hand mode, the Hub window, ghost mode, agent-watch, plus the offline STT and a release-build Kokoro. Add the `[RAM]` sampler first (read-only) so every later step is checked by the same numbers. | 0.5-1 (needs you at the PC for ~10 min) | table with M for all rows |
| P1 | Quick wins: voice ONNX settings (R1), voice idle unload tuning (R4), NLU idle unload, stock-cache i16, orb GPU release | 1 | all-on drops by >= 150 MB; first-audio latency unchanged within +10 % |
| P2 | Compact pronunciation dictionary (R2) with golden-corpus equality test | 1.5 | voice speaking <= 200 MB; 20 000-sentence phoneme equality 100 % |
| P3 | Governor + Hub "Memory" card + `ram_status` IPC | 2 | forced test: start every feature, total never exceeds 700 for 10 min |
| P4 | Conditional items from P0 data: hand-mode hosting, hub suspend, offline STT replacement, ORT-format model | 3 | each must show a measured saving, otherwise dropped |
| P5 | WebView2 flag experiments (in-process GPU, JS heap cap) one at a time | 0.5-1 | stable for a full day + no stage blackout (watchdog) |
| P6 | Ratchet: `scripts/baselines/ram-budget.json` + ignored tests (`mem_breakdown`, `mem_nlu_load_parse_unload`) + `scripts/ram-budget.ps1` scenario script; a regression above the cap fails the check | 0.5 | gate wired into the verify checklist |
Total about 9-10 days of work for the full target; **P0+P1+P2 (about 3 days) should already bring "everything on" below ~900 MB and typical use below ~350 MB.**

## 5. Features at risk of becoming slower (not removed)
* First sentence after the voice was unloaded: +1-2 s -> hidden by the clip bank of stock lines.
* First command after the NLU was unloaded: +0.7 s once.
* Offline turn that needs STT after the voice spoke: +~1 s while models alternate.
* Hand mode restart after eviction: ~1 s camera/model warm-up (the 1-min idle exit already requires that today).

## 6. Decisions needed
1. Confirm the metric: **resident working set of the whole NEXUS tree**, hard cap **700**, soft target **600-650**?
2. Is a **0.7-2 s reload delay** acceptable in the rare "everything on" situation instead of removing anything?
3. Default profile **Balanced (650)** with a Low-RAM profile for 8 GB PCs?
4. P0 needs about 10 minutes of you opening hand mode, the Hub, and ghost mode while a sampler runs: when?

## 7. Not verified
Hand, Hub, ghost, agent-watch, offline-STT rows are still estimates. Kokoro speeds are debug-build figures. WebView2 flag effects, `TrySuspend` savings and the sherpa-onnx footprint are unmeasured. "Everything on" has never been run on this PC. The NLU number (+42 MB, first parse 662 ms, ~37 ms afterwards, only ~23 MB returned on drop) is one run in a unit-test process (~14 MB baseline).

---

# Addendum (same day) — no hard-coded cap: how far can it compress WITHOUT touching speed or features, and is "below 700" possible?

The owner clarified: 700 is a goal, not a number to hard-code; the question is how much can be compressed with no loss of features, speed or response. So the plan is re-grouped by what each saving costs.

## A. Free compression (no speed, no response, no feature impact)
| Item | Saves (resident) | Why it is free | Evidence |
|---|---|---|---|
| Compact pronunciation dictionary (binary, memory-mapped, same phonemes) | **-100..-115 MB** | removes the start-up JSON parse, so the voice also LOADS faster; lookups are the same speed or faster | **M** (dictionary = ~125 MB) |
| ORT-format model used in place (no second copy of the 88 MB weights) | -40..-90 MB | load becomes faster | E |
| Stock-line cache f32 -> i16, Rust heap trim | -10..-20 MB | none | E |
| Orb WebGL context released while hidden, recreated by the wake word | -5..-10 MB resident (-30..-50 commit) | hidden by the wake animation | E |
| Node heap cap for agent-watch | -10..-20 MB | none | E |
| **Subtotal** | **about -170..-250 MB** | | |

## B. Nearly free, must be measured first (small possible speed cost)
| Item | Saves | Risk |
|---|---|---|
| ONNX CPU arena off + memory-pattern off | **-88 MB (M)** | possible few-% slower synthesis; the pinned `ort` exposes only arena on/off for the CPU provider (no "grow by exact amount" option), so release-build timing decides. Reject if first audio gets > 10 % slower. |
| Fewer ONNX threads (2-4 instead of all 12) | ~0..-10 MB, frees CPU | slower on big sentences; measured noisy in debug, re-test in release |

## C. Costs a reload, but the reload can be hidden by loading earlier (pre-warm)
| Item | Saves | How the delay disappears |
|---|---|---|
| Unload NLU when idle (0.7 s to reload) | -42 MB | load it on the **wake word** (the user speaks for >= 1-2 s before the transcript arrives) |
| Unload the voice when idle (reload ~1 s today; shorter after A) | -300 MB when idle | load it when the transcript arrives / while the cloud model is thinking (0.6-1.5 s); the stock-line clip bank answers instantly meanwhile |
| Hand-mode / Hub / STT restructuring (host in the existing WebView2, suspend unfocused window, in-process fallback STT) | E: -250..-450 MB when those features are on | no per-turn delay; Hub reopen ~ the same as today |
These need measuring because I have no number for hand mode, the Hub window or the offline STT.

## D. Not recommended (would cost quality or speed)
Smaller/weaker voice model; fewer orb particles; killing WebView2 or the stage window; lowering wake-word model size; forcing Windows to hard-trim the working set (it just makes the next action page-fault and feel slower).

## E. Resulting memory (resident, E-based except where stated)
| State | Today | After A only | After A + B | After A + B + C |
|---|---|---|---|---|
| Idle, nothing loaded | 236 (M) | ~225 | ~225 | ~225 |
| Warm idle (NLU resident) | ~280 | ~270 | ~270 | ~230 (NLU unloaded, pre-warmed on wake) |
| **Speaking (voice + NLU)** | **~650** | **~470-510** | **~380-420** | **~340-380** |
| Everything on at once | ~1.2-1.6 GB | ~1.0-1.4 GB | ~0.9-1.3 GB | **~0.65-0.85 GB** |

## F. The straight answer to "is below 700 MB possible?"
* **Day-to-day use (idle, listening, thinking, speaking, ghost, even with the Hub open): YES, with high confidence.** Speaking lands around 340-510 MB depending on how many of the optional steps are taken; the measured pieces (dictionary 125 MB, ONNX arena 88 MB, unload returning 304 MB, NLU 42 MB) already add up to that.
* **EVERYTHING on at the same moment (voice + hand mode + Hub + agent-watch + ghost + offline STT): POSSIBLE, NOT YET PROVEN.** My estimate is 650-850 MB, so it could land just under or just over 700. It depends on three numbers I have not measured (hand mode, Hub window, offline STT). If hand mode and the Hub come in at the hoped 100 + 90 MB it fits; if they stay at today's 150-300 + 100-250 it does not, and the governor would have to evict idle parts (reload delay) to stay under.
* **Everything on AND under 600 MB: not promised.** It needs the stretch items (ORT-format model, a smaller base, hand/Hub hosting) all to work.
* **Without any slowdown:** group A is truly free. Group B is free only if release timing confirms it. Group C is free only because it is loaded earlier than needed. Anything that fails its gate (first audio +10 %, command latency +50 ms, no quality change in the golden phoneme set) is dropped, not shipped.
