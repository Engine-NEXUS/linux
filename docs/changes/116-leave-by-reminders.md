# 116 - Leave-by reminders ("Time to leave for your dentist appointment")

## What it does
`memcore/leavewatch.rs` watches your Google Calendar and, for timed events that have a **street address**, asks the Routes API (traffic-aware, driving) how long the trip from **your remembered home** takes. It speaks via `proactive_policy`:
- 5 minutes before the leave time (Medium): "Leave in about 5 minutes for Dentist, sir. It's about 20 minutes from home and starts at 3 PM."
- At leave time (High): "Time to leave for Dentist. ..."
- If already too late (High): "You're running late for Dentist. ..."
Leave time = start - trip - 5 min buffer. Once per event+start (a moved event reminds again); remembered across restarts (`leaveby:<id>:<start>` in the sealed store).

## Needs (Memory page shows what is missing)
Connected Google account with calendar access, a Google Maps & Routes API key (API Keys), a saved home ("remember that my home is ..."). Switch: Memory -> "Tell me when to leave..." (`memcoreLeaveBy`, default on; does nothing until all three exist).

## Cost control
Calendar read every 10 min (1 call). Routes lookups only for events starting within 90 min, <= 1 per 10 min per event, <= 3 per minute. Roughly <= 9 Routes calls per event.

## Ignored on purpose
All-day and cancelled events, past events, no location, video-call/online/phone locations (Zoom, Meet, Teams, links, "TBD"), trips > 3 h (wrong geocode).

## Honest limits
- No device location: the trip is always from HOME. If you are elsewhere, the minutes are wrong. The spoken line says "from home".
- Driving only; traffic is "now", not forecast for the departure time.
- Event locations that Google cannot geocode produce no reminder (silently).
- Not run live: a real calendar event reminder (the unit tests cover timing/wording; Routes API verified separately in change 115). Needs a Google sign-in first.

## Files
`memcore/leavewatch.rs` (new, 4 tests), `memcore/mod.rs` (Status.leave_by), `commands.rs` (`memcore_leave_by`), `lib.rs` (start), `wa.rs`/`orchestrator.rs` (two helpers made pub(crate)), Memory page switch + `leaveBySummary` (+test).
Verify: Rust 1181, vitest 216, tsc and cargo check clean.
