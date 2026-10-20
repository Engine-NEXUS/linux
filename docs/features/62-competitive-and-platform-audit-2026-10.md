# 2026-10 Competitive & Platform Audit — Findings, Decisions & Work Order

**Date:** 2026-10-02
**Status:** Research complete. Nothing implemented yet.
**Scope:** Ten-angle audit of NEXUS against the live state of the art (Oct 2026),
covering competitive landscape, Linux platform constraints, voice UX, model
selection, security, and distribution.
**Companion compendium:** [`Engine-NEXUS/NEXUS-PAPERS` → `research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
**Detailed research:** [`docs/features/research/11-19`](research/README.md)

---

## 1. What this document is

Ten parallel research agents were run, each scoped to one angle, each required to
separate **shipping now** from **announced but unusable**, with source URLs, dates
and measured numbers. Vendor self-claims are labelled as such.

This file is the **engineering decision record**: what we found, what we decided,
and what we will do. The evidence lives in [`research/11-19`](research/README.md).

**Two recommendations made earlier in this cycle are retracted on the evidence.**
See §6.

---

## 2. Three findings that change the plan

### 2.1 🔴 BLOCKER — the orb architecture cannot work on Linux Wayland

`src-tauri/src/dyn_windows.rs:43-50` creates the `main` window with
`always_on_top: true`, `transparent: true`, `decorations: false`, then
`builder.maximized(true)` at `:174` to make a fullscreen work-area overlay, with
click-through via `set_ignore_cursor_events`.

On Wayland every one of those is broken:

| Capability we rely on | Reality | Evidence |
|---|---|---|
| `always_on_top` | **Empty function** — silently nothing | GTK 3.24.50 `gdk/wayland/gdkwindow-wayland.c:4631-4634`; vtable `:5125-5126` |
| `set_position` | **Impossible by design** | w3c/window-management#68: *"Wayland was explicitly designed to prohibit clients from… changing their global window coordinates"* |
| Click-through on **GNOME/Mutter** | **Broken** — events arrive indefinitely | electron/electron#51808; reproduced 170×; KWin works (30–290 ms, 3/3) |
| `set_ignore_cursor_events` | **Live panic**, unfixed upstream | `tao 0.37.1 event_loop.rs:456` — `.window().unwrap()` on an unrealized window |
| Shaped / non-rectangular window | **Empty function** | `gdkwindow-wayland.c:3873` |
| `set_visible`, shadow, `windowEffects`, `backgroundThrottling` | Unsupported on Linux | Tauri `WindowConfig` docs |

Open upstream issues, all still open: `tauri#14913`, `tauri#15913`, `tao#1316`,
`tauri#15914`, `tauri#3117`, `tao#1134`.

**The sanctioned escape does not save us.** `wlr-layer-shell`:
- Tauri exposes no builder hook for it (`tauri#15913`, open since 2026-08-25) —
  it must initialise before window realisation
- `gtk4-layer-shell` needs **GTK4**; Tauri v2 on Linux is **GTK3**
  (`wry#1767` port unfinished since 2026-07-14)
- **GNOME does not support layer-shell at all** (`mutter#1922`)

**Also expensive:** a fullscreen transparent window alpha-blends the whole screen
every frame, keeps the dGPU awake (`mutter#2969`), and collapses to CPU on
software rendering — gnome-shell at **100-150% CPU** on a 2-core VM under llvmpipe.

**We already document half of this.** `window_manager.rs:19-22`:
> *"Linux/Wayland no-op: the compositor forbids client positioning… Kept as a
> command so the settings slider + lib.rs handler compile; accepts and ignores the
> values."*

The `always_on_top` half is unfixed and uncommented.

**Decision:** replace the fullscreen overlay with a **~120×120 undecorated
transparent window + tray icon**, and split X11 and Wayland into honest, separate
capability paths. Full analysis: [`research/11`](research/11-platform-constraints-wayland-tauri.md).

### 2.2 The 2026 design consensus is the opposite of a full-size animated orb

Google, Microsoft and Apple independently converged on: **small, monochrome,
collapses hard, geometry carries state instead of personality.**

- **Microsoft removed the personality on purpose.** Copilot redesign 2026-05-28
  (Jon Friedman, CDO): *"how little color it has,"* panels *"which collapse when not
  in use."* Consumer Copilot is still *"bright, colorful and (occasionally) blobby."*
- **Google's Neural Expressive** drew **~26% actively negative** (n=445: 49% pos /
  13% disliked merged tools / 13% sidebar too crowded). Praise is about *looks*;
  criticism is about *usability regression*.
