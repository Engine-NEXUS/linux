# 126 — G0 cleanup: dead relisten watchdog removed + review of the Gemini features (108–113)

Date: 2026-10-13

## Removed (wire-or-delete: deleted)
- `ghost.rs`: relisten watchdog (state, poll, spawn, `wall_ms`, `note_ghost_activity`, its tests). It was never started, and the frontend `ghost:relisten` listener in `orbRuntime.ts` waited for an event nothing emitted. Both gone, so the event/IPC baselines can shrink.

## Review of the Gemini features (docs/features 108–113)
Everything is wired (orchestrator handlers, `memcore::leases::start`, `coder`), compiles, and nothing was deleted. What the review found, and fixed, was **speech that claimed more than happened**:

| Feature | Problem | Fix |
|---|---|---|
| Block contact (108) | "I have blocked X on WhatsApp/phone": only Instagram has an executor; Instagram script was fired and never awaited; the lease was created even if the block failed; a memory-off save failure silently left a *permanent* block with a promised unblock | Awaited `run_instagram_action` (real exit status + reason); unsupported platforms are refused out loud; lease only after a successful block; lease-save failure is spoken |
| Unblock / lease expiry (108) | Lease marked `completed` and "I have unblocked them" spoken without checking the script ran | `process_due_leases` takes an executor, status `completed`/`failed`, failure is announced and not retried every 30 s; unsupported platform is a failure, not a success |
| Voice reminders (113) | "I will remind you" spoken even when the lease was not stored | Refuses with "nothing is set" when the store write fails |
| Doc analysis (109) | When no file matched, it analysed **AGENTS.md** and presented it as the contact's document; hard-coded user profile path. NEXUS does not download WhatsApp attachments | Honest "not found in Downloads, I can't download from WhatsApp yet" |
| Coding agents (111) | `agy` spawned without the prompt; IDE agents "Ready in editor clipboard" but nothing copied; always spoke "dispatched" | `Reach::Spawn` only for claude/aider (prompt flag known); everyone else gets the spec on the clipboard and the reply says so |
| Localhost runner (110) | `Command::new("npm")` cannot find `npm.cmd` on Windows; with no project in the title it used the process dir (the installed app's folder) | Run via `cmd /C` on Windows; unrecognised dir is refused; port is "usually" not a promise |
| Internship radar (112) | `contains("intern")` matched "internal"/"international" | Whole-word match |

## Not changed (needs a decision)
- `claude -p <prompt>` and aider are spawned straight from a voice transcript (could be TV noise). Should be confirm-gated in `center::action_disposition`.
- `find_project_directory` hard-codes `c:\PROJECTS`.
- WhatsApp unblock/block, "phone" blocking and document download have no executors; docs 108/109 describe more than exists.
- `scripts/instagram_client.py` and `doc_analyzer.py` are not bundled with the installer (path resolved relative to cwd/manifest dir); `python` must be on PATH.

## Verify
See AGENTS.md entry. New tests: lease failure/unsupported, coder reach table, localhost refusal.
