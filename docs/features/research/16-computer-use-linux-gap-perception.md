# Computer-Use Agents, the Linux Gap & Screen Perception

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** The Linux moat (verified from primary vendor docs), OCR/GUI-grounding SOTA, AT-SPI, and pointer patterns.


**Date:** 2026-10-02
**Scope:** Competitive position of NEXUS's screen-vision/OCR/pointer stack. Is our
approach current or obsolete? Where is the white space? What is SOTA for local
screen element detection?

**Headline:** The Linux gap is real and it is our entire market. But our OCR tier
is improvable, and we are leaving free accuracy on the table by not using AT-SPI.

---

## 1. Computer-use agents — shipping status and OS support

| System | Shipping? | **Desktop OS actually supported** | Linux? |
|---|---|---|---|
| **Claude Computer Use (API)** | ✅ GA | **None natively** — it's a *client toolset*. Anthropic ships no desktop backend; you implement the harness. Reference impl runs in an X11/Docker container. | ⚠️ Indirect only |
| **Claude Cowork computer use** | ✅ Beta | macOS, Windows | ❌ **No** |
| **Claude Code CLI `computer-use`** | ✅ Research preview | macOS only | ❌ **No** |
| **ChatGPT agent** (ex-Operator) | ✅ | Web, iOS/Android, macOS, Windows desktop | ❌ **No** |
| **Gemini in Chrome** | ✅ Rolling preview | Chrome on **Chromebook Plus, Mac, Windows**, U.S., AI Pro/Ultra | ❌ **No** |
| **Antigravity 2.0 / IDE** | ✅ | Desktop orchestrator + IDE. Linux builds exist but **apt/rpm channel dropped** for 2.0 — tarball only | ⚠️ Client yes, **computer-use on Linux: no** |
| **Google Antigravity Agent (API)** | ✅ Preview | Remote **Linux sandbox** (headless, cloud-hosted) | ⚠️ Server-side Linux only |
| **Microsoft UFO² / UFO³** | ✅ Open source | **Windows only** (UIA + Win32 + COM + PiP) | ❌ No |
| **Cua Driver** | ✅ v0.5.7+ | macOS, Windows, **Linux (X11/XWayland)**; native Wayland behind a flag | ✅ **Yes** |
| **UI-TARS Desktop** | ✅ | Windows / macOS / browser | ❌ No |
| **Google Project Mariner** | ❌ **Shutdown 2026-05-04** | Folded into Antigravity / Gemini Spark | — |

### Verified primary quotes

> *"Computer Use: app and screen control isn't available on Linux."* — Anthropic,
> `code.claude.com/docs/en/desktop-linux` (Linux beta docs, live today)

> *"You're on macOS. Computer use in the CLI is not available on Linux or Windows."*
> — `code.claude.com/docs/en/computer-use`

> *"Agent is supported on ChatGPT Web, mobile (iOS/Android), and desktop apps
> (macOS/Windows)."* — OpenAI Help Center

> *"Use a Chromebook Plus, Mac, or Windows computer."* — Gemini in Chrome availability

---

## 2. The Linux gap — verified individually, not inferred

**No commercial vendor ships a Linux desktop computer-control agent.**

- **Anthropic** — Linux desktop app exists (Ubuntu 22.04+/Debian 12+, x64/arm64,
  July 2026). Chat, Cowork, Claude Code all work. **Computer Use explicitly absent.**
  Fedora/RHEL also absent.
- **OpenAI** — shipped a ChatGPT desktop app for Linux **2026-08-11 (preview)**,
  DEB+RPM, x64+ARM64, Ubuntu 24.04/26.04, Debian 13, Fedora 43/44. Contents:
  ChatGPT, ChatGPT Work, **Codex**. Agent mode documented as macOS/Windows desktop
  only. ⚠️ *We could not find primary OpenAI documentation confirming whether the
  Linux app exposes agent mode — secondary sources imply not. Worth verifying.*
- **Google** — everything is Chrome-on-ChromeOS/Mac/Win, or a cloud Linux sandbox.
  Antigravity's Linux story is a packaging mess (apt repo stuck at 1.23.2,
  AppArmor freeze, tarball-only for 2.0).
