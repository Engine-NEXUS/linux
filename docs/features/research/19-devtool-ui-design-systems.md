# Developer-Tool UI & Design Systems — What to Adopt in 2026

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** Token architecture, dark-mode convention, and the AI-slop tells already present in our CSS.


**Date:** 2026-10-02
**Scope:** What "professional developer tool UI" looks like in 2026, current
design-token standards, dark-mode convention, and — critically — the specific
AI-slop tells our CSS already ships as first-class tokens.
**Canvas artifact:** `devtool-ui-2026-research.canvas.tsx`

---

## 1. Verdict

**Tailwind v4.3 + shadcn's token *semantics*, with the palette replaced.** Colour
ladder from Radix Colors, state ladder from Zed, one lint rule.

---

## 2. Two corrections that would otherwise send us the wrong way

### 2.1 ✅ Tailwind v4 is current and safe

| Fact | Value |
|---|---|
| npm version | **4.3.3** |
| Docs version | 4.3 |
| Weekly installs | **110M** |
| Licence | MIT |
| **Tailwind Labs joined Shopify** | **2026-09-09** |

**The framework risk we'd worry about is now lower, not higher.** CSS-first config
via `@theme`, and the v3 → v4 migration is mechanical.

### 2.2 🔴 Do not adopt Geist

`@vercel/geistcn` is **404 on the public npm registry** (verified `npm view` +
registry search). **Dashboard-only, file-copy install.** Its components are
unusable in a lockfile-resolved Tauri build.

**Copy its token spec; don't install the package.**

---

## 3. 🔴 We ship the four most-clocked AI-slop tells

Measured against our repo:

| Slop tell | Where we have it |
|---|---|
| Purple/indigo gradient accent | `--nx-accent-grad: linear-gradient(135deg, #6aa8ff, #a855f7)` |
| Neon glow shadows | three `--nx-shadow-glow-blue` / `-purple` / `-green` tokens |
| Bouncy spring easing | `--nx-ease-spring` |
| Purple as a state colour | `#a855f7` as the thinking accent |

**Delete all four.** The trace is documented — Adam Wathan apologised in Aug 2025
for `bg-indigo-500`, and the training corpus never recovered.

### Repo measurements (taken during this audit)

| Metric | Value |
|---|---|
| Token namespaces | **5, not 4** — `--nx-*`, `--apple-*` (duplicated verbatim across two files), `--lg-*` (twice, already labelled *retired*) |
| Hardcoded hex literals | **244 total / 79 unique** |
| `@media` queries in 4,704 lines of CSS | **1** — `prefers-reduced-motion` |
| Vite rollup inputs documented in `AGENTS.md` | 4 |
| Vite rollup inputs that actually exist | **8** |

---

## 4. Dark theme convention is now settled

Radix, Geist, Primer and Zed independently agree:

1. **3–5 named surface levels.** Not more.
2. **Borders carry structure; shadows only on true overlays.** Radix and Geist both
   bake a **1px border *into* every shadow token** so the rule can't be violated by
   accident. 🔑 *This is a good engineering trick — adopt it.*
3. **Four text steps**, mirrored onto icons.
4. **One accent, for interactive/selected state only.**
5. **Never pure black.** (`--nx-bg` should not be `#000`.)

### M3 shape scale (for the orb container)

`0 / 4 / 8 / 12 / 16 / 20 / 28 / 32 / 48 dp / fully rounded`

Nesting rule: *"Outer radius − padding = inner radius"* (48 dp − 14 dp = 34 dp).
*"Do: use different corner radii values for nested components so they have optical
roundness."*

---

## 5. Local-first has a visual language — and it's mostly copy, not colour

How products that emphasise "runs on your machine" present themselves:

| Product | Device |
|---|---|
| **Joplin** | Signed Warrant Canary |
| **Standard Notes** | Funding/audit figures in-app |
| **Zed** | Inspectable theme JSON |

🔑 **We have unusually strong raw material** — wake-model SHA-256 manifest,
on-device model paths, a public API-key list — **and nobody in the AI-assistant
category is using it.** The wake-word manifest already proves local inference. We
just never surfaced it.

**This is the cheapest differentiation available: it's copy, and copy is free.**

---

## 6. State ladder — from Zed

The useful part of Zed's system is **geometry carries state, not colour**:

| State | Radius | Fill | Scale | Rationale |
|---|---|---|---|---|
| Idle | small | transparent | 1.0 | Not present |
| Hover | small | subtle | 1.0 | Acknowledges |
| Active / listening | medium | full | 1.05 | Clearly engaged |
| Working | medium | full + pulse | 1.05 | Alive but not blocked |
| Error | large | error | 1.08 | Demands attention |

**Key insight: radius and fill carry the signal; scale is a redundant channel for
peripheral vision.** This maps directly onto the Sidekick finding
(`02` §4) — preattentive properties, no reading required.

It also solves our three-states-share-one-segment problem *structurally*: each
state has a distinct geometric signature, so it survives being 64px and dim.