- **Apple Liquid Glass** needed 3 betas, 12 months, and a user-facing slider to
  admit it was unreadable.
- **The Alexa+ failure is our exact risk:** a personality surface grafted onto a
  control surface. *"Looks like you're already stocked up on that creamy goodness!"*
  **"Apple Reminders: one tap. Alexa: a diatribe."**

**Strongest positive evidence — Sidekick** (UIST 2026, arXiv 2607.17527, n=30):

| Condition | Score | Agent errors/session |
|---|---|---|
| Working alone | 115 | — |
| Peripheral **text** | 139 | — |
| Chat-only feedback | 148 | 2.51 |
| **Ambient colour + sound + spatial replay** | **162** | **1.31** |

Errors cut **48%**. **Peripheral text alone barely beat doing nothing** —
*"Text in the corner is still text."* **Perceived workload was identical across all
three UIs.**

Full analysis: [`research/12`](research/12-ambient-ui-design-language-2026.md).

### 2.3 🔴 Endpointing is a bigger bug than the visuals

**SRI Enhanced End-of-Turn Detection (2021-12):**

| Latency | Premature cut-off, fixed silence gate |
|---|---|
| 500 ms | ~100% |
| 500 ms + pre-pausal acoustics | ~36% |
| **100 ms + pre-pausal acoustics** | **20.3%** |

Two corpora, 4,409 + 8,061 non-final pauses, natural open-ended elicitation.

**This is our documented "Speech Onset Decapitation" bug and it has a known fix.**
Human floor is **+208 ms** (Stivers, PNAS 2009, 10 languages), so 100 ms
endpointing is achievable. Our current fixed 500 ms gate leaves ~200 ms of latency
*and* a ~100% cut-off rate on the table.

**Decision:** replace the fixed silence gate with pre-pausal-acoustic endpointing at
100 ms. **This outranks all visual work.** Full analysis:
[`research/13`](research/13-voice-ux-conversation-latency.md).

---

## 3. Per-area verdicts

| Area | Verdict | Headline | Detail |
|---|---|---|---|
| **Windowing / Tauri** | 🔴 Blocker | `always_on_top` empty on Wayland | [11](research/11-platform-constraints-wayland-tauri.md) |
| **OAuth / security** | 🔴 Blocker | 3 unauthenticated token endpoints | [17](research/17-security-oauth-prompt-injection.md) |
| **Intent layer** | 🔴 Reject Jev + Laya | Both fail our constraints | [15](research/15-nlu-intent-decision-models.md) |
| **Endpointing** | 🔴 Fix first | 100% → 20.3% cut-off available | [13](research/13-voice-ux-conversation-latency.md) |
| **Voice approval window** | 🔴 Injection vector | 5s auto-accept on voice keyword | [17](research/17-security-oauth-prompt-injection.md) |
| **Wake word** | 🟡 Head outdated | conv-attention: FPPH 8.50 → 0.08 | [14](research/14-voice-stack-sota-wakeword-vad-stt-tts.md) |
| **VAD** | 🟢 One flag from done | v6 already in our package | [14](research/14-voice-stack-sota-wakeword-vad-stt-tts.md) |
| **STT** | 🟢 Already right | verify the v2 checkpoint | [14](research/14-voice-stack-sota-wakeword-vad-stt-tts.md) |
| **TTS** | 🟡 Dead upstream | pin `kokoro-onnx`; Piper is now GPL-3.0 | [14](research/14-voice-stack-sota-wakeword-vad-stt-tts.md) |
| **Computer-use** | 🟢 Moat is real | **no commercial vendor ships Linux desktop** | [16](research/16-computer-use-linux-gap-perception.md) |
| **Screen perception** | 🟢 Improvable | PP-OCRv4 → v6; **AT-SPI unused** | [16](research/16-computer-use-linux-gap-perception.md) |
| **Distribution** | 🟡 Flathub AI policy | Tauri can't use GlobalShortcuts portal | [18](research/18-distribution-linux-packaging.md) |
| **Dev-tool UI** | 🟢 Tailwind v4.3 safe | 4 AI-slop tokens ship as first-class | [19](research/19-devtool-ui-design-systems.md) |