- **Microsoft** — UFO²/UFO³ are Windows. `microsoft/UFO` is the only serious
  OS-integrated CUA and it is Windows/UIA-only.

### Who *does* ship on Linux — all open source, all MCP-shaped

| Project | Approach | Wayland status |
|---|---|---|
| **Cua Driver** (trycua/cua) | AT-SPI + XTEST + **painted agent cursor** | X11/XWayland GA; native Wayland behind `CUA_DRIVER_RS_ENABLE_WAYLAND=1` |
| **computer-use-linux** (agent-sh) | AT-SPI + RemoteDesktop portal + ydotool/wtype; GNOME/KDE/Hyprland/i3/COSMIC | **Wayland-first** ✅ |
| **Deskbrid** (brauliobo) | Single Rust daemon, compositor auto-detect | GNOME/Hyprland/KDE/Sway/COSMIC ✅ |
| **UI Act** | X11 **Multi-Pointer X** — a *second virtual pointer*, human + agent simultaneously | X11 only |
| **PeekabooX** | Screen capture + AT-SPI + OCR + MCP | Wayland/X11 ✅ |
| `claude-linux-mcp`, `LukeLamb/claude-linux-mcp` | xdotool + scrot + tesseract MCPs | **X11 only, explicitly not Wayland** |
| Aulinx | AT-SPI scene-graph daemon | ⚠️ ~0 stars, 3 issues, Apr 2026 — **vaporware** |

### 🔴 Wayland is the real blocker, and everyone says so

From **Cua's own engineering blog (2026-06-18)**, on native-Wayland-only apps
bypassing XWayland — *"including some modern Firefox and GTK4 builds — may not be
visible to the backend at all."*

From **`claude-linux-mcp`**: *"Wayland requires a totally different approach
(ydotool + privileged daemon, portal APIs for screenshots). Adding Wayland support
is on the roadmap but nontrivial."*

---

## 3. Accessibility-API automation — where the field moved in 2026

### 🔴 This is the most important result in this document

**A11y-Compressor** (358 OSWorld tasks):

| Agent input | Success |
|---|---|
| Screenshot only | **7.0%** |
| Linearized accessibility tree | 15.6% |
| **Compressed accessibility tree** | **20.7%** |

Compressed trees = **22% of the original token count**, +5.1 pp average success,
per-domain input under ~3,500 tokens. Best result everywhere = **both**.

**DailyDroid** (75 Android tasks, 300 trials, Apr 2026, Univ. of Melbourne +
Auckland):
- Text-only: 26.7% / 29.3%
- + screenshot: 32.0% / 33.3%
- → **structured text delivers ~85% of the result**; pixels add 4–5 points.

### The latency gap is brutal

**ScreenMemory** (Windows COM/UIA, n=3 agents × 50 iterations):

| Method | Latency |
|---|---|
| UIA scan | **192 ms** ± 13.3 ms (95% CI [159, 226]) |
| Screenshot + OCR | **10,034 ms** |
| **Speedup** | **52×** |

OCR was **89.5%** of total pipeline time (2,125 of 2,375 ms). Semantic a11y-tree
compression of browser DOM: ~100,000 → ~1,400 tokens (**71×**). Region-targeted
capture 1.5–6.6× (n=100, p<0.001).

### Why a tree beats pixels

| | Accessibility tree |
|---|---|
| **Wins** | Deterministic · DPI-invariant · ~100× cheaper · **free preconditions and postconditions** (`showing`/`enabled`/`sensitive` before; `pressed`/`expanded` after) · no inference to perceive |
| **Blind to** | Canvas, WebGL, video, chart plots, custom-drawn controls, Electron with thin trees, **layout defects** (a misaligned button reports a perfectly normal frame) |

**Tree coverage reality:** GTK3/4 excellent · Qt5/6 good · Electron needs
`--force-renderer-accessibility` (Cua flips this session-wide) · Java/Swing
hit-or-miss · games/custom UIs nothing.

**macOS gotcha:** AXPress silently no-ops on many Chrome/Safari web views → every
production engine keeps a hardcoded browser bypass list.

