# 23 — Coverage and Guardrails: What NEXUS Can Actually Reach

**Date:** 2026-10-02
**Status:** Honest accounting. Two direct questions answered with measurements
where available and explicit "no" where not.
**Triggered by:** *"I don't want you to hardcode it to only work for certain apps.
I want it all-rounder to have its free hand, but also guardrails so it doesn't
touch credentials or important information."*

Builds on [20](20-linux-computer-control-architecture-rethink.md),
[21](21-linux-computer-control-full-comparison.md),
[22](22-linux-perception-actuation-codebase-audit.md).

---

## 1. Is anything hardcoded to specific apps? — No.

Checked by grep across the new tiers. Every app-adjacent string in
`atspi_server.py` / `cdp_server.py` / `semantic.rs`:

| Occurrence | What it actually is |
|---|---|
| `is_chromium()` matching `"chromium"` in the **toolkit name** | A capability test, not an app list. It detects that Chromium's a11y bridge is off, which is a *property of the toolkit*, and every Chromium and Electron app on the desktop benefits from the CDP route because of it. |
| `process.mainModule.require('electron')` | The Electron **API module**, not an app. Required to reach any Electron renderer. |
| `"code"`, `"1Password"` in tests | Test fixtures. |

The tiers never name an app to decide behaviour. They ask capability questions:
*does this toolkit publish a tree?* *is this node a credential field?* *is this
window a vault?* That generalises to software neither of us has heard of.

The one place a *list* exists is `policy.rs` — and that is the guardrail, not the
capability. It is a deny-list of sensitive window classes, deliberately
fail-closed outside it (see §3), and user-extensible.

---

## 2. Can NEXUS reach everything? — No. Here is the real matrix.

Measured where I could measure; reasoned and marked where I could not.

| App class | Examples | Semantic channel | Status |
|---|---|---|---|
| **GTK 3/4** | Calculator, Text Editor, Files, most GNOME apps | AT-SPI: full tree, `DoAction`, `EditableText` | ✅ **works, no launch flags** |
| **Qt** | Most Plasma apps | AT-SPI (needs `QT_ACCESSIBILITY=1`) | ⚠️ works if the app opts in |
| **Chromium browsers** | Chrome, Brave, Edge, Chromium | CDP AX tree + JS | ⚠️ **needs `--remote-debugging-port`, or a browser extension (§5)** |
| **Electron** | VS Code, Discord, Slack, Notion | CDP via the Node inspector | ⚠️ **needs `--inspect` at launch, and cannot be attached to an already-running app** |
| **Canvas / WebGL** | Games, video editors, Figma's canvas, maps | *none* — pixels only | ❌ |
| **GUI terminals** | gnome-terminal, konsole | AT-SPI: usually one text node | ⚠️ text is visible, controls are not |
| **Wine / Proton** | Windows apps and games | AT-SPI, inconsistent | ⚠️ |
| **Document viewers** | PDF viewers, LibreOffice | LibreOffice ✅ (AT-SPI); PDF ❌ (canvas) | mixed |
| **Remote desktop windows** | RDP / VNC client viewports | *none* — a picture of another machine | ❌ |
| **Compositor UI** | GNOME Shell panels, KDE shell, notifications | none yet — the compositor tier is not built | ❌ |
| **Sandboxed** | Flatpak, Snap | may be denied a11y-bus access | ⚠️ app-dependent |

### What that means in practice

On a typical GNOME developer laptop the *windows* NEXUS can see semantically are
most of them, but the honest number for **interactions** is lower and dominated by
one thing: **whether the app was launched with a flag NEXUS does not control.**

- GTK/Qt: **immediate.** No user action, no configuration.
- Browsers and Electron: **not immediate.** The user must have launched them a
  particular way.

That is the single largest coverage gap, and it is a *launch-time* problem, not a
parsing or perception problem. §5 is what closes it.

### The two flags, and why Electron must not use the obvious one

| App class | Flag | Verified |
|---|---|---|
| Chromium | `--remote-debugging-port=<port>` | ✅ tested against Chrome 151: AX tree + semantic click |
| Electron | `--inspect=<port>` | ✅ tested against VS Code: **1026 AX nodes**, `resolveNode` + `callFunctionOn` click landed |
| Electron | ~~`--remote-debugging-port`~~ | ❌ **never use this.** Shipped Electron has an authenticated-CDP gate that *terminates the app* when it is on argv |

Also verified, and not obvious: **an already-running Electron app cannot be
attached to afterwards.** The flag is a launch decision. NEXUS can launch apps
itself, but it cannot retroactively instrument one the user started.

---

## 3. Guardrails: what exists, and the gap I found in my own work

### The gap, stated plainly

While building the semantic tiers I created the ability to **read a field's text
and type into any field by name** — capabilities the vision path does not have,
because a vision path only ever produces a coordinate.

I built that capability **before** its guardrail. As of this commit there are zero
call sites for `atspi::fill` / `cdp::fill` outside their own modules, so nothing
is exploitable today. But capability-before-guardrail is the wrong order, and
`policy.rs` now makes the ordering structural rather than a matter of
remembering.