---

## 4. Scope change: multilingual dropped

Hindi/Telugu is **out of scope** as of 2026-10-02. Net simplification:

- **Removes the largest unmeasured risk.** No verified Telugu benchmark exists for
  *any* sub-6B model as of Oct 2026. Dropping the claim removes the gap rather than
  shipping around it.
- **Unlocks the published literature.** English-only lets CLINC150 / Banking77 /
  HWU64 numbers apply directly.
- **Enables the SiFT footprint optimisation.** DeBERTa-v3-small+SiFT shares the
  128K-vocab embedding — a 22M backbone that beats the 44M plain model. A large
  share of the RAM saving comes from not needing a multilingual vocab.
- **Wake word too.** livekit-wakeword's multilingual DET curves are worse because
  the frozen Google speech-embedding is English-dominant.

**Revisit when** there is a verified benchmark for a model we already ship, and a
use case that genuinely requires it.

---

## 5. Work order

Ordered by (blocks shipping) × (effort), not by interest.

### P0 — close the security hole

**Rationale: live and exploitable now, on a public endpoint, with a legal clock
(GDPR Art. 33 = notify within 48h of discovery if exploited). Small,
self-contained, blocked on no design decision.**

**Status: 5 of 7 items implemented.** See
[`docs/changes/46-device-auth-and-oauth-csrf-fix.md`](../changes/46-device-auth-and-oauth-csrf-fix.md)
for the full change record, verification matrix and deploy steps.

| # | Task | Status |
|---|---|---|
| 1 | Stop the token endpoints being anonymous | ✅ **done — differently.** The endpoints were **authenticated, not deleted.** Deleting them breaks `github_cmd.rs` (octocrab calls GitHub on-device) and `auth_vault.rs` (MCP needs Google/Swiggy locally). The broker is unchanged; it is just no longer anonymous. Rationale in the changelog §2 |
| 2 | Make `device_token` a real credential | ✅ **done** — SHA-256 hashed, expiry + revocation enforced in the SQL predicate. Migration in `server/worker/migrations/0001_device_auth.sql` |
| 3 | Random, single-use, TTL'd OAuth `state` | ✅ **done** — 32 random bytes, hashed at rest, 10-min TTL. `server/worker/src/auth.ts`. Also fixes the deep-link leg (`/oauth/exchange`), which previously had **no CSRF check at all** |
| 4 | Deny-by-default route middleware | ✅ **done** — `PUBLIC_ROUTES` allowlist; `requireAuthUserId()` is the only identity source. Six handlers that trusted `user_id` from the query/body now take it from the device |
| 5 | Remove `Access-Control-Allow-Origin: *` | ✅ **done** — reflects request origin with `Vary: Origin` |
| 6 | Retroactively check Workers Logs for `user_id` enumeration | ⚠️ **not done** — needs Cloudflare access. **Do this before declaring the incident closed** |
| 7 | Fix the 5s voice-approval window (injection vector) | ❌ **not done** — needs a Verifiable-Action-Card style default-deny redesign, not a patch. An injected instruction in an email can still say "say proceed" |

