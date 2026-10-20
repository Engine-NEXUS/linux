# Competitive & Platform Audit 2026-10 — Research and Decisions

**Date:** 2026-10-02
**Type:** Documentation only. No code changed.

---

## Overview

A ten-angle research audit of NEXUS against the live state of the art
(2026-10-02), run to answer three questions:

1. Where does NEXUS actually stand competitively?
2. What is genuinely broken versus merely unfinished?
3. What is available now that we should adopt, and what is announced but unusable?

Ten parallel research agents were scoped to one angle each, each required to
separate **shipping now** from **announced but unusable**, with source URLs,
dates and measured numbers. Vendor self-claims are labelled as such, and every
claim that could not be verified is listed explicitly.

**No code was changed. No dependency was added. Nothing was installed.**

---

## Deliverables

**Decision record (engineering):**
[`docs/features/62-competitive-and-platform-audit-2026-10.md`](../features/62-competitive-and-platform-audit-2026-10.md)

**Evidence (research):** [`docs/features/research/10-19`](../features/research/README.md)

**Companion compendium (canonical home):**
[`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
— that repository is, per `AGENTS.md`, the dedicated home for research papers,
acoustic DSP investigations, NLU data science studies, and architecture
compendiums.

---

## Three findings that change the plan

### 1. 🔴 The orb architecture cannot work on Linux Wayland

`src-tauri/src/dyn_windows.rs:43-50` creates the `main` window with
`always_on_top: true`, `transparent: true`, `decorations: false`, then maximizes
it at `:174` to form a fullscreen work-area overlay, with click-through via
`set_ignore_cursor_events`.

On Wayland, every one of those is broken:

| Capability relied on | Reality |
|---|---|
| `always_on_top` | **Empty function** in GTK 3.24.50 (`gdkwindow-wayland.c:4631-4634`) — silently does nothing |
| `set_position` | **Impossible by design.** w3c/window-management#68 |
| Click-through on GNOME/Mutter | **Broken.** electron#51808 — events arrive indefinitely |
| `set_ignore_cursor_events` | **Live panic.** `tao 0.37.1 event_loop.rs:456` — `.window().unwrap()` on an unrealized window |
| Shaped / non-rectangular window | **Empty function** (`gdkwindow-wayland.c:3873`) |

`wlr-layer-shell`, the sanctioned escape, does not save us: Tauri exposes no
builder hook (`tauri#15913`), `gtk4-layer-shell` requires GTK4 while Tauri v2 on
Linux is GTK3, and **GNOME does not support it at all** (`mutter#1922`).

**We already document half of this.** `window_manager.rs:19-22` says:
> *"Linux/Wayland no-op: the compositor forbids client positioning… accepts and
> ignores the values."*

### 2. The 2026 design consensus is the opposite of a full-size animated orb

Google, Microsoft and Apple independently converged on small, monochrome,
collapsed, geometry-carries-state. Microsoft removed its assistant's colour
**deliberately**. Google's Neural Expressive drew ~26% actively negative on the
year's most colourful AI redesign — with praise about looks and criticism about
usability regression.

The Alexa+ shopping-list failure is our exact risk: a personality surface
grafted onto a control surface.

### 3. Endpointing is a bigger bug than the visuals

SRI's Enhanced End-of-Turn Detection: a fixed 500 ms silence gate causes **~100%**
premature cut-off; **100 ms with pre-pausal acoustics drops it to 20.3%**. The
human turn-taking floor is +208 ms, so 100 ms is achievable.

This is our documented **"speech onset decapitation"** bug and it has a known fix.

---

## Two earlier recommendations retracted

| Earlier advice | Retraction |
|---|---|
| "Wire up `speakSeq` for mouth animation / visemes" | **Do not build visemes.** Böck et al. 2023 (n=44): audio sync had **no significant effect on any rating** (p = .62). Synthesized beat tracked capture (p < .001). Audio-only ≈ audio+video. Some users prefer lip-sync off. Desync is *worse than nothing*. |
| "Give thinking a slow ambient breathing animation" | Partly. **Apple HIG: avoid oscillation near 0.2 Hz** (≈5 s period). Our `.orb` fallback at `styles.css:84-93` has `breathe 3s` — dead centre in the danger band. |

---

## Security finding (documented, not fixed)

**Three unauthenticated OAuth token endpoints** in the deployed Worker:

```
server/worker/src/index.ts:1886  GET /oauth/github-token?user_id=X  → live GitHub token
server/worker/src/index.ts:1896  GET /oauth/google-token?user_id=X  → Gmail / Calendar / Drive
server/worker/src/index.ts:1905  GET /oauth/swiggy-token?user_id=X  → Swiggy token
```

No authentication. The only thing protecting them is knowing a `user_id`, which:

- is **self-assigned** at registration (`index.ts:2041`)
- **leaks through the OAuth redirect URL** — `state = \`${provider}:${userId}\``
  (`index.ts:2079`) travels to the provider as a query parameter

A `device_token` column exists in `schema.sql:33` and is written at
`index.ts:2047`, but has **zero read sites** — a security control that does
nothing.

This is a **direct violation** of the MCP Authorization spec (2026-07-28):
*"keying stored state as `<user_id>:<handle>` where the user ID is derived from the
**verified token rather than supplied by the client**."*

**This document does not fix it.** It records it. The fix is the first item in the
work order in the spec above, and nothing else should start before it.

---

## Scope change

**Multilingual (Hindi/Telugu) is out of scope** as of 2026-10-02.

This is a net simplification: no verified Telugu benchmark exists for any sub-6B
model as of Oct 2026, so dropping the claim removes the largest unmeasured risk in
the stack rather than shipping around it. It also unlocks the English-only
intent-classification literature and the SiFT footprint optimisation.

---

## What is genuinely well-positioned

1. **The Linux gap is real and it is the entire market.** Verified from primary
   vendor documentation: *no commercial vendor ships Linux desktop computer
   control.* Anthropic: *"Computer Use: app and screen control isn't available on
   Linux."* OpenAI, Google and Microsoft likewise.
2. **Our three-tier perception stack already matches the 2026 architecture.**
   `ocr_server.py`'s own docstring describes the router every 2026 writeup
   converges on. We are not behind.
3. **We are leaving free accuracy on the table by not using AT-SPI.** A11y-tree
   input scores 20.7% vs 7.0% screenshot-only on 358 OSWorld tasks, at 22% of the
   token count, and works identically on X11 and Wayland.
4. **The wake-word work is a real moat**, and the architecture upgrade to beat it
   is available.

---

## Verification gaps

Stated explicitly in the spec so nobody builds on them:

- **Whether the orb looks good.** Structure, tokens, contrast and regressions are
  verifiable; visual quality is not. Validate with a server-side flag, old/new on
  two devices — Google's own method.
- **No trustworthy x86_64 CPU tok/s table exists for any 2026 small model.**
- **livekit-wakeword's benchmark is vendor self-reported** with no independent
  reproduction.
- **No published CPU latency for any GUI-grounding VLM.**
- **Flathub's AI policy is new (2026-09-04) and may change.**
- **No published number exists for the post-TTS wake re-arm grace period.**