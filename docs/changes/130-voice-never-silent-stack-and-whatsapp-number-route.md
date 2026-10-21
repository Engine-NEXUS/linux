# 130 — "Never silent, never late" voice stack + WhatsApp number-first route (2026-10-15)

Implements Option B of `docs/features/125-response-delay-and-whatsapp-learning-analysis-and-plan.md`.

## Voice
- **Root cause of the 4–5 s "Ok sir"**: the cloud voice (Edge) had been failing (stale `edge-tts-rust`, HTTP 403) and each line paid the full timeout before falling back; the cache staleness test asked "is bing.com reachable" instead of "did Edge actually work". Fixed: `edge-tts-rust 0.1.6` (+ explicit MP3 output format), live probe passes (~0.9 s).
- `tts_health.rs`: circuit breaker (2 consecutive failures -> degraded 10 min; `recently_ok` 5 min), clock-injected, 6 tests. Local engine is not unloaded while Edge is degraded.
- `tts_system.rs` + `tts_wav.rs`: Windows built-in voice (WinRT `SpeechSynthesizer`, 10–45 ms warm) as the safety net when the cloud voice is degraded or late. Never persisted to disk; the whole utterance stays one voice (`CaptionTrack.system`).
- `tts_cache_disk.rs`: persisted short-line cache (`<app data>/tts_cache`, <=200 files, <=9 words, per voice and engine origin). 29 fixed lines pre-rendered in the persona voice; cache entry counts as stale only when it is estimated-timing AND Edge was recently OK.
- Strict yes/no classification for pending confirmations (frontend `confirmReply.ts`, Rust `offer.rs` ANSWER_VOCAB): "okay so what is the weather" no longer confirms a send/merge/order; 2 min TTL.

## WhatsApp
- `wa_open.rs`: bounded (9 s total) open-chat flow; detects UI Automation blindness once (unfiltered list is empty), session breaker 10 min; keyboard route (Ctrl+F, type, Down, Enter) verified by on-device OCR of the chat header; `Unverified` outcome never types; successful opens teach `wa_alias_<name>`.
- `contacts_book.rs`: `.vcf` parser (folded lines, QP-free, `N:` fallback), country-code inference, WhatsApp-ready numbers, merge that never overwrites manual entries, name lookup (exact -> unique partial -> ambiguous asks for the full name).
- `ghost_drill.rs`: if a number is known (learned or contact book) the chat opens by `whatsapp://send?phone=` first; otherwise the UI ladder.
- Voice: **"import my contacts"** (`ParsedIntent::ImportContacts`, local-only) reads the newest `.vcf` in Downloads/Desktop/Documents and reports honestly (none found -> says how to export).
- `ghost_drill_running` IPC so the hot-mic stall watchdog does not cancel a running drill.

