# 20 — Is Pixels the Right Abstraction? Rethinking Linux Computer Control

**Date:** 2026-10-02
**Status:** Research + design. No code written yet.
**Answers:** "Is this the best possible way for AI to control my laptop, especially a Linux one?"

**Short answer: no.** Not merely suboptimal — on a native Wayland session the
current stack *cannot click anything*, and it says otherwise. The deeper problem
is that screenshot-plus-vision is the wrong abstraction for a machine you own.

---

## 1. What we actually have today

Full audit with `file:line` citations in
**[22 — Appendix: Linux Perception & Actuation Codebase Audit](22-linux-perception-actuation-codebase-audit.md)**.
Read that first — this document argues from what the platform offers; doc 22
records what the code actually does. Summary:

| Capability | Linux status |
|---|---|
| Mouse actuation | ❌ **absent entirely.** `screen.rs:250` gates `click_element` behind `#[cfg(target_os = "windows")]`. `orchestrator.rs:2137` speaks `"Screen clicking needs Windows, sir."` and does nothing |
| Keyboard actuation | ⚠️ **X11 only.** `enigo = "0.5"` (`Cargo.toml:112`) with no features ⇒ `x11rb` backend only. Works only because `install.sh:76` forces `GDK_BACKEND=x11` |
| Window focus | ❌ **a no-op that returns `true`.** `live/commands/window.rs:116`, `app_registry.rs:589`. `try_focus_existing` returns `true` and the app says *"Ok sir"* (`command_executor.rs:799`) |
| Perception | ⚠️ screenshot portal + OCR + cloud VLM. Pixel-only |
| Accessibility tree | ❌ **absent.** `screen.rs:253` returns `None` |
| CDP / DOM | ❌ **absent.** Zero hits for `devtools`, `remote-debugging`, `9222` |
| live-mode actuation | ❌ **dead code from voice.** 14 `live_*` commands registered, **zero frontend callers**; those NLU intents terminate in `Subsystem::WorkerBackend` (`orchestrator.rs:462`) — i.e. in a text reply |
| Tool mix | ≈19 generic-GUI vs ≈66 API-integration — **1 : 3.5** |

Two items are not capability gaps, they are **safety bugs**:

- **The privacy exclusion gate is inert on Linux.** `pointer::exclusion_gate`
  (`pointer.rs:156`) short-circuits on `foreground_title()`, which returns `None`
  off-Windows (`pointer.rs:116`). Banking and password-manager windows are
  therefore screenshotted and OCR'd with no objection.
- **`focus_window` lies.** It returns `true` on Linux while doing nothing, and the
  caller speaks a success message.

The one Wayland-native input path is `wtype` at `command_executor.rs:752`, behind
`.or_else(xdotool)`, both `.spawn()` with **discarded `Result`**. It covers four
fixed hotkeys and fails silently. And `wtype` speaks `wlr-virtual-keyboard`, which
**does not exist on GNOME** — i.e. it cannot work on the most popular Linux
desktop.

---

## 2. Why pixels is the wrong abstraction here

The 2026 frontier — UI-TARS, Claude Computer Use, OpenAI CUA — is deliberately
pixel-only. That is correct *for them*: they must generalise to arbitrary unknown
apps, datasets, and evaluation environments, and an accessibility tree does not
exist for games, canvas, PDFs, or remote desktops.

NEXUS is not that. It is a **personal assistant on one known machine**, with a
persistent app inventory, running continuously. Two consequences:

1. **Coverage is knowable.** You can enumerate what is installed and what each app
   exposes. The long tail you can *measure* rather than assume.
2. **Identity is available.** An accessibility node or a DOM handle is a *stable
   reference* that survives a layout shift. A pixel coordinate is not.

On a machine you own, the accessibility tree is not a nice-to-have tier. It is
strictly better than vision wherever it exists: exact, ~192ms rather than
~10,034ms, free of API cost, and — critically — **it is addressable**.

---

## 3. What is actually true in 2026

### 3.1 Actuation: `libei` + the RemoteDesktop portal is the Wayland answer

- `libei` is the freedesktop Emulated Input library; the compositor runs the EIS
  server. Currently **libei 1.6.0 (2026-05)**, with 1.7.0 adding stylus support.