🔑 **Linux structural advantage:** AT-SPI rides D-Bus, so it works **identically on
X11 and Wayland**. Input injection does not — `xdotool`/XTEST on X11,
`ydotool`+uinput / `RemoteDesktop` portal / `wtype` on Wayland.

### Production tools using a11y APIs instead of pixels

- **Windows:** pywinauto / FlaUI / UIAutomation
- **Linux:** **AT-SPI2** + pyatspi / dogtail / Accerciser
- **macOS:** AXUIElement
- Terminator's `terminator-computer-use` — a `VisionType` router defaulting to
  `UiTree` with `omniparser`/`gemini` fallbacks
- ScreenMemory/GodMode, Stash MCP

---

## 4. Our three-tier stack already matches the 2026 architecture

`ocr_server.py`'s own docstring describes:
> *UIA tree (~5 ms) → local OCR (~300 ms, free) → vision LLM fallback (~1–3 s)*

**That is verbatim the router architecture every 2026 writeup converges on:**
Terminator's `VisionType`, UFO²'s hybrid detector, Cua's AT-SPI→XTEST cascade.

**We are not behind.** But see §5 — we are not actually using tier 1 either.

### UFO²'s ablation is directly relevant

UIA-only **25.3%** → **+OmniParser hybrid 27.9%** (o1) on WindowsAgentArena;
**hybrid beat pure-vision in both directions** on OSWorld-W (22.4% hybrid vs
14.3% OmniParser-only).

**The hybrid pattern wins.** Ours is already hybrid.

---

## 5. 🔴 Our biggest gap: we don't use AT-SPI

`pointer.rs`'s `exclusion_gate` already queries foreground window title — **the
whole AT-SPI tree is one D-Bus hop away.**

What we would gain:
- **52× faster element location** (192 ms vs 10,034 ms)
- **Free verification** — read `showing`/`enabled`/`sensitive` before clicking and
  `pressed`/`expanded` after. **A pixel agent infers success; an AT-SPI agent proves
  it.** Cua's engineering writeup makes exactly this point: *"an agent cannot
  recover from a driver that quietly does nothing and reports success, which is
  the failure mode I spent the most effort designing out."*
- Works **identically on X11 and Wayland**

**Tier ordering for NEXUS:**
```
1. AT-SPI2 (D-Bus)      — free, exact, ~5-192ms, verifiable. NOT IMPLEMENTED.
2. Local OCR (RapidOCR)  — ~300ms, text targets. IMPLEMENTED.
3. Vision LLM fallback   — 1-3s, everything else. IMPLEMENTED.
```

---

## 6. Local OCR — state of the art

### We are on PP-OCRv4; **PP-OCRv6 exists**

PP-OCR recognition end-to-end CPU latency (PaddleOCR docs, official):

| Model | params | onnxruntime E2E |
|---|---|---|
| PP-OCRv5_mobile_rec | 16M | **4.91 ms** |
| PP-OCRv5_server_rec | 81M | 5.98 ms |
| **PP-OCRv6_tiny_rec** | — | **3.12 ms** |
| PP-OCRv6_small_rec | 7.7M | 4.46 ms |
| PP-OCRv6_medium_rec | — | 4.97 ms |

