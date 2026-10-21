# NEXUS Production-Hardening Implementation Plan (2026-10-05)

**Goal:** from "tests green, live uncertain" to a measured production bar: scripted live drills passing, model/config alignment gated in CI, every feature with a recorded live acceptance.
**Rule:** research first per phase, implement second, live-verify third. Nothing lands on claims alone anymore.

## 0. Baseline truth (prove what runs before fixing what breaks)

- **0.1 Binary provenance.** Stale-shortcut launches (2026-08-31 binary) explain a whole class of "nothing works" reports. Log build hash + date at startup; `nexus check` prints it; docs say `nexus start` only.
- **0.2 `nexus check` extension.** Keys present (Groq/Gemini — presence only, never values), STT/TTS health, mic RMS sanity (>0 on live mic), display scaling %, **onnx output-dim vs labels.json length** (automates the P0 class of desync forever — see §1).
- **0.3 Single-writer discipline.** Two sessions on one dirty tree caused mid-task rewrites (braided knot → starburst) and mixed binaries (15:09 vs 16:12 builds same day). Rule: commit before build; never build on another session's dirty tree.

## 1. Screen-analysis correctness (close the researched defects)

- **1.1 P0 verify-or-retrain.** Surprise from verification: `labels.json` (both copies) now has **64 intents with `screen_analysis`@62**, matching `nlu_server.py`/`train.py`, and both `.onnx` files are stamped **2026-10-02** (after the off-by-one finding) — a retrain *may* have fixed it. Unverified: the onnx graph's actual output dim (63 = still broken, 64 = fixed) + an offline unknown-utterance test. If 63: `nexus train` retrain, then re-verify. Then gate it (§4).
- **1.2 P1 dpr hitbox.** `stage/main.tsx:57-63` — drop the second `* dpr` (pins are already physical). Live-verify at 150% scaling.
- **1.3 P1 warm-window OCR.** OCR path sets pending text but never emits `sidebar:show` — warm-mounted assistant view never re-fetches. Fix: emit or fetch-on-view-switch.
- **1.4 P2 compound routing.** `route_intent`/`to_local_intent` need a `screen_analysis` arm (local runner) or compounds containing it must abort honestly instead of sending words to the Worker with no screenshot.
- **1.5 P2 overlay dismissal.** Esc + voice ("hide the boxes") + timeout for the spatial overlay; stop `stage_show` from reversing the Ctrl+Alt+X kill-switch silently.
- **1.6 Doc refresh.** Doc 86 (event/payload/coords/provider all diverge) + AGENTS.md entry + features README index (stops at 80).

## 2. Ghost-mode live-drill closure

- **2.1 Run the pending matrices.** F1–F7 failure drills + H1–H7 hotkey matrix (doc 57, "user-run acceptance, pending" since 09-30). Record results; every failure becomes a tracked defect like §1.
- **2.2 Keyless vision path.** Gemini key absent → spoken guidance + OCR/UIA fallback, never silent failure. Groq-vision-removed paths must not 404-retry visibly.
- **2.3 Abort under load.** Mid-glide Esc/voice-stop with modifier keys down — prove no stranded modifiers live (code claims atomicity; prove it).
- **2.4 Takeover regression.** Keyboard-only-flow yield misfire class (the 00:15:37 incident) gets a permanent regression test.

## 3. Voice-pipeline robustness

- **3.1 Wake-during-TTS latency.** v4 sustain+verify ≈1–2 s. Measure it; decide accept vs DSP follow-up (explicitly NOT in this plan unless measured bad).
- **3.2 Empty-transcript triage.** Every miss must arrive with `stt:turn_stats` + holders trace (instrumentation exists; enforce the process).
- **3.3 Meeting-suppression self-heal.** The stuck-active state that "ate every wake with zero logs" has throttled logging now — add a timeout/self-heal so it can't stick forever.
- **3.4 TTS-init spin.** Just shipped (doc 79) — live-verify, don't assume.

## 4. E2E harness (the structural fix — nothing regresses across sessions again)

- **4.1 Scripted voice-loop rig.** Fixture transcripts → `process_transcript` → assert intents/results with mocked STT/TTS (no mic/speakers). Runs in CI.
- **4.2 Live-drill recorder.** Guided script (wake → command → observe) logging pass/fail — makes "user-run acceptance" reproducible instead of folklore.
- **4.3 Alignment gate in CI.** `labels.json` ⟷ server `INTENTS` ⟷ `train.py` ⟷ onnx output dim — the P0 can never recur silently.

## 5. Docs & release discipline

- **5.1 Stale-doc sweep** (§1.6 covers the worst; sweep remainder, starting with anything contradicting current code).
- **5.2 Versioned releases** (build stamps version; changelog per release; binary provenance from §0.1).
- **5.3 Definition of done:** unit green + E2E green + recorded live drill. No feature closes on unit tests alone.

## Ordering & needs
Phases run 0→5; §1 and §2 need live user participation (drills, keys, scaling %); §0.2, §4 are pure-agent work and unblock everything else. Proposed start: §0.2 (diagnostics gate) → §1.1 (P0 verdict) → §1.2–1.5 fixes → live drills.
