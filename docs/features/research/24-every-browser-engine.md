# 24 — Every Browser Engine, Not Every Chromium App

**Date:** 2026-10-02
**Status:** Correction to the framing in [23](23-coverage-and-guardrails.md) §2, plus a
bug this work found in its own AT-SPI tier.
**Triggered by:** *"You are so focused on Chromium again — what if the user uses
any other browser? NEXUS should be able to control anything and everything."*

---

## 1. The correction, stated up front

[23](23-coverage-and-guardrails.md) presented coverage as a list of *apps* and
organised the gaps around Chromium. That was the wrong axis, and it made the
product look far less general than it is.

The right axis is the **rendering engine**, because engines — not applications —
decide what a toolkit can see. Every Linux browser is one of exactly three
engines, and they behave very differently.

| Engine | Linux browsers | Publishes an AT-SPI tree? | Gate |
|---|---|---|---|
| **WebKitGTK** | Epiphany / GNOME Web, MiniBrowser, any WebKitGTK app | ✅ **yes, with no gate at all** | none |
| **Gecko** | Firefox, Thunderbird | ✅ yes — but only with `MOZ_ACCESSIBILITY_ATK2=1` | env var |
| **Blink** | Chrome, Brave, Edge, Chromium, Vivaldi, Opera, **and every Electron app** | ❌ 1-node skeleton by default | launch flag or extension |

So the honest headline is the opposite of what I implied last time:

- **WebKitGTK needs nothing.** Verified below.
- **Firefox needs one environment variable.** Gecko implements ATK and bridges
  it over D-Bus; it is reachable by the tier that already exists.
- **Blink is the only engine that fights back** — and it is also the engine
  behind VS Code, Discord, Slack and Notion, which is why it dominated my
  attention.

---

## 2. Verified: WebKitGTK needs no flag at all

Tested on this machine. A GTK window containing a `WebKit2.WebView` with an HTML
`<button>` and `<input>`, inspected over AT-SPI:

```text
button   WKE859E8_SEND        <- the HTML <button> inside the web view
entry    WKE859E8_FIELD       <- the HTML <input>
button   WKE859E8_GTKBUTTON   <- a native GTK button in the same window
```

21 live nodes, roles `application / button / entry / frame / label`, resolvable
by accessible name, with real geometry.

**Nothing was launched with a special flag.** This matters because WebKitGTK is
the engine behind Safari and GNOME Web — so the largest non-Chromium browser
family on Linux is already covered by the AT-SPI tier, and Safari's engine
specifically is *not* a Chromium special case.

---

## 3. A real bug this found in the AT-SPI tier

Verifying WebKitGTK exposed a defect in code I had already shipped.

The tier collected `Accessible` proxies during traversal and read their
properties *afterwards*. On GTK that happens to work. On **WebKitGTK it silently
returns nothing**:

| Approach | Nodes | Usable |
|---|---|---|
| read each node **during** traversal | 21 live, 9 named | ✅ |
| collect proxies, read them **after** | 21, **every property raising `AttributeError`** | ❌ |

The broken version is worse than a crash: it returns the right *count* of nodes
with empty names, so `searchable()` reported a fully interactive WebKit app as
`False` and lookups returned "not found" — a healthy-looking empty answer.

Every GTK test passed, because GTK tolerates the pattern. It would have shipped
broken for WebKitGTK and for anything else stricter.

**Fixed by snapshotting properties during traversal** (`NodeSnapshot` in
`atspi_server.py`). No live proxy is ever retained, so the whole class of bug is
removed rather than patched. Re-verified after the fix:

```text
searchable():  False -> True
resolve the HTML button in the web view:  FOUND, with bounds
```

Two further confirmations of the same design, from the platform literature:

- *"Each platform mints a brand-new handle every time you cache or re-read an
  element… key elements by a stable property instead"* — AT-SPI's stable key is
  `(bus_name, object_path)`, not the handle.
- *"A snapshot captured before an action is invalidated the moment that action
  rebuilds the tree. The dependable pattern is to re-resolve the selector after
  every mutation rather than holding element handles."*

That is precisely the `Re-verify-before-dispatch` design from
[20](20-linux-computer-control-architecture-rethink.md) §4.3 — now backed by
evidence rather than argument.

---

## 4. Firefox (Gecko) — the one-line gate

Not tested here, because no Gecko browser is installed on this machine and none
is available in the configured repositories. That is a gap in my evidence and I
am not going to paper over it.

What is established, from Mozilla's own documentation and source:

- **Firefox implements ATK and exposes it over D-Bus via `at-spi2-atk`.**
  `AccessibleWrap` / `MaiAtkObject` / `PlatformInit` are the Linux backend. It is
  a first-class implementation, not an afterthought.
