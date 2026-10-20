# 2026-10 Competitive & Technology Audit — Executive Summary

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.


**Date:** 2026-10-02
**Scope:** Ten-angle research audit of NEXUS against the live state of the art
(Oct 2026), covering competitive landscape, platform constraints, voice UX,
model selection, security, and distribution.
**Method:** 10 parallel research agents, each scoped to one angle, each required
to separate *shipping now* from *announced but unusable*, with source URLs,
dates, and measured numbers. Vendor self-claims are labelled as such.
**Scope change:** Multilingual (Hindi/Telugu) is **explicitly out of scope** for
this cycle. See §7 — this removes the single largest measurement risk in the stack.

---

## 1. The three findings that change the plan

### 1.1 The orb architecture cannot work on Linux. At all.

`src-tauri/src/dyn_windows.rs:43-50` creates the `main` window with
`always_on_top: true`, `transparent: true`, `decorations: false`, then
maximizes it to a fullscreen stage (`:174`) and drives click-through with
`set_ignore_cursor_events`. On Wayland, every one of those is broken:

| Capability relied on | Reality on Wayland | Evidence |
|---|---|---|
| `always_on_top` | **Empty function.** Silently does nothing. | GTK 3.24.50 `gdk/wayland/gdkwindow-wayland.c:4631-4634` — `gdk_wayland_window_set_keep_above() { }`, wired to vtable at `:5125-5126` |
| `set_position` / `x,y` | **Impossible by design.** Security model, not a bug. | w3c/window-management#68 (pq): *"Wayland was explicitly designed to prohibit clients from introspecting or programmatically changing their global window coordinates"* |
| Click-through on **GNOME/Mutter** | **Broken.** Events keep arriving indefinitely. | electron/electron#51808 — Mutter may never send `wl_pointer.leave`; 170 replications with divergent results. KWin works (30–290 ms, 3/3) |
| `set_ignore_cursor_events` | **Live panic.** Unfixed upstream. | `tao 0.37.1 src/platform_impl/linux/event_loop.rs:456` — `.window().unwrap()` panics on unrealized window |
| Shaped / non-rectangular window | **Empty function.** No shaped orb, ever. | `gdk_wayland_window_shape_combine_region() { }` at `gdkwindow-wayland.c:3873` |
| `set_visible` | Unsupported — no Wayland API to hide/show | tauri#6162 |
| Window shadow, `windowEffects`, `backgroundThrottling` | Unsupported on Linux | Tauri `WindowConfig` docs |

Open upstream issues, all still open at time of writing:
`tauri#14913` (position + always-on-top silently no-op, filed 2026-02-08),
`tauri#15913` (layer-shell not exposed, filed 2026-08-25), `tao#1316`,
`tauri#15914`, `tauri#3117`, `tao#1134`.

