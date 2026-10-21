# Change 83 — Proactive-speech policy (when NEXUS may speak unprompted)

**Date:** 2026-10-05 · **Plan:** `docs/research/jarvis-landscape/08-case-2-plan-2026-10-05.md` §C2-5 · **Phase:** 9

## Problem (found by reading the code)
1. The Sentinel's only spoken alert (mail **deadline change**) went `speak_proactive_alert → speak_line` the instant it fired, gated by nothing but its urgency. It never asked whether the user was mid-sentence, NEXUS was already talking, or a Ghost drill was running. The frontend `speak()` calls `stopTts()` first, so an alert **cut the current reply off**.
2. During a **meeting** the frontend `speak()` is muted (`meeting_active` → `should_suppress_tts`). The alert was therefore **silently lost** — contrary to your decision that important alerts should speak in meetings.

## Research basis
Eliciting Spoken Interruptions to Inform Proactive Speech Agent Design (CUI 2021, arXiv 2106.02077 — abstract only): people interrupt sooner when urgent, time interruptions at *breakpoints*, vary delivery by urgency, sometimes give a heads-up. **The paper gives no numbers — every threshold below is our hypothesis to tune.**

## What it does (`src-tauri/src/proactive_policy.rs`)
| Urgency | Behaviour |
|---|---|
| Low | card only, never voice |
| Medium | voice only when nothing is happening **and** the user has been idle ≥ 30 s; gives up after 15 min of eligible waiting |
| High | waits for a breakpoint: user not speaking, NEXUS not speaking, no Ghost drill (a drill is tolerated for at most 60 s, never over the user); **in a meeting it waits for the meeting to end** (≤ 30 min, else card); gives up after 10 min of eligible waiting |
| Critical | speaks within seconds; waits ≤ 8 s for the user to finish a sentence or NEXUS to finish its own line; **speaks during a meeting** (setting `proactiveCriticalInMeeting`, default **on**) by lifting the meeting TTS mute for 30 s (`MeetingState::allow_tts_override`, self-expiring) |
| all spoken | heads-up: `proactive:nudge` event + 300 ms beat first; Critical opens with "Urgent, sir." |

Rate limits (non-critical only): ≥ 20 s between spoken alerts, ≤ 6 per hour (extra ⇒ card); one line per 2-second tick, most urgent first; each alert id handled once; `proactive_snooze(minutes)` ("not now", default 15 min) holds non-critical alerts (Critical ignores it).
Expiry counts only time the alert **could have been spoken** (not time in a meeting or under snooze) — two bugs the timeline tests caught: (a) a High alert held through a long meeting was demoted to a card the moment the meeting ended; (b) a newly submitted alert was credited with idle time from before it existed.

## Files
`proactive_policy.rs` (new: pure `Engine` with injected clock + runtime glue + 2 s ticker) · `meeting_detect.rs` (self-expiring TTS-mute override) · `wakeword_oww.rs` (`stt_user_speaking`, user-activity note) · `orchestrator.rs` (`speak_proactive_alert` now submits to the policy) · `google/sentinel.rs` (passes urgency + alert id) · `lib.rs` (module, ticker start, `proactive_snooze`).

## Verification
`cargo test --lib -- --test-threads=1`: **927 passed, 0 failed** (6 ignored dev helpers); 15 new tests: per-urgency behaviour, deferred-then-spoken-exactly-once timelines (user speaking / NEXUS speaking / drill / meeting), meeting expiry, Critical in a meeting with override and lead-in, Critical opt-out, Critical's short wait, Medium idle rule and expiry, one-line-per-tick ordering, hourly cap (Critical bypasses), snooze (Critical bypasses), dedup, an exhaustive no-panic / "High and Medium never speak over the user or in a meeting" sweep, and the meeting-mute override expiring. No new compiler warnings in touched files. Frontend unchanged.

## Honest limits
* **Not live-tested** (no real meeting, no real deadline alert, no audible check of the heads-up beat or the "Urgent, sir." lead-in).
* **Nothing in the code produces a Critical alert today.** The Sentinel's only spoken alert (deadline change) is classified **High**, so it still will **not** speak during a meeting — it waits until the meeting ends. Which events deserve Critical (e.g. a deadline within hours, an urgent commute delay) is a product decision I have not made for you.
* Medium/Low alerts that the Sentinel currently tracks silently (replies, attachments) are **still silent** — I kept its "only deadline changes interrupt by voice" contract. The Medium/Low paths exist in the policy but nothing feeds them.
* `proactive:nudge` / `proactive:card` events have **no frontend listener yet** (no orb pulse / card UI), and the **voice command "not now"** is not wired (only the `proactive_snooze` IPC exists).
* All thresholds (30 s idle, 60 s drill ceiling, 8 s Critical wait, 20 s gap, 6/hour, 10/15/30-minute expiries) are untested hypotheses.
* Speaking a Critical line during a meeting is audible to the call if the speakers feed the microphone — your decision, but worth knowing.
