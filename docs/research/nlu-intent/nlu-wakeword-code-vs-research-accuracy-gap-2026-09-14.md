# NEXUS NLU and Wake-Word Accuracy Gap: Code vs Research

**Date:** 2026-09-14  
**Status:** Code and research comparison; implementation recommendations  
**Constraints:** Low RAM, low latency, English users, safe local execution, three independent checks before each phase

## Executive conclusion

NEXUS can become substantially more accurate without immediately replacing BERT-Mini or increasing runtime RAM significantly. The highest-value improvements are evaluation correctness, calibrated rejection, hard-negative boundary learning, and wake-word data/evaluation quality. Adding larger unfiltered datasets or lowering thresholds is not the correct next step.

The current BERT model is capable enough for the 52-intent reflex layer, but its training and selection procedure is weaker than the repaired dataset. The current wake runtime contains useful protections, but its trigger logic is tuned from a handful of owner tests rather than a speaker-independent DET curve, and the repository does not contain the v3 training assets claimed by historical documents.

## Current NLU strengths

- Deterministic parsing runs before BERT-Mini.
- BERT-Mini is only a fallback.
- Required slots are checked before many actions are converted to executable intents.
- A global confidence gate rejects predictions below 0.85.
- The dataset currently has zero exact train/test leakage, zero conflicting labels, and complete 52-intent test coverage.
- Tokenizer-offset slot alignment and ordinary `O`-token supervision fixed the largest slot-training defects.
- External records now have provenance, license, review, and split metadata.
- CLINC OOS records remain staged and do not enter training automatically.

## Critical NLU gaps

### 1. The frozen test set is reused as validation data

`train.py` shuffles the curated test set and splits it 50/50 into validation and final test. The best checkpoint is selected using half of the frozen test corpus. This means repeated experiments indirectly tune against the evaluation set and make the final score optimistic.

**Required correction:** create three immutable, phrase-family-separated datasets: train, validation, and test. Model selection may use validation only. Test must run once after candidate selection.

### 2. Model selection optimizes only intent accuracy

The checkpoint is selected by validation intent accuracy. It ignores:

- macro F1;
- weak-intent recall;
- slot exact match;
- false slots;
- OOS recall;
- destructive-command safety;
- calibration error.

**Required correction:** use a composite promotion metric with hard safety blockers. Slot and OOS failures must be able to reject a checkpoint even when average intent accuracy improves.

### 3. The 0.85 confidence threshold is uncalibrated

The runtime uses raw softmax probability and a hardcoded global threshold. Research on neural-network calibration shows softmax confidence is commonly overconfident and temperature scaling is an effective low-cost post-hoc correction.

The provisional CLINC result demonstrates this problem:

- raw `unknown` recall: 32.23%;
- recall after the 0.85 gate: 95.10%;
- 49 records still route to a supported intent above the threshold.

This benchmark is not yet authoritative because some CLINC OOS records are valid NEXUS actions, but the high-confidence failures prove raw softmax alone is insufficient.

**Required correction:** fit temperature scaling on an independent calibration set, report expected calibration error and Brier score, then select thresholds from risk/coverage curves.

### 4. One threshold is used for every intent

`type_text`, browser search, app focus, PR merge, branch deletion, and read-only listing do not have equal risk or score distributions.

**Required correction:** calibrate per-intent acceptance thresholds and use stricter policies for side-effecting intents. Destructive operations must also require deterministic safety confirmation regardless of model confidence.

### 5. Unknown is treated as an ordinary 52nd class

Only 44 current training examples belong to `unknown`. Cross-entropy teaches a closed-world classifier and does not necessarily create a robust open-set boundary.

Research supports:

- discriminative OOS training using real and pseudo-outliers;
- energy-based unknown detection;
- embedding/prototype distance methods;
- supervised contrastive learning to tighten in-scope clusters;
- open-domain outlier exposure.

**Recommended experiment order:**

1. Balanced reviewed OOS examples plus calibrated softmax baseline.
2. Energy score derived from existing logits, requiring no larger runtime model.
3. Intent-centroid distance from BERT embeddings.
4. Small supervised-contrastive auxiliary loss during training.

Compare all four on the same frozen sets. Do not assume a more complex method wins.

### 6. Intent and slot heads are only weakly joint

The two heads share BERT features, but intent predictions do not constrain slot predictions and slots do not inform intent classification. Research on explicit joint and supervised-contrastive SLU reports benefits from stronger intent-slot interaction, especially with limited data.

