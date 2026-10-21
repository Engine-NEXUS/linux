# 11 — P0 Baseline Report (Ghost reliability + UIA measurements), 2026-10-08

**Status:** measured. Companion to `docs/features/103-...` (plan), research docs 09 and 10.
**What was measured:** `src-tauri/dev_tools/uia_probe.rs` (new, read-only) on three Windows apps that were launched by the probe run itself
(Notepad on a scratch file, Calculator, File Explorer on `C:\Windows\System32\drivers\etc`) — **never the user's own windows**.
Raw data: `docs/research/ghost-mode/data/p0-2026-10-08/probe_*.json` (names recorded only for controls that report a shortcut/mnemonic).
**Caveats up front:** debug build; one machine; three Microsoft apps (WinUI / UWP / Win32-shell) — **no Chromium or Electron app was measured** (Brave, VS Code, WhatsApp need the user to run the probe, §5);
warm figures are the minimum of 5 repeats (spread ≤ 2 ms Notepad/Calculator, ≤ 8 ms Explorer). The run was repeated twice with matching results.

---

## 1. Headline: the current UIA lookup is slow *and* blind when focus is on a leaf element (M2, M3 — confirmed)

| App (focus at probe time) | Today: `screen::list_actionables()` | Controls it returned | One cached, window-rooted query | Actionable controls that exist | Speed-up |
|---|---:|---:|---:|---:|---:|
| Notepad (focus = **Document**, a leaf) | **5,094 ms** | **0** (recall 0 %) | 25 ms warm / 50 ms cold | 23 | ≈ 200× |
| Calculator (focus = **Text**, a leaf) | **5,069 ms** | **0** (recall 0 %) | 18 ms warm / 30 ms cold | 35 | ≈ 290× |
| File Explorer (focus = the **Window**) | 2,456 ms | 64 (recall 97 %) | 76 ms warm / 79 ms cold | 68 | ≈ 32× |

**Mechanism (confirmed from the numbers, matches the code reading in doc 09 §1.3):** `list_actionables` roots its search at `get_focused_element()`.
When that element is a leaf (a text box, document, display) its subtree is empty, so all 10 per-control-type queries find nothing and each burns its 500 ms timeout
(10 × ≈ 509 ms = the 5.07–5.09 s seen, to within 1 %). When focus is on the window itself (Explorer) the queries succeed but are still 10 sequential round trips.
**Consequence for Ghost clicks:** in 2 of 3 apps today's click path gets *no UIA answer after 5 s* and falls through to cloud vision (seconds more, quota-limited, needs a key).
This is very likely the "5–6 s to click the search bar" symptom from change 99; it is a **bug, not a tuning problem**.

One cached window-rooted `FindAllBuildCache` returned the full set in 18–79 ms, with the cached-property read costing < 1 ms for 47–137 elements.

### 1.1 After the fix (E0.5, applied 2026-10-08, same day)
`screen::list_actionables()` now runs ONE cached query rooted at the **foreground window**, server-side filtered to the clickable control types, with a 2.5 s hard budget; the old code is kept as `list_actionables_legacy()` (automatic fallback only if UIA cannot be used, and `NEXUS_UIA_LEGACY=1` kill switch).

| Notepad (focus = Document leaf) | Before | After |
|---|---:|---:|
| Time | 5,097 ms | **24–44 ms** (first call 44 ms, then 24–27 ms) |
| Controls returned | 0 of 23 | **23 of 23** (recall 100 %) |

(`data/p0-2026-10-08/after_fix_notepad.json`, process confirmed `Notepad.exe`.)
**Not re-measured after the fix:** Calculator and Explorer. The re-run was blocked because the foreground kept being taken by apps the user was working in (the probe correctly refused to read them: `--expect-exe` guard), and I will not keep stealing focus from a live session. Their pre-fix numbers above stand; the raw cached query on both was 18–79 ms, and the shipped path adds only in-memory filtering. Re-run when the machine is idle or run the probe yourself (§5).
**Behaviour change to be aware of:** ordinal commands ("click the 3rd option") now count controls across the whole foreground window in reading order (the documented intent in `screen.rs`), instead of whatever happened to sit under the focused element; off-screen controls are no longer offered. Validate "3rd result"-style commands live.

