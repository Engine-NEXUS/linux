# 131 — Per-turn latency clock, stock acknowledgements, wake-time NLU pre-warm, voice-settings consistency (P0 + P1 of doc 134/136)

Date: 2026-10-17. Plan: `docs/features/136-voice-connectivity-and-ram-master-compendium.md` (phases P0, P1). No GPT-sol files were touched.

## Built
1. **`[LAT]` per-turn latency clock** (`src-tauri/src/latency.rs`, new). One console line per spoken turn, e.g. `[LAT] stt 322ms | parse 289ms | answer 614ms | tts 1278ms | total 2503ms`. Hooks (one line each): capture stop (`wakeword_oww.rs`, uses `try_lock`, never blocks the audio callback), transcript ready, intent known (`orchestrator.rs`), a Result event leaves the orchestrator (`emit`), first audio queued (`tts.rs`, three playback paths). Stale turns (> 60 s) and proactive speech print nothing. `[LAT]` added to the `run.ps1` whitelist.
2. **Stock acknowledgements**: "Right away sir", "Working on it sir", "Let me check that sir" added to the pre-rendered lines (with "On it sir", "One moment sir" they now cover every ack `pick_ack()` can return). Before, "Working on it sir." was synthesized live (0.84 s) and then discarded when the answer arrived first (log 2026-10-16 15:55), so the ack was never heard.
3. **Wake-time NLU pre-warm** (`nlu_local::prewarm`, called when a capture starts). The command model (+42 MB, 0.66 s first load, measured) now loads while the user is still speaking, removing the 0.29 s first-turn parse stall seen in the log. Race fix: `get_model` now serialises the first load with a lock, so a parse that arrives during the load waits instead of silently skipping the local classifier for that turn.
4. **Voice settings consistency** (`commands.rs`, hub frontend):
   * `normalize_voice_fields` (called by `save_settings`): when `edgeTtsVoice` is a catalog voice, `selectedVoice`, `ttsVoice` and `offlineVoiceModel` are derived from it, so a stale window can no longer write a mixed record (the 2026-10-15 23:02:44 `bm_george` overwrite). Custom (non-catalog) voices are never touched.
   * New IPC `settings_patch` (patch only the named keys onto the CURRENT settings): the Command Hub theme toggles use it instead of re-saving the whole stale form.
   * The hub's voice-card handler now mirrors all five voice fields.
   * Tests: stale-offline repair, stale-persona follows the online voice, custom voice untouched, patch changes only named fields, wrong-type patch rejected.

## Verify
* `cargo check --features custom-protocol,admin-brain` clean; new/affected tests pass (latency 4, settings_voice_tests 5, nlu_local, tts, orchestrator: 109 passed).
* Full lib: 1569 passed, **2 failed** - `intent_parser::tests::test_launch_spotify` and `test_app_phonetic_mishearings` expect "open spotify" to be `OpenApp`, but another session's music-control change (`OpenMusicPlayer`) now parses it differently. **Not caused by this change; not edited.**
* vitest 315/315. `tsc` clean.
* Gates (all caused by other sessions' in-progress work, not by this change): turn-end gate flags `frontend/src/stage/orbRuntime.ts:251` (raw `reset()`); dead-code ratchet shows a NEW `vision.rs: field quota_hit` (and two baseline items now used, one of them `voice_catalog::find_by_cloud_id`, which this change uses) - the baseline must not be updated while a new item from another session is present; IPC gate lists `agent_watch:*` and `ghost:action_receipts` events. `settings_patch` is registered and invoked, so it adds nothing to the IPC gate.

## Not done / not run live
* No release build was made (NEXUS was running, the exe is locked). The `[LAT]` lines, the heard ack and the pre-warm need `nexus build` + `nexus start` + a few spoken turns.
* Edge connection pre-warm was NOT built: a separate warm-up connection would not be reused by the Edge client (new WebSocket per call); a persistent connection is planned in P9.
* P0 items still open: `[RAM]` sampler, AC-power Kokoro re-measure, hand/Hub/ghost/agent-watch measurements.