**Low-cost first improvement:** keep the architecture but tune the intent/slot loss weights and select checkpoints using sentence-level semantic frame accuracy. Consider a CRF or explicit joint head only if the simpler candidate fails.

### 7. Training runs 50 epochs without early stopping

The scheduler and fixed epoch count can overfit a small, template-heavy corpus. Checkpoint selection does not track calibration, macro F1, or slot quality.

**Required correction:** patience-based early stopping, multiple seeds, validation curves, and mean/standard-deviation reporting. A candidate should not be promoted from one lucky seed.

### 8. Label definitions are duplicated

Intent and slot lists are independently defined in training code, server code, and `labels.json`. Comments still claim 47 or 58 intents while the actual count is 52.

**Required correction:** make `labels.json` the generated single source of truth for training, export, runtime, and audits. Fail when ONNX output dimensions disagree.

### 9. External OOS labels require NEXUS-specific review

CLINC's `oos` means outside CLINC's own 150 intents, not outside NEXUS. Explicit requests such as browser search may be supported by NEXUS.

**Required correction:** review every retained CLINC row against the typed NEXUS capability map. Keep three outcomes:

- `unknown` training example;
- mapped supported intent;
- excluded/ambiguous.

No pending row should be trainable.

## Current wake-word strengths

- Continuous 80 ms openWakeWord processing.
- Native 16 kHz feature pipeline.
- Energy gate blocks digital silence before classification.
- Capped AGC improves quiet-speech sensitivity.
- Ten-second startup/restart grace handles observed Intel SST transients.
- Three-second refractory period limits duplicate wakes.
- Post-detection confirmation rejects low-energy tails.
- ONNX model hashes and runtime operating parameters are now locked.
- Rust tests verify model loading, silence behavior, and tract/ONNX Runtime parity.

## Critical wake-word gaps

### 1. The deployed model is not speaker-independent validated

Existing evidence is seven owner detections and roughly four minutes of silence. This cannot establish recall across English accents or a meaningful false-activation rate.

**Required correction:** speaker-, session-, device-, and room-disjoint evaluation with enough negative hours to estimate false activations per hour.

### 2. The repository lacks the documented v3 assets

The v3 notebook and local recordings referenced in historical documentation are absent. Reproducible retraining is therefore impossible from this workspace.

**Required correction:** recreate a deterministic training pipeline with pinned dependencies, source manifests, feature hashes, checkpoints, and ONNX parity tests. Private audio remains ignored; manifests and tooling remain versioned.

### 3. Trigger smoothing effectively accepts any single frame over 0.35

The max path returns the highest frame whenever it exceeds `0.35`. The separate `0.5` high-confidence path is therefore not a real bypass; both paths can be single-frame triggers. This maximizes recall but increases exposure to isolated high-score false alarms.

**Required correction:** compare max score, consecutive-frame rules, hysteresis, and learned second-stage verification on a fixed DET curve. Select the operating point from measured recall at a target false-alarm rate.

### 4. Post-trigger confirmation tests energy, not keyword identity

After a raw trigger, the runtime checks whether the following 500 ms has RMS above `0.002`. Background speech/noise can confirm a false trigger, while a valid user who stops speaking immediately can be rejected. This is not acoustic keyword verification.

**Required correction:** replace or supplement this with a second-stage verifier on the buffered keyword audio. Research on successive refinement reports large false-alarm reductions with lightweight hierarchical verification.

### 5. Speaker verification is not active

The engine documentation says speaker verification is part of the pipeline, but the current acceptance branch is hardcoded to `true`. For the selected English-user target, universal wake recognition should remain the default, but optional local owner personalization can provide a stricter mode later.

### 6. Pure digital silence is out of distribution for the classifier

The runtime comments state the model can produce high wake probability on all-zero input. The RMS gate masks this symptom, but the model should also train on digital silence, microphone dropout, reconnect transients, and near-silence.

### 7. Data augmentation over-reuses a tiny source set

Fifty derivatives from one recording increase acoustic variety but not speaker diversity. Randomly splitting derivatives would create severe leakage; the new manifest validator prevents that only after manifests exist.

**Required correction:** prioritize original recordings from many speakers and devices. Treat augmentation as robustness support, not independent evidence.

### 8. Hard-negative mining is missing from the executable workspace

OpenWakeWord's current training guidance uses adversarial phrases, large negative feature corpora, false-positive validation data, cyclic negative weighting, adaptive high-loss batches, early stopping, and checkpoint averaging. NEXUS currently has a basic augmentation script but no reproducible false-positive mining loop.

