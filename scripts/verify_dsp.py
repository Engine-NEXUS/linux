#!/usr/bin/env python3
"""
Diagnostic DSP Verification Script
Directly benchmarks model accuracy on positive, negative, and background files.
"""

import os
import glob
from pathlib import Path
import numpy as np
import onnxruntime as ort
from scipy.io import wavfile

OWW = Path('src-tauri/resources/oww')
CHUNK = 1280
LOOKBACK = 480
MEL_PER_CHUNK = 8
MEL_CIRC = 10
EMB_FRAMES = 16
THRESHOLD = 0.35
SILENCE_RMS = 0.002
TARGET_RMS = 0.03
MAX_GAIN = 15.0


def score_file(wav_path, mel, emb, clf, names):
    sr, data = wavfile.read(wav_path)
    if data.ndim > 1:
        data = data.mean(axis=1)
    audio = data.astype(np.float32) / 32768.0

    # 1. High-pass filter (80Hz)
    dt = 1.0 / 16000.0
    rc = 1.0 / (2.0 * np.pi * 80.0)
    alpha = rc / (rc + dt)
    filtered = np.empty_like(audio)
    prev_y = 0.0
    prev_x = 0.0
    for i in range(len(audio)):
        y = alpha * (prev_y + audio[i] - prev_x)
        prev_x = audio[i]
        prev_y = y
        filtered[i] = y
    audio = filtered

    lookback = np.zeros(LOOKBACK, dtype=np.float32)
    mel_buf = [np.zeros((MEL_PER_CHUNK, 32), dtype=np.float32)] * MEL_CIRC
    emb_buf = [np.zeros(96, dtype=np.float32)] * EMB_FRAMES
    best_p = 0.0

    for i in range(0, len(audio) - CHUNK + 1, CHUNK):
        chunk = audio[i:i + CHUNK].copy()
        rms = float(np.sqrt(np.mean(chunk ** 2)))
        
        # Freezes buffer during silence (does not wipe, does not score)
        if rms < SILENCE_RMS:
            continue
            
        # Gain adaptation
        if rms < TARGET_RMS:
            chunk = np.clip(chunk * min(TARGET_RMS / rms, MAX_GAIN), -1.0, 1.0)
            
        framed = np.concatenate([lookback, chunk]) * 32768.0
        lookback = chunk[-LOOKBACK:]
        
        m = mel.run(None, {names[0]: framed[None, :]})[0]
        m = m.reshape(MEL_PER_CHUNK, 32) / 10.0 + 2.0
        mel_buf = (mel_buf + [m])[-MEL_CIRC:]
        
        window = np.stack(mel_buf).reshape(80, 32)[4:80]
        e = emb.run(None, {names[1]: window[None, :, :, None].astype(np.float32)})[0]
        emb_buf = (emb_buf + [e.reshape(-1)])[-EMB_FRAMES:]
        
        p = clf.run(None, {names[2]: np.stack(emb_buf)[None, :, :].astype(np.float32)})[0]
        score = float(p.reshape(-1)[0])
        best_p = max(best_p, score)
        
    return best_p


def main():
    mel = ort.InferenceSession(str(OWW / 'melspectrogram.onnx'))
    emb = ort.InferenceSession(str(OWW / 'embedding_model.onnx'))
    clf = ort.InferenceSession(str(OWW / 'nexus.onnx'))
    names = (mel.get_inputs()[0].name, emb.get_inputs()[0].name, clf.get_inputs()[0].name)

    print("=== POSITIVE RECALL EVALUATION (20 Clips) ===")
    pos_files = sorted(glob.glob('wake_word_data/positive/*.wav'))[:20]
    pos_hits = 0
    for f in pos_files:
        score = score_file(f, mel, emb, clf, names)
        hit = score >= THRESHOLD
        if hit:
            pos_hits += 1
        print(f"  {Path(f).name:20s}: score = {score:6.3f} -> {'[TRIGGER]' if hit else '[MISS]'}")
    print(f"Positive Recall: {pos_hits}/{len(pos_files)} ({pos_hits/len(pos_files):.1%})\n")

    print("=== BACKGROUND NOISE REJECTION (20 Clips) ===")
    bg_files = sorted(glob.glob('wake_word_data/background/*.wav'))[:20]
    bg_rejects = 0
    for f in bg_files:
        score = score_file(f, mel, emb, clf, names)
        reject = score < THRESHOLD
        if reject:
            bg_rejects += 1
        print(f"  {Path(f).name:20s}: score = {score:6.3f} -> {'[CLEAN]' if reject else '[FALSE ALARM]'}")
    print(f"Background Rejection: {bg_rejects}/{len(bg_files)} ({bg_rejects/len(bg_files):.1%})\n")


if __name__ == "__main__":
    main()
