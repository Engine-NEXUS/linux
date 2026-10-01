# 100-Combination Review — v30 vs v2, full gate (2026-09-19)

## Method
- `wake_camp/combo100.py`: 60 pos (10 sealed heldout + 50 stratified train) + 39 neg
  (20 SAPI TTS + room-tone mixes + formant/lexus negatives). 1 poison-deleted case
  drops the total to 99. Thresholds: KWS 0.5, near-floor 0.6, veto 0.85.
- Gate under test = production mirror: streaming KWS → Groq STT (temp 0, verbose,
  no_speech veto) → word gate (exact + 11 near-words) → 1 retry on suppress.
- Transcripts cached (`wake_camp/stt_cache.json`): reruns are Groq-free, KWS-local.
- Second run widened NEAR with measured d1 forms: nixus, nixis, nexat, nexo, nexos.

## Headline (identical for both models)
- **v30: recall 26/60 (43.3%), FA 2/39 (5.1%)** — `combo100_results2.txt`
- **v2: recall 26/60 (43.3%), FA 2/39 (5.1%)** — `combo100_v2.txt`
- The models trade single cases (v30 takes BN01, v2 takes BV05). The STT+word
  gate dominates the outcome, not the classifier.

## Miss breakdown (34 pos misses)
1. **STT confabulation despite KWS fire (~28):** stage-1 fires (e.g. BASE02 0.977,
   H01 0.9996, A02 0.929) but Groq returns fluent non-wake text on BOTH attempts
   ("I'm going to be a song", "in excess", "One day. Bye!"). Verifier correctly
   suppresses garbage — but for TRUE clips this is a miss caused by STT, not KWS.
2. **Genuine KWS-weak clips (~5):** BASE01 0.230, BASE03 0.459, BV04 0.298,
   BX01 0.156 (v30 streaming scores; `probe_base.py`). Both models agree BASE01
   is weak (v2: 0.007). These are the only misses a retrain could address.
3. **Fused-form tail (1):** A20 "Canixis" — whole-word gate misses prefix-fused
   spellings. Accepted loss (documented; substring matching would accept "alexis").
4. **Fixed by widened NEAR:** BN05 "Hey Nixus!" now fires (was suppressed in run 1).

## False alarms (2, both by design)
- SAPI00/SAPI15 "Lexus" at prob ≥ 0.6 → two-key fires. Correct per gate spec
  (clean near-word + strong stage-1). Live exposure ≈ zero (requires TTS-clean
  "lexus" + loud stage-1 in the same second).
- SAPI18 (silence formants) and all 20 room-tone mixes correctly suppressed.

## v30 vs v2
- Matrix tie; profiles differ: v30 stronger on near_normal/confusion
  (A01 0.68 vs 0.005, BASE05 0.74 vs 0.001), v2 stronger on loud
  (BV04 0.99 vs 0.30, BV05 0.998 vs 0.485).
- The 10-clip heldout (v30 9/10) is too thin to crown a champion. No retrain
  is justified: only ~5/34 misses are KWS-side, and they split across models.

## Open discrepancies (flagged, not hidden)
- An earlier "eval_recall 185/190 + BASE 5/5" result is NOT reproducible from
  current files (BASE01 = 0.230 v30 / 0.007 v2 at any sane threshold). Possible
  causes: pre-purge manifest, different threshold/model arg, or re-recorded BASE
  clips. Current `eval_recall.py` (v30 @0.5) is self-consistent with the matrix:
  `nexus/loud/train` 11/18 = 61%, heldout near_normal 7/9.
