# 111 — Memory Core P4: timetable from an image, slot reminders, "Shall I start?"

Plan: `docs/features/100-memory-core-plan.md` (phase P4). Follows change 110 (P3).

## What was built
- **Timetable from a picture** (`memcore/timetable.rs`, `timetable_io.rs`). "Analyse this and add section 2 to my timetable" captures the screen; "add the picture I copied to my timetable" reads the clipboard image (`arboard`). The strongest Gemini model reads it (JSON schema: sections → slots with title, days, start, end). Code, not the model, does the validation: 12/24-hour and `6.30pm` times, `Mon-Fri`/`weekdays`/`daily` days, control-character stripping, 60-char titles, 40-slot cap, de-duplication by a stable slot id, and section selection ("section 2", "the second section", "part b"). Asking for a section that isn't there gets an honest answer ("I can only see 2 sections…").
- **Confirm before saving.** The slots found are shown in a sidebar card and read out ("I found 3 slots in section 2: DSA practice from 6 PM to 7:30 PM; … Shall I add them?"). Nothing is written until you say yes. Slots already on the timetable are not proposed again. A draft stays valid for 10 minutes so "add those slots" works after the short answer window closed.
- **Slots are sealed with the rest of the memory** (tier `slot`, trust `user_owned`, pinned). The secret screen is not applied to timetable titles ("Auth module review" is a normal slot).
- **Reminders** (`memcore/scheduler.rs`). Every 30 s a slot whose start time has arrived (10-minute grace) fires once per day, remembered across restarts. Delivery goes through the proactive policy, so it waits out meetings and speech. Computer activities (currently DSA/LeetCode) ask "It's time for DSA, sir. Shall I start?". Others ("Gym", "Lunch") are only announced.
- **Answering by voice** (`memcore/offer.rs`). A pending question only listens once it has actually been spoken. After it, the frontend opens one 8-second mic window (no wake word), skips ghost sessions and meetings. The reply classifier is strict (yes / no / later / other; long or unrelated speech drops the question and is handled normally). The directed-speech gate accepts the answer instead of treating a bare "yes" as filler.
- **"Yes" starts the activity** the way you prefer. First time for an activity it asks "in the app or in the browser?" and remembers it as `study_app_<activity>`; saying it again, at any time ("use the browser for DSA", "switch DSA to the app", "yes, in the browser"), overwrites it. It then opens LeetCode where you left off and the last DSA video you watched *during a DSA slot*. Resume points are now tagged with the timetable activity running when they were recorded, so an unrelated music video is never opened. If no DSA video exists it says so and leaves YouTube alone. If you prefer the app and none is installed it opens the browser and tells you.
- **"Later"** asks again in 10 minutes; **"no"** skips it today.
- **Voice**: show/next ("show my timetable", "what's next on my timetable"), clear (asks first), add (screen or clipboard, with section), commit, and the app/browser preference. All parsed before screen analysis so "analyse this and add…" is a timetable command, not a screen tour.
- **Command Hub → Memory**: Timetable list with per-slot Remove, and a switch for reminders (`memcoreTimetable`).

## Verify
- Rust: **1116 passed, 0 failed** — 1095 main + 12 `tts_kokoro` + 9 `tts_bench` (the two TTS modules still abort when run together in one process, see change 110). New: timetable (14), offers/drafts (5), scheduler, timetable_io speech, store tier/secret/get_value, parser (timetable phrases + misfire guards).
- `cargo check --features custom-protocol,admin-brain`: clean, 0 warnings. Frontend: `tsc` clean, vitest 206/206 (+1), `npm run build` ok.
- **Live, on this PC:** (1) a picture placed on the real Windows clipboard was read back as a 82 KB JPEG; (2) one real Gemini request on a generated two-section timetable image returned all 5 slots with correct days and times, and "section 2" selected exactly DSA practice (Mon–Fri 6–7:30 PM), Revision (Sat 10–11 AM) and Dinner (daily 8:30 PM) in 2.8 s.

## Not verified / limits
- The spoken round trip was not heard: reminder → spoken question → 8 s mic window → "yes" → pages open. It depends on the frontend turning the captured "yes" into `process_transcript`, which is untested live.
- The test image was clean and typed. Photos, handwriting or busy designs will read worse, which is why the confirm card exists.
- Instagram reels cannot be downloaded: only what is on screen (pause on the timetable frame) or a pasted screenshot can be read. The orb and sidebar can appear in a screen capture.
- YouTube position is still not recorded: the last video reopens, not the second you stopped at. Only DSA/LeetCode activities have something to open; other activities are announced only.
- An "app" preference only works where an installed app is found by name; for LeetCode that will normally fall back to the browser (it says so).