- It is the transport layer for `org.freedesktop.portal.RemoteDesktop` and
  `InputCapture` (portal ≥ 1.17, mid-2023). `liboeffis` wraps the portal so a
  client needs one call to get an fd.
- **Session persistence landed in portal 1.21.0** — `persist_mode` /
  `restore_token`, so consent need not be requested every run.
- **XWayland 23.2.0+ translates XTEST into libei** and routes it through the
  portal. So existing X11 automation keeps working on Wayland, *with* user consent
  and with the sender visible to the user.

So `enigo`-over-`XTEST` is not dead on Wayland — but only via that bridge, and only
with consent.

**Two gaps that constrain any plan:**

- The portal backend is implemented by `xdg-desktop-portal-gnome` and `-kde`
  **only**, and needs a GDM/SDDM session. **There is no working portal backend on
  wlroots compositors (Sway, Hyprland) or headless.** For those users the fallback
  is `/dev/uinput` (`ydotool`, `xdotool --using-uinput`), which requires `input`
  group membership — global input read/write, comparable to macOS Input
  Monitoring.
- `libei` sends *logical* events, so a keysym that does not exist in the current
  keymap needs the 1.6 `ei_text` interface. Fine, but it is a version floor.

### 3.2 Perception + actuation: AT-SPI2 can *act*, not just read

This is the fact that changes the architecture, and it is widely misunderstood.

`org.a11y.atspi.Action.DoAction(index)` performs an action on a component
programmatically. **No input synthesis, no XTEST, no portal, no consent dialog.**
For a GTK/Qt app, NEXUS can press a button on a Wayland session today, by asking
the application itself.

The friction is real and specific:

- **Action names are not standardised.** GTK calls it `"click"`, Qt calls it
  `"Press"`, Chromium `"doDefault"`. A client hardcoding `"press"` gets
  `ActionNotSupported` on half the ecosystem. An alias table is mandatory.
- **`GtkMenuButton`/`AdwMenuButton`/`AdwSplitButton` expose an outer button with
  `NActions = 0`**, hiding the real action on an inner `toggle button`. This
  affects every stock GNOME app — Calculator, Text Editor, Clocks, Baobab. The
  workaround is a bounded subtree walk, gated on `ToolkitName == "GTK"`.
- **Chromium/Electron expose a skeleton over AT-SPI by default.** Without
  `--force-renderer-accessibility`, the tree is `application → frame` and nothing
  else. Measured node counts on Ubuntu 24.04 + GNOME 46 (Wayland):

  | app | without flag | with flag |
  |---|---|---|
  | VS Code | 1 | **140** |
  | Cursor | 1 | **116** |

  VS Code — the single most valuable target on a developer laptop — is *empty*
  over AT-SPI by default. Detectable: `Application.ToolkitName == "Chromium"` plus
  zero filtered children.
- **No Chromium node implements `EditableText`** (checked across 500+ nodes). You
  cannot set a text field's value in Chrome or VS Code over AT-SPI. Only keyboard
  synthesis works.
- The accessibility bus is **separate from the session bus**: call
  `org.a11y.Bus.GetAddress`, then connect there. `AtSpiSurface::open` sets
  `org.a11y.Status.IsEnabled` and `ScreenReaderEnabled`, which is how a client
  gets trees without the user toggling anything manually.

### 3.3 CDP is strictly better than AT-SPI for Chromium/Electron

`Accessibility.getFullAXTree` over the DevTools Protocol returns the same semantic
tree — and then some:

- You get **`backendDOMNodeId`**, which `DOM.resolveNode` turns into a live
  JavaScript object handle, and `Runtime.callFunctionOn` can call methods on it.
- So the full cycle is: `getFullAXTree` → pick node by accessible name and role →
  `DOM.resolveNode` → `Runtime.callFunctionOn("function(){ this.click(); }")`.
  **A semantic click on a native Wayland surface with no input synthesis at all.**
- `Runtime.evaluate` is available too, so for the pixels-free path you get real
  CSS selectors and `querySelector`.