- ~10 far/base clips transcribe as fluent chat ("See you next week", "Thanks,
  sir", "trying to make a little bit of a day"). If the mic caught TV/other
  voices during those rounds, they are contaminated and must be quarantined —
  only ears can settle this (see Ask below).

## Verdict
NOT perfect. The product is SAFE (FA 5%, suppressions correct, zero regressions
across runs, 449 Rust tests green) but recall on this matrix is 43% — gated
almost entirely by Groq confabulating on short clips, plus ~5 genuinely weak
KWS clips. Do NOT retrain until the Ask below is answered.

## Ask (needs owner ears, 5 min)
Listen to these 5 files — is it YOU saying the wake word, or something else?
- `wake_camp/raw/hey_nexus/far/BF02.wav` → STT: "See you next week"
- `wake_camp/raw/hey_nexus/near_normal/BASE02.wav` → STT: "I'm going to be a song"
- `wake_camp/raw/hey_nexus/far/F04.wav` → STT: "We met you"
- `wake_camp/raw/hey_nexus/near_normal/A06.wav` → STT: "One day. Bye!"
- `wake_camp/raw/hey_nexus/quiet/Q04.wav` → STT: "Very nice, though"
- If ANY is not-you: those rounds are contaminated → quarantine + re-record,
  then rescore (free, cached). If ALL are you: STT is the ceiling → accept
  verifier as-is and proceed to live soak.

## Threshold sweep (2026-09-19, KWS-only, Groq-free)
- v30: 93.3% @0.35-0.45 (56/60), 90.0% @0.5-0.6 (54/60). v2: 78.3% / 75.0%. v30 champion CONFIRMED on 60 clips (was crowned on 10 — now properly backed).
- All 17 TRUE negatives (room/TV/conversation backgrounds) score <=0.31: real background never reaches stage-1, never spends a Groq call. The 22 stage-1 'FA' are all SAPI TTS near-word lookalikes, caught downstream by the two-key rule (only 2 become product FAs).
- Live Rust threshold is ALREADY 0.45 (= sweep optimum): live stage-1 recall = 93%. The 43% full-gate number is STT confabulation, not KWS. No threshold change needed, no retrain needed.
- Files: wake_camp/thresh_sweep.py, sweep_v30.txt, sweep_v2.txt.

## Proceed: v4 binary + retrain staging (2026-09-19)
- Release binary built with --features custom-protocol to src-tauri/target/v4stage/release/nexus.exe (51MB, 12m23s). Contains: v4 gates (backward-confirm, verify-retry, barge-in hook, webrtc-vad pre-gate), widened 11-word NEAR set, split-model loader (model_for_path). Standard target/ untouched because the live app is running (exe locked) — swap on next restart via nexus build/start.
- Bundle verified without launching: tauri.conf.json resources/oww/**/* covers nexus.onnx (13KB) + nexus.onnx.data (856KB); runtime resolver tries prod layouts then dev fallback. 449 lib tests green on the same code.
- Retrain STAGED, not launched: wake_camp/pack_retrain.py exports 187 ACCEPT clips (0 missing, deletion_record-honored) to wake_camp/retrain_pack/{train,heldout}/ + dataset_card.json. Kaggle upload + kernel launch still gated on: (1) 5-clip contamination answer, (2) explicit approval.

## Live debug: verify-ring dump (2026-09-19 ~19:20 IST)
- Owner shouted NEXUS (old binary): stage-1 fired 0.975 (model heard it), verifier suppressed on Groq confabulation 'or if I don't explain this to me'. Loud/clipped audio confabulates — camp loud-recall 61% vs 93% normal. Shouting backfires; normal voice + close mic wins.
- wakeword_oww.rs: dump_verify_debug + encode_wav_pcm16 (dep-free, rotating verify_00..04.wav in %APPDATA%/com.nexus.assistant/verify_debug/, best-effort, 1 new test, 41 lib tests green).
- Side-binary lesson: target/v4stage2/release needed a manual copy of src-tauri/resources (exe_dir/resources lookup); without it the app exits 0 silently. Live now: target/v4stage2/release/nexus.exe (dump build). Next owner Nexus -> pull verify_XX.wav and listen to exactly what the verifier heard.

## P1-P4 implemented: anime-TV cascade (2026-09-19 ~19:40 IST)
Cause (user-confirmed: Japanese anime on TV): TV audio -> 2.2s capture -> correct Japanese transcript -> no script guard -> LLM answered in Japanese -> English TTS -> byte-slice panic at tts_piper.rs:129 (byte 50 inside no) + panic=abort -> whole app dead. Edge content-rejection had also flipped the global network-down flag.
- P1 stt.rs: CJK/kana/hangul >=2 chars -> '' (retry, never action) + tests incl. exact crash sentence.
- P2 tts.rs truncate_for_log (char-safe) used at tts_edge.rs:46 + tts_piper.rs:129; audit found no other str-slicing sites. Test with crash string.
- P3 tts.rs: only timeout marks network down; API/content rejections fall through to Piper for that call only.
- P4 wakeword_oww.rs: dump_capture_debug -> capture_NN.wav (rotating 5) in verify_debug/.
- 461 lib tests green. Deploy via nexus build + start.
