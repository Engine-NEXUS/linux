# 135 — Smart fallback: how long the switch takes, and NEXUS telling you "I'm offline" / "I'm back" (PLAN, nothing built)

Date: 2026-10-17. Owner decisions in this round: **Smart mode**; the spoken notices must be varied (never the same sentence twice in a row), tell the user the connection is low or gone, say "I'm back, sir" after local mode is ready and again after online returns, and do it **automatically even if the user did not call NEXUS**. Builds on `134` (state machine, bank, standby) and replaces its section 6 default (Always ready -> **Smart**).

---

## 1. "How long does the switch to local take? Is it 4 seconds?" — your guess is right for TODAY
Numbers come from the code constants and from measurements already taken; only the marked rows are estimates.

| Situation | Today (code) | With this plan (Smart) |
|---|---|---|
| Wi-Fi/cable disconnects (Windows raises a network event) | nothing notices until a request fails | event seen in **<= ~1 s**; local gets warmed immediately; the next line already goes local |
| Internet silently dies (router up, upstream down: **no** OS event) | the first line waits **4 s** for Edge (`EDGE_FIRST_AUDIO_TIMEOUT_SECS`) then up to **2 s** more for the local voice (`LOCAL_FIRST_AUDIO_BUDGET_MS`), and the Windows voice may speak: **~4-6 s of delay and a different voice** | something is **heard at ~2 s** (a banked "give me a minute" phrase, instant), the real answer follows; later lines go local at once |
| Speaking a **banked** line offline (stock lines, notices, previously spoken lines) | n/a | **~10-40 ms**, Edge voice |
| Speaking a **never-spoken** sentence with local Kokoro | ~4.3 s for a 3-word line on this laptop **on battery** (RTF 2.4, measured); earlier recorded 0.7-1.7x real time on AC | same engine, so 1.7-4.3 s for a short line; hidden behind the notice phrase; long novel sentences are rare offline because the cloud model is gone too |
| Local voice **cold** (not preloaded) | 1-2 s model load + first-synthesis warm-up (first call ~8 s in a debug build) | avoided by Smart pre-warm (below); if still cold: notice phrase covers it |
| Offline **speech recognition** cold start | **10-15 s** (Python Moonshine, killed after 60 s idle) | pre-warmed on the same triggers; unmeasured replacement engine (doc 134 P4) |
So the realistic worst case in the new design is the **first line after a silent upstream outage: ~2 s before you hear the notice, then ~2-4 s for a novel answer** — but only once; and only if the link looked healthy right up to that moment.

## 2. Smart mode: when the local engines are kept warm
Local voice + local STT + NLU are **loaded** (ready in ~0 s) whenever ANY of these is true, and **unloaded** after 30 min of continuous stability:
1. Windows reports a connectivity change, roaming, metered/limited connection, or captive portal (HTTP 204 check fails).
2. Wi-Fi signal below a threshold or link speed collapsed (WLAN signal-quality query).
3. Any Edge, Groq or 9Router request failed or was slow (first-audio > 2 s, answer > 4 s) in the last 10 min.
4. Probe round-trip p95 > 800 ms or any probe loss.
5. The first 2 min after start, the first 2 min after resume from sleep, and **10 min after any recovery** (relapse window).
6. Learned pattern: this PC lost the internet at least twice in the same hour-of-day in the last 14 days (cheap rule, local data only).
Probing is adaptive: every **15 s** while not stable, **45-60 s** while stable (one tiny HTTPS request to the Edge speech host). Cost on the RAM budget: **~280 MB typical (stable), ~690 MB while warm** (doc 134 table).
Warm-up includes one silent dummy synthesis so the first real sentence does not pay the first-run allocation cost.