**The sanctioned escape (`wlr-layer-shell`) does not save us:**
- Tauri has no builder hook to initialize it — it must happen *before* window
  realization (tauri#15913). Workarounds are vendor-patching tao or transplanting
  the wry WebView into a self-created `GtkWindow` (documented to cause ghosting).
- `gtk4-layer-shell` requires **GTK4**; NEXUS is on **GTK3** (Tauri v2 = GTK3).
  The GTK3 `gtk-layer-shell` is in maintenance mode.
- **GNOME does not support layer-shell at all** (mutter#1922, open since 2021).
- SDL's maintainers call it protocol abuse: *"Using layer-shell to implement
  always-on-top would be a huge abuse of the protocol"* (libsdl-org/SDL#5779).

**Also expensive:** a fullscreen transparent window alpha-blends the whole screen
every frame, keeps the dGPU awake (mutter#2969), and degrades to CPU on software
rendering — gnome-shell at **100-150% CPU** on a 2-core VM under llvmpipe.

> **Conclusion:** the fullscreen transparent stage must be replaced. See `02` for
> the recommended architecture and the X11/Wayland split.

### 1.2 The 2026 visual consensus is the opposite of a full-size animated orb

Google, Microsoft, and Apple independently converged on the same answer:
**small, monochrome, collapses hard, geometry carries state instead of personality.**

- **Microsoft removed the personality on purpose.** Copilot's 2026-05-28
  redesign (Jon Friedman, Chief Design Officer): *"how little color it has,"*
  side panels *"which collapse when not in use."* Consumer Copilot remains
  *"still bright, colorful and (occasionally) blobby"* — two Copilots with
  different personalities at once, which Engadget called evidence that
  *"Microsoft's AI strategy is very much in flux."*
- **Google's Neural Expressive** (2026-05-19, Android/iOS/web) drew **~26%
  actively negative** in the largest poll (n=445: 49% positive / 13% disliked
  the merged tools menu / 13% said sidebar too crowded). The praise is almost
  entirely about *looks*; the criticism is about *usability regression*.
- **Apple Liquid Glass** needed 3 betas, 12 months, and a user-facing slider to
  admit it was unreadable. Jun 2025: *"Beautiful and Hard to Read."* Oct 2025:
  added Clear/Tinted toggle. Jun 2026: rebuilt foundations for readability.
- **The Alexa+ shopping-list failure is our exact risk.** A personality surface
  grafted onto a control surface. Asked to add sour cream to a list, Alexa+
  replied *"Looks like you're already stocked up on that creamy goodness! Sour
  cream is already chilling in your cart."* User: *"No. Thank you."* The Verge
  concluded: **"Apple Reminders: one tap. Alexa: a diatribe."**

The strongest positive evidence is **Sidekick** (UIST 2026, arXiv 2607.17527,
n=30, U. Michigan + Adobe): a delegated agent injected 2 errors per column while
users did arithmetic.

| Condition | Score | Agent errors/session |
|---|---|---|
| Working alone | 115 | — |
| Peripheral **text** display | 139 | — |
| Chat-only feedback | 148 | 2.51 |
| **Sidekick (ambient colour + sound + spatial replay)** | **162** | **1.31** |

- Errors cut **48%**. Time-per-check 8.6 s vs 12.8 s. Self-rated ability to catch
  errors 2.39 → 3.98 (5-pt).
- **Peripheral text alone barely beat doing nothing.** Nielsen: *"Text in the
  corner is still text."*
- **Perceived workload was identical across all three UIs** — "we must look at
  the hard data and not go by users' subjective impressions."

Nielsen's four derived rules: status goes ambient · sound signals *change*, not
content · history goes spatial · **interrupt only for real decisions**.

> ⚠️ Caveat: n=30, primary task was arithmetic, read via Nielsen's summary not the
> paper. Treat 48% as directional.

### 1.3 Endpointing is a bigger bug than the visuals

SRI's Enhanced End-of-Turn Detection (Dec 2021) is the most actionable result
in the entire audit:

| Latency | Premature cut-off, fixed silence gate |
|---|---|
| 500 ms | ~100% |
| 500 ms + pre-pausal acoustics | ~36% |
| **100 ms + pre-pausal acoustics** | **20.3%** |

Two corpora, 4,409 non-final pauses (Template) + 8,061 (Freeform), natural
open-ended elicitation under cognitive load. This is NEXUS's documented
"Speech Onset Decapitation" bug, and it has a known fix.

The human floor: **+208 ms** mean response offset (Stivers et al., PNAS 2009,
10 languages). Moshi's authors benchmarked 160 ms against Stivers' 230 ms
pooled figure as the design target. So **100 ms endpointing is achievable** — the
current fixed 500 ms gate is simply leaving ~200 ms of latency and a 100%
cut-off rate on the table.

⚠️ Honest caveat: Heldner & Edlund found **40-41.7% of human turn transitions are
overlaps**, with only 0.4-0.7% being clean no-gap-no-overlap handoffs. You cannot
pick a threshold that never cuts people off *and* never feels slow. For a
wake-word-gated mostly-command pipeline, cut-off is the far more damaging failure
because it forces re-utterance *and* a wake-word re-fire. Prefer the generous end.

---

## 2. What we got wrong in earlier advice (self-correction)

Two prior recommendations are **retracted** on the evidence:

| Earlier advice | Retraction |
|---|---|
| "Wire up `speakSeq` for mouth animation / visemes" | **Do not build visemes.** Böck et al. (TU Dortmund, 2023, n=44): audio sync had **no significant effect** on any rating (χ²(1)=0.25, p=.62). Synthesized expressions rated **more** natural than tracked capture (b=−0.5, p<.001). Audio-only ≈ audio+video for naturalness (Aspöck DAGA 2021, p<.001). Some users **prefer lip-sync off** (Abdulrahman 2024). And desync is *worse than nothing* — render ahead, never behind (IJMRS 2015, n=113). |
| "Give thinking a slow ambient breathing animation" | Partly. Apple HIG: **avoid oscillation near 0.2 Hz** (≈5 s period) — *"people can be very sensitive to this frequency."* NEXUS's `.orb` fallback already has `breathe 3s` — dead centre in the danger band. Use non-oscillating or much slower motion. |

The correct shape for state is **OpenAI ChatGPT Dots' three-axis decomposition**:
`activity` (idle/listening/processing/working/speaking) ⊥ `emotion` ⊥ `speech`
are **independent inputs**. NEXUS's 4-state orb is exactly the `activity` axis —
missing `emotion` and `speech`, but more importantly missing the *orthogonality*.

---

## 3. Per-area verdicts

| Area | Verdict | Headline |
|---|---|---|
| **Windowing / Tauri** | 🔴 Blocker | `always_on_top` is an empty function on Wayland. Fullscreen overlay must go. See `02`. |
| **Wake word** | 🟡 Head is outdated | `livekit-wakeword` conv-attention head: AUT 0.0720→**0.0012**, FPPH 8.50→**0.08**, recall 68.6%→**86.1%**. Apache-2.0, Rust crate v0.1.3. See `04`. |
| **VAD** | 🟢 One flag from done | Already shipping v5. `@ricky0123/vad-web` bundles `silero_vad_v6.onnx` — 16% fewer errors on noisy data. See `04`. |
| **STT** | 🟢 Already right | Moonshine v2 (arXiv 2602.12241) — verify we have the **v2** sliding-window checkpoint, not v1. See `04`. |
| **TTS** | 🟡 Good model, dead upstream | `hexgrad/kokoro` last push 2025-08-06. Pin `thewh1teagle/kokoro-onnx` instead. Piper went **GPL-3.0**. See `04`. |
| **Intent layer** | 🔴 Reject Jev + Laya | Both fail our constraints. Fine-tune DeBERTa-v3-small+SiFT (22M) on existing rows. See `05`. |
| **Computer-use** | 🟢 Moat is real | **No commercial vendor ships Linux desktop control.** Verified from primary vendor docs. See `06`. |
| **Screen perception** | 🟢 Competent, improvable | On PP-OCRv4; **v6 is out** (2-3× faster). **Not using AT-SPI** — the free, exact, X11+Wayland tier. See `06`. |
| **Security** | 🔴 Blocker | Three unauthenticated token endpoints. Also a **direct MCP spec violation**. See `07`. |
| **Distribution** | 🟡 Flathub has an AI policy | Disclosure-based since Sep 2026. Tauri can't use the GlobalShortcuts portal. See `08`. |
| **Dev-tool UI** | 🟢 Tailwind v4.3 is safe | Tailwind Labs joined Shopify Sept 2026. We ship the 4 most-clocked AI-slop tells as first-class tokens. See `09`. |

---

## 4. Recommended order of work

Ordered by (blocks shipping) × (effort), not by interest.

### P0 — unblock Linux entirely
1. **Replace the fullscreen overlay with a small window + tray.** ~120×120
   transparent, undecorated, non-resizable. Cuts blended pixels ~200-400× and is
   the only orb architecture Tauri supports everywhere. Split X11 and Wayland
   into honest, separate capability paths.
2. **Guard the `set_ignore_cursor_events` panic.** Never call before realization;
   `.ok()` not `.unwrap()`. Better: on Linux, stop calling it — a small window
   with only the orb's own pixels interactive has nothing to click through.

### P0 — close the security hole
3. **Delete `/oauth/github-token`, `/oauth/google-token`, `/oauth/swiggy-token`.**
   Replace with capability-scoped RPC that returns *results*, never tokens.
4. **Stop using client-supplied `user_id` as an authorization input.** This is a
   direct violation of MCP's 2026-07-28 spec.
5. **Store tokens in Secret Service**, not in the Worker's public D1. This also
   likely removes us from Google **annual CASA assessment** scope.

### P1 — fix the actual voice bugs
6. **Endpointing**: pre-pausal acoustics, 100 ms target.
7. **VAD v6** (one config flag), **PP-OCRv6**, **AT-SPI tier**.
8. **Wake word**: retrain with the conv-attention head; add the **prosody gate**
   (Aware UIST'21: F1 **0.93** vs Amazon Echo's **0.56**).

### P2 — visual identity
9. **Collapsed-by-default orb.** Small, monochrome, state via geometry.
10. **Flatten 5 token systems to 1.** Delete the 998-line unreachable settings UI,
    112 orphaned classes, and the four AI-slop tokens.
11. **Ship an opacity/dim control in v1** if the orb is ever translucent —
    Apple needed three betas and 12 months to admit that lesson.

---

## 5. What we are actually well-positioned on

Not everything here is bad news. Three genuine advantages:

1. **The Linux gap is real and it is the whole market.** Verified from primary
   vendor documentation, not inferred:
   - Anthropic, `code.claude.com/docs/en/desktop-linux`: *"Computer Use: app and
     screen control isn't available on Linux."*
   - Anthropic, `code.claude.com/docs/en/computer-use`: *"You're on macOS.
     Computer use in the CLI is not available on Linux or Windows."*
   - OpenAI Help: agent mode is macOS/Windows desktop only.
   - Google: Gemini in Chrome is Chromebook Plus / Mac / Windows.
   - Microsoft: UFO²/UFO³ are Windows/UIA-only.

   The only shipping Linux desktop control is open-source: `Cua Driver`
   (X11 GA, native Wayland behind a flag), `computer-use-linux`, `Deskbrid`,
   `PeekabooX`.

2. **Our three-tier perception stack already matches the 2026 architecture.**
   `ocr_server.py`'s own docstring describes *UIA tree (~5 ms) → local OCR
   (~300 ms) → vision LLM fallback (~1-3 s)*. That is verbatim the router every
   2026 writeup converges on (Terminator's `VisionType`, UFO²'s hybrid detector,
   Cua's AT-SPI→XTEST cascade). **We are not behind.**

3. **The accessibility tier is where we are leaving free accuracy on the table.**
   A11y-Compressor's ablation (358 OSWorld tasks): screenshot-only 7.0%,
   linearized a11y tree 15.6%, **compressed a11y tree 20.7%** — at 22% of the
   original token count. ScreenMemory measured the latency gap: UIA scan
   **192 ms** vs screenshot+OCR **10,034 ms** — a **52× speedup**. And an
   accessibility tree gives you **free preconditions and postconditions**
   (`showing`/`enabled` before, `pressed`/`expanded` after) that a pixel agent
   can only infer. On Linux, AT-SPI rides D-Bus and works **identically on X11
   and Wayland**.

---

## 6. Cost-of-inaction summary

The two blockers are independent and both are shipping-blocking:

| Blocker | Cost of leaving it |
|---|---|
| Wayland orb | The product's headline feature does not work on the default GNOME Linux desktop. Every design conversation downstream is built on an architecture that cannot function. |
| Unauthenticated token endpoints | Live GitHub + Google (gmail.readonly, gmail.send, drive) + Swiggy tokens retrievable by anyone who learns a `user_id`. Direct MCP spec violation. Possible GDPR Art. 33 breach-notification obligation and Google CASA assessment scope. |

---

## 7. Scope change: multilingual dropped

Hindi/Telugu is out of scope for this cycle. This is a **net simplification**:

- **Removes the largest unmeasured risk in the stack.** No verified Telugu
  benchmark exists for *any* sub-6B model as of Oct 2026 — Qwen3.5, Gemma 4,
  Ministral 3, SmolLM3, Phi-4, Nemotron 3 Nano 4B all unmeasured on Telugu.
  Dropping the claim removes that gap rather than shipping around it.
- **Unlocks better model selection.** English-only enables the published
  in-domain intent-classification literature to apply directly (Banking77,
  CLINC150, HWU64) instead of extrapolating from multilingual benchmarks.
- **Simplifies the wake-word retrain.** livekit-wakeword's multilingual DET
  curves are explicitly worse than English because the frozen Google
  speech-embedding is English-dominant. English-only gets the good numbers.
- **Revisit when:** there is a verified benchmark for a model we already ship,
  and a use case that genuinely requires it — not because the negative set
  once included Hindi audio.

---

## 8. Document index

| Doc | Contents |
|---|---|
| [`11-platform-constraints-wayland-tauri.md`](11-platform-constraints-wayland-tauri.md) | Tauri v2 / Linux Wayland capability matrix, the panic, the alternatives |
| [`12-ambient-ui-design-language-2026.md`](12-ambient-ui-design-language-2026.md) | 2026 design consensus, Motion specs, what failed and why |
| [`13-voice-ux-conversation-latency.md`](13-voice-ux-conversation-latency.md) | Turn-taking, latency ladder, state signalling, error recovery, confirmation |
| [`14-voice-stack-sota-wakeword-vad-stt-tts.md`](14-voice-stack-sota-wakeword-vad-stt-tts.md) | Wake word / VAD / STT / TTS state of the art with concrete upgrades |
| [`15-nlu-intent-decision-models.md`](15-nlu-intent-decision-models.md) | Jev + Laya verdict, the fine-tune path, local model landscape |
| [`16-computer-use-linux-gap-perception.md`](16-computer-use-linux-gap-perception.md) | The Linux moat, OCR/GUI-grounding SOTA, AT-SPI, pointer patterns |
| [`17-security-oauth-prompt-injection.md`](17-security-oauth-prompt-injection.md) | The token hole, MCP spec violations, prompt injection, threat models |
| [`18-distribution-linux-packaging.md`](18-distribution-linux-packaging.md) | Flathub AI policy, Tauri bundle reality, mic portal problems |
| [`19-devtool-ui-design-systems.md`](19-devtool-ui-design-systems.md) | Token architecture, dark-mode convention, AI-slop tells in our CSS |