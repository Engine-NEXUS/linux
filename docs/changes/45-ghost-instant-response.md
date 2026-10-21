# 45 — Ghost Instant Response: Scrollbars, Capture Gap, In-Session Routing (2026-09-25)

Closes the three findings from the live-log analysis (doc 44
follow-up): the stage's scrollbars, the hot-mic that showed listening
without capturing, and ghost-blind app opening. Plus the honest latency
map for "respond in milliseconds."

---

## 1. Scrollbars (presentational, certain)

`frontend/stage.html` shipped zero CSS — browser-default 8px body
margin plus visible overflow on a fullscreen window guarantees both
scrollbars. Fix: 5-line `<style>` (`margin:0`, `overflow:hidden`,
transparent base, `#root` locked to viewport). The window was always
correctly transparent/click-through; only the chrome was wrong.

## 2. Capture gap (the #1 live find)

`triggerFollowupListen()` showed the orb and waited — but never told
Rust to start cpal capture (wake-word, hotkey, and the confirm window
all do; this shared path didn't). Log proof: zero `BATON stt-capture:
started` lines in 50+ post-entry seconds. Fix: invoke
`start_stt_capture` inside `triggerFollowupListen` (guarded, warn on
failure). This repairs the ghost hot-mic loop AND the legacy
follow-up path, which shared the defect silently.

## 3. Ghost-aware routing + registry-first open

With a ghost session live (no drill running), `OpenApp` transcripts now
route to `run_ghost_open` instead of the plain LocalCommand path: same
safety screen, session guards, ring narration, session stays open for
follow-ups. Everything else (questions, chats) routes normally.

Open strategy per call (not per religion): registry fast path
(`resolve_and_open_app`, ~ms, now `pub(crate)`) first with a focus
top-up; Win-search drill only on miss. The WhatsApp drill uses the same
order (fresh launches get 1s + warn-and-continue instead of a false
failure). Net: instant path where possible, robust path where needed.

## 4. Latency map (measured, not promised)

End of speech → understood ≈ 650ms (400ms VAD endpointing + ~250ms
Groq + <1ms parse) — already real. Transcript → ack speech <100ms.
Cursor glides at 25ms/step. Full app-open settles in ~1–3s of OS time
under every implementation; the plan never claims otherwise — what the
user *feels* (instant speech, ring, immediate motion) is instant, the
window arriving seconds later is OS physics.

## Verification (twice)

Rust 547/547 serial + clean check, zero new warnings; frontend tsc
clean (one self-inflicted brace caught by tsc, fixed) + 33/33 vitest;
`nexus trace-ghost` static now 12/12 on repeat runs (was 10/12 — the
two misses were exactly these fixes). Live procedure for the built app:
rebuild, "ghost mode" → no scrollbars → "open whatsapp" → ack in
<100ms, focused window, session still open → speak next command with no
wake word (BATON line must appear this time).
