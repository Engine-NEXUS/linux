# 112 — Memory Core P5: important-mail watcher and Calendar

Plan: `docs/features/100-memory-core-plan.md` (phase P5). Follows change 111 (P4).

## What was built
- **Inbox watcher** (`memcore/mailwatch.rs`, `google_io.rs`). Polls Gmail `users.history.list` (not push: push needs a Cloud Pub/Sub topic and a public HTTPS endpoint, which a desktop app lacks). A stored `historyId` per account means each poll returns only what changed. Cadence: every 90 s while you are at the PC, every 8 min when you are away; backoff on errors (auth → 30 min, rate limit → 10 min, other → 1…15 min). Expired history ids (Gmail keeps about a week) re-baseline quietly. Only `format=metadata` is requested: **sender, subject and Gmail's own ~200-character preview. Message bodies are never fetched.** A test asserts no request ever asks for `format=full`.
- **Quiet first connect.** The first poll records "now" as the starting point and files the last 3 days of important mail for the briefing without speaking any of it. After that only new mail can interrupt.
- **Deterministic triage** (`memcore/mailtriage.rs`, 12 tests). Explainable keyword/sender rules, each alert has a "why":
  - *Database services* (Supabase, Neon, MongoDB, Render, Railway, Firebase, …): paused / inactive / will be deleted / suspended / over the limit / payment failed → High; usage or billing → Medium. Lookalike domains do not match.
  - *GitHub*: security alerts, leaked secrets, token expiry → High; failed CI runs, review requests, mentions → Medium; ordinary chatter is ignored.
  - *Hackathons* (Devpost, Unstop, MLH, Devfolio, …): shortlisted / selected / waitlist / next round → High; deadlines → High; "application received" → Medium; promotions ignored.
  - *Exams*: a strong notice (hall ticket, exam schedule, results declared, …) from an institution → High; weaker words from an institution → Medium; the same words from a promotion or a non-institution sender do not alert.
  - *Plain deadlines* (non-promotional) → Medium.
  - Spam, trash, drafts and sent mail are skipped; Gmail's Promotions/Social/Forums tabs suppress weak matches. Text inside an email can never steer anything: an "ignore previous instructions" subject is just a subject (tested).
- **Speaking.** Via `proactive_policy` (High / Medium), so alerts wait out meetings and speech. Three or more at once become one summary line. Storage: Medium+ only, as untrusted `mail` records (sender, subject, redacted snippet, category, reason), sealed, 7-day ring, never in the cloud context pack.
- **"That's not important."** Right after an alert it mutes that sender. The mute is a visible, forgettable pinned fact (`mail_mute_<address>`), so it appears under "You told me" and "forget mail mute …" undoes it. Domain mutes are exact-domain, not substring.
- **"Any important emails?"** speaks the top item and opens a sidebar list. It says plainly when Gmail is not connected, when alerts are off, or when the inbox is quiet.
- **Calendar.** "What's on my calendar today/tomorrow" (and "my agenda") read events via `events.list` and speak the first four. "Add dentist to my calendar tomorrow at 5 PM" (also `for 2 hours`, `on friday`, `next monday at noon`) parses the request, checks that day for overlaps, reads it back ("Add Dentist tomorrow at 5 PM to your calendar? Note, that overlaps with …") and only inserts after a yes (`events.insert`). Missing title, missing time and times already past get specific answers.
- **Briefing.** The boot / on-demand briefing now includes today's calendar and the important mail filed in the last 3 days (most urgent first), phrased by category.
- **Command Hub → Memory**: switch for inbox + calendar (`memcoreMail`), a one-line honest status (off / no Google account / nothing important / N filed), and an "Important mail" list with the reason for each item and a "Not important" button.

## Verify
- Rust: **1150 passed, 0 failed** — 1129 main + 12 `tts_kokoro` + 9 `tts_bench` (still run separately, see change 110). New this phase: mail triage (12), agenda (8), mailwatch (9 incl. 3 against a mock server), google_io scope/keys, briefing mail+calendar (2), parser phrases + misfire guards.
- **Mock-server integration tests** (real HTTP against a local server standing in for Google): quiet baseline then two paginated history pages with label filtering (a SENT message is ignored, High sorted before Medium), expired history id (404) re-baselining instead of alerting, 401/429/500 mapped to the right errors, and Calendar list + insert including URL encoding of RFC 3339 offsets.
- `cargo check --features custom-protocol,admin-brain`: clean, 0 warnings. Frontend: `tsc` clean, vitest 209/209, `npm run build` ok.

## Not verified / limits
- **No live Gmail or Calendar run.** No Google account is connected on this machine: Credential Manager holds none, because the old keyring mock never persisted tokens. Reconnect Google once in the Command Hub and the watcher starts by itself. Real response shapes are covered only by fixtures written from the documented API.
- The spoken alert and "that's not important" flow were not heard.
- Rules are keyword-based: a hackathon or exam notice that uses unusual wording will be missed, and a newsletter that mimics one could alert. There is no sender-frequency or "people you reply to" signal yet.
- Calendar writes go to the primary calendar of the primary Google account only. Event time parsing understands today/tomorrow/weekday + a clock time, not dates like "the 14th".
- Deadlines found in mail are not turned into calendar events automatically.