- **The tree is only published when `MOZ_ACCESSIBILITY_ATK2=1` is set in the
  browser's environment.** Same class of problem as Blink's renderer bridge,
  different knob, and documented only in Firefox's source tree.
- An already-running Firefox cannot be changed retroactively — it is a launch
  environment variable.

**What I changed:** `atspi_server.py` now carries per-engine launch hints keyed
by *engine*, not app:

```python
ENGINE_ENV_HINTS = {
    "gecko":     {"MOZ_ACCESSIBILITY_ATK2": "1"},
    "blink":     {},   # needs a command-line flag, not an env var
    "webkitgtk": {},
}
```

so the caller can be told how to launch a given engine rather than the
information being scattered across the codebase as folklore.

**What to do to confirm it:** install Firefox, launch it once with
`MOZ_ACCESSIBILITY_ATK2=1`, and check whether its window appears on the a11y bus
with a full tree. That is a ten-minute experiment and it should happen before
anyone claims Gecko is covered.

---

## 5. The genuinely cross-browser route: WebDriver BiDi

`WebDriver BiDi` is a **W3C standard** (Working Draft 2026-02-17) for remote
control of user agents, over WebSocket, with a module system covering script
evaluation, DOM interaction, browsing contexts and browser chrome. Firefox
implements it; Chrome implements it; it is designed to be engine-neutral, and it
replaces both the CDP and Marionette dialects it grew out of.

This is the strategically important row. Right now NEXUS has two engine-specific
paths (AT-SPI for WebKit/Gecko, CDP for Blink). **BiDi is the one protocol that
would collapse the browser cases into a single implementation**, and it also
reaches *browser chrome* — the tab strip, the URL bar, the back button — which
AT-SPI reaches only if the toolkit bothers to expose it and CDP does not.

The catch, stated honestly: BiDi is still maturing on the chrome-scope side.
Firefox's own February 2026 notes describe initial support for chrome contexts
behind `--remote-allow-system-access`, with other commands still to come. A
chrome-independent path is not ready to be the primary implementation today.

Recommended position: **keep AT-SPI as the primary browser path** (it is already
built, works on WebKitGTK with zero configuration, and covers browser chrome for
toolkits that expose it), and treat BiDi as the convergence point to adopt when
chrome scope matures. It is the answer to "any browser", eventually, without a
per-engine tier.

---

## 6. Revised coverage, keyed by engine

| Target | Engine | Channel | Gate | Confidence |
|---|---|---|---|---|
| **WebKitGTK** — Epiphany, GNOME Web, WebKit apps | WebKit | **AT-SPI** | **none** | ✅ **verified here** |
| Native GTK/Qt apps | — | AT-SPI | none | ✅ verified |
| **Firefox, Thunderbird** | Gecko | AT-SPI | `MOZ_ACCESSIBILITY_ATK2=1` | ⚠️ documented, **not tested here** |
| **Firefox, Thunderbird** | Gecko | Marionette / BiDi | launch flag | ⚠️ documented, not tested |
| Chrome, Brave, Edge, Vivaldi, Opera | Blink | CDP | `--remote-debugging-port` | ✅ verified (Chrome 151) |
| VS Code, Discord, Slack, Notion | Blink/Electron | CDP | `--inspect`, restart required | ✅ verified (VS Code, 1026 nodes) |
| Any browser, any engine | — | **WebDriver BiDi** | session | ⚠️ standard, chrome scope still maturing |
| Canvas / WebGL (any engine) | — | none | — | ❌ pixels only |
| Remote desktop viewports | — | none | — | ❌ |

---

## 7. The general principle this establishes

**Nothing here is keyed on an application.** NEXUS asks the engine and the
toolkit what they can do. That is why the tier reaches software neither of us has
heard of, and why adding a new browser is not a code change — it is a question
answered at runtime.

The corollary is uncomfortable but true: **the tiers are as general as the
toolkits are honest.** Every gap in the table above is a gap in some toolkit's
willingness to publish its tree, not in NEXUS's willingness to ask.

---

### Sources

- Mozilla, *Marionette* and *Introduction to Marionette* source docs
- Mozilla, `accessible/platform/atk` — `AccessibleWrap`, `MaiAtkObject`,
  `PlatformInit` (Linux ATK/AT-SPI backend)
- W3C, *WebDriver BiDi* Working Draft, 2026-02-17
- Firefox WebDriver Newsletter 148, 2026-02-24 — chrome-scope support status
- xa11y, *Accessibility API Quirks* — `MOZ_ACCESSIBILITY_ATK2=1`, handle
  instability, snapshot invalidation, AT-SPI per-node cost (6–10 D-Bus round
  trips, no bulk subtree fetch)
- Measured on this machine: WebKitGTK 4.1 via `gi`, GNOME on Wayland