## 3. Spoken connection notices
### 3.1 Three situations (all can happen without the user doing anything)
| # | Trigger | What NEXUS does | Gate |
|---|---|---|---|
| A | **User calls NEXUS while the link is OFFLINE or very low** (wake word / hotkey fires) | speaks a banked phrase **within ~0.3 s of the wake**, before any transcript exists (covers the cold-STT wait): offline phrase or low-connection phrase | once per call; in the same offline episode later calls get a **short** variant (setting: *Normal / Minimal*) |
| B | **Connection lost and confirmed** (OFFLINE for >= 12 s, so a Wi-Fi blip never talks) | proactive line when local mode is ready: "I lost the connection, sir. Local mode is on." Visual chip `OFFLINE · local mode` | at most **1 offline notice per episode**; not in a meeting; not over speech/ghost drill; user present (input in last 10 min) |
| C | **Connection back and PROVEN** (doc 134 recovery proof) | proactive "I'm back online, sir." | only if an offline notice was spoken or the outage lasted >= 60 s; at most **1 per episode**; same presence/meeting gates |
Extra cases: **D** a request that needs the internet while offline ("what's the weather", cloud chat) -> "That needs the internet, sir. I can still open apps, type and control windows." **E** *low connection* (slow, not gone) is announced only when you call, not unprompted (setting *Announce slow connection unprompted*: off by default).
"Proactive" goes through the existing `proactive_policy` (never talks over the user or NEXUS, waits out meetings, 20 s gap, 6/hour), so notices cannot pile up.

### 3.2 Variety (never the same line twice)
Phrase bank as data (`connection_phrases.json`), categories x persona (butler "sir" / friend uses your name). Every category has >= 12 variants; the picker keeps a history and never repeats any of the last 4 used in that category, weights shorter lines when the user called in a hurry, and may append context ("it's been a few minutes", "still offline"). Examples (butler tone; final copy to be reviewed by you):
* **OFFLINE_ON_CALL**: "Give me a minute, sir, I can't reach the server." · "I'm offline at the moment, sir. Local commands still work." · "No connection right now, sir. I'll handle what I can here." · "I've lost the internet, sir. One moment while I switch to local." · "The server isn't answering, sir. Working locally for now."
* **LOW_CONNECTION_ON_CALL**: "My connection is slow, sir. Bear with me." · "Weak signal, sir. This may take a moment longer." · "The line is shaky, sir. I'll be a little slower than usual." · "Low internet, sir. Still with you."
* **OFFLINE_PROACTIVE (B)**: "I lost the connection, sir. Local mode is on." · "We're offline, sir. I'm ready to work locally." · "The internet dropped, sir. Local mode is ready."
* **BACK_ONLINE (C)**: "I'm back online, sir." · "Connection restored, sir. All systems back." · "The internet is back, sir. I'm fully online." · "Back online, sir. Thanks for waiting."
* **LOCAL_READY_ON_CALL (first call after switching)**: "I'm back, sir. Working locally." · "Local mode is ready, sir. Go ahead."
* **NEEDS_INTERNET (D)**: "That needs the internet, sir. I'll answer when it's back." · "I can't look that up offline, sir. Local tasks are fine."
All variants <= 14 words and contain no digits, so they can be banked; the 9-word cache limit in `tts_cache_disk::cacheable` is raised only for these curated notices.

### 3.3 They must work with NO internet
Offline there is no Edge. So **every phrase is pre-rendered in the Edge voice while online** (paced 400 ms apart, stop at the first 429), stored in the bank (original MP3 + word boundaries), per persona voice and per user name. Size: ~120 phrases x ~3 s x 6 KB/s = **~2 MB**. Re-rendered automatically when the voice persona, prosody or user name changes. If a phrase is missing (first run offline), the line is spoken by local Kokoro, or shown as a caption + chime if that is not ready either; never by the Windows voice.

### 3.4 Visual side
Orb/capsule status chip: `OFFLINE · local` / `LOW SIGNAL` / (disappears after "back online"). Caption mirrors the spoken line. No new window.

## 4. Settings (all in the Hub, patch-saved so a stale window cannot revert them)
`Connection announcements`: *Always (default)* · *Only when I speak to NEXUS* · *Off*. `Instant offline fallback`: *Smart (default)* · *Always ready* · *Save RAM*. `Announce slow connection unprompted`: off.