- `DOM.resolveNode` handles cross-frame nodes natively and `callFunctionOn` runs
  in the node's own execution context, so iframes are not a special case.

Two gotchas, both learned the hard way by other people:

- **Electron apps kill themselves if `--remote-debugging-port` is on argv.** The
  shipped Electron has an authenticated-CDP gate that exits the app. The working
  route is to attach the **Node inspector at runtime** (`SIGUSR1` /
  `--inspect`, which the gate does not check) and reach the renderer through
  `webContents.debugger`. This is exactly what the `claude-desktop-debian`
  test-harness does, and it is the reference implementation worth copying.
- **Setting a framework-controlled input value needs care.** React and Vue track
  their own state; assigning `.value` does not notify them. The working technique
  is to walk to the native property descriptor on the prototype chain and use
  `Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set`.
- CDP is **Chromium-only**. It does nothing for GTK, Qt, or native apps. It is
  complementary, not a replacement.
- `Accessibility.getFullAXTree` needs Chrome ≥ ~115 in the target context. It
  returns `-32601 wasn't found` against a DevTools *panel* context rather than a
  content context.

### 3.4 The failure mode nobody instruments: stale coordinates

The most valuable finding, and it reframes verification from "nice" to
"mandatory".

An agent screenshots at time *T*, a VLM reasons for seconds, and the click is
dispatched at *T+n*. **The OS does not pause in between.** The agent reasoned
about one screen and acted on another that happens to share a coordinate system.

- You do not get an error. You get a **wrong success**. The click lands, the
  trajectory continues on a screen state the agent never saw, and every later step
  compounds the divergence.
- It is also an **attack surface**. A notification fired in the observation-to-
  action gap can redirect a click with a near-perfect hit rate and *no evidence in
  the screenshot the agent reasoned over*. Modal dialogs are the most dangerous
  variant precisely because their primary button sits where agents reflexively
  click.
- Your test suite will not catch it, because a test environment does not move
  between observation and action.

Consequence for architecture: **cheap change-detection must gate expensive
irreversible actions** — targeted region diff, global screenshot diff, window-list
diff. And destructive actions need a stricter threshold than a scroll.

### 3.5 False completion is the dominant failure mode, by a wide margin

VLAA-GUI on OSWorld: even *with* a completeness verifier, **86%+ of failed tasks
are ones the agent believes it completed.** "When the agent fails, it almost
always believes it has succeeded."

VeriGUI makes the same case from the training side: all 3B-scale baselines without
explicit verification achieve **zero** success-under-perturbation.

Context for calibration: OSWorld full computer use sits around **38% for agents
vs ~72% for humans**. Anyone claiming near-human GUI control is describing a
narrow slice.

### 3.6 Set-of-Mark beats coordinates for the pixel tail

When you must use pixels, do not ask a model for coordinates.

- Original SoM result: GPT-4V predicting coordinates directly scored **25.7** on
  referring expression comprehension — "significantly poor". With numbered marks
  overlaid on segmented regions, it beat specialist grounding models
  (Grounding DINO, PolyFormer) and several open LMMs.
- The paper is explicit that asking a VLM to emit coordinates *"hurts the spatial
  understanding ability"* of vision models.
- UI-TARS trains SoM as a first-class capability; mark *type* should be chosen per
  screenshot (numeric marks confuse a page full of digits), and **centre is not
  always the right mark position**.

NEXUS already has an ordinal-addressed click path (`screen_click(ordinal)`) — but
only via UIA, only on Windows. SoM generalises the existing idea rather than
inventing one.

### 3.7 The frontier agrees that pure GUI is the wrong default

UI-TARS-2, problem (3), verbatim: *"Pure GUI interaction is often insufficient
for realistic workflows. Many tasks—data processing, software development, or
system administration—are more naturally handled through file systems, terminals,
or external tools rather than by simulating mouse clicks and keystrokes."* They
train a hybrid environment for this reason.

This independently validates NEXUS's existing 1 : 3.5 API-to-GUI bias. **That
ratio is right, and it is currently accidental rather than designed.**

---

## 4. The proposal

### 4.1 The one-line idea

**Stop addressing the screen by pixels. Address it by identity — and let pixels
be the fallback, not the default.**