**Bonus, not in the original work order:** the renderer no longer touches the
Worker at all. `ArchitectApp.tsx` and `setup/oauth.ts` made **8 direct calls**
that would otherwise have needed a credential in the WebView. They now go through
`worker_request`, an allowlisted Rust proxy — the credential stays in the OS
keyring. `npm ci` aside, this removed the last reason for a token to exist in the
renderer.

**What is still true:** the tokens still transit the Worker. They are no longer
readable by an attacker, which is what the vulnerability *was*. Moving them
fully onto the device (the recommended architecture, §"What we are well-positioned
on" in `research/17` §11) is P2 work.

### P1 — fix the actual voice bugs

**Status: 3 of 7 items implemented (8, 9, 10).** Full record, measurements and
the two bugs found during wiring in
[`docs/changes/47-…`](../changes/47-p1-voice-latency-vad6-ppocr6-adaptive-endpoint.md).

| # | Task | Status |
|---|---|---|
| 8 | **Endpointing:** pre-pausal acoustics | ✅ **done.** Adaptive cut-off with a pre-pausal inclusion buffer. 3000ms → ≤400ms, falling back to the old redemption whenever the tail is not unambiguously dead. 17 unit tests, mutation-verified. **Thresholds are uncalibrated** — the repo has no speech fixtures |
| 9 | **VAD v6** | ✅ **done.** v5 → v6 via `vad-web` 0.0.31. `npm ci` re-verified |
| 10 | **PP-OCRv6** | ✅ **done**, and **3.1x faster than the build it replaces** (4138ms → 1312ms). Note: the upstream default (`small`) would have been **27% slower** than v4; `tiny` is pinned instead |
| 11 | **AT-SPI tier** | ❌ not started — free, exact, works on X11 *and* Wayland. **52x faster than screenshot+OCR (192 ms vs 10,034 ms)** and gives free pre/post-conditions. Largest remaining perception gap |
| 12 | **Wake word retrain** with conv-attention head | ❌ not started. AUT 0.0720 → 0.0012, FPPH 8.50 → 0.08, recall 68.6% → 86.1%. Apache-2.0 |
| 13 | **Prosody gate** | ❌ not started. Aware UIST'21: **F1 0.93 vs Amazon Echo's 0.56** |
| 14 | **Pre-roll suppression** buffer, 300–500 ms | ❌ not started. Google patent 11,557,293. Distinct from item 8, which added a pre-roll *inclusion* buffer for endpointing |

### P2 — visual identity

| # | Task | Notes |
|---|---|---|
| 15 | **Small-window orb rebuild** | Unblocks Linux. Gate Wayland-unsupported calls on `XDG_SESSION_TYPE` |
| 16 | Guard the `set_ignore_cursor_events` panic — `.ok()`, never `.unwrap()` | `tao` bug |
| 17 | Fix the **two lying settings** — Orb Position (Wayland no-op) and Orb Size (read by nothing) | Wire or remove |
| 18 | **Collapsed-by-default orb**, small, monochrome, state via geometry | Every platform holder |
| 19 | **Flatten 5 token namespaces to 1** | 46 unused tokens, 112 orphaned classes, 998-line dead settings window |
| 20 | **Setup wizard → dark** | It is the first-run impression, and white next to 6 dark windows |
| 21 | Ship an **opacity/dim control in v1** | Liquid Glass needed 3 betas to learn this |

---

## 6. 🔴 Retracted recommendations

Two things advised earlier in this cycle are **withdrawn on the evidence.**

| Earlier advice | Retraction |
|---|---|
| *"Wire up `speakSeq` for mouth animation / visemes"* | **Do not build visemes.** Böck et al. (TU Dortmund, 2023, n=44): audio sync had **no significant effect on any rating** (p = .62). Synthesized expressions beat *tracked* capture (p < .001). Audio-only ≈ audio+video (p < .001). Some users **prefer lip-sync off**. And desync is *worse than nothing* — render ahead, never behind. |
| *"Give thinking a slow ambient breathing animation"* | Partly. **Apple HIG: avoid oscillation near 0.2 Hz** (≈5 s period) — *"people can be very sensitive to this frequency."* Our `.orb` fallback at `styles.css:84-93` has `breathe 3s` — dead centre in the danger band. |

Correct shape for state is **OpenAI ChatGPT Dots' three-axis decomposition**:
`activity` ⊥ `emotion` ⊥ `speech` are **independent inputs**. Our 4-state orb is
exactly the `activity` axis — and three of the four states share one animation
segment (`Avatar.tsx:40-46`). **The fix is orthogonality, not three more
animations.**

---

## 7. What we are genuinely well-positioned on

Not everything is bad news.

1. **The Linux gap is real and it is the entire market.** Verified from primary
   vendor docs, not inferred:
   - Anthropic, `code.claude.com/docs/en/desktop-linux`: *"Computer Use: app and
     screen control isn't available on Linux."*
   - Anthropic, `code.claude.com/docs/en/computer-use`: *"Computer use in the CLI
     is not available on Linux or Windows."*
   - OpenAI Help: agent mode is macOS/Windows desktop only.
   - Google: Gemini in Chrome is Chromebook Plus / Mac / Windows.
   - Microsoft: UFO²/UFO³ are Windows/UIA-only.

2. **Our three-tier perception stack already matches the 2026 architecture.**
   `ocr_server.py`'s docstring describes *UIA tree (~5 ms) → local OCR (~300 ms) →
   vision LLM (~1–3 s)* — verbatim the router every 2026 writeup converges on.
   **We are not behind.**

3. **We are leaving free accuracy on the table by not using AT-SPI.** A11y-tree
   input scores **20.7% vs 7.0% screenshot-only** on 358 OSWorld tasks, at **22% of
   the token count**, and works identically on X11 and Wayland. Our `pointer.rs`
   already queries the foreground window title — the whole tree is one D-Bus hop away.

4. **The wake-word work is a real moat.** 3,300-file benchmark, vocal-friction
   negatives, soundalike negatives. The architecture upgrade to beat is available.

---

## 8. Cost of inaction

| Blocker | Cost of leaving it |
|---|---|
| **Wayland orb** | The product's headline feature does not work on the default GNOME Linux desktop. Every design conversation downstream is built on an architecture that cannot function. |
| **Unauthenticated token endpoints** | Live GitHub + Google (gmail.readonly, gmail.send, drive) + Swiggy tokens retrievable by anyone who learns a `user_id`. Direct MCP 2026-07-28 spec violation. Possible GDPR Art. 33 obligation and Google CASA assessment scope. |

---

## 9. Verification gaps we could not close

Stated plainly so nobody builds on them:

- **Whether the orb looks good.** We can verify structure, tokens, contrast and
  regressions. **We cannot judge visual quality from code.** Validate with a
  server-side flag, old/new on two devices (Google's Neural Expressive method).
- **No trustworthy x86_64 CPU tok/s table exists for any 2026 small model.** Run
  `llama-bench` on our target hardware.
- **livekit-wakeword's benchmark is vendor self-reported** on their own phrase, with
  no independent reproduction. The architecture argument is sound and matches the
  academic literature, but treat the 100× FPPH figure as directional.
- **All WER numbers** come from different datasets. ⚠️ CORAAL: every model roughly
  quintuples its error rate on African American English (Parakeet 2.77% TEDLIUM →
  **15.57% CORAAL**). Test on our users' audio.
- **No published CPU latency for any GUI-grounding VLM.**
- **Whether the Aug 2026 Linux ChatGPT app exposes agent mode.**
- **Flathub's AI policy is new (2026-09-04) and may change.** Re-read before
  submitting.
- **Confirmation-fatigue threshold for voice: no study exists.** Our confirmation
  gate is **ahead of the published evidence base**, not validated by it.
- **No published number exists for the post-TTS wake re-arm grace period.** Ours is
  3000 ms on Linux (`tts.rs`) and 500 ms on Windows — unsupported by any source.