## Verify
Rust lib 1543 passed (12 ignored), e2e 2, offline 10, user cmds 7; vitest 311/311; `tsc` clean; `npm run build` clean; turn-end + hygiene gates OK; dead-code ratchet 157 -> 156 (baseline shrunk, none new).
IPC gate FAILS on `agent_watch_*` (another session's untracked work, not part of this change).

## NOT run live
Edge cache upgrade path at real boot, Windows-voice fallback timing in a real late-Edge case, the WhatsApp keyboard route + header OCR on the real app, contacts import on a real `.vcf`, release rebuild.

## Plans 123 / 124 items built in the same round
- **Boot hello (123 P1-P3)**: `memcore/briefing.rs` no longer sleeps 45 s: it waits for the stage renderer (`stage::is_ready`, max 60 s) + a 3 s settle, then speaks the cached time-of-day hello on **every logon** (not twice within 2 h): 05:00-11:59 "Good morning", 12:00-16:59 "Good afternoon", otherwise "Good evening" (+ friend persona name). The once-a-day briefing follows (gathered in parallel, 3 s cap per part; health no longer delays the hello), without repeating the greeting. Presence = input in the last 5 min (was a 30-60 s band). Hello and briefing go through `proactive_policy` at High urgency (ignores the 30 s idle rule) and ids containing `boot_` are exempt from the 20 s spoken-alert gap.
- **Autostart task (123 P1)**: PowerShell registration moved off the setup path (background thread); task now `-Priority 4 -StartWhenAvailable` (was priority 7). **Stage window is created first in `setup()` (123 P3)** so WebView2's cold start overlaps the rest of setup.
- **Resume hello (123 P4, partial)**: after waking from sleep with >= 20 min away, the plain hello is spoken once the user has typed (silent while locked). Lock/unlock WITHOUT sleep is not detected (needs a session-notification window); not built.
- **Shutdown (123 section 9)**: "shut down my laptop" no longer asks. It snapshots the session, says "Good night, sir." (20:00-04:59) or "Shutting down, sir. See you soon." (friend persona: name), waits for the audio to end, closes every ordinary window with `WM_CLOSE` (`power_close.rs`; shell, system UI and NEXUS are never touched), **stops and names the app if a window stays open** ("Notepad is asking to save, sir..."), gives a final 3 s for "cancel", then `shutdown /s /t 0` (no `/f`). It asks once only when a meeting is detected or a Ghost drill is running. Restart / sign-out keep the old ask + 12 s countdown.
- **Questions (124 Q1/Q2-lite/Q3)**: strict yes/no (done above); the mic waits up to 10 s for an answer to START (was 5/8 s; an answer already started is never cut); a pending offer stays answerable by risk: shutdown/erase 2 min, calendar/timetable 5 min, start/reopen/open-app 10 min (was a flat 90 s); every locally handled turn is now recorded (first spoken line pairs with the transcript; rejected audio never) and a bare "do it / go ahead" with nothing pending answers "Do what, sir? ... My last words were: ..." instead of guessing; the cloud thread keeps the last 15 min regardless of relevance score. **On-device turns never enter the cloud prompt or the brief** (privacy: they never left the device before).

## Verify (final, after all of the above)
Rust lib 1558 passed (12 ignored), e2e 2, offline 10, user cmds 7; vitest 311/311; `tsc` clean; turn-end + hygiene OK; dead-code ratchet 156, none new.

## NOT built (honest list)
`PendingQuestion` as one unified Rust object and on-screen Yes/No buttons / orb "waiting" mark (Q2 full), structured cloud `proposed_action` (Q4), mic restart after resume, lock/unlock without sleep, "good night nexus"-style soft shutdown phrases, agent_watch tasks in the shutdown safety check (that module belongs to another in-flight change), STT vocabulary bias with contact names (needs `&'static str` prompt plumbing changes in `stt_groq.rs`), ask-once-for-unknown-contact, parallel ack synthesis, hand-mode start latency instrumentation, boot timeline file (P0), second-relisten bug fix.

## NOT run live
Everything audible or visual: hello timing after a real reboot, Windows-voice fallback, shutdown close step against real apps (the tests use a fake window list; try it with a throwaway Notepad with unsaved text first), resume hello after sleep, cached "Good night" line, WhatsApp number route and keyboard route on the real app, contacts import on a real `.vcf`.

## First live log (2026-10-15 17:35) and the fixes it caused
Worked: the boot hello spoke 5 s after launch ("Good evening, sir."), `[BOOT] skip briefing: already briefed today`, Windows voice took over when Edge failed (200-870 ms).
Found and fixed:
1. **Shutdown never heard right**: Whisper wrote "send on my laptop", "set down my laptop", "to set down my laptop"; all went to the cloud, which explained how to shut down. `power_cmd::parse` now accepts those mishearings of "shut down" only when what follows is exactly the computer (test: "set down my coffee" does not match).
2. **429 storm at start-up**: the 29 stock lines were requested back-to-back and drew "429 Too Many Requests", which then also hit the first real reply. Pregeneration is now paced (400 ms), waits for any user speech, and stops using the cloud for that pass at the first 429 (a 429 does not count against the breaker).
3. **Half-spoken answer**: when the cloud voice failed on a LATER sentence the rest of the reply was dropped (orb silent for 10 s, then the stuck-speaking watchdog). The remaining sentences now continue in the Windows voice.
Rust lib 1558 + integration suites pass (`turn_detect::model_loads_and_runs` is a known timing flake: fails under load, passes alone).