---

## 7. Dev-tool UI examples worth studying

| Tool | Design position |
|---|---|
| **Linear** | Dark-first, one accent, dense keyboard-first. Closest to what we should be. |
| **Raycast** | <50ms activation, everything in one surface, no visual richness in extensions — *"beautiful consistency"* |
| **Warp / Zed** | Terminal/editor density; Zed's state ladder above |
| **Cursor / Windsurf** | Everything documented above is what we are **not** copying |
| **GitHub Copilot** | Native-feeling within an existing host app; no AI chrome |
| **Claude Code / Gemini CLI / opencode** | Terminal, no visual design at all — a deliberate choice |

**The common thread:** dev tools that people trust are *quiet*. The AI ones that
announce themselves look like a consumer app and get read as such.

---

## 8. Token architecture

### Standard: layered primitives → semantics

```
── primitives (the ramp) ────────────────────────────────────
--nx-blue-50 … --nx-blue-950
--nx-neutral-50 … --nx-neutral-950

── semantics (what a surface is FOR) ─────────────────────────
--nx-surface-base       /* the app background   */
--nx-surface-raised     /* cards, panels         */
--nx-surface-overlay    /* modals, the sidebar   */
--nx-surface-sunken     /* wells, code blocks    */

--nx-border-subtle
--nx-border-default
--nx-border-strong

--nx-text-primary
--nx-text-secondary
--nx-text-tertiary
--nx-text-inverse

--nx-accent / --nx-accent-hover / --nx-accent-soft
--nx-danger / --nx-warning / --nx-success
```

🔑 **The naming rule: name the *purpose*, not the *appearance*.** `--nx-surface-2`
is meaningless. `--nx-surface-raised` tells you where to use it.

### The lint rule that holds it

Tailwind v4's `@theme` gives us this almost free:

```js
// tailwind.config / @theme — forbid arbitrary values
"colors": {
  "surface": {
    "base":    "var(--nx-surface-base)",
    "raised":  "var(--nx-surface-raised)",
    "overlay": "var(--nx-surface-overlay)",
    "sunken":  "var(--nx-surface-sunken)",
  },
  // ...
}
```

Then `bg-surface-raised` resolves, and `bg-[#1a1a1a]` / `text-[#999]` do not exist
as sanctioned values. **This is the single mechanism that prevents the drift we
currently have.** Every one of our 244 hardcoded hex literals is a lint rule that
wasn't there.

### Tools

| Tool | Status |
|---|---|
| **Tailwind v4 `@theme`** | ✅ CSS-first, native in v4. Our pick. |
| **Radix Colors** | ✅ The palette ladder. 12 steps, scale-invariant, light+dark. |
| **shadcn/ui** | ✅ Copy-in components, not a dependency. Token semantics worth copying. |
| **Design Tokens Community Group (DTCG)** | The standard. Stable. |
| Style Dictionary | ✅ Build-time token pipeline. Overkill for us. |
| Tokens Studio | Figma-side. Not applicable. |

**For us:** Tailwind v4 `@theme` + Radix palette + one `eslint` rule banning
arbitrary colour values. Simple, maintainable, and enforceable without a design hire.

---

## 9. 🔴 The specific fixes for our CSS

### 9.1 Four token namespaces → one

| Namespace | Where | Disposition |
|---|---|---|
| `--nx-*` (68 tokens) | `theme/tokens.css` | **Keep as the single source.** Re-layer into primitives/semantics. |
| `--apple-*` (22) | `sidebar/sidebar.css:13-44` | **Delete** |
| `--apple-*` (15) | `architect/architect.css:8-24` | **Delete** (verbatim duplicate, strict subset) |
| `--lg-*` | `settings-sidebar.css:9-12` (**live: 0.35 / 1.5px**), `sidebar.css:39-43` + `pr-list.css:8-12` (**dead: 0 / 0px**) | **Delete.** Already labelled *retired*. The same token pair has *different values* in two files — a live inconsistency. |

### 9.2 46 of 68 `--nx-*` tokens are never referenced

Including `--nx-listening`, `--nx-thinking`, `--nx-speaking` — **which are exactly
the orb state colours.** The design intent was written down and never implemented.

**And 3 `var()` references resolve to nothing:** `--nx-surface-2`
(`SetupApp.tsx:555`), `--nx-bg-secondary`, and `--sidebar-backdrop-image` (the last
is intentional, JS-injected).

### 9.3 The four slop tokens — delete

`--nx-accent-grad`, `--nx-shadow-glow-blue`, `--nx-shadow-glow-purple`,
`--nx-shadow-glow-green`, `--nx-ease-spring`.

### 9.4 112 of 461 CSS classes are orphaned

Biggest cluster: **~95 lines of dead pie-chart/legend CSS** in
`sidebar.css:958-1075` left behind when `Charts.tsx` was rewritten to use
`.minimal-list-*`.

Also dead: 17 classes in `setup.css` (step 0 was rewritten from "Welcome" to
"Persona & Voice"), 7 in `styles.css` (`.transcript`, `.caption` are
`display:none` for elements that no longer exist).

