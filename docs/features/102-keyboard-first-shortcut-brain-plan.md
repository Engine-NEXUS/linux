# 102 — Keyboard-First Shortcut Brain + Zero-Delay Mouse Fallback (PLAN)

**Date:** 2026-10-08 · **Status:** PLAN. Phase-0 measurements partly done (`docs/research/ghost-mode/11-p0-baseline-report-2026-10-08.md`): AcceleratorKey is 0–3 % populated (so §4 L2 is a minor channel), Invoke is 52–91 % (pattern tier is the main lever), and `list_actionables` is 2.5–5.1 s / blind on leaf focus. Sequencing is now owned by doc 103 §5. **E1 (Shortcut Map + keyboard-tier router) is built** - `docs/changes/119-shortcut-map-and-action-router-e1.md`.
**Evidence:** `docs/research/ghost-mode/09-keyboard-first-shortcut-knowledge-and-zero-delay-mouse-fallback-2026-10-08.md`

## 1. Goal (user's words, made testable)
1. **Keyboard** — NEXUS learns the shortcuts of the apps it drives and, whenever an action has one, uses it.
2. **Not possible → mouse**, with **no perceptible delay** and **perfectly**.
3. Plan and research more before building.

| Term | Definition used by this plan |
|---|---|
| "Learn all shortcuts" | All shortcuts an app **exposes** (UIA/menus/published files) or that are **documented/seeded/taught**. Win32 accelerator tables are not enumerable from outside; we say so aloud rather than pretend. |
| "No delay" | Targets, to be validated on real hardware: keyboard path **≤ 40 ms** from decision to keys sent; UIA-invoke path **≤ 80 ms** warm; mouse path **≤ 250 ms** warm (resolve ≤ 60 + move ≤ 150 + click). Today ≈ 1.5 s+ of motion alone (research §1.2). |
| "Perfect" | Not 100 % (impossible). Defined as: every action is **verified**; on a failed verify exactly **one** automatic fallback to the next tier; on final failure, a clear spoken reason — never a silent miss, never a wrong-window keystroke. Success rate measured on a fixed corpus (§8), release gate ≥ 98 % on seeded apps. |

## 2. Core idea: one decision ladder, one knowledge store

```
 voice intent ("new tab", "click Send", "save", "3rd result")
        │
        ▼
 [A] Canonical action?  ── yes ──► [B] App Shortcut Map has keys for (foreground app, action)?
        │ no (element-named)                    │ yes                     │ no
        ▼                                       ▼                         ▼
 [C] Resolve element (cached UIA snapshot)   KEYBOARD  ──verify──► ok / fallback ─► next
        │                                                                 
        ├─ element reports AcceleratorKey/AccessKey ─► KEYBOARD (harvested, same verify)
        ├─ element supports Invoke/Toggle/Select/Expand ─► PATTERN (instant, no cursor)
        ├─ element has bounds only ───────────────────► MOUSE (fast glide, click)
        └─ not in tree ───────────────────────────────► VISION (existing, last resort)
```
Tier order: **Keyboard → UIA pattern → Mouse(UIA bounds) → Vision.** Each tier is gated by a foreground/focus check and a
post-action verify; tier N+1 runs only after a *verified* failure of tier N (max one fallback per action).

**"Where the mouse is genuinely needed"** (tagged on the action so the system can say so):
`drag/resize/select-region`, `hover-only menus`, `canvas/custom-drawn controls`, `click-at-position`, `control with bounds but no pattern & no shortcut`.
Everything else must never reach the mouse tier by default.

## 3. Data model — the Shortcut Map

```text
ShortcutRecord {
  app_key      : exe name lower + framework hint   // "brave.exe|chromium"
  action_id    : canonical id | "ui:<automation_id or normalized name>"
  label        : text shown in the UI (for "what's the shortcut for…" answers)
  keys         : Vec<Chord>            // chord = [VK/scan codes], supports "Ctrl+K Ctrl+C"
  when         : Global | Editor | List | Dialog | Unknown   // v1: Unknown unless seeded
  source       : Seed | PublishedFile | UiaHarvest | MenuCrawl | NameParse | UserSaid | ExecLearned
  confidence   : f32  // source prior × recency × success ratio
  ok / fail    : u32  // execution feedback
  app_version  : file version (invalidates on change)   locale : UI language
  last_verified: timestamp
}
```
- **Storage:** plain JSON per app under app-data `shortcuts/<app_key>.json`, loaded lazily into a `HashMap<(app_key,action_id), Vec<ShortcutRecord>>`
  (lookup is µs). Not secret, so not sealed; never sent to cloud; **never** harvested from sensitive windows (`live::safety` denylist) or password fields.
