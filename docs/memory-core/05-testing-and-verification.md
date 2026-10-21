# 05 - Testing and verification

## Running the tests
Running the whole Rust lib suite in one process aborts in the two TTS modules
(Kokoro loaded twice under low RAM); run three groups:

```
cargo test --lib -- --skip tts_bench --skip tts_kokoro --test-threads=1
cargo test --lib -- tts_bench --test-threads=1
cargo test --lib -- tts_kokoro --test-threads=1
cargo check --features custom-protocol,admin-brain
cd frontend && npx tsc --noEmit && npx vitest run && npm run build
```
Latest (P6): Rust 1163 + 9 + 12, frontend vitest 215, tsc and build clean.
Known flake: `turn_detect::model_loads_and_runs` (a <1500 ms timing assertion)
can fail while other builds run; it passes alone. LNK1104 on the bench group
means a test exe is still running.

## Techniques used
- Pure engines with an injected clock (proactive policy, scheduler, scoring).
- Local mock HTTP servers: `google_io::TEST_BASE` (Gmail/Calendar),
  `mcp_client::TEST_WHATSAPP_URL` (WhatsApp bridge). The WhatsApp mock advertises
  `mark_read`/`send_presence` and tests assert they never reach the wire.
- Keychain isolation via the `kr()` mock; a regression test proves the token-leak fix.
- Parser tests with misfire guards (plain "mute", "read this", "make a note"...).
- Live probes through the running app (CDP + Tauri IPC) for status/encryption.

## Verified live
Encrypted store and new IPC commands in the release binary; instant sign-in
failure message; clipboard image read; a real Gemini request returned 5/5 slots
of a generated timetable; foreground/idle probe; two-process keychain probe.

## NOT verified
- A real Google sign-in, real Gmail/Calendar data.
- Spoken reminder -> mic window -> "yes" round trip; a real reboot briefing.
- Photos/handwritten timetables.
- Anything against the real WhatsApp bridge, including **whether its read calls
  send receipts/presence** (manual two-phone test required).
- Visual check of the Memory page; first-launch migration on real data;
  name redaction against a real Worker.