The audit that surfaced it:

- `exclusion_verdict` had **1** call site — inside `run_screen_vision`. The vision
  path was gated fail-closed. The semantic tiers had **no** gate at all.
- `live::safety::safety_check` is wired into the 14 `live_*` commands and is
  **never consulted by the semantic tiers**.

### The existing deny-list was not a boundary

`live/safety.rs` matches `DENIED_TARGETS` against the **string the model
supplied**. That is a speed bump:

- A prompt injection embedded in an email can name a different app.
- A real window can be titled `"1Password 8 — Vault"` while the model says
  `"1Password"`.

A deny-list keyed on model input cannot be a security boundary, because the
model is the untrusted party.

### What `policy.rs` does instead

**Check what is OBSERVED, not what is CLAIMED.** The authoritative identity comes
from the accessibility bus — the application name and window title the toolkit
reports — and the role from the node itself. A lie about the target changes
nothing.

Verified live on this machine: a GTK window titled `1Password 8 - Vault` is
observed and classified **SENSITIVE**.

**Risk is not uniform**, so the policy is not a blanket deny:

| Operation | Discloses / does | Sensitivity | Sensitive window | Unknown window |
|---|---|---|---|---|
| `LocateGeometry` | element exists, and where | Low | **Deny** | Allow |
| `Activate` | clicks a named element | Medium | **Deny** | **Confirm** |
| `ReadText` | field contents | High | **Deny** | **Deny** |
| `WriteText` | types into a field | High | **Deny** | **Deny** |
| `ReadTree` | every accessible name in a window | High | **Deny** | **Deny** |

Locating a control is not the same act as typing a secret into it, and treating
them identically would make the assistant useless in exactly the windows where a
user most wants help.

**Field roles beat window titles.** A password entry is caught by its role
(`password text`, `secure text`) even in a window called "Vault" — which is the
case that survives a renamed or mislabelled window.

**Unknown is not safe.** Where the bus cannot identify the window, high-risk
operations are **denied** and medium-risk ones **ask**. This is the same
mistake `research/22` recorded as severity 1, inverted deliberately.

**Deny reasons never echo the sensitive name back**, because the error message is
itself a disclosure. There is a test for that.

### Honest limits of the policy layer

1. **It is a window-and-role filter, not a taint tracker.** NEXUS cannot tell
   that the *content* of an ordinary Notes window is a bank password. If the
   user pastes a secret into a plain text field, the window is not sensitive and
   a read is allowed. Solving this needs content classification, and it is a
   different mechanism.
2. **It cannot stop a legitimate-looking action in a legitimate-looking window.**
   "Delete" in a document editor is allowed because the window is not sensitive.
   Destructive-action confirmation is separate and still thin — §6.
3. **`ReadTree` on a large app is a large disclosure.** It is denied on sensitive
   and unknown windows, but on an ordinary window it still returns every
   accessible name. That is by design, and it is why `read_tree` should never be
   a default.
4. **Screen vision still has its own gate**, unchanged and fail-closed. The policy
   layer is additive, not a replacement.

---

## 4. What "free hand" means here

The user's ask was both *general* and *unguarded*. Those conflict, and the
resolution is that the hand is free **within a boundary that is derived from
observation rather than from the instruction**:

- NEXUS does not need to know what an app is to control it.
- NEXUS does need to know what a window is to refuse to touch it.
- Those are different questions, and only the second one needs a list.

---

## 5. Closing the coverage gap (next work, in order)

1. **Browser extension for Chromium.** A Chrome extension can use the
   `chrome.debugger` API **with no launch flag at all**, and `chrome.debugger`
   exposes the same `Accessibility`/`DOM`/`Runtime` domains used here. This makes
   browser coverage universal with zero user configuration. Highest value per
   effort of anything remaining.
2. **Electron: prefer `--force-renderer-accessibility`.** For many Electron apps
   this is a lighter unlock than the Node inspector — it makes the app publish a
   real AT-SPI tree (VS Code: 1 node → 140) so the *existing* AT-SPI tier covers
   it, with no second code path. The inspector is the fallback for apps that
   ignore it.
3. **Have NEXUS launch apps it wants to control**, rather than requiring the user
   to. Resolves "already running" for both classes.
4. **Compositor tier** (a narrow GNOME shell extension / KWin script) for
   windows, workspaces and shell UI — the one class with no channel today.
5. **Sandbox interop**: verify whether Flatpak/Snap apps can be reached, and
   document the answer rather than discovering it per-app.

---

## 6. Still thin, and not solved by any of the above

- **Destructive-action confirmation.** `live/safety.rs` gates `whatsapp_send`,
  `confirm_send` and `close_app`. It does not gate "click the button named
  Delete". Closing that is the same default-deny work as the 5-second voice
  approval window in `features/54` — an item this work order has flagged
  repeatedly and has not yet touched.
- **Content-level secret detection** (§3 limit 1).
- **The 5-second voice-approval window** remains an injection vector: it
  auto-accepts "proceed" from the microphone, and an instruction inside an email
  can simply say "say proceed". Unfixed.