- **Bundled seed packs** (hand-verified, versioned in repo): Windows shell, Explorer, Chromium browsers, Firefox, Windows Terminal, Notepad,
  VS Code defaults, WhatsApp desktop, Settings. Seed has the highest prior except where the user's own file overrides it.
- **Canonical action vocabulary** (~60, closed set; the router's contract with the NLU/parser): `new_tab, close_tab, reopen_tab, next_tab, prev_tab, goto_tab_n, address_bar,
  find, find_next, save, save_as, open, new, print, undo, redo, copy, cut, paste, select_all, zoom_in/out/reset, refresh, back, forward, new_window, close_window,
  switch_window, minimize, maximize, fullscreen, rename, delete, new_folder, properties, command_palette, goto_line, devtools, bookmark, screenshot…`.
  Voice synonyms → `action_id` via a table (extends `intent_parser`, deterministic, no model).
- **Priority when sources disagree:** `UserSaid` > `PublishedFile` (user's own `keybindings.json`) > `Seed` (verified) > `UiaHarvest` > `MenuCrawl` > `NameParse` > `ExecLearned`.
  A shortcut with ≥3 consecutive failures is demoted and re-learned.

## 4. How it learns (all background, all optional, all bounded)
| Channel | Trigger | Cost | Notes |
|---|---|---|---|
| L1 Seed + published files | install / app first seen | 0 at runtime | VS Code: parse default+user keybindings, honor `when`, chords |
| L2 UIA piggy-back | every UIA snapshot NEXUS already takes | ≈ 0 (same cache request adds 2 properties) | stores `AcceleratorKey`/`AccessKey` per control |
| L3 Tooltip/name parse | during L2 | ≈ 0 | regex `\((Ctrl|Alt|Shift|Win)\+…\)` and `\tCtrl+S` suffix; per-app trust until verified |
| L4 Menu crawl | voice "learn this app" or idle ≥ N s, **user opt-in** | ~0.5–3 s of visible menu flicker | ExpandCollapse top-level menus only; **never invoke**; abort on focus change; skip sensitive apps |
| L5 User teaching | "remember that Ctrl+Shift+K deletes a line in VS Code" | 0 | stored as `UserSaid` (pinned) |
| L6 Execution feedback | every keyboard attempt | 0 | ok/fail counters, demotion, per-version invalidation |

## 5. Making the mouse path "no delay" (the part that is mostly ceremony)
Changes are ordered by gain per risk; **each one has a before/after timing from the P0 harness.**
1. **One cached UIA snapshot, window-rooted, deep.** `find_all_build_cache(Descendants, OrCondition(control types), cache{Name, ControlType, BoundingRectangle, IsEnabled, AutomationId, AcceleratorKey, AccessKey, IsOffscreen, patterns Invoke/Toggle/SelectionItem/ExpandCollapse})` rooted at the **foreground window** with raised depth — replaces 10 sequential queries and the focused-element root. All reads then come from the cache (no per-property cross-process calls).
2. **Warm snapshot.** Refresh on foreground-change and (throttled) structure-change events, so "click X" is an in-memory lookup (< 1 ms). Limited to the foreground window, auto-off for huge trees, and tracked by timing (§8).
3. **Pattern tier before cursor.** `Invoke` / `Toggle` / `SelectionItem.Select` / `ExpandCollapse` via the element — no motion at all. Verify by state change; on a no-op (Chromium sometimes accepts and does nothing — M5), one click fallback.
4. **Fast mouse tier.** Real cursor, eased, **120–180 ms** (setting `ghostMouseMs`, `0` = instant), no 100 ms dwell unless a hover menu needs it, **instant** return home (the user's pointer) instead of a second 650 ms glide.
5. **No ceremony per click.** One spoken line at session start (already exists); drop "Taking the mouse… / Done" per action; speak only on failure or a slow (> 1.2 s) step.
6. **Vision stays last** and is short-circuited when the snapshot says the element exists (the current silent UIA-miss → seconds of cloud vision is the worst delay).
7. **Keystroke layer:** send **virtual-key/scan codes** (layout-independent) rather than `Unicode(c)`; add F13–F24, Insert, PrintScreen, Menu, numpad; send modifiers + key in one `SendInput` batch (no inter-key sleeps) — removes the 200–300 ms sleeps scattered in `browser.rs` where the app reacts instantly (keep a *measured* settle wait only where required, e.g. after `Ctrl+T`).

## 6. Safety rails (non-negotiable)
- **Foreground gate before any keystroke:** the intended app/window must own the foreground *immediately* before keys go out; otherwise abort and say why (keys are broadcast to whoever is in front).
- Destructive or irreversible actions (`delete`, `Alt+F4`, `Ctrl+W` with unsaved state, `send`, `pay`, `purchase`) keep the existing confirm gates regardless of tier. A harvested accelerator on a control named *delete/remove/send/pay/buy* never auto-fires.
- Sensitive windows / password fields / elevated targets: existing denylist + UIPI check apply to every tier (UIPI already implemented for mouse — extend to keys).
- Esc / voice-stop abort at any step; fullscreen-exclusive pauses (existing).
- Learning never stores what was typed, only shortcut strings and control labels; no cloud upload.

## 7. Phases (each ends with verify; stop at any gate that fails)
| Phase | Deliverable | Gate to continue |
|---|---|---|
| **P0 Measure (1–2 d)** | `nexus shortcuts probe <app>` dev command: dumps per-control Name/Type/AcceleratorKey/AccessKey/patterns + coverage %; timing spans in `ghost_click` and `list_actionables`; answers M1–M6 (research §5). **No behavior change.** | Written coverage table per app. If AcceleratorKey/Invoke coverage < ~30 % in the main target apps, shift weight to Seed + Published-file + teaching. |
| **P1 Keyboard core (3–4 d)** | Canonical actions + `ShortcutMap` + seed packs + `ActionRouter` (keyboard tier) + foreground gate + verify + layout-safe key layer. Existing hard-coded shortcuts (browser/tab/search) switch to the map. | Corpus pass (§8): keyboard-able actions ≥ 98 %, 0 wrong-window sends. |
| **P2 Fast UIA (2–3 d)** | Cached window-rooted snapshot; harvest L2/L3 into the map; Invoke/Toggle pattern tier. | Warm resolve ≤ 60 ms p95; clicks that previously fell to vision drop measurably. |
| **P3 Fast mouse (1–2 d)** | `ghostMouseMs`, no per-click ceremony, instant home, one-fallback verify. | Mouse path ≤ 250 ms p95 warm; takeover safety tests still green. |
| **P4 Deep learning (3–4 d)** | Menu crawl (opt-in), VS Code published-file reader, user teaching, exec-feedback demotion, per-version invalidation. | Crawl never changes app state (diff UIA tree before/after). |
| **P5 Ask NEXUS (1–2 d)** | "What's the shortcut for X here?", "Can I do this with the keyboard?", "Which actions here need the mouse?" — spoken + sidebar list. | — |
Local grounding model (OmniParser/ShowUI, doc 07 P4) stays a separate, later decision.

## 8. Test & measurement plan
- **Corpus:** ≥ 100 voice commands × target apps (browser, Explorer, Notepad, VS Code, WhatsApp, Settings, Office if present) each tagged `expect_tier ∈ {keyboard,pattern,mouse,vision}`. Unit-level: canonical mapping, chord parsing, source priority, demotion, version invalidation, layout-independent key codes. Integration: scripted run against real apps on this PC, results logged as JSON.
- **Latency telemetry:** per-phase spans (`decide`, `resolve`, `act`, `verify`) in the unified console and a p50/p95 table in a new doc; compare against §1 targets.
- **Learning accuracy:** harvested shortcut is correct if pressing it produces the intended action in the verify check; report precision per source.
- **Negative tests:** wrong-window (focus stolen between decision and send), password field, elevated app, remapped shortcut, locale/layout change, app update changing version.
- All new behavior behind flag `shortcutBrain` (default **on after P1 gate**, off before). New settings must be added to `NexusSettings` (known pitfall: unknown keys get dropped on save).

## 9. Decisions needed from the user (they change the design)
1. **Cursor speed:** the visible 550–750 ms glide was a deliberate "see the ghost" feature. Proposed default **150 ms** with `0 = instant` available. OK?
2. **Per-click announcements:** drop "Taking the mouse / Done" on every click (keep only first-in-session and failures)? Proposed yes.
3. **Menu crawl:** allow NEXUS to open an app's menus to learn them (visible flicker)? Proposed **opt-in by voice** ("learn this app"), never automatic.
4. **Which apps first?** Proposed: Brave/Chrome, Explorer, VS Code, WhatsApp, Notepad, Windows Settings. Tell me others you use daily.
5. **UIA invoke without a visible cursor:** acceptable that, when a control supports Invoke, NEXUS clicks it with no cursor movement at all? (Fastest, but the user won't *see* the action.)

## 10. Risks / honest limits
- Some apps expose nothing (canvas UIs, some Electron views until accessibility is enabled): those fall to vision/mouse — we will say so.
- Shortcut `when` contexts are only approximated in v1 (a shortcut may do something else when focus is in a text box).
- Electron/Chromium trees are large; the warm-snapshot optimization could cost CPU — it is gated by measured p95 and has an off switch.
- Unverified assumptions are listed in research §5 and **block** their dependent phase until measured.
