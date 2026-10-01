# STT Vocabulary Bias — Why the Decoder Votes "cervix" (Research, 2026-09-22)

## 1. The mechanism: two models glued together

Whisper = **encoder** (ears: audio → sound features) + **decoder**
(brain: features + language prior → words, one token at a time).
"Servx" vs "cervix" are ~95% identical acoustically — the ears genuinely
can't decide. The decoder then asks *"given these sounds AND everything
I know about English, what's most likely?"* "Cervix" has millions of
prior sightings; "servx" has zero. Same audio, the vote goes cervix
every time. **A vocabulary failure, not a hearing failure** — which is
why Zinc beats Zync and "service" beats Servx by the same mechanism.

## 2. What the `prompt` field is

Sent as if it were the previous sentence of the conversation. The
decoder conditions on preceding text, so the prompt re-weights priors:
"servx just appeared in context — probably that again." Properties that
constrain the design:

- A nudge, not a command: clearly-spoken other words still transcribe
  normally (a doctor's "cervix" stays "cervix").
- Capped at ~244 tokens: ~40 terms max, not a dictionary.
- Style-continuous: the decoder continues the prompt's style, so
  command-styled sentences bias vocabulary *and* phrasing; a bare list
  biases vocabulary only.

## 3. The trio (verified in-tree)

| Setting | Location | Job |
|---|---|---|
| `language="en"` | `stt_groq.rs` (all three Groq call shapes) | Locks English; kills language-ID misfires on Telugu-English mixing |
| `temperature="0"` | verbose/pinned path | Greedy decoding; same audio → same output |
| `prompt` | plumbing existed (`transcribe_with_groq(..., prompt)`), all live callers passed `None` | **The gap**: Groq transcribed with zero knowledge of NEXUS terms |

2 of 3 running = "accurate but ignorant."

## 4. Method comparison (why this shape and not alternatives)

| Option | Verdict | Reason |
|---|---|---|
| Whisper `prompt` on Groq path | **Adopted** | Zero data, zero training, one param, reversible; attacks the prior directly |
| Fine-tune Whisper on collected audio | Rejected | 24 samples; small-data fine-tuning of large acoustic models degrades general speech (matches ChatGPT's own caveat) |
| Vendor switch on benchmark tables | Rejected | Unverifiable Sept-2026 model/WER claims; Groq stays until an on-mic NEXUS-command comparison says otherwise |
| Moonshine-side biasing | N/A | This integration's Moonshine path exposes no prompt mechanism; covered by alias map + NLU rows |
| Verbose wake-verifier prompt | Deferred | Short "hey nexus" utterances; bias could distort verification — minimal blast radius |

## 5. Relationship to the alias map (defense in depth, not duplication)

Prompt prevents upstream (fewer cervix-class errors at transcription);
`canonical_repo_name` repairs downstream (accent/noise/fast-speech
outliers); heard-text NLU rows generalize across both. Same doctrine as
401-retry + card: never rely on one layer. The prompt is also what keeps
future datasets clean — fewer garbled rows entering BERT training,
compounding each retrain.

## 6. Measurement protocol (no faith required)

Next `nexus collect` session is the experiment (same phrases, mic,
accent; only variable = prompt). Metric: **cervix-rate** = fraction of
Servx-utterances transcribed without the repo-intended term; plus
retries burned on entity words. No movement → one-line revert.
