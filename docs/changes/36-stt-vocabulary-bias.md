# 36 — STT Domain Vocabulary Bias (2026-09-22)

Seeds Groq's Whisper decoder with NEXUS's world so rare entities win
acoustic ties ("servx" over "cervix"). One constant, two call sites, one
guard test. No training, no data merge, no model swap. Research
companion: `docs/research/stt-vocabulary-bias-2026-09-22.md`.

---

## 1. What changed and why

The Groq STT plumbing already accepted a `prompt` decoder-bias field
(`transcribe_with_groq(..., prompt)`, with `language="en"` and
`temperature="0"` on the pinned path) — but every live caller passed
`None`. Groq therefore transcribed NEXUS commands with zero knowledge
that Servx, Zync, or Eesha exist, and the alias map repaired the damage
downstream. This change fixes it at layer 1 while keeping every
downstream safety net exactly where it is.

## 2. Implementation

- **`src-tauri/src/stt_groq.rs` — `pub const NEXUS_VOCABULARY`** (~45
  words): command-styled sentences carrying every alias-map entity
  (Servx, Zync, Eesha, Prem, Lakshya, Congi, Shopkart, Ledger…),
  command verbs (analyse, pull request, merge, approve…), and the user's
  phrasing openers ("show me", "list the", "check", "and all"). Placed
  in `stt_groq.rs` because prompt semantics are Whisper-API-specific.
  Documented inline with the rules it honors (nudge-not-command,
  style continuity, token cap, safety-net relationship).
- **Call site 1 — `src-tauri/src/stt.rs` (`transcribe_audio` Tauri
  command):** `None` → `Some(NEXUS_VOCABULARY)` (with an inline comment
  stating the reason: rare entities winning decoder bets).
- **Call site 2 — `src-tauri/src/wakeword_oww.rs` (Rust-side STT
  capture, the live command path):** `None` →
  `Some(NEXUS_VOCABULARY)`. No signature changes — it flows through
  `transcribe_samples`' existing pass-through `prompt` parameter.
- **Deliberately untouched:** the verbose wake-verifier path (short
  "hey nexus" utterances where bias could distort verification) and the
  Moonshine fallback (no prompt mechanism in this integration).

## 3. Guard test

`test_nexus_vocabulary_budget_and_coverage` (`stt_groq.rs` tests):
fails the build if the prompt exceeds 140 whitespace-words (≈195 tokens
at ~1.3/word — safe margin under Whisper's ~244-token cap; trim terms,
never raise the cap), and asserts all ten critical entities
(servx, zync, eesha, nexus, github, whatsapp, prem, lakshya, congi,
shopkart) are present (lowercased match).

## 4. Verification (twice)

`cargo check --lib` clean; `cargo test --lib stt` 22/22; new test green
on repeat runs. No frontend, Worker, dataset, or model changes — the
understand-fast/ack-instantly/answer-slowly chain is behaviorally
identical except for cleaner input text.

## 5. Measurement (pending live session)

Cervix-rate before/after on the next `nexus collect` (same phrases, mic,
accent), plus retries burned on entity words. No movement → revert the
two `Some(...)` arguments. Expected direction (honest, not promised):
majority reduction of rare-term errors, never 100% — outliers stay the
alias map's job.
