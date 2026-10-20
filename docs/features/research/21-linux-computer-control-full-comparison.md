# 21 — Every Way to Control a Linux Laptop, Compared

**Date:** 2026-10-02
**Status:** Comparative analysis. Builds on [20 — Is Pixels the Right Abstraction?](20-linux-computer-control-architecture-rethink.md); current-state evidence in [22 — Codebase Audit](22-linux-perception-actuation-codebase-audit.md)
**Answers:** "Are you sure that is the only / best possible way for AI to control my laptop, especially a Linux one?"

**Verdict up front:** the semantic-first *thesis* is right. My earlier tier list was
**incomplete in two places**, and I found them only by enumerating the alternatives
instead of defending what I had already proposed. Corrected in §5.

I cannot prove global optimality — nobody can. What I can do is enumerate the
space, score it on the axes that actually decide whether it works, and name the
specific evidence that would overturn the recommendation (§8).

---

## 1. What "ways it is possible" actually means

Any design is a choice on **two independent axes**. Most of the confusion in this
space comes from discussing them as one question.

- **PERCEPTION** — how do I learn what is on screen?
- **ACTUATION** — how do I make something happen?

They are separable, and they have *different* best answers. The most important
structural fact in this whole document:

> Perception and actuation have **different optimal channels**, and the
> perception answer does not imply the actuation answer.

Concretely: you can perceive perfectly via an accessibility tree and still be
unable to click, because the tree is read-only. Conversely you can have full input
synthesis and perceive nothing.

So the real design space is **perception-channel × actuation-channel × routing
policy**, not a single ladder.

---

## 2. Perception channels — complete enumeration

