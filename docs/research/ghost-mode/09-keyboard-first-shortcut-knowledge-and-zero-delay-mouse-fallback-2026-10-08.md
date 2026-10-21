# 09 — Keyboard-First Shortcut Knowledge & Zero-Delay Mouse Fallback: Research

**Date:** 2026-10-08
**Status:** Research. **Update 2026-10-08:** M1–M4 measured for Notepad/Calculator/Explorer — see `11-p0-baseline-report-2026-10-08.md` (the §1.3 UIA problems are confirmed: 0 controls and 5.1 s on leaf focus; AcceleratorKey 0–3 %; AccessKey carries `"Alt, F"` sequences; Invoke 52–91 %). Chromium/Electron (M4/M5) still unmeasured. Plan: `docs/features/102-keyboard-first-shortcut-brain-plan.md`, sequencing in `103`.
**User request (intent):** NEXUS should *learn* the keyboard shortcuts of the apps it controls so it knows
(1) which actions can be done from the keyboard, and (2) where the mouse is genuinely required — and
when the mouse is required it must act with **no perceptible delay and perfectly**.
Companions: `01-clicky-and-grounding.md`, `06-dual-grammar-...`, `07-offline-first-local-control-...`.

> Evidence legend used below: **[code]** read in this repo today · **[docs]** vendor documentation ·
> **[crate]** read in the vendored `uiautomation-0.25.0` source · **[unverified]** plausible, MUST be measured
> before it is relied on. Timing figures marked *estimate* are arithmetic on constants in the code, not measurements.

---

## 1. Where NEXUS stands today (what actually exists)

### 1.1 Keyboard knowledge is hand-written and scattered [code]
| Where | What it knows |
|---|---|
| `live/commands/browser.rs` | `Ctrl+T`, `Ctrl+L` (+ 200–300 ms sleeps) |
| `screen.rs::switch_browser_tab` | `Ctrl+1..9` |
| `live/commands/whatsapp.rs`, `ghost_drill.rs` | WhatsApp search/send key flows |
| `intent_parser.rs` | `press <key>`, `press <k1> <k2>` raw passthrough |
| `docs/research/ghost-mode/06` §3 | A *table in a document* mapping process → verbs (browser/chat/editor/shell). Not data, not code. |

There is **no per-app shortcut store, no canonical action vocabulary, and no component that answers "can this be done by
keyboard in the app that is in front right now?"**. Each shortcut is a special case someone wrote after a live failure.

### 1.2 The mouse path is slow by construction [code]
`ghost_click` (`live/commands/mouse.rs:355`) performs, in order:
1. `ghost_enter` + a **spoken** "Taking the mouse, sir. Tap Esc any time to cancel."
2. `focus_app_by_title` (polling) and an elevation check
3. `resolve_element` → `screen::list_actionables()` (see 1.3)
4. Glide: `calculate_glide_duration` clamps **550–750 ms** (`mouse.rs:57`), 16 ms steps
5. **100 ms** arrival dwell, then click
6. Focus re-verify
7. Glide **home** again, another **550–750 ms** (`restore`)
8. `ghost_exit` + "Done, sir."

*Estimate:* ≈ 1.3–1.6 s of deliberate motion + dwell alone, before UIA time, TTS speech or session enter/exit.
The motion is a **designed** feature (a visible, human-looking cursor), so "no delay" is a product decision to revisit, not just a bug.

### 1.3 The UIA grounding tier has three structural problems [code + crate]
`screen.rs::list_actionables()`:
1. **Wrong root.** `.from(focused.clone())` roots every search at the *focused element* (`get_focused_element`), not the foreground
   window. When focus is a leaf (a text box, the page document) the search covers almost nothing.
   *[unverified in practice]* — confirm with the probe in Plan §P0 before "fixing".
