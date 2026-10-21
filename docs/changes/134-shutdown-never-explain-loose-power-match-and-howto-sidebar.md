# 134 — "Shut down my laptop" is never explained aloud: loose power match, learned mishearings, how-to answers in the sidebar

Date: 2026-10-17. Plan: `docs/features/138-shutdown-never-explain-misheard-power-commands-and-howto-in-sidebar-plan.md`. Owner decisions (delegated): D1 = only how-to-shaped questions / procedure-shaped answers go to the sidebar (conversation stays spoken); D2 = a weak (structural) shutdown match asks once and learns; D3 = line "The steps are in the sidebar, sir."

## Built
1. **Strict list extended with every mishearing the owner actually produced** (`power_cmd.rs`): "tick on", "send down" added (with the earlier "check down / chuck down / shuck down"). All six logged variants are now obeyed at once.
2. **Structural loose match** (`power_cmd::parse_loose`): <= 5 words, `<1-2 plain words> <down|off> <computer word>`, not a question, first words not an ordinary verb (denylist). "on" is deliberately NOT a particle ("music on my laptop"). Weak evidence: NEXUS asks "Did you want me to shut down your laptop, sir?" (existing `Offer::Power` yes/no) and never sends it to the cloud.
3. **Learned phrases** (`power_aliases.json`, local, max 50, only phrases the loose rule matches): a "yes" teaches the heard phrase, next time it is obeyed at once (`is_learned_shutdown` -> `run_power_action`).
4. **`howto.rs`** (new, pure): how-to question detection; a local steps card for power topics ("how do I shut down / restart / sleep / lock / sign out") shown in the sidebar with one spoken line, no cloud call; `delivery_for(transcript, answer)`: an answer over 160 characters that is a how-to question's answer OR looks procedural (>= 3 step verbs or >= 3 list lines) goes to the sidebar with "The steps are in the sidebar, sir." instead of being read out. The logged 288-character tutorial (it slipped under the old 300-character sidebar threshold) is now caught by its shape.
5. **Hooks (orchestrator.rs, narrow):** `try_power_and_howto` inside `try_unknown_local_fallbacks` (shared by the normal and ghost branches, Unknown intents only); the yes-arm of `Offer::Power` teaches the alias; the Worker result path adds `howto_sidebar` to the existing sidebar decision.
6. **STT bias** (`stt_groq.rs`): "Shut down my laptop. Restart my laptop. Lock my laptop." added to the vocabulary prompt (136 of 140 words).

## Verify
* New tests: `power_cmd::loose_tests` (5), `howto::tests` (9, incl. "the deterministic parser must not claim these"). Real mishearings replayed; 35-sentence negative corpus; learn/cap/expiry behaviour.
* Mutation-checked: removing the benign-verb denylist, the question-word guard, the plain-word guard, the particle-position guard, or the procedural-verb rule each makes a test fail.
* Full lib (skipping tts_bench/tts_kokoro): 1599 passed. Failures not from this change: the two Spotify parser tests (music-control session), `turn_detect::model_loads_and_runs` (timing assertion, fails in debug under load), and `auth_vault::google_profile_phone_address...` only in the parallel run (passes alone). Dead-code gate shows `vision.rs: quota_hit` from another session; baseline not touched.

## Not verified / needs a rebuild + the owner
* Whether Whisper now hears "shut down my laptop" correctly with the new prompt words.
* The 160-character threshold is a first guess; the sidebar card look in both themes; the spoken question window with a real mic.
* An NLU (BERT) classification of a mishearing would claim it before the Unknown fallback; none was seen in the logs.
