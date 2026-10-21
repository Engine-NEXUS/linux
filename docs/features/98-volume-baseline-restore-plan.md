# Speaker Volume Baseline Restore Plan (2026-10-07)

**Ask:** user sets speaker to 70%; after NEXUS uses the speaker it must return to the original %. Today it can get stuck at TTS level. Plan only.
**Mechanism on disk:** `volume.rs` RAII leases (`acquire` saves baseline on 0→1, `release` restores at 0) + `force_restore_volume()` on stop + 15s watchdog. Three TTS acquire sites (streaming, legacy, cached); all drop correctly on normal/error paths; no task-abort leaks (generation polling, not aborts).

## Root causes (ranked, all verified by reading)

**R1 — Barge-in race poisons the baseline (primary).** `stop_tts` → `force_restore_volume()` zeroes the counter AND clears saved baseline while the old task still holds its lease. A new utterance acquiring immediately after sees count 0 → reads current volume (still at TTS target, old playback not yielded) → the `is_already_target` guard fails (saved is now invalid) → stores the TTS target AS the baseline → restore puts back TTS level forever. The 15s watchdog restores to the same poisoned value — useless.
**R2 — force_restore is unconditional.** It clears saved + zeroes count even with outstanding leases that would have restored correctly on drop.
**R3 — No adopt-user-change rule.** User moves the mixer mid-speech → release clobbers it with the stale baseline.
**R4 — Unreadable-baseline fallback invents 30%.** If `get_system_volume` fails transiently, restore later forces 0.30 over the user's real level.
**R5 — Counter underflow on double-drop** (`fetch_sub` on 0 wraps; currently harmless by accident via `prev <= 1`, but fragile).

## Fixes (in order)
**F1 — Generation-tied baseline.** Store `(generation, baseline)`; only the generation that saved may overwrite; acquire while a newer generation owns the baseline keeps it (never re-saves from TTS-target level). Kills R1 structurally.
**F2 — force_restore respects outstanding leases.** Cut audio (set volume now) but do NOT clear saved/counter when leases are held; their drops reconcile. Watchdog keeps its 15s role for genuinely leaked leases only.
**F3 — Adopt user changes.** On release, if current volume differs from TTS target by >3% (user moved the mixer), adopt current as the new truth instead of restoring. Never fight the user.
**F4 — No fabricated baselines.** If baseline unreadable and no valid saved value, don't touch the mixer at all (leave user's level alone) instead of forcing 0.30.
**F5 — Saturating counter + full-value logging.** `saturating_sub`, and every save/restore/force logs old→new values (log-completeness: cross-checkable in console).
**F6 — Test seam + tests.** Indirection for get/set volume (mock backend in tests; the existing `test_save_and_restore` touches the REAL mixer — quarantine it `#[ignore]`/mock it). Tests: barge race (acquire→force→acquire→drop→drop ends at original), user-change adoption, unreadable-baseline no-touch, underflow, watchdog-leak drain.

## Acceptance & ordering
F1 → F2 → F3 → F4 → F5 → F6. Live script: baseline 20% → speak → 75% during → back to 20% after; barge mid-speech twice → still 20%; mixer to 70% mid-speech → stays 70%; all transitions visible in logs with values.