RapidOCR vs PaddleOCR speed (RapidAI's own bench, Feb 2026, ONNX Runtime CPU):
**0.93–1.26 s vs 1.75–1.93 s** per image — RapidOCR ~1.6× faster.

A RapidOCR user measured PP-OCRv6-small at **839 ms** for a full 1448×1086 image
on M4 Max CPU, 12/14 exact string matches, mean confidence 0.984 (⚠️ M-series CPU,
not desktop x86 — our latency will be worse).

**PP-OCRv6 also handles vertical and slanted text**, which matters for real
desktops.

### Accuracy comparison (800×600 invoice, Apple M-series CPU, Mar 2026)

⚠️ **Secondary source, methodology weak, use with caution.**

| Engine | Accuracy | Latency |
|---|---|---|
| PaddleOCR | **100%** | 4.85 s |
| Surya | 95.8% | ~2.1 s |
| docTR | 91.7% | ~1.8 s |
| Tesseract 5.5.2 | 87.5% | **0.162 s** |
| RapidOCR | 75.0% | **0.212 s** |
| EasyOCR | 62.5% | 0.656 s |

Licences: PaddleOCR / Tesseract / RapidOCR / Surya Apache-2.0. **EasyOCR is
GPL-3.0** — copyleft, relevant if we ship.

### 🔴 None of these detect icons or buttons

They are text detectors + text recognisers. **Our `ocr.rs` → text-bbox path is
complete as a *text* tier and cannot be stretched to icon grounding.**

### Icon/button detection — OmniParser V2 (still the reference)

- Pipeline: YOLO interactable-region detector + Florence-2 icon captioner +
  optional PaddleOCR for text
- **Latency: 0.6 s/frame on A100, 0.8 s on a single 4090.** ⚠️ **GPU numbers. No
  published CPU latency found — treat CPU as unverified.**
- **ScreenSpot-Pro: 39.6% with GPT-4o** (up from GPT-4o's raw 0.8%)
- 🔴 **Licensing trap:** `icon_detect_v3` + `icon_caption` (Florence-2) = **MIT**.
  The older `icon_detect` (Ultralytics YOLO) = **AGPL-3.0**. **If we ship, use the
  MIT path.**
- `desktop-touch-mcp` already has a Rust ONNX port of OmniParser-v2 stage 2
  (`vision_backend/omniparser.rs`) — a free reference implementation.

### Set-of-Marks (SoM)

Overlay numbered boxes on the screenshot and let the model pick an integer.
OSWorld explicitly supports a "Set-of-Mark" observation mode; every ablation that
tested SoM + tree/image ranked it first. **Orthogonal to any detector** — we could
render it ourselves.

---

## 7. GUI grounding models

### ScreenSpot-Pro (1,581 tasks, 23 pro apps, 3 OSes) — the benchmark that matters

| Model | Score | Notes |
|---|---|---|
| UI-Venus-72B | **61.9%** | SOTA open |
| GTA1-72B | 58.4% | |
| GUI-Owl ~30B | ~58 | aggregator, unverified |
| Qwen2.5-VL-72B (general VLM) | 53.3% | |
| **GUI-Spotlight** (UI-TARS-1.5-7B init) | **52.8%** | 18.5K samples |
| V2P-7B | 50.5% / 52.5% | depends on revision |
| ScreenSeekeR (OS-Atlas-7B, **training-free**) | 48.1% | +254% over base |
| **Claude Computer Use** | **17.1%** | ★ |
| OS-Atlas-7B | 18.9% | original baseline |
| **GPT-4o** | **0.8%** | |
| OmniParser-V2 + GPT-4o | 39.6% | |

★ **Claude's own computer-use model scores 17.1% on ScreenSpot-Pro.** Its 83.4%
OSWorld comes from scaffolding + harness, **not grounding**.

### ScreenSpot-v2 (1,272 tasks)

V2P-7B 92.4% · JEDI-7B 91.7% · GUI-Actor-7B 92.1% · UI-TARS-7B 91.6% ·
UGround-V1-7B 87.6% · **ZonUI-3B 86.4%** (WACV 2026, best under 4B, trained on one
RTX 4090) · UI-TARS-2B 84.7%.

### 🔴 On CPU: essentially no

- `uitars-mcp` (community): UI-TARS-2B, **~4.1 GB VRAM, ~1.2 s per element find**,
  90.7% on ScreenSpot desktop-text
- **No published CPU-only GUI-grounding latency exists for any of these models.**
  ZonUI-3B is the strongest "small" claim but inference hardware is unreported.

**Integration footgun:** coordinate output formats differ. UI-TARS-style models use
**absolute** coords; Qwen2.5-VL-based variants use **absolute**; others normalise to
`[0,1000]` → `abs = round(W * v / 1000)`. Two conventions in the wild.

**Conclusion:** every competitive grounding model is a 2B–72B VLM requiring a GPU
and 1–3 s/frame. **Our CPU OCR path at ~100–300 ms is faster and more reliable for
text targets — and that is a legitimate position, not an obsolete one.**

---

## 8. Pointer / visual-attention patterns — already a shipped pattern

### The painted agent cursor is standard, not novel

**Cua Driver**, verbatim from their 2026-06-18 post:
> *"The cursor does not move. Focus does not need to change for most operations.
> **The agent gets its own painted cursor**, and the tools are available through
> both MCP and the CLI… The overlay is a visual trace of what the driver is doing,
> **not the input path itself**."*

Each agent gets a distinct `cursor_id`. **This is precisely what our `pointer.rs`
(`show_direct`, `emit_physical`, `emit_hide`, `decide`, `exclusion_gate`) does.**

| Pattern | Product | Status |
|---|---|---|
| **Separate painted agent cursor** | Cua Driver | ✅ Shipping, 3 OSes |
| **Mouse animation toggle** | Claude Desktop (`getMouseAnimationEnabled`, `getHideBeforeActionEnabled`) | ✅ macOS+Windows |
| **Hide-before-action** | Claude Desktop — hide the window so it can't see itself | ✅ Shipping |
| **Picture-in-Picture** | Microsoft UFO² | ✅ Open source |
| **Background windows, doesn't take your pointer** | Claude Cowork on macOS 15+ | ✅ macOS only |
| **Full control vs background mode toggle** | Claude Desktop Settings → General | ✅ Shipping |
| **Multi-Pointer X** — true human+agent parallel pointers | UI Act (X11) | ✅ OSS only |
| **Auto-zoom on click / click ripple** | Pointerful, playwright-recast `.clickEffect()` | ✅ Shipping (screen-recording niche) |
| **Click-to-highlight + dim rest** | Spotlight, Playwright/Cobble `.highlight()` | ✅ Shipping, dev-tool niche |
| **Teach-by-demonstration with highlighting** | Rabbit R1 teach mode | ⚠️ Beta since Nov 2024; web-only, **3/6 success in hands-on**. *Not the differentiator it was promised to be.* |
| Accessibility magnifier / cursor follower | OS built-ins (GNOME Zoom, VoiceOver cursor) | ✅ 30-year-old commodity |

🔑 **There is no shipping commercial product that visually points at a screen
element for a *voice/LLM assistant* on desktop Linux.** The nearest analogues are
agent cursors (Cua, Claude) and dev-tool highlighters. **This is a genuinely open
niche.**

### ⚠️ The negative evidence on visual affordances

The most-upvoted complaint about **Claude Code's** mouse behaviour is that clicks
land *too close* and cause unintended actions — a focus-restoring click registered
as a Bash-permission rejection and **approved a `git worktree` deletion**
(anthropics/claude-code#75599).

**Visual pointing is not a solved UX problem — it's a trust liability if the point
is imprecise.** Accuracy is the whole ballgame.

---

## 9. Benchmarks — for context only

### OSWorld-Verified (361 tasks; humans 72.36%)

| System | Score | Source |
|---|---|---|
| Claude Mythos 5 / Fable 5 | **85.0%** | Anthropic self-report |
| Claude Opus 4.8 | 83.4% | Anthropic self-report, "revised harness" |
| Gemini 3.6 Flash | 83.0% | aggregator |
| Claude Sonnet 5 | 81.2% | Anthropic self-report |
| GPT-5.4 | 75.0% | OpenAI self-report |
| OpenAI Operator (CUA) | 32.6% | Jan 2025, 50 steps |

⚠️ Aggregators show a *different* top set (Qwen3.8 Max 86.1%, Muse Spark 1.1 80.8%,
Holo3) we **could not verify** against primary sources.

### OSWorld 2.0 (108 long-horizon tasks, ~1.6 h median human, ~318 tool calls)

**This is the number that actually matters, and it is brutal:**

| Config | Binary | Partial | Cost/task |
|---|---|---|---|
| Claude Opus 4.8 (max thinking, batched) | **20.6%** | 54.8% | ~$72 |
| Claude Opus 4.7 (batched) | 18.2% | 48.9% | ~$34 |
| GPT-5.5 (batched) | 13.0% | 49.5% | ~$26 |
| Claude Sonnet 4.6 (max) | 8.3% | 41.5% | ~$22 |
| Kimi 2.6 / MiniMax M3 | 4.6% | ~22% | $7 / $2 |

Snorkel AI independently reproduced Claude Opus 5 at **31.43% binary / 68.31%
partial**.

### AndroidWorld — mobile is essentially solved, desktop is not

Qwen3.8-Omni-Flash 87.1% · Qwen3.8 Max 85.3% · Qwen3.8-27B 81.9% (all Alibaba
self-report) · Gemini 2.5 Computer Use 69.7% · Claude Opus 4.6 Max 62% · UI-TARS
46.6% · GPT-4o 47.4%. Humans ~80%.

⚠️ **That gap is the whole story.**

---

## 10. Announced / not usable

| Item | Announced | Reality |
|---|---|---|
| Claude Computer Use on Linux | Implied by "Linux beta" | **Explicitly absent.** No date |
| ChatGPT agent on Linux desktop | — | **No primary doc either way.** Unknown |
| Antigravity 2.0 Linux | Marketed cross-platform | apt/rpm abandoned, AppArmor freeze on Ubuntu 24.04, tarball only |
| **HUMAIN OS × KORA** — AI-native Linux OS, on-device 26B on Qualcomm NPU, "agentic computer-use" | LEAP 2026, 2026-08-31 | Early demo. **Commercially available 2027** |
| Gemini Spark | I/O 2026 | Trusted-tester rollout. Not GA |
| **Mariner Studio** | Google 2026 roadmap | Roadmap only; **Mariner killed 2026-05-04** |
| Gemini agent marketplace | Q4 2026 roadmap | — |
| Cross-device sync desktop↔Android | Q3 2026 roadmap | — |
| **Canonical/Ubuntu AI tiers** — on-device STT, "AI agent workflows" | Announced 2026-04-27, staged over ~1 year | Phase 1 in progress; Ubuntu 26.10 targeting "context-aware desktop". **2027+** |
| KDE "Kadai" — per-user encrypted AI kernel | KDE 30 talk, Akademy Sep 2026 | **A question, explicitly not a roadmap** |
| Cua native Wayland | Preview flag | Screen capture + full AT-SPI parity still landing |

---

## 11. Strategic conclusion

1. **The Linux gap is real and it is the entire market.** Zero commercial vendors
   ship Linux desktop control. Verified from primary vendor docs.

2. **Our three-tier design already matches the 2026 state of the art.** We are not
   behind on architecture.

3. **The OCR tier is improvable at near-zero cost.** PP-OCRv4 → **v6** (2–3× faster,
   better slanted/vertical handling, same Apache-2.0 wrapper).

4. **Our largest genuine gap is that OCR can't see icons** — and text grounding is
   commoditized. Options: OmniParser MIT path, a self-rendered Set-of-Marks
   overlay (orthogonal, no detector needed), or **AT-SPI2, which is free, exact,
   works on X11 and Wayland, and which we are not using at all**.

5. **AT-SPI would give us the thing pixel paths can't: verification.** A pixel
   agent infers success; an AT-SPI agent proves it.

6. **The pointer overlay is a differentiator, not a gimmick** — but only if it is
   accurate. Claude Code's users file P1s about imprecise clicks landing on
   permission prompts.

7. **Do not chase benchmark parity.** OSWorld 2.0 best-in-class is 20.6% binary at
   ~$72/task. Nothing we build will be measured against that and win. **The
   defensible pitch is the opposite: local, free, private, sub-100ms, works offline,
   no API key, no sandbox** — the axis every commercial vendor structurally cannot
   compete on because it requires them to ship a Linux screen-control backend they
   have no reason to build.

---

## 12. Verification gaps

- Whether the Aug 2026 Linux ChatGPT app exposes agent mode.
- **Any published CPU latency for OmniParser V2 or any GUI-grounding VLM.**
- Aggregator-reported 2026 frontier scores (Qwen3.8 Max, Muse Spark, Holo3) lack
  primary-source confirmation.
- AndroidWorld top-3 are vendor self-reports and are tied at 97.4% in one aggregator
  versus 85% in another — the benchmark's dynamic task instantiation makes
  cross-source comparison unreliable.