**Required correction:** replay licensed speech/noise/music and deployment recordings through the candidate, retain high-score windows, review them, and retrain. Research reports substantial false-rejection reductions at a fixed one-false-alarm-per-hour point from regional hard-example mining.

## Ranked improvements

| Rank | Improvement | Expected gain | Runtime RAM | Risk |
|---:|---|---|---:|---|
| 1 | Independent validation/test/calibration splits | Makes every metric trustworthy | None | Low |
| 2 | CLINC semantic review plus NEXUS-specific hard OOS | Large safety gain | None | Low |
| 3 | Temperature scaling and risk/coverage threshold selection | Large rejection gain | Negligible | Low |
| 4 | Per-intent and per-risk acceptance policy | Large safety gain | None | Medium |
| 5 | Multi-seed early stopping and composite checkpoint selection | Moderate broad gain | None at runtime | Low |
| 6 | Energy/prototype OOS comparison | Moderate-to-large OOS gain | Negligible | Medium |
| 7 | Real multi-speaker wake collection with grouped splits | Largest wake recall gain | None | Medium |
| 8 | Wake hard-negative mining and long-duration replay | Largest false-alarm gain | None | Medium |
| 9 | DET-calibrated wake trigger and hysteresis | Large reliability gain | None | Medium |
| 10 | Lightweight second-stage wake verifier | Large false-alarm gain | Small | Medium |
| 11 | Supervised-contrastive NLU auxiliary loss | Possible boundary gain | None at inference | Medium |
| 12 | Larger NLU encoder | Uncertain until above work is complete | Higher | High |

## Recommended next implementation sequence

### Phase A — Correct NLU evaluation first

1. Create immutable validation, calibration, and final-test splits grouped by phrase family.
2. Stop splitting the test set inside `train.py`.
3. Add macro F1, per-intent recall, slot exact match, semantic-frame exact match, ECE, Brier score, OOS AUROC/AUPR, and risk/coverage output.
4. Cross-check split hashes, leakage, and repeatability three times.

### Phase B — Review and calibrate OOS

1. Review the 100 CLINC training candidates and 999 provisional benchmark records against NEXUS capabilities.
2. Build balanced near-OOS, far-OOS, developer, and sensitive subsets.
3. Train the calibrated-softmax baseline.
4. Compare energy and centroid-distance rejection without increasing BERT size.
5. Promote only if supported-intent recall does not regress and sensitive false execution is zero.

### Phase C — Improve known-intent boundaries

1. Add real ASR transcripts for weak live intents.
2. Import small reviewed MASSIVE/SLURP subsets only after OOS is stable.
3. Add confusion-pair batches and supervised contrastive loss as an experiment.
4. Run at least three random seeds.

### Phase D — Rebuild wake training reproducibility

1. Recreate the missing training pipeline and manifests.
2. Collect speaker-independent English data.
3. Add digital silence, microphone resets, TTS, conversational speech, confusables, and deployment noise.
4. Mine hard false positives.
5. Train DNN and conv-attention candidates.
6. Select thresholds from DET curves, not manual tuning.
7. Run at least 100 negative replay hours and device-specific continuous tests.

## Required three-check policy

Every phase must pass:

1. **Structural check:** schemas, hashes, source/license metadata, split-family isolation, and deterministic reruns.
2. **Model check:** independent metrics, candidate-versus-production comparison, calibration, edge cases, and ONNX parity.
3. **Runtime check:** Rust cascade, safety confirmation, latency/RAM, long-running audio behavior, and rollback.

Do not proceed to the next phase when any check fails. Fix the root cause and restart that phase's three checks.

## Primary research used

- Guo et al., *On Calibration of Modern Neural Networks*, ICML 2017.
- Zhan et al., *Out-of-Distribution Intent Detection with Self-Supervision and Discriminative Training*, ACL 2021.
- Ouyang et al., *Energy-based Unknown Intent Detection with Data Manipulation*, Findings of ACL 2021.
- Liu et al., *An Explicit-Joint and Supervised-Contrastive Learning Framework for Few-Shot Intent Classification and Slot Filling*, Findings of EMNLP 2021.
- Hou et al., *Mining Effective Negative Training Samples for Keyword Spotting*, ICASSP 2020.
- openWakeWord current custom-model configuration and automatic-training guidance.
- *To Wake-up or Not to Wake-up: Reducing Keyword False Alarm by Successive Refinement*, 2023.
