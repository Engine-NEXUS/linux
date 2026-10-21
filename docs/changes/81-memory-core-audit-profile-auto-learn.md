# Memory Core — Audit & Forget, Unified Profile, Deterministic Auto-Learning (2026-10-06)

**Plan:** `docs/features/91-memory-first-plan.md` (M0/M1/M2-core executed; M2-cloud, M3–M5 queued).
**Priority:** memory before everything else (user directive) — hardening plan (doc 90) queues next.

## Analysis findings (ground truth, verified before coding)
- `facts.json` was documented ("extracted semantic facts") but **never written** — semantic recall didn't exist; only substring recall on manual facts.
- Memory injection reached the Worker dialog context only; recall capped at 800 chars / 10 hits / 3 episodes, no ranking.
- Google accounts, GitHub identity, voice embedding, contacts.json: four disconnected islands, no unified profile, no audit/forget UX.
- P0 label note: `labels.json` (both copies) already carries 64 intents with `screen_analysis`@62 and both `.onnx` files are stamped 2026-10-02 — a retrain *may* have fixed the off-by-one; onnx output-dim verification stays a §1.1 hardening item.

## What changed
- **M0 audit & forget (local-only intents):** `MemoryAudit` ("what do you remember about me"), `MemoryForget{key}` ("forget my birthday"), two-step `MemoryForgetAll` ("forget everything" → spoken warning) + `MemoryForgetAllConfirm` ("yes, forget everything" — its own intent, no state machine). Dispatch placed before the typing enclave; arms in both normal + ghost pipelines; `route_intent` → LocalCommand; `center_for` → MemoryCenter. Wipe removes core/facts/episodes/user.json; credential islands untouched (profile rebuilds).
- **M1 unified profile (`memory/user.json`):** pure builder merging core facts + contacts.json names (read-only, cap 20) + primary Google email + voice-enrolled bool (GitHub login stays None until an authed call caches it — M5, refresh never touches network). Refreshed on every audit; audit summary speaks name, fact count, top-3 facts, people, conversation count.
- **M2 deterministic auto-learning (local-only):** `extract_learned_facts` (employer/likes/nickname/birthday/city patterns, generic-value + nickname-shape guards) runs inside `log_episode`; upserts to the now-real `facts.json` (cap 200); `recall` searches core + learned with key dedupe. Secrets denylist (`is_secret`, word-exact so "shopping"/"author" stay learnable) enforced in both `remember()` and mining.
- **Bug fixed en route:** `recall` early-returned on missing core.json, making learned facts unreachable — restructured to empty-object fallthrough.

## Verify
- 24 new/affected tests green (15 memory incl. extraction/denylist/mining/profile/wipe, 1 parser with 8 audit + 3 forget + 2-step wipe + 4 guards); full Rust **968/968** serial; tsc/vitest untouched (no frontend changes); release binary **86.4 MB** (size reflects whole tree incl. concurrent session's in-process BERT work).
- Uncommitted. Live matrix (`nexus start`): "remember my birthday is June 1" → "what do you remember" lists it (+name/people if credentials exist) → "I work at ServX" learned silently → "forget my birthday" removes it → "forget everything" warns → "yes, forget everything" wipes.
