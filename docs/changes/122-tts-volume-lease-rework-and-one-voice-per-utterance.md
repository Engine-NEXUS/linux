# 122 - TTS volume lease rework + one voice per utterance (2026-10-09)

## Problem
Ghost-mode log analysis: (1) the Command Hub TTS volume (70%) was not held while NEXUS spoke and the
original volume was not reliably restored; (2) the voice changed within/between utterances.

## Root causes (code reading; not reproduced live)
- V1 `tts::narrate` called `save_and_set_volume()`, whose lease was a dropped temporary -> volume set then
  instantly restored; the later `restore_volume()` was an unmatched release that could steal another speaker's count.
- V2 `stop_tts` -> `force_restore_volume()` zeroed the counter while the old task still owned a lease; its late
  drop decremented the NEW utterance's count (or underflowed) -> restore mid-speech. Ghost calls `stopTts()` constantly.
- V3 frontend `speak()` split replies >=150 chars into one `speak_text` invoke per sentence: lease per sentence,
  volume dipped to the original level between sentences.
- V4 15 s watchdog measured time since last *acquire*, so long replies were force-restored by the next ack.
- V5 baseline only in memory: a crash while at 70% made 70% the next "original".
- V6 restore + re-set flicker between back-to-back utterances.
- A1 `synthesize_chunk_owned` silently switched a single sentence to Kokoro on any edge error (no timeout/retry).
- A2 frontend fell back to the browser Web Speech voice when `speak_text` failed.
- A4 cached acks synthesized by Kokoro at boot (offline) stayed the local voice after the cloud returned.

## Changes
- `volume.rs`: `Controller` state machine (injectable `VolumeBackend`): epoch leases (stale lease = no-op),
  saturating counter, deferred restore (1.2 s, cancelled by any acquire), heartbeat watchdog (10 s without a
  `volume::heartbeat()`), durable baseline `volume_baseline.json` + `init()` crash recovery (ignored if >24 h old),
  no touch if the current volume cannot be read. Removed the `is_already_target` heuristic.
- `tts.rs`: `stop_tts` uses `defer_restore_volume`; `narrate` holds a real lease; heartbeats in all playback loops
  (streamed player uses `recv_timeout` so it keeps beating while the next sentence synthesizes); one engine per
  utterance (`synthesize_chunk_owned(.., force_local)`: chunk 0's engine decides, edge retried once with 8 s timeout,
  failure => silence/stop, never a different voice); stale local-voice cache entries are skipped and the cache is
  regenerated once when the cloud is back; `[TTS] engine= voice= volume= req=` log line per utterance.
- `lib.rs`: `volume::init(app_data_dir)` at setup, `RunEvent::Exit` restores; `tray.rs` quit restores.
- `frontend/src/audio/ttsPlayer.ts`: single `speak_text` invoke per utterance; removed Web Speech fallback and
  `splitForSpeech`. (The optional `ttsWebSpeechFallback` setting from the plan was NOT added - no fallback at all.)
- `scripts/run.ps1`: `[VOL]` and `tts: edge chunk` lines are shown.

## Verify
14 `volume::` unit tests (fake backend), mutation-checked (removing the epoch check + restore-cancel fails
`stale_lease_after_force_restore_is_a_noop` and `defer_restore_then_next_utterance_keeps_level`);
`cargo check` clean; lib suite 1236 pass (3 tests flake when run 4-parallel, pass serially - shared global state);
`npx tsc --noEmit` clean; vitest 224/224.
**Not run**: the live OS-volume probe (it changes the real system volume), a real Ghost session with `nexus build`.
Deviation from plan: watchdog is 10 s (not 4 s) because streaming chunk 0 can take several seconds to synthesize.
