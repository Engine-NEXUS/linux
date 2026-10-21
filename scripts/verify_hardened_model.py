#!/usr/bin/env python3
"""
Comprehensive Audit & Verification Script for the Retrained NEXUS Wake Word Model.
Tests:
  1. Problem Phrase: "Documentation Created and Synchronized" (test_doc.wav)
  2. Fast Multi-Syllabic Speech Negatives (186 synth_fast_*.wav files)
  3. Pristine Positive Recall (89 nexus_*.wav files)
  4. Vocal Friction (Throat Clearing, Gargling, Coughing)
"""

import glob
from pathlib import Path
import numpy as np
import onnxruntime as ort
from scipy.io import wavfile

ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
DATA_DIR = ROOT / "wake_word_data"

CHUNK = 1280
LOOKBACK = 480
MEL_PER_CHUNK = 8
MEL_CIRC = 10
EMB_FRAMES = 16

mel = ort.InferenceSession(str(OWW_DIR / "melspectrogram.onnx"))
emb = ort.InferenceSession(str(OWW_DIR / "embedding_model.onnx"))
clf = ort.InferenceSession(str(OWW_DIR / "nexus.onnx"))

comfort_emb = np.load(str(OWW_DIR / "comfort_embedding.npy")).astype(np.float32)

def evaluate_wav(wav_path):
    sr, data = wavfile.read(wav_path)
    if data.ndim > 1:
        data = data.mean(axis=1)
    audio = data.astype(np.float32) / 32768.0

    lookback = np.zeros(LOOKBACK, dtype=np.float32)
    mel_buf = [np.zeros((MEL_PER_CHUNK, 32), dtype=np.float32)] * MEL_CIRC
    emb_buf = [comfort_emb.copy() for _ in range(EMB_FRAMES)]

    peak = 0.0
    for i in range(0, len(audio) - CHUNK + 1, CHUNK):
        chunk = audio[i:i + CHUNK].copy()
        rms = float(np.sqrt(np.mean(chunk ** 2)))
        if rms < 0.002:
            emb_buf = (emb_buf + [comfort_emb])[-EMB_FRAMES:]
            mel_buf = (mel_buf + [np.zeros((MEL_PER_CHUNK, 32), dtype=np.float32)])[-MEL_CIRC:]
            continue
        if rms < 0.03:
            gain = min(0.03 / rms, 15.0)
            chunk = np.clip(chunk * gain, -1.0, 1.0)
        framed = np.concatenate([lookback, chunk]) * 32768.0
        lookback = chunk[-LOOKBACK:]
        
        m = mel.run(None, {mel.get_inputs()[0].name: framed[None, :]})[0]
        m = m.reshape(MEL_PER_CHUNK, 32) / 10.0 + 2.0
        mel_buf = (mel_buf + [m])[-MEL_CIRC:]
        
        window = np.stack(mel_buf).reshape(80, 32)[4:80]
        e = emb.run(None, {emb.get_inputs()[0].name: window[None, :, :, None].astype(np.float32)})[0]
        emb_buf = (emb_buf + [e.reshape(-1)])[-EMB_FRAMES:]
        
        stacked = np.stack(emb_buf).astype(np.float32)[None, :]
        pred = clf.run(None, {clf.get_inputs()[0].name: stacked})[0]
        score = float(pred[0][0])
        if score > peak:
            peak = score
    return peak

def main():
    print("=" * 70)
    print("  NEXUS MODEL AUDIT: MULTI-SYLLABIC & POISONING ELIMINATION")
    print("=" * 70)

    # Audit 1: Problem Phrase
    doc_path = ROOT / "test_doc.wav"
    if doc_path.exists():
        doc_peak = evaluate_wav(str(doc_path))
        status = "PASSED (0 TRIGGERS)" if doc_peak < 0.68 else "FAILED"
        print(f"\n[AUDIT 1] Problem Phrase ('Documentation Created and Synchronized'):")
        print(f"   Peak Score: {doc_peak*100:.3f}% (Threshold: 68.0%) -> {status}")

    # Audit 2: Fast Multi-Syllabic Speech Negatives
    fast_files = sorted(glob.glob(str(DATA_DIR / "negative" / "synth_fast_*.wav")))
    if fast_files:
        fast_peaks = [evaluate_wav(f) for f in fast_files]
        fa_fast = sum(1 for p in fast_peaks if p >= 0.68)
        print(f"\n[AUDIT 2] Fast Multi-Syllabic Speech Negatives ({len(fast_files)} files):")
        print(f"   False Alarms: {fa_fast}/{len(fast_files)} ({fa_fast/len(fast_files)*100:.2f}%)")
        print(f"   Average Peak: {np.mean(fast_peaks)*100:.2f}% | Max Peak: {np.max(fast_peaks)*100:.2f}%")

    # Audit 3: Pristine Positive Recall
    pos_files = sorted(glob.glob(str(DATA_DIR / "positive" / "*.wav")))
    if pos_files:
        pos_peaks = [evaluate_wav(f) for f in pos_files]
        pos_hits = sum(1 for p in pos_peaks if p >= 0.68)
        print(f"\n[AUDIT 3] Pristine Positive NEXUS Recall ({len(pos_files)} files):")
        print(f"   Recall at 0.68: {pos_hits}/{len(pos_files)} ({pos_hits/len(pos_files)*100:.1f}%)")
        print(f"   Average Max: {np.mean(pos_peaks)*100:.1f}% | Median: {np.median(pos_peaks)*100:.1f}%")

    # Audit 4: Vocal Friction & Throat/Gargle
    throat_files = sorted(
        glob.glob(str(DATA_DIR / "negative" / "*throat*.wav")) +
        glob.glob(str(DATA_DIR / "negative" / "*gargle*.wav")) +
        glob.glob(str(DATA_DIR / "negative" / "*cough*.wav"))
    )
    if throat_files:
        throat_peaks = [evaluate_wav(f) for f in throat_files]
        fa_throat = sum(1 for p in throat_peaks if p >= 0.68)
        print(f"\n[AUDIT 4] Vocal Friction & Throat/Gargle ({len(throat_files)} files):")
        print(f"   False Alarms: {fa_throat}/{len(throat_files)} ({fa_throat/len(throat_files)*100:.2f}%)")
        print(f"   Average Peak: {np.mean(throat_peaks)*100:.2f}% | Max Peak: {np.max(throat_peaks)*100:.2f}%")

    print("\n" + "=" * 70)

if __name__ == "__main__":
    main()