### 4.2 Actuation priority inverts

The usual ordering is *"screenshot → reason → click coordinates."* For an
assistant on a machine you own, it should be:

```
resolve target semantically          AT-SPI node id  /  CDP backendNodeId  /  API
        │                                     │                            │
        │  act on it                          │                            │
        ▼                                     ▼                            ▼
  Action.DoAction              Runtime.callFunctionOn(click)      HTTP / DBus / octocrab
        │                                     │                            │
        └──────────────┬──────────────────────┴────────────────────────────┘
                       ▼
        only if the semantic path is unavailable:
        synthesize input  ── libei/portal (GNOME/KDE) → /dev/uinput (wlroots)
        │
        └── last resort only: vision + Set-of-Mark ordinal → coordinate

verify  →  re-read the semantic layer and confirm the effect landed
```

**The load-bearing consequence:** because most actions go through the
application's own API, **the Wayland input lockdown largely stops being our
problem.** Wayland forbids *synthetic input*; it does not forbid a GTK app from
activating its own button on request. Semantic actuation sidesteps the constraint
instead of fighting it.

### 4.3 Three ideas I would genuinely call new here

**(a) Identity-addressed actuation — the security property pixels cannot have.**

Identify a target as *"AX node with `backendNodeId` 4711, accessible name
'Confirm', role `push button`, in window W"* — not as *(840, 612)*. Then:

- A layout shift does not invalidate it.
- **A modal cannot become your target.** The identity is checked immediately
  before dispatch. This is the concrete mitigation for the notification-hijack
  attack in §3.4, and it is *structural*, not a heuristic — there is no pixel to
  spoof.
- It is self-verifying: after `DoAction`, re-reading that node's
  `StateSet` tells you whether it worked, with no screenshot and no model.

NEXUS's current pointer path has none of this: `pointer::show_direct`
(`pointer.rs:198`) never re-checks anything after dispatching.

**(b) A capability registry, so channel choice is data instead of a heuristic.**

Every app adapter declares what it can be driven through:

```rust
trait AppAdapter {
    fn channels(&self) -> Vec<Channel>;  // Atspi | Cdp | Dbus | HttpApi | Pixels
    fn perceive(&self) -> SemanticTree;  // unified node shape across channels
    fn actuate(&self, node: &NodeRef, action: Action) -> Result<Effect>;
}
```

One node type (`{ name, role, bounds, backend_ref, states, actions }`) normalised
across AT-SPI and CDP, so the agent layer never learns which channel produced it.
The router picks the cheapest capable channel. This is what makes the cascade
*decidable* instead of a pile of heuristics — and it degrades honestly: an app with
no adapter is simply `Pixels`.

**(c) Identity-stable bindings, learned per app.**

Cache `(app, role, accessible-name-pattern) → backend_ref`, invalidated whenever
that node's identity changes. Repeated tasks then compile down to a single direct
semantic call. Over time the assistant stops *operating* the GUI for apps it knows
and starts *calling* them — which is the endpoint UI-TARS-2 argues for, reached by
observation rather than by training.

### 4.4 What must be added regardless of philosophy

Cheap, and not optional:

- **`XDG_SESSION_TYPE` detection.** Zero occurrences in the repo today. Without
  it the app cannot choose a backend, cannot explain a failure, and cannot fall
  back.
- **Fix `focus_window`.** Returning `true` while doing nothing, then saying "Ok
  sir", is worse than an error.
- **Fix the inert exclusion gate.** Make it fail *closed* when the foreground
  window is unknown, instead of screenshotting a bank window.
- **Never `.unwrap()`/discard a `.spawn()` result** on an input path. Silent
  failure on a control path is how this class of bug survives.

---

## 5. Honest limits

