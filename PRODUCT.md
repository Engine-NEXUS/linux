# Product

<!-- impeccable:product-schema 1 -->

## Platform

web (Tauri 2 desktop app: WebKitGTK on Linux, WebView2 on Windows)

## Users

Primary: laptop owner operating their own machine hands-free. They wake the orb mid-task via wake word ("NEXUS") or the global hotkey to dictate commands — open/close apps, type, browse, WhatsApp, GitHub, order/search via MCP — then return to work without window management.

Other audiences (family devices, admin testing new intents) exist in the repo but are not the orb's design target.

## Product Purpose

NEXUS is a local-first voice overlay for full-laptop control. It listens on-device, routes each transcript to the right subsystem (deterministic parser → BERT-Mini NLU → Worker / 9Router / MCP / local executor), speaks the result via TTS, and confirms gated actions by voice plus one click in the sidebar.

Success: wake → understood → spoken/confirmed → gone. No permanent furniture, no focus stolen, no repeat prompt.

## Positioning

A transparent overlay that lives on the desktop, not in a tab: an orb that appears at the top centre (or the bottom, a user setting), ringed by an audio-reactive aura on Linux, click-through everywhere except the orb, an on-device wake word, offline local commands, and lazy windows. A browser assistant or cloud chatbot cannot truthfully copy the non-activating, never-steals-focus overlay contract.

## Operating Context

- Device: Windows/Linux laptop. Stage = `frontend/stage.html` → `src/stage/main.tsx`, a transparent full-screen window.
  - Linux: on Wayland compositors with layer-shell (COSMIC, KDE, Hyprland, Sway, …) and `libgtk-layer-shell0` installed, the stage is a layer-shell overlay surface (keyboard NONE, input region = the orb) that keeps the orb below the desktop's panels; the sidebar is a layer-shell drawer (keyboard on demand). Otherwise an X11/XWayland window that never accepts focus. See `docs/features/141-linux-native-shell-and-aura.md`.
  - Windows: WebView2, `WS_EX_TOOLWINDOW` stage.
- Waking: cpal capture in Rust + openWakeWord ("NEXUS", 80 ms chunks), or the global hotkey; the mic goes through Rust, not the WebView's `getUserMedia`. Linux opens the PipeWire/Pulse capture path and never holds the raw `hw:` microphone.
- Hearing: Groq `whisper-large-v3-turbo` primary, Moonshine local fallback (port 39217); NLU sidecar BERT-Mini ONNX (39218) when the deterministic parse misses.
- Answering: 9Router cascade for general Q&A, Cloudflare Worker for PR/GitHub/session work (reached only through Rust's allowlisted `worker_request`), MCP servers for Swiggy/Amazon/WhatsApp.
- Orb contract: a 200 px frame placed by Rust (`window_manager::rect_for`, top/bottom user setting). Styles: **aura** (Linux default — the bare particle orb ringed by `@saystack/core@0.2.1`), **classic** (black capsule), **pebble** (bezel scoop). The aura reacts to the same level as the orb: mic while listening, TTS envelope while speaking. Spoken replies are captioned word by word on real Edge word timings; the sidebar's "Last heard" row lets the user fix a misheard command and resend it. 8 s listening auto-hide.

## Capabilities and Constraints

Confirmed:

- State is geometry + mechanism, never hue: listening = active/reactive aura, thinking = working/orbit, idle = paused resting rim, speaking = mirrored reactive rim.
- Overlay rules: the stage never takes the keyboard and only the orb (plus pins/marker hit areas while they are shown) takes clicks; everything else passes through to the desktop.
- Low-RAM + non-intrusive: lazy window create/destroy, STT/NLU lazy-spawned with idle kill, the aura's render loop parks while the orb is hidden, no focus steal by the sidebar or the orb.
- Privacy + honesty: `localSttOnly` keeps audio on-device; hallucination filter triggers a retry, never a fabricated answer; tokens kept out of the audit log and out of the renderer.
- Reduced motion: honoured as calmer motion (the aura runs at 12% motion, no glide), not by removing the aura.

Explicitly undecided / deferred: streaming transcript (needs an STT change, not UI); real spectral aura bands (Rust publishes one level, not FFT bins).

## Brand Commitments

Name: NEXUS. Voice address "sir" in ack TTS ("On it sir") is existing copy — preserve unless the user changes it. No logo, palette, typeface, or marketing claims confirmed as binding; future work must not invent them here.

## Evidence on Hand

- Stage + orb: `frontend/src/stage/main.tsx`, `frontend/src/stage/OrbFrame.tsx`, `frontend/src/avatar/Avatar.tsx`, `frontend/src/avatar/voice-orb.js`, `frontend/src/styles.css`
- Aura: `frontend/src/aura/AuraCanvas.tsx`, `aura/stageAuraHost.ts`, `aura/orbPresentation.ts`, `frontend/src/audio/levels.ts`
- Captions + last heard: `frontend/src/stage/ResponseCaption.tsx`, `frontend/src/sidebar/readAlong.ts`, `frontend/src/sidebar/SubtitleBar.tsx`, `src-tauri/src/last_heard.rs`
- Pointer marker: `frontend/src/overlay/PointerOverlay.tsx`, `pointerLifecycle.ts`, `pointerMath.ts`, `src-tauri/src/pointer.rs`
- Linux shell: `src-tauri/src/overlay_linux.rs`, `src-tauri/src/gdk_backend_linux.rs`, `src-tauri/src/audio_linux.rs`
- Preview: `frontend/src/eyeball/` (`stage.html?eyeball`, dev builds only) — `docs/testing/stage-eyeball-harness.md`
- Absences future work must not fabricate: no testimonials, customers, benchmarks, pricing, or deployment claims on file.

## Product Principles

1. The desktop is the user's — the overlay recedes to a resting rim and leaves no furniture behind.
2. State must read at a glance without colour: motion geometry, the orb's own morphology, and caption text carry it.
3. Local-first and cheap: heavy models load only on first use and die when idle.
4. Never steal context: no activation, no focus grab, Esc / barge-in always wins.
5. Honest voice: retry or say it didn't catch it rather than hallucinate speech, intent, or evidence.

## Accessibility & Inclusion

Caption parity: spoken replies are captioned with word-by-word read-along, and the last heard command is shown (and editable) in the sidebar. Colour is never the sole state signal. Reduced motion calms the aura instead of hiding it. The pointer marker dismisses on Esc and on any new turn. No WCAG audit on file — do not claim a conformance level.
