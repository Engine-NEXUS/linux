# Distribution Channels & Linux Packaging — Flathub, Snap, and the AI Policy Problem

> **Part of the 2026-10 competitive & platform audit.**
> Decision record: [`docs/features/62-competitive-and-platform-audit-2026-10.md`](../62-competitive-and-platform-audit-2026-10.md)
> Companion compendium (identical content, canonical home):
> [`NEXUS-PAPERS → research/2026-10-competitive-audit/`](https://github.com/Engine-NEXUS/NEXUS-PAPERS)
>
> This file is **evidence**. It records what was verified on 2026-10-02, from
> where, and what could not be verified. It is not a work order — see the spec above
> for that.

> **What this covers:** Flathub's Generative AI policy, Tauri bundle reality, and the microphone-portal gap.


**Date:** 2026-10-02
**Scope:** How a small open-source Linux team actually reaches non-technical users
in Oct 2026. Channel ranking, Flathub's Generative AI policy, Tauri bundle
reality, and the microphone-portal gap.

**Full detail report:** `~/.local/share/opencode/nexus-linux-distribution-research.md`

---

## 1. The two findings that overturn the obvious plan

### 1.1 🔴 Flathub has an explicit Generative AI policy — and `AGENTS.md` is the problem

| Date | Event |
|---|---|
| **2026-05-29** | **Blanket ban** on AI-generated code |
| **2026-09-04** | PR #641 merged → relaxed to **disclosure-based** |

Current binding text requires us to **disclose AI-generated parts and their
extent**, states *"Reviewers may reject a submission, including without further
review"*, and **forbids agents from opening the submission PR or writing its
description or review replies.**

- **GNOME Circle paused new submissions** (2026-05-30) citing an AI-driven backlog
- **73% of AI-rejected repos were dead within months**
  ([audit](https://www.omgubuntu.co.uk/2026/07/flathub-ai-slop-ban-data))
- Reviewer sentiment did **not** soften. A *clean, exemplary* AI-assisted project
  was pre-emptively told *"I would deny your submission"*
  ([Discourse #12450](https://discourse.flathub.org/t/asking-community-and-maintainers-about-posting-my-tool-for-everyone-to-flathub/12450))

**What this means for us:** our `AGENTS.md` is a 79 KB document written in an
assistant-heavy style. It reads as machine-generated. It is now a submission risk.
It should be restructured into a human-authored doc — which is worth doing anyway.

### 1.2 🔴 Tauri cannot use the GlobalShortcuts portal — PR open, not merged

Our separate `register-hotkey.sh` step is **exactly the thing reviewers reject.**
A launchable app should not require a companion script for a core feature.

**Source:** Tauri PR for `GlobalShortcuts` portal support is open and unmerged as
of this audit.

---

## 2. Second-order landmines

- **No microphone portal exists.** `--socket=pulseaudio` grants playback **and** mic
  inseparably. The audio portal discussion has been open since 2021 with no dates.
- **"Host-dependent applications" rejection clause** — `espeak-ng` / `libasound`
  dependencies must be **vendored into the manifest**, not installed on the host.
- **"Insufficient development history"** — the same Discourse rejection cited
  history, *not* AI.
- **Review time is long and unpredictable:**
  - Vocalinux PR unreviewed for months
  - **YazSes closed 2026-08-13** after a reviewer said *"No thanks"* — **manifest
    built green, blocked only by a missing demo video**
- **PackageKit 2.0 has a full API break incoming.**

---

## 3. Channel ranking by effort-to-reach-user

| # | Channel | Effort | Time to user | Notes |
|---|---|---|---|---|
| 1 | **Flathub** | High | **3–12 months + rejection risk** | Only channel giving one-click install, auto-updates, zero terminal. **438M downloads/yr.** Flathub GPG-signs every build automatically. |
| 2 | PPA / own APT repo | Very high | 1–2 wks | Full control, no review, but no discoverability |
| 3 | **Snap** | Medium | 2–4 wks | **Best `audio-record` confinement model** |
| 4 | **GitHub Releases + attestations** | **Low** | **Days** | Where we are now |
| 5 | Homebrew on Linux | Medium | ~1 wk | Power users |
| 6 | AUR | Medium | ~1 wk | Arch only |
| 7 | Nix | High | 1–2 wks | Power users |
| 8 | AppImage | Very low | — | **No reach** |
| 9 | Debian/Ubuntu official | **Infinite** | — | Effectively unreachable |
| 10 | cpak | Unproven | — | Too early |

**Recommendation: start Flathub now** despite the AI-policy risk — it is the only
channel that delivers one-click install, auto-updates, and zero terminal. **Do the
cheap wins this week** so they help every channel.

---

## 4. Do this week (~4 hours, free, helps every channel)

1. **Enable Immutable Releases** on GitHub.
2. **Add `actions/attest@v4`** — build provenance attestation.
3. **Sign git tags.**
4. **Add the Tauri updater** — deb/rpm/AppImage self-update is fragile; a signed
   updater is the portable answer.

---

## 5. Flathub-specific work

1. **Package `espeak-ng` / `onnxruntime` into the manifest** to defeat the
   host-dependency clause.
2. **Make the app visibly not-tray-only** — reviewers read a tray-only app as a
   server/service helper, not an application.
3. **Record the 20-second demo video.** *That is what killed YazSes.* Real app,
   real interaction, no slideshow.
4. **Restructure `AGENTS.md`** into a human-authored doc with an explicit
   AI-assistance disclosure.
5. **Do not let an agent open the submission PR** — write it ourselves.
6. **Justify every permission** individually in the manifest comments.

### Microphone permissions

| Need | Flatpak | Snap |
|---|---|---|
| PulseAudio | `--socket=pulseaudio` (bundles mic) | `audio-record` — **the cleanest model** |
| PipeWire native | Bundled by the pulseaudio socket | `audio-record` |
| Device access | `--device=dri` for OpenGL | — |

**There is no microphone-specific portal.** `--socket=pulseaudio` is the standard
workaround and it bundles playback with record.

---

## 6. Tauri bundle reality (from `01`, cross-referenced)

### Officially supported

| Format | Status | Notes |
|---|---|---|
| **AppImage** | ✅ 1st-class | ⚠️ **glibc trap: build on the OLDEST base system.** 2-6 MB → **70+ MB.** No ARM cross-compile. `bundleMediaFramework: true` for GStreamer — **we need this, we play TTS audio** |
| **Debian (.deb)** | ✅ 1st-class | ⚠️ same glibc rule. Supports metainfo |
| **RPM** | ✅ 1st-class | Same. ARMv7/ARM64 cross-compile via `--target` |
| **Snapcraft** | ✅ 1st-class | ⚠️ Needs `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`. **Strict AppArmor/cgroup sandbox vs an always-on mic — test the wake-word cpal path under snap confinement before committing.** |
| **AUR** | ✅ 1st-class | Arch only |
| Flatpak / Flathub | ⚠️ **DE-PRIORITISED** | Docs page is thorough (updated 2026-09-21) but the **Flathub LinkCard is commented out** of the Linux grid in `tauri-docs/.../distribute/index.mdx` while the sidebar still lists it |

### Windows / macOS / mobile

| Target | Status |
|---|---|
| Windows MSI/NSIS | ✅ 1st-class |
| Microsoft Store | ✅ 1st-class |
| macOS .app / DMG | ✅ 1st-class |
| macOS App Store | ✅ 1st-class (separate `tauri.appstore.conf.json`) |
| **Google Play (AAB)** | ✅ 1st-class. `tauri android build -- --aab`. Min SDK 24. Docs **last updated 2025-03-29** (stalest page). ⚠️ *"Tauri currently does not offer a way to automate the process of creating Android releases… but it is a work in progress."* **First upload must be manual.** |
| **iOS App Store** | ✅ 1st-class. Tauri 2.12 bumped to Gradle 9 / Kotlin 2 / **targetSdk 37 (Android 17)** |

**So yes — Play Store and App Store submission paths exist and work today.** The
gap is automation, not capability.

### Flatpak-specific blockers from Tauri's own docs

- Tray icon needs `--talk-name=org.kde.StatusNotifierWatcher` **and**
  `--filesystem=xdg-run/tray-icon:create`
- Wayland needs `--socket=wayland`
- Suggested `--env=WEBKIT_DISABLE_COMPOSITING_MODE=1` for black-webview-on-Wayland
- 🔴 **Always-on-top + transparent + click-through under Flatpak = a third axis of
  uncertainty** on top of the Wayland problems in `01`

---

## 7. Auto-update

`tauri-plugin-updater` + signing. **Linux is the weakest target:**
- AppImage self-update is supported but fragile
- deb/rpm users should update via their package manager
- `tauri-plugin-window-state` 2.5.0 and `tauri-plugin-autostart` 2.7.0 (2026-10-01)
  are current and ready for a persistent app

**Portable answer:** ship the signed Tauri updater *in addition to* package-manager
updates, so neither channel leaves users stranded.

---

## 8. What NOT to do

🔴 **Do not build a PackageKit dependency installer as the primary story.** It is
the pattern Flathub rejects, and PackageKit 2.0's API break will break it anyway.
Handle missing system packages with a **clear preflight error naming the exact
package**, not an installer.

---

## 9. Ordered plan

**This week (free, every channel benefits):**
1. Immutable Releases + `actions/attest@v4` + signed tags + Tauri updater.

**Before Flathub submission:**
2. Restructure `AGENTS.md` into a human-authored document with AI disclosure.
3. Package `espeak-ng`/`onnxruntime` into the manifest.
4. Record the 20-second demo video.
5. Make the app visibly not-tray-only.

**Then:**
6. Start the Flathub PR ourselves (not via an agent).
7. Ship deb + rpm + AppImage with `bundleMediaFramework: true`.
8. Treat snap and flatpak as separate engineering projects — both add a sandboxing
   axis to an already-fragile Wayland story.

---

## 10. Unresolved

- **Our in-product auto hotkey registration** (`register-hotkey.sh`) needs a
  Tauri-side solution. If `GlobalShortcuts` doesn't land, options are: GNOME Shell
  extension, `xbindkeys`-free compositor-specific hints, or a settings UI that
  teaches the manual keybind. **A required companion script is not acceptable for
  Flathub.**
- **Microphone portal:** none exists. `--socket=pulseaudio` bundles playback with
  record on both Flatpak and Snap. Not fixable by us.
- **PackageKit 2.0 timeline** unknown.
- **Flathub AI policy durability:** PR #641 is recent (2026-09-04) and may change
  again. Re-read before submitting.