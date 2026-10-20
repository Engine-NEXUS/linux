# Linux Voice Loop — Rounds 4–6 (wake storm, mic deny, ALSA flood, silent hello)

**Date:** 2026-09-04
**Commits:** `72f0f82` → `c19a439` → `1a14bee` → (this) punctuation fix
**Status:** Implemented, tested, pushed to `linux main`

---

## Round 4 — Orb flash, stale config, hidden start (`72f0f82`)

**Symptom:** orb invisible; blank window flashed 1s then vanished on wake.

**Root causes:**
- Native window started visible before React mounted → white flash.
- `nexus-config.json` held stale example worker URL + empty IDs → backend calls failed silently.
- Diagnostics wrote to Windows-style path, no Linux HOME fallback.

**Fix:**
- `dyn_windows.rs` builder `.visible(false)`; `window_manager::init` native `hide()` + click-through OFF.
- `lib.rs` repairs stale config on startup against hardcoded `WORKER_URL`.
- Orb shows via `show_overlay` immediate, hides 600ms deferred (0.5s slide-out animation).
- Wake path `--wake` → `show()` + `configure_non_activating_overlay` + `set_ignore_cursor_events(false)` + `eval(window.__NEXUS_WAKE__)`.

## Round 5 — Mic deny, wake storm, loopback probe (`c19a439`)

**Symptoms:** wake detected but no voice, no UI; model probability 0.8–1.0 every 1–3s with `wake: model` flap.

**Root causes:**
- WebKitGTK `getUserMedia` denied — no permission handler registered.
- No wake cooldown → phantom re-triggers every few seconds.
- Probe order hit PipeWire `Monitor of` / `easyeffects_sink` / `spotify` loopbacks first (5s probe each, false-trigger on own TTS).

**Fix:**
- `mic_permissions.rs`: Linux `connect_permission_request`, own-origin auto-allow (`tauri.localhost`, `localhost`, `ipc`), foreign deny.
- `wakeword_oww.rs`: 10s cooldown after confirmed wake, mic-first probe order, `try_device_silent` for loopbacks.
- `CPAL_STREAM` pause/resume baton (`play()`/`pause()`), 300ms post-TTS gate, `MEETING_STATE.should_suppress_wake`.

## Round 6 — ALSA POLLERR flood (`1a14bee`)

**Symptom:** hundreds of `alsa::poll() returned POLLERR` per second.

**Root cause:** rodio `OutputStream::try_default()` opened + dropped per `speak_text` call → tore down shared ALSA duplex handle → killed wake-word input stream.

**Fix:** single shared `OutputStreamHandle` via `OnceLock`, stream leaked (`mem::forget`) for process life; `err_cb` log throttled (first + 1/5s).

## Round 7 — Silent `hello.` (this commit)

**Symptom:** wake worked, STT transcribed (`stt: transcript: 'Hello.'`), but no reply; `open youtube` voice worked fine.

**Root cause:** faster-whisper appends sentence punctuation (`Hello.`, `open youtube?`). Greeting regexes anchored `^...$` rejected `hello.` → unknown → backend → silence. `open youtube.` parsed only because its regex tolerated trailing tokens.

**Fix:** `parse_deterministic` strips trailing `.?!…,` once before `normalize_whitespace` — single choke point instead of per-regex. Regression test `test_greeting_stt_punctuation` covers `hello.`, `Hello.`, `hi?`, `open youtube.`, `thanks!`.

**Note:** Piper `en_US-amy-medium` 63MB lazy-downloads from HuggingFace on first speak — first TTS after fresh install pauses, later speaks instant.

## Verify
- `cargo test --lib` → 105+ passed incl. `test_greeting_stt_punctuation`.
- `./scripts/dev.sh` → say `hello.` → audible greeting reply.
- `git push linux main` only — never `origin/main`.