| Limit | Consequence |
|---|---|
| Portal backend missing on wlroots | Sway/Hyprland users need `/dev/uinput` + `input` group, or XWayland |
| Portal consent dialog | First-run UX; mitigate with `restore_token` from portal 1.21.0 |
| No `EditableText` on Chromium | Text entry in Chrome/VS Code still needs keyboard synthesis. **CDP `Runtime.evaluate` can set the value directly**, so this is solvable for CDP-reachable apps and *not* for AT-SPI-only ones |
| GTK `MenuButton` `NActions = 0` | Needs a bounded subtree walk keyed on `ToolkitName` |
| Non-standard action names | Mandatory alias table (`click` / `Press` / `doDefault`) |
| Electron kills itself on `--remote-debugging-port` | Must attach the Node inspector at runtime |
| Canvas / games / PDF / video editors | Permanently pixel-only. SoM is the best available answer |
| No global AT-SPI window list on Linux | `wmctrl` is X11-only, so window enumeration needs AT-SPI or a portal |
| Multi-monitor | Screenshot capture is single-monitor today (`screen.rs:393`) |

And the strategic one: **38% vs 72% on OSWorld.** A semantic-first stack will beat
pixel-first on a real desktop, but "AI controls my laptop" is not solved by any
architecture in 2026. It is mitigated. Anyone telling you otherwise is quoting a
benchmark slice.

---

## 6. Recommended order

Deliberately sequenced so each step is independently useful and testable:

| # | Step | Why first |
|---|---|---|
| 1 | `XDG_SESSION_TYPE` detection + fix the two lying paths + fail-closed exclusion gate | Correctness and safety. Small. Independent of everything else |
| 2 | AT-SPI tier: `GetAddress` → tree → node identity, with the alias table and the GTK `MenuButton` workaround | Biggest single win: 52× faster than OCR, works identically on X11 and Wayland, and gives *actuation* for free |
| 3 | CDP tier for Chromium/Electron: `getFullAXTree` → `resolveNode` → `callFunctionOn`; Node-inspector attach | Recovers VS Code, Chrome, Slack, Discord from the "1 node" dead end |
| 4 | Identity-addressed actuation + mandatory re-verify-before-dispatch | The security property. Cheap once 2 and 3 exist |
| 5 | libei via RemoteDesktop portal, `restore_token` persistence, `/dev/uinput` fallback | Needed only for the residue that semantics cannot cover |
| 6 | SoM ordinals for the pixel tail | Reuses the existing `screen_click(ordinal)` idea |
| 7 | Capability registry + learned bindings | Optimisation, and the thing that makes it scale |

Steps 2 and 3 are independent and can land in either order or in parallel.

---

## 7. Open question for you

Whether to keep the cloud VLM in the loop at all for routine control. A semantic
tree is small, structured and text — it is a far better input for a **small local
model** than a 1280px JPEG is. That suggests routing ordinary operations through a
local model and reserving the cloud VLM for genuinely visual reasoning, which
would also make the assistant work offline. Worth deciding before step 2, because
it changes what the tree is optimised for.

---

### Sources

- Peter Hutterer, *libei integrations in the XDG RemoteDesktop and InputCapture
  portals*, who-t.blogspot.com, 2026-07-22
- libei 1.6.0 announcement, lore.freedesktop.org/wayland-devel, 2026-05-15
- libei / ei protocol documentation, libinput.pages.freedesktop.org/libei
- `python-libei` 0.6.1 (PyPI) — portal `restore_token` / `persist_mode` notes
- GNOME at-spi2-core development guide — `org.a11y.atspi.Action`
- xa11y, *Accessibility API Quirks* — Chromium renderer-bridge node counts,
  action-name divergence, GTK `MenuButton`, portal backend availability
- Set-of-Mark prompting, arXiv 2310.11441; `microsoft/SoM`
- UI-TARS, arXiv 2501.12326; UI-TARS-2, arXiv 2509.02544
- VeriGUI, *Don't Act Blindly*, ACL 2026 (aclanthology.org/2026.acl-long.1335)
- VLAA-GUI, arXiv 2604.21375 — false-completion and loop metrics
- *The GUI Agent That Clicked the Right Button on the Wrong Screen*,
  tianpan.co, 2026-05-18 — screen-state drift
- `claude-desktop-debian` test-harness `inspector.ts` — working Electron
  `getAccessibleTree` + `clickByBackendNodeId` via Node-inspector attach
- Chrome DevTools Protocol `Accessibility` domain; `chromedp/cdproto/accessibility`