## 5. Plan additions to doc 134
| Step | Work | Days | Gate |
|---|---|---|---|
| P2b | `connection_voice.rs`: phrase data, variety picker (no repeat in last 4), episode tracker, presence/meeting/rate gates via `proactive_policy`, `[NET-SAY]` logs | 1.5 | unit tests: variety, one-per-episode, blip silence (< 12 s), back only after offline notice or >= 60 s, quiet in meetings |
| P3b | Bank pre-render for notices (persona x name), regenerate on change, offline playback test | 0.5 | all notices play offline in the Edge voice <= 50 ms |
| P4b | Smart warm/cold triggers (events, signal quality, outcome history, probe stats), warm-up dummy synthesis, cold unload after 30 min stable | 1.5 | warm within 3 s of a trigger; RAM back to ~280 MB after stability |
| P4c | Call-time hook: on wake while OFFLINE/LOW -> banked phrase <= 0.3 s | 0.5 | measured wake-to-sound <= 0.3 s with local STT cold |
| P2c | Chaos test (30 min random drops, silent outages, flapping, captive portal, DNS-up/HTTPS-down) | 1 | no mid-utterance switch, no flapping, no more than 1 offline + 1 back notice per episode |
Ownership: all in our files (`connection_voice.rs`, `tts*.rs`, `tts_network.rs`, `proactive_policy.rs` hook, bank). Shared files get one-line hooks only. GPT-sol's Ghost files untouched.

## 6. Decisions to confirm
1. Default `Connection announcements = Always` (unprompted "I'm offline" / "I'm back")? You asked for automatic; the plan gates it so a blip never talks.
2. **12 s** of confirmed offline before the first unprompted notice, and **60 s** minimum outage (or a spoken offline notice) before "I'm back online": OK?
3. Notice wording reviewed by you before the bank is rendered (I will draft 12+ variants per category).
4. Low-connection notices only when you call (not unprompted): OK?
5. Short variants for the 2nd+ call in the same offline episode: OK?

## 7. Not verified
The 4-6 s "today" figure is from code constants (4 s + 2 s), not a live capture of a real outage; the new timings are targets until P0 timing and the chaos test exist. Windows connectivity-event behaviour on your Wi-Fi/ISP, WLAN signal thresholds and captive-portal detection need real-network testing. Local Kokoro speed was measured on battery; plugged in it may be faster. Offline STT readiness depends on the unmeasured in-process engine. Nothing here is built.

---

## FINAL OWNER DECISIONS (2026-10-17) — these override sections 3.1/6 above where they differ
1. **Notices are reactive only**: NEXUS speaks a connection notice **only when the user tries to reach it** (wake word / hotkey) while it is offline or on a weak link. **No unprompted "I'm offline"** (situation B in 3.1 is removed). Setting default = on; user can turn it off.
2. Thresholds accepted: **12 s** confirmed offline, **60 s** proof before returning online.
3. Wording does not need review first; the full research + plan was consolidated into `136-voice-connectivity-and-ram-master-compendium.md` for later review.
4. **Slow-connection** notices only when the user calls. **"I'm back" (local ready, and back online) is spoken only if the user had tried to reach NEXUS during that outage** (situation A happened). If the user never called, NEXUS stays silent when the connection returns.
5. Repeat calls in the same offline episode get **different phrases, not shorter ones**. From the **4th call onward** ("more than thrice") NEXUS uses the dedicated category **STILL_OFFLINE**, e.g. "Still the same, sir. I'll let you know when I'm back." (>= 8 variants, same no-repeat-last-4 rule).
Resulting event flow: user calls while OFFLINE -> banked phrase <= 0.3 s (call 1-3: OFFLINE_ON_CALL variants; call >= 4: STILL_OFFLINE) -> local ready -> "I'm back, sir, working locally." (LOCAL_READY) -> command handled -> later, connection proven -> "I'm back online, sir." (BACK_ONLINE, only because the user called during the outage).