### 9.5 Real bugs

| Location | Bug |
|---|---|
| `settings-sidebar.css:104` | 🔴 **Selector typo — `settings-tabs,` is missing the leading `.`** so the z-index rule never applies to the tab bar |
| `setup.css:574-578` | `gap: -8px;` followed immediately by `gap: 12px;` — the first is invalid and silently dead |
| `styles.css:99` vs `setup.css:47` | `@keyframes breathe` defined **twice** with identical bodies |
| `styles.css:101` vs `sidebar.css` | `@keyframes spin` defined **twice** |
| `sidebar.css` / `pr-list.css` / `settings-sidebar.css` / `architect.css` | **4 near-identical carbon-copy `.sidebar-card` blocks** — comments literally say *"carbon copy"* / *"Same transparent backdrop blur"* |
| `sidebar.css:41`, `architect.css:56` | `linear-gradient(180deg, rgba(255,255,255,.08) 0%, transparent 40%)` — the "removed top-highlight gradient" that `AGENTS.md` claims was stripped |
| `App.tsx:182` | Template literal has no space before the conditional — works but fragile |

### 9.6 998 lines of unreachable UI

`settings.html` + `SettingsApp.tsx` + `settings.css` = **998 lines**, **zero
callers**. `open_settings_window` / `close_settings_window` are registered
(`lib.rs:968-969`, `commands.rs:1498-1513`) but **never invoked from the
frontend**. `tray.rs:81` routes "Settings…" to `show_settings_sidebar`.

It also **disagrees with the live settings UI**: white theme vs dark glass,
`nx-*` vs `settings-*` prefixes, duplicate `Toggle` implementations, duplicate
`Settings` interfaces and `DEFAULT_SETTINGS` that disagree on `hotkey`
(`Ctrl+Super+Space` vs `Ctrl+Space`) and `ttsVoice` (`en_US-amy-medium` vs
`af_sky`).

**Delete it.** It's 1/4 of our CSS for a screen nobody sees.

### 9.7 The setup wizard is white

`setup.css` is the **heaviest `--nx-*` user** (171 references) and it is a **white
theme** sitting next to six dark-glass windows. **This is the first-run
impression.** It is also the one component with a ~50/50 class/inline-style split
(53 `className` vs **52** `style={{}}`).

### 9.8 Two settings that lie

`orbSize` is surfaced at `SettingsSidebarApp.tsx:331` and **read by nothing**. Size
is hardcoded `180` in four places: `styles.css:35-36`, `Avatar.tsx:177-187`
(×3), `useRoam.ts:3`.

`set_orb_position` is a Wayland no-op — and `window_manager.rs:19-22` even says so:
*"accepts and ignores the values."*

### 9.9 Six write-only zustand fields

`transcript` (appended by ~30 call sites, **never rendered**), `speakSeq`
(documented *"for avatar mouth animation"*, never read), `audioVolume`,
`clearTranscript`, `setSpeakSeq`, `pendingGithubCommand`.

---

## 10. Recommended plan

### Phase 1 — make the tokens real (before any visual change)

1. Re-layer `theme/tokens.css` into primitives + semantics. Adopt the Radix
   ladder.
2. Delete `--apple-*`, `--lg-*`, the four slop tokens, and the 46 unused `--nx-*`.
3. Wire the three orphaned state tokens to actual orb states — or delete them.
4. **Add the lint rule banning arbitrary colour values.** This is what stops it
   recurring.

### Phase 2 — delete before redesigning

5. Delete the 998-line dead settings window.
6. Delete the 112 orphaned classes, focusing on the ~95-line dead chart block.
7. Deduplicate `@keyframes breathe` and `spin`.
8. Consolidate the four carbon-copy `.sidebar-card` blocks into one shared rule.
9. Fix the `settings-tabs` selector typo and the `gap: -8px` invalid declaration.

### Phase 3 — unify the theme

10. **Setup wizard → dark.** It is the first-run impression.
11. One theme across all eight windows.
12. Wire orb size end-to-end, or remove the slider.

### Phase 4 — validate

13. **Copy Google's method:** server-side flag, old/new on two devices, same user.
    It is the cheapest A/B available for a UI change.
14. **Ship an opacity/dim control in v1** if the orb is ever translucent.

---

## 11. What Tailwind will **not** fix

Tailwind does not solve the missing elevation ladder, and it does not solve
white-next-to-dark. **Those are token-contract problems.** The lint rule in step 4
is the part that actually holds.

---

## 12. Sources we could not verify

- **`m3.material.io`** is JS-rendered; motion specs came from a third-party mirror
  generated 2026-09-28. **Verify against the live page before shipping.**
- **`@vercel/geistcn`** — 404 confirmed on the public registry, so its token spec
  could only be read second-hand.
- Exact Radix/Geist colour values — the convention above is confirmed across four
  independent systems; the specific hex ramp should be read from the live source.