## 2. What the apps expose for keyboard-first (M1)

Actionable = enabled, on-screen, named, with bounds, of the clickable control types used by `screen.rs`.

| | Notepad (WinUI) | Calculator (UWP) | Explorer (Win32 shell) |
|---|---:|---:|---:|
| `AcceleratorKey` non-empty | **0 %** | 0 % | 2.9 % (`F5`, `Alt+Up`) |
| `AccessKey` non-empty | **69.6 %** | 0 % | 1.5 % |
| Shortcut written in the control's **name** (`Bold (Ctrl+B)`) | 21.7 % (5/23) | 0 % | 2.9 % (2/68) |
| **Invoke** pattern available | **69.6 %** | **91.4 %** | **51.5 %** |
| Any of the above (shortcut, mnemonic, name, or a pattern) | 100 % | 100 % | 55.9 % |
| Other patterns: Toggle / ExpandCollapse / SelectionItem | 5 / 8 / 2 | 0 / 1 / 0 | 0 / 15 / 10 |

Findings:
1. **`AcceleratorKey` is almost never populated** (0–3 %). The assumption in doc 09 §2.1 that UIA harvesting would yield the shortcut table is **refuted for these apps**. Shortcut knowledge must come from other channels.
2. **`AccessKey` can carry multi-key sequences** (`"Alt, F"`, `"Alt, H"`, …) in modern Notepad — this *answers* the open question in doc 09 §2.1 ("do Office-style KeyTips surface?"): at least for this app, yes, as comma-separated text. The parser must therefore understand `"Alt, X"` / `"Space"` forms, not just single characters.
3. **Names carry shortcuts** in some toolbars (`Bold (Ctrl+B)`, `Strikethrough (Ctrl+Shift+X)`, `Clear formatting (Ctrl+Space)`, `Refresh "etc" (F5)`, `Up to "drivers" (Alt + Up Arrow)`) — a regex over names is a real, cheap channel (22 % of Notepad's actionable controls), but absent in Calculator.
4. **The Invoke pattern is the big lever:** 52–91 % of actionable controls can be triggered with **no mouse and no cursor movement**. For Calculator every button (91 %) is invoke-able, though the app exposes no keyboard hints at all — number/operator keys there can only come from a seed pack.
5. The 102 plan's Phase-0 gate ("if AcceleratorKey/Invoke coverage < ~30 %, shift weight to seed + published files + teaching") resolves as: **AcceleratorKey fails the gate (≤ 3 %); Invoke passes it (52–91 %)**. So the strategy is pattern-first for controls, and seed + name-parse + AccessKey + published files + teaching for keyboard shortcuts.

## 3. Phase R — what was fixed (all tested)
| Finding from the ghost audit | Fix | Test |
|---|---|---|
| Watchdog threshold (12 s) equalled the invoke budget (12 s): it could re-arm the mic while the "Didn't catch that" fallback was still speaking | `GHOST_STALL_AFTER_MS` 12 000 → 20 000, shared constant `GHOST_INVOKE_BUDGET_MS`; invariant test pins stall ≥ budget + 5 s | `ghostHotMic.test.ts` (invariant) |
| Timeout did not close the turn, leaving it "open" for the watchdog | `processViaOrchestrator` calls `noteGhostTurnClosed()` on `GhostInvokeTimeout` | `orchestrator.test.ts` new regression test — **mutation-checked**: fails without the fix |
| Watchdog re-armed with a raw `triggerFollowupListen` (no TTS-idle or meeting check) | watchdog now uses the guarded `maybeGhostRelisten()` (same path as the Rust `ghost:relisten` poke) | `ghostHotMic.test.ts` (meeting case + re-arm case) |
| Independent re-arm paths could land together and start the mic twice | central duplicate guard in `triggerFollowupListen` (same origin within 1.2 s is one request; different origin never blocked) | `listenDedupe.test.ts` (5 cases) |
| A second native capture start cleared the live buffer and reset VAD | `is_duplicate_capture_start` guard in `start_stt_capture_with_origin` (400 ms window, explicit turns may still take ownership, **fails open** on clock oddities) | Rust `duplicate_capture_start_guard` — **the test caught a fail-closed bug in my first version**, fixed |
| Stale comment claiming wake used `eval` | corrected | — |
Not changed (by design, needs a live repro): the exact cause of the "1-frame-apart" double `__NEXUS_WAKE__` in the original log. The 117 source tagging stays; with the guards above a repeat is now both prevented and attributable.
Not done: renumbering duplicate `docs/changes/115*` and `116*` (other sessions' files are referenced by number in `AGENTS.md`; renaming would break those references).

## 4. P0 instrumentation shipped
- **`[GHOST-TIMING]`** line per ghost click: `enter_announce, focus_checks, resolve_uia|resolve_vision, glide_dwell_click, verify, restore_home, exit, total` (`live/commands/mouse.rs`, `PhaseTimer`). No live data yet — needs a real ghost click.
- **`sttLogConfidence`** setting (default **off**): command turns go through Groq `verbose_json` and log `[STT-CONF] dur words segs min_avg_logprob max_no_speech text`. Enable by setting `"sttLogConfidence": true` in `settings.json`, use Ghost Mode normally for a few days, then `grep STT-CONF` to choose thresholds (research doc 10, Q3).
- **`uia_probe`** example (this report's source), **`phrasing_probe`** test + generator/analysis scripts (research doc 10), **`collect_heldout.py`** and **`record_ambient.py`** (need the user; not run).

## 5. What still needs the user (P0 items that cannot be done without them)
| Item | Why | Command |
|---|---|---|
| Probe the real apps: Brave/Chrome, VS Code, WhatsApp, Discord | Chromium/Electron behaviour is the biggest unknown (M4, M5); I deliberately did not read their personal windows | `cd src-tauri; cargo build --example uia_probe; .\target\debug\examples\uia_probe.exe --label brave --delay 6 --out probe_brave.json` — focus the app during the 6 s countdown. Names are written only for shortcut-bearing static controls; check the file before sharing. |
| Held-out phrasings (≈ 10 min, typed) | The headline accuracy needs phrasings the lexicon author never saw | `python scripts/phrasing_probe/collect_heldout.py` |
| 2 h ambient recording (explicit consent prompt, local only) | False-executions per hour cannot be measured without it | `python scripts/phrasing_probe/record_ambient.py --minutes 120` |
| A few days of Ghost use with `sttLogConfidence` on, and a live double-wake / click session | Real confidence distributions and `[GHOST-TIMING]` data | edit `settings.json`; use Ghost Mode |
| Dynamic-prompt A/B (needs the audio above) | Whether prompt biasing helps or makes Whisper echo prompt words | after held-out/ambient audio exists |

## 6. Consequences for the plan (applied to `103`)
1. **New quick win, ahead of everything in track E:** root the UIA lookup at the foreground window and replace the 10 sequential queries with one cached query. Measured effect: 2.5–5.1 s → 18–79 ms, and 0 → 100 % recall when focus is on a leaf. It is small, local (`screen.rs`), and removes the main cause of slow/vision-dependent clicks. **Not applied in this pass** (outside phase R/P0 as requested) — recommended as the next step.
2. Pattern tier (Invoke/Toggle/ExpandCollapse/SelectionItem) moves up in value: it reaches 52–91 % of controls with no mouse.
3. Keyboard-shortcut knowledge: AcceleratorKey harvesting is demoted to a minor channel; AccessKey (incl. `"Alt, F"` sequences), name-embedded `(Ctrl+X)`, seed packs, published files (VS Code), menu crawl and teaching carry the load.
4. Chromium/Electron coverage stays an explicit open risk until the user probes Brave/VS Code/WhatsApp.