| # | Channel | Native Wayland | Consent dialog | Privilege | Latency (measured/cited) | Semantic precision | Verifiable | Verdict |
|---|---|---|---|---|---|---|---|---|
| P1 | **App native API** (HTTP/DBus/MCP/octocrab) | ✅ n/a | ❌ none | none | 50–400 ms network | **exact** | ✅ trivially | **Best where it exists** |
| P2 | **CDP JS/DOM** (`Runtime.evaluate`, `querySelector`) | ✅ in-process | ❌ **none** | none | ~1–40 ms | **exact, richest** | ✅ re-query | **Best for Chromium/Electron** |
| P3 | **CDP AX tree** (`Accessibility.getFullAXTree`) | ✅ in-process | ❌ none | none | ~100 ms (subtree-scoped) | exact, role+name | ✅ | Same targets as P2, fewer privileges needed |
| P4 | **AT-SPI2 tree** | ✅ identical on X11/Wayland | ❌ none (set `IsEnabled` yourself) | none | **192 ms** vs 10,034 ms OCR | exact | ✅ | **Best for GTK/Qt** |
| P5 | **Compositor ext / KWin script** over D-Bus | ✅ | ❌ none | none | ~1–10 ms | exact for *shell* state | ✅ | Only channel for windows/workspaces |
| P6 | **Compositor introspection** (`Shell.Introspect`) | ✅ read-only | ❌ | none | ~1 ms | exact, shell only | — | Subset of P5 |
| P7 | **OCR** (PP-OCRv6-tiny) | ✅ | ❌ | none | **1312 ms** measured, 1080p | text only, no role/state | ⚠️ weak | Text layer only |
| P8 | **Screenshot + VLM** | ✅ | ❌ | cloud key | 1–3 s + reasoning | inferred, can hallucinate | ❌ | Fallback of last resort |
| P9 | **Screenshot + SoM ordinals** | ✅ | ❌ | cloud key | 1–3 s | ordinal, not pixel | ⚠️ partial | Better P8. **26.7** vs **25.7** referring accuracy; beats Grounding DINO |
| P10 | **DRM/KMS framebuffer** (below compositor) | ✅ even headless/login screen | ❌ **bypasses portal entirely** | ⚠️ **`CAP_SYS_ADMIN`** helper + `setcap` | low | pixels only | ❌ | Only for **unattended**; overkill otherwise |
| P11 | **X11 `XGetImage`** | ❌ X11 only | ❌ | none | fast | pixels | ❌ | Legacy path |
| P12 | **AT-SPI event tap** (subscribe, don't poll) | ✅ | ❌ | none | ~0 (push) | exact + *history* | ✅ | See §6 — genuinely underused |

---

## 3. Actuation channels — complete enumeration

| # | Channel | Native Wayland | Consent | Privilege | Bypasses compositor? | Semantic? | Verdict |
|---|---|---|---|---|---|---|---|
| A1 | **App native API** | ✅ | ❌ | none | n/a | ✅ | Best where it exists |
| A2 | **AT-SPI `Action.DoAction`** | ✅ **works on Wayland** | ❌ none | none | **yes** | ✅ | **The sleeper hit.** Wayland forbids synthetic input, not an app self-activating |
| A3 | **AT-SPI `EditableText.SetTextContents`** | ✅ | ❌ | none | yes | ✅ | ✅ GTK/Qt. **❌ absent on all Chromium nodes** |
| A4 | **CDP `Runtime.callFunctionOn`** (`this.click()`) | ✅ | ❌ **none** | none | **yes** | ✅ | Full mouse+keyboard-equivalent for Chromium |
| A5 | **CDP `Input.dispatch*`** | ✅ | ❌ **none** | none | **yes — injects at browser-process level** | ❌ coordinate | Underrated: real key/mouse with no portal. See the reliability caveat below |
| A6 | **libei via RemoteDesktop portal** | ✅ GNOME/KDE only | ✅ first run, then `restore_token` | none | no | ❌ | **The sanctioned generic path** |
| A7 | **`/dev/uinput`** (ydotool) | ✅ any compositor | ❌ | ⚠️ `input` group = **global input R/W** | yes (kernel) | ❌ | The only option on wlroots |
| A8 | **X11 XTEST** | ⚠️ via XWayland 23.2+ bridge | ✅ via portal | none | no | ❌ | Legacy; keep as X11 fast path |
| A9 | **wlr-virtual-keyboard** (`wtype`) | ⚠️ **wlroots only — absent on GNOME** | ❌ | none | no | ❌ | NEXUS's current only Wayland path. Wrong for most desktops |
| A10 | **Compositor script** (KWin `workspace`, GNOME ext) | ✅ | ❌ | none | yes | ✅ | Window/workspace ops |
| A11 | **VLM coordinate → pixel click** | ✅ depends on A6/A7/A8 | varies | varies | no | ❌ | Last resort |

### The caveat that matters most in that table

**A5 is not trustworthy enough to rely on.** `chrome-devtools-mcp#2199` documents
a state where, after a password dialog or re-navigation, **all** CDP-injected
input silently stops reaching the renderer while the tool reports success — zero
DOM events recorded behind a "Successfully clicked" result. `element.click()`
(A4) kept working throughout.

That is not a reason to drop A5; it is the strongest possible evidence for the
ranking of **A4 above A5**, and for mandatory verification (§6).

---

## 4. Desktop coverage — what actually works where

Coverage is the axis most proposals get wrong, because they assume a single
channel works everywhere.

| Capability | GNOME Wayland | KDE Wayland | Sway/HyPrland | X11 |
|---|---|---|---|---|
| AT-SPI tree + `DoAction` | ✅ | ✅ (needs `QT_ACCESSIBILITY=1`) | ✅ | ✅ |
| CDP (Chromium/Electron) | ✅ | ✅ | ✅ | ✅ |
| Native API | ✅ | ✅ | ✅ | ✅ |
| Window/workspace control | ✅ **via extension** | ✅ **native KWin scripting** | ⚠️ `swaymsg` | ✅ `wmctrl` |
| Generic input synthesis | ✅ libei/portal | ✅ libei/portal | ❌ **no portal backend** | ✅ XTEST |
| — fallback needed | — | — | ✅ `/dev/uinput` | — |
| Screenshot | ✅ portal | ✅ portal | ✅ portal | ✅ |
| — unattended/login screen | ❌ portal needs a session | ❌ | ❌ | — |
| — needs `CAP_SYS_ADMIN` | ✅ DRM/KMS only | ✅ | ✅ | — |

**Three facts that eliminate whole designs:**

1. **Portal exists only on GNOME and KDE.** No backend on wlroots. So any design
   whose *only* input path is the portal excludes Sway/HyPrland entirely.
2. **`Shell.Eval` has been disabled since GNOME 41.** Arbitrary JS in the GNOME
   compositor is gone as a security decision. Looking Glass is interactive-only.
   The sanctioned route is a **narrow, auditable shell extension over D-Bus**
   (the `ws-dbus` project is the reference — it explicitly declines to re-expose
   code execution).
3. **No Chromium node implements `EditableText`** (verified across 500+ nodes).
   So text entry in Chrome/VS Code over AT-SPI is impossible. A4 sidesteps this;
   there is no AT-SPI equivalent for AT-SPI-only targets.

---

## 5. What I got wrong in doc 20

Enumerating instead of defending surfaced two omissions. Both are corrections, not
refinements.

| Missed | Why it matters | Corrected rank |
|---|---|---|
| **A5 — CDP `Input.dispatch*`** | I framed CDP as semantic-only. It also delivers **real keyboard and mouse with no portal, no consent, no privilege** — because it injects at the browser-process level. I under-sold an entire permissionless input channel | Insert as A5, below A4 on reliability grounds but **above A6** for Chromium targets |
| **P5/A10 — compositor extensions & KWin scripting** | I missed the *shell* tier entirely. AT-SPI covers app windows; nothing covered workspaces, launchers, or shell state. This is also the only real answer to `focus_window`, which is currently a no-op that lies | New tier, sits alongside AT-SPI/CDP |

Also added: **P12 (AT-SPI event tap)** and **P10 (DRM/KMS)** as documented but
lower-priority options.

---

## 6. Why the cascade beats every single-channel design

### 6.1 The decisive argument

Perception channels have **disjoint coverage**. There is no channel that covers
GTK apps, Chromium apps, canvas-rendered UIs, games, PDFs, and video editors. Any
single-channel design has a large blind spot; the blind spot is where all the
failures live.

The cascade has no blind spot, because pixels always exist as a floor. That is
the entire architectural argument, and it is why "semantic-first" beats
"semantic-only" — not because semantics always work, but because they work
*often enough* to make the common case exact.

### 6.2 Independent corroboration of the identity-addressing idea

I did not expect this, and it is the strongest support for §6.3.

**Lightpanda** (a headless browser vendor) hit exactly the same wall from the
opposite direction. Their words: *"CDP doesn't have a command to click an element
directly. Instead, every click operation transforms into a three-step
coordinate-based process… You wanted to interact with a DOM element, but CDP
forced you through the rendering layer."* Their listed harms: *race conditions
(the element might move between getting coordinates and clicking)*, *viewport
dependencies*, *multiple round trips*.

Their fix: make the "coordinate" an **index into a flat semantic renderer** — an
element identifier wearing a coordinate's clothes.

An independent browser team, solving for headless automation, arrived at
*coordinates-as-identity* from first principles. That is the same insight as
identity-addressed actuation, and it is worth taking as validation rather than
coincidence.

### 6.3 The property pixels cannot have

Address a target as *"AX node `backendNodeId` 4711, accessible name `Confirm`,
role `push button`, in window W"* rather than *(840, 612)*, and re-check that
identity immediately before dispatch. Then:

- A layout shift does not invalidate it.
- **A modal cannot become your target.** There is no coordinate to hijack.
- It is self-verifying: re-reading the node's `StateSet` afterwards tells you
  whether it worked, with no screenshot and no model call.

This is a structural mitigation for the stale-coordinate attack — where a
notification fired in the observation-to-action gap redirects a click with no
evidence in the screenshot the agent reasoned over. Pixels **cannot** defend
against this, because a coordinate has no identity to check.

### 6.4 Verification is mandatory, not a feature

False completion is the dominant failure mode in this field: with a verifier in
place, **86%+ of failed tasks are ones the agent believes it succeeded**. And the
A5 caveat in §3 is the same disease in a different place — a click tool reporting
success with zero events delivered.

So: every action declares an expected effect, and the effect is confirmed against
the **semantic** layer. Screenshot diffs are the fallback, not the primary check.

---

## 7. Whole-system designs, compared

| Design | Coverage | Wayland-safe | Exactness | Failure mode | Verdict |
|---|---|---|---|---|---|
| **A. Pixels-only** (current NEXUS) | total | ✅ via portal/uinput | low | hallucinated coords, stale coords, unverifiable | ❌ cannot click on Wayland today |
| **B. Semantic-only, no fallback** | poor — dies on canvas/games/PDF | ✅ | high where it works | **hard failure** on uncovered app | ❌ unacceptable |
| **C. Semantic-first cascade** | total (pixels floor) | ✅ | high, degrades gracefully | needs a router; some apps slow | ✅ **recommended** |
| **D. Own the whole stack** (VM / nested X11 / container) | total | ✅ trivially | high | user leaves the session; second-class UX; huge packaging cost | ❌ not a desktop assistant |
| **E. Per-app adapters, no generic path** | partial by construction | ✅ | highest | every new app needs code before *any* control | ❌ wrong default; good as an optimization layer |
| **F. Train an end-to-end GUI model** | total | ✅ | medium | 38% vs 72% humans on OSWorld; needs your trajectory data | ❌ as a foundation; fine as the pixel *fallback* |
| **G. C + learned per-app bindings** | total | ✅ | high, improving | staleness of cached bindings | ✅ **the end state** |
| **H. Accessibility-spy model** | total (passive) | ✅ | n/a — observation only | unbounded memory; needs relevance filter | ⚠️ see below |

### On H, the one genuinely underused idea

AT-SPI is a **push** protocol, not just a query API. A client can subscribe to
`org.a11y.atspi.Event.Object` (state changes), `Focus`, `TextCaretMoved`,
`WindowCreate`, `WindowDestroy` — and build a **persistent, continuously-updated
semantic model of every application on the desktop**, without polling.

Nobody is doing this for personal assistants, and it changes the economics:

- No per-command tree walk. The model is already warm.
- Every `DoAction` gets a **free post-condition**: you were told the state
  changed, or you were not.
- Zero latency cost for ambient awareness ("what's on my screen right now").
- The exclusion/privacy gate in §6 of doc 20 stops depending on
  `foreground_title()`, which currently returns `None` on Linux and silently
  disables it.

The honest cost: it is a firehose. It needs a relevance filter or it will eat
RAM, and it is a monitoring capability, so it has real privacy weight. Both are
manageable; neither is a reason not to try it.

---

## 8. What would change my mind

Stated so this recommendation is falsifiable rather than merely asserted:

| Finding | Would force a change |
|---|---|
| A 2026 OSWorld-class result showing **semantic-first underperforming pixels-only** on desktop tasks | The core thesis is wrong; re-evaluate |
| AT-SPI tree coverage measured below ~60% of real desktop usage | Reorder tiers; pixels move earlier |
| `EditableText` gaining Chromium support | Text entry unblocks broadly; A4's advantage shrinks |
| A compositor-agnostic portal backend landing on wlroots | A6 becomes universal; the `/dev/uinput` privilege ask can be deleted |
| `Shell.Eval` returning in any form | Compositor scripting becomes first-class (though I'd still resist it — it is arbitrary code execution in the compositor) |
| Measured truncation/latency regressions from the cascade's routing overhead | Simplify routing; maybe per-app fixed routes |

---

## 9. Final position

**The cascade is the best available design, and the reason is structural rather
than empirical:** perception channels have disjoint coverage, so any
single-channel design has a blind spot, and the cascade is the only architecture
whose blind spot is empty.

Where I was wrong: **under-selling CDP input synthesis** and **missing the
compositor/shell tier entirely**. Both are corrected above.

What I am *not* claiming: that it is provably optimal. What I am claiming is that
I enumerated the space, the enumeration changed my answer twice, and the
recommendation now dominates every alternative on the axes that decide whether a
system works in practice — **coverage, exactness, permission cost, and
verifiability**.

And the number to keep in view while building any of it: **38% vs 72% on
OSWorld.** Every design here is mitigation. None is a solution. Anyone claiming
otherwise is quoting a benchmark slice.

---

### Sources (in addition to doc 20's)

- chrome-devtools-mcp issue #2199 — CDP input silently not delivered, tool
  reports success
- Lightpanda, *CDP Under the Hood*, 2025-11-28 — coordinate-forced clicks;
  element-identity workaround
- GNOME Shell docs, `docs/looking-glass.md`; GNOME Wiki Looking Glass archive
- `Shell.Eval` disabled since GNOME 41 (verified via multiple sources)
- `kemallette/ws-dbus` — GNOME shell extension exposing window/workspace control
  over D-Bus on Wayland, built for agents, deliberately not code execution
- KWin Scripting API (develop.kde.org, 2026-04-06); KWin Scripting Reference —
  KWin 6 / QJSEngine migration
- xa11y *Accessibility API Quirks* — portal backend availability, action-name
  divergence, Chromium renderer-bridge node counts
- libei 1.6.0; Peter Hutterer on XWayland XTEST→libei bridging (23.2.0+)
- RustDesk discussion #15417 — DRM/KMS capture, `CAP_SYS_ADMIN` helper trade-off
- dbus `org.a11y.atspi.Event.*` interface; at-spi2-core bus README