2. **Depth cap.** The crate's `UIMatcher` defaults to `depth: 7` ([crate] `core.rs:1748`). Chromium/Electron trees are deeper.
3. **Ten sequential cross-process queries.** One `find_all` per `ControlType` (`CLICKABLE` has 10), each with a 500 ms timeout →
   up to ~5 s worst case. Microsoft's own guidance: *each property read on a live element is a cross-process call; fetch
   everything in one cached request* ([UIA caching](https://learn.microsoft.com/en-us/windows/desktop/WinAuto/uiauto-cachingforclients),
   `FindAllBuildCache` / `BuildUpdatedCache`). The crate exposes `find_all_build_cache` + `UICacheRequest::add_property/add_pattern` [crate].

And one **missed opportunity**: the same pass reads `Name`, `ControlType`, `BoundingRectangle` — and never reads
`AcceleratorKey` / `AccessKey`, which are exactly the "this control has a keyboard equivalent" properties (§2).

### 1.4 The key layer is layout-fragile [code]
`keyboard.rs::parse_key` maps letters to `Key::Unicode(c)`. On non-US layouts (or with an IME active) a *Unicode* key is not the
*physical* key a shortcut binds to. It also has no F13–F24, Insert, PrintScreen, Menu/Apps, numpad, or OEM keys. Shortcuts that
the learner discovers (e.g. `Ctrl+Shift+/`, `Ctrl+[`, `Alt+Left`) can't all be expressed today.

---

## 2. Where shortcut knowledge can come from (sources, ranked by trust)

### 2.1 UI Automation — free, per-control, already in the process
| Property | Meaning | Source |
|---|---|---|
| `AcceleratorKey` (UIA id 30006) | Shortcut string for the control's action, e.g. `Ctrl+O` | [UIA property ids](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids) |
| `AccessKey` (30007) | Single mnemonic char, `Alt`+char opens the item (File → `Alt+F`) | same |
| `InvokePattern` | Fire the control's action **without moving the mouse** | [IUIAutomationInvokePattern](https://msdn.microsoft.com/en-us/library/dd375399(v=vs.85)) |

Key doc facts: both strings default to empty (empty ≠ "no shortcut", it means "not reported"); Microsoft says elements that set
`AccessKey` *always* implement Invoke, and an element with an `AcceleratorKey` *may/can* (the .NET page says "always" — wording differs between pages,
so do not assume).

Crate support, read from the vendored source [crate]: `get_accelerator_key`, `get_access_key`, `get_cached_accelerator_key`,
`get_cached_access_key`, `find_all_build_cache`, `create_cache_request`, `UIInvokePattern::invoke`, `UIExpandCollapsePattern`,
`UITogglePattern`, `UISelectionItemPattern`, and `UILegacyIAccessiblePattern::get_keyboard_shortcut` / `do_default_action`
(the MSAA route that covers older Win32 apps).

**Gap found:** I could *not* confirm from public docs how Office ribbon KeyTips (`Alt` → `H` → `FP`) surface — the UIA `AccessKey` is
documented as a single character, so multi-key KeyTips may not appear. **[unverified]**.

### 2.2 Web content & Chromium-based apps (Brave, Chrome, Edge, Electron: VS Code, Discord, Slack, WhatsApp desktop?)
- Chrome ≥ 138 ships **native UIA** by default (previously an MSAA emulation layer)
  ([Chrome blog](https://developer.chrome.com/blog/windows-uia-support-update)); the policy `UiAutomationProviderEnabled` reverts it until Chrome 146.
  The article does **not** list which properties are exposed. The W3C's 2015–16 thread on mapping `aria-keyshortcuts` to UIA concluded there
  was no clean equivalent. → Whether page-level `AcceleratorKey` appears is **[unverified]**; browser *chrome* (toolbar, menu) is a better bet.
- Chromium turns its accessibility tree on lazily when it detects an assistive-tech/UIA client; Electron documents the same
  ([Electron accessibility](https://electronjs.org/docs/latest/tutorial/accessibility)) and warns it can cost performance.
  Practical consequence: **the first UIA query of an Electron app may be slow or return a thin tree**, then improve. Needs measuring per app.
- Whether VS Code / Discord / Slack / WhatsApp actually expose a full tree to a UIA client: **[unverified]** — I found no source.

### 2.3 Application-published shortcut data (authoritative, offline, versioned)
| App | Machine-readable source | Notes |
|---|---|---|
| VS Code | Command *"Preferences: Open Default Keyboard Shortcuts (JSON)"*; user overrides in `keybindings.json`, file-watched; rules have `key`, `command`, `when`, chords like `Ctrl+K Ctrl+C` ([docs](https://code.visualstudio.com/docs/configure/keybindings)) | The **user's** `keybindings.json` must override the defaults. Reading that file needs no UI at all. |
| Browsers | Documented, stable, near-universal (`Ctrl+T/W/L/F/R/Tab`, `Ctrl+Shift+T`…) | Seed by hand; verify once per browser. |
| Windows shell | Documented (`Win+…`, `Alt+Tab`, `Alt+F4`, Explorer `F2`, `Ctrl+Shift+N`…) | Seed by hand. |
| Microsoft Office | KeyTips documented ([access keys](https://learn.microsoft.com/windows/apps/design/input/access-keys)) | Hierarchical; harvest only if UIA exposes them. |

### 2.4 Menu crawl (the "learn all shortcuts" mechanism for classic apps)
Menu items in Win32/WPF/WinForms/UWP apps are UIA `MenuItem` elements; the visible text often includes `\tCtrl+S` and/or UIA reports it in
`AcceleratorKey`. A crawl = open each top-level menu (ExpandCollapse pattern or `Alt`/`F10`), read items, close with `Esc`.
Side effects: menus visibly open (flicker), so it must be user-triggered or idle-only, never run on sensitive apps, and abort on any
foreground change. Win32 **accelerator tables (`ACCEL`) cannot be enumerated from outside the process** — apps that bind keys without
menu entries are invisible to every discovery method here. → "all shortcuts of an app" is **not** a promise any design can keep; "all that
the app exposes or documents" is.

### 2.5 Other learning signals
- **Tooltip / name parsing:** Chromium and many toolbars put the shortcut in the name or help text (`New tab (Ctrl+T)`). Cheap regex over `Name`/`HelpText`. **[unverified per app]**
- **User teaching:** "remember that Ctrl+Shift+K deletes a line in VS Code." Zero discovery cost, highest trust (existing memory model has a `UserSaid` provenance).
- **Execution feedback:** every attempt records success/failure; a shortcut that keeps failing is demoted (the app remapped it).

---

## 3. What the literature says about keyboard vs mouse for agents
- **Action spaces already include hotkeys**: OSWorld (`hotkey(Ctrl+C)`), OpenCUA, Agent S, EvoCUA all expose hotkey actions
  ([OSWorld](https://arxiv.org/pdf/2404.07972), [Agent S](https://arxiv.org/pdf/2410.08164), [OpenCUA](https://arxiv.org/pdf/2508.09123)).
  OS-Symphony argues action-space design can matter more than architecture ([arXiv:2601.07779](https://arxiv.org/pdf/2601.07779)).
- **Hybrid > GUI-only when a shortcut/API exists**: MAS-Bench (mobile) reports hybrid agents up to 68.3 % success and 39 % more efficient than GUI-only
  ([arXiv:2509.06477](https://arxiv.org/pdf/2509.06477)); UFO² reports native API *or* GUI choice improving completion by 8 %+ and cutting latency
  ([arXiv:2504.14603](https://arxiv.org/pdf/2504.14603)). Note: MAS-Bench shortcuts are APIs/deep-links, not keystrokes; UFO² gives no per-action timings.
  **I found no paper that benchmarks keystroke-vs-click specifically for desktop agents** — NEXUS would be measuring this itself.
- **Humans:** sighted users are mouse-dominant, blind/low-vision users keyboard-dominant (keyboard:mouse ≈ 0.6–0.7 : 1 even in browsing/editing)
  ([A11y-CUA, arXiv:2602.09310](https://arxiv.org/pdf/2602.09310)). Implication: a keyboard-first agent walks the path an accessibility user does — which is
  also the path the app's accessibility tree was built to support. Good news for UIA coverage, bad news for apps that never tested it.

---

## 4. Findings that change the design
1. **Three tiers, not two.** Between "keyboard" and "mouse" sits **UIA pattern invoke** (instant, no cursor, no focus theft in many cases). It is neither keyboard nor mouse
   and should be tried before moving the cursor. It also removes the "no delay, perfect" problem for the majority of mouse cases.
2. **The delay the user feels is mostly ceremony, not physics**: spoken announcement, two ~650 ms glides, 100 ms dwell, session enter/exit, and (when UIA misses) a seconds-long cloud vision call.
3. **UIA tier is likely under-performing for fixable reasons** (root, depth, 10 queries) — which also silently routes many clicks to vision.
4. **Learning is mostly a by-product** of work NEXUS already does (`list_actionables`), plus a bundled seed, plus VS Code's published keybindings, plus user teaching. A full menu crawl is optional.
5. **Wrong-window keystrokes are higher blast radius than a wrong click.** Keys go to whatever is foreground; focus must be verified immediately before sending.
6. **Honesty limit:** shortcuts are invisible for apps that don't expose them; canvas/custom-drawn UIs (many games, some design tools) need vision/mouse.

## 5. What must be measured before committing (Phase 0 gate)
| # | Question | How |
|---|---|---|
| M1 | Per target app: % of actionable elements reporting non-empty `AcceleratorKey` / `AccessKey`; % supporting Invoke | UIA probe tool over Brave, Edge, Notepad, Explorer, VS Code, WhatsApp, Settings, Word/Excel if present |
| M2 | Real latency of today's `ghost_click` per phase | timing spans in `ghost_click` + `list_actionables` |
| M3 | Does `list_actionables` return a useful set when focus is a leaf? | probe: same app, focus on edit box vs toolbar; compare counts to a window-rooted cached query |
| M4 | Electron cold/warm tree size and time | probe VS Code/Discord/Slack: first vs second query |
| M5 | Does Invoke actually act (not just return OK) in Chromium controls? | invoke + observe state change |
| M6 | Layout safety of `Key::Unicode` | send `Ctrl+L` under a non-US layout / IME |

## Sources
- UIA property ids — https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-automation-element-propids
- UIA caching for clients — https://learn.microsoft.com/en-us/windows/desktop/WinAuto/uiauto-cachingforclients
- InvokePattern — https://msdn.microsoft.com/en-us/library/dd375399(v=vs.85)
- Access keys / KeyTips — https://learn.microsoft.com/windows/apps/design/input/access-keys
- Chromium native UIA — https://developer.chrome.com/blog/windows-uia-support-update
- Electron accessibility — https://electronjs.org/docs/latest/tutorial/accessibility
- VS Code keybindings — https://code.visualstudio.com/docs/configure/keybindings
- W3C UIA mapping for aria-keyshortcuts — https://www.w3.org/WAI/ARIA/track/actions/1695
- UFO² — https://arxiv.org/pdf/2504.14603 · docs https://microsoft.github.io/UFO/ufo2/core_features/control_detection/uia_detection/
- MAS-Bench — https://arxiv.org/pdf/2509.06477 · A11y-CUA — https://arxiv.org/pdf/2602.09310 · OSWorld — https://arxiv.org/pdf/2404.07972
