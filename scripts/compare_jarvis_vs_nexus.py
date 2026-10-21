import os
import glob
import time
import numpy as np
import scipy.io.wavfile as wavfile
import onnxruntime as ort
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
DATA_DIR = ROOT / "wake_word_data"

jarvis_path = ROOT / "scripts" / "hey_jarvis_v0.1.onnx"
nexus_path = OWW_DIR / "nexus.onnx"

mel_path = OWW_DIR / "melspectrogram.onnx"
emb_path = OWW_DIR / "embedding_model.onnx"

opts = ort.SessionOptions()
opts.inter_op_num_threads = 1
opts.intra_op_num_threads = 1
opts.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL

mel_sess = ort.InferenceSession(str(mel_path), opts)
emb_sess = ort.InferenceSession(str(emb_path), opts)
nexus_sess = ort.InferenceSession(str(nexus_path), opts)
jarvis_sess = ort.InferenceSession(str(jarvis_path), opts)

mel_in = mel_sess.get_inputs()[0].name
emb_in = emb_sess.get_inputs()[0].name
nexus_in = nexus_sess.get_inputs()[0].name
jarvis_in = jarvis_sess.get_inputs()[0].name

print("=" * 76)
print("  OFFICIAL OPEN-SOURCE 'HEY JARVIS' VS PERSONALLY TRAINED 'NEXUS'")
print("=" * 76)

# 1. Model Specs
print(f"1. Model Specifications:")
print(f"   • Official Hey Jarvis Model: {jarvis_path.stat().st_size / 1024:.1f} KB (Input: {jarvis_sess.get_inputs()[0].shape})")
print(f"   • Personal NEXUS Model     : {nexus_path.stat().st_size / 1024:.1f} KB (Input: {nexus_sess.get_inputs()[0].shape})")

comfort_path = OWW_DIR / "comfort_embedding.npy"
if comfort_path.exists():
    comfort_emb = np.load(str(comfort_path)).astype(np.float32)
else:
    comfort_emb = np.zeros(96, dtype=np.float32)

CHUNK = 1280
LOOKBACK = 480
MEL_PER_CHUNK = 8
MEL_CIRC = 10
EMB_FRAMES = 16

def evaluate_file(wav_file):
    sr, data = wavfile.read(str(wav_file))
    if data.ndim > 1:
        data = data.mean(axis=1)
    if sr != 16000:
        total_samples = len(data)
        num_16k = int(total_samples * 16000 / sr)
        indices = np.linspace(0, total_samples - 1, num_16k)
        audio = np.interp(indices, np.arange(total_samples), data).astype(np.float32) / 32768.0
    else:
        audio = data.astype(np.float32) / 32768.0

    lookback = np.zeros(LOOKBACK, dtype=np.float32)
    mel_buf = [np.zeros((MEL_PER_CHUNK, 32), dtype=np.float32)] * MEL_CIRC
    emb_buf = [comfort_emb.copy() for _ in range(EMB_FRAMES)]

    nexus_scores = []
    jarvis_scores = []

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
        
        m = mel_sess.run(None, {mel_in: framed[None, :]})[0]
        m = m.reshape(MEL_PER_CHUNK, 32) / 10.0 + 2.0
        mel_buf = (mel_buf + [m])[-MEL_CIRC:]
        
        window = np.stack(mel_buf).reshape(80, 32)[4:80]
        e = emb_sess.run(None, {emb_in: window[None, :, :, None].astype(np.float32)})[0]
        emb_buf = (emb_buf + [e.reshape(-1)])[-EMB_FRAMES:]
        
        clf_inp = np.stack(emb_buf)[None, :, :]
        n_out = float(nexus_sess.run(None, {nexus_in: clf_inp})[0].reshape(-1)[0])
        j_out = float(jarvis_sess.run(None, {jarvis_in: clf_inp})[0].reshape(-1)[0])
        nexus_scores.append(n_out)
        jarvis_scores.append(j_out)

    return (max(nexus_scores) if nexus_scores else 0.0,
            max(jarvis_scores) if jarvis_scores else 0.0)


# Benchmark 1: Problem phrase
print("\n2. Problem Phrase Benchmark ('Documentation Created and Synchronized'):")
doc_wav = ROOT / "test_doc.wav"
if doc_wav.exists():
    n_score, j_score = evaluate_file(doc_wav)
    print(f"   • Personal NEXUS Model     : Peak {n_score * 100:5.2f}% -> {'PASSED (0 Triggers)' if n_score < 0.50 else 'TRIGGERED'}")
    print(f"   • Official Hey Jarvis Model: Peak {j_score * 100:5.2f}% -> {'PASSED (0 Triggers)' if j_score < 0.50 else 'TRIGGERED'}")

# Benchmark 2: Fast Speech Negatives (186 files)
print("\n3. Fast Multi-Syllabic Speech Negatives (186 files):")
fast_neg_files = sorted(glob.glob(str(DATA_DIR / "negative" / "synth_fast_*.wav")))
if fast_neg_files:
    nexus_peaks = []
    jarvis_peaks = []
    for f in fast_neg_files:
        n_s, j_s = evaluate_file(f)
        nexus_peaks.append(n_s)
        jarvis_peaks.append(j_s)
    
    n_fa = sum(1 for s in nexus_peaks if s >= 0.50)
    j_fa = sum(1 for s in jarvis_peaks if s >= 0.50)
    
    print(f"   • Personal NEXUS Model     : {n_fa}/{len(fast_neg_files)} False Alarms ({n_fa/len(fast_neg_files):.2%}) | Avg Peak: {np.mean(nexus_peaks)*100:.2f}% | Max: {max(nexus_peaks)*100:.2f}%")
    print(f"   • Official Hey Jarvis Model: {j_fa}/{len(fast_neg_files)} False Alarms ({j_fa/len(fast_neg_files):.2%}) | Avg Peak: {np.mean(jarvis_peaks)*100:.2f}% | Max: {max(jarvis_peaks)*100:.2f}%")

# Benchmark 3: Vocal Friction / Throat clearing / Coughs (90 files)
print("\n4. Vocal Friction, Throat Clearing & Gargling (90 files):")
throat_files = sorted(glob.glob(str(DATA_DIR / "negative" / "throat_clear_*.wav")))[:90]
if throat_files:
    nexus_peaks = []
    jarvis_peaks = []
    for f in throat_files:
        n_s, j_s = evaluate_file(f)
        nexus_peaks.append(n_s)
        jarvis_peaks.append(j_s)
    
    n_fa = sum(1 for s in nexus_peaks if s >= 0.50)
    j_fa = sum(1 for s in jarvis_peaks if s >= 0.50)
    
    print(f"   • Personal NEXUS Model     : {n_fa}/{len(throat_files)} False Alarms ({n_fa/len(throat_files):.2%}) | Avg Peak: {np.mean(nexus_peaks)*100:.2f}% | Max: {max(nexus_peaks)*100:.2f}%")
    print(f"   • Official Hey Jarvis Model: {j_fa}/{len(throat_files)} False Alarms ({j_fa/len(throat_files):.2%}) | Avg Peak: {np.mean(jarvis_peaks)*100:.2f}% | Max: {max(jarvis_peaks)*100:.2f}%")

# Benchmark 4: Positive Recall on NEXUS Dataset (178 files)
print("\n5. Positive Target Recall on 178 Clean NEXUS Samples:")
pos_files = sorted(glob.glob(str(DATA_DIR / "positive" / "*.wav")))
if pos_files:
    nexus_pos_peaks = []
    jarvis_pos_peaks = []
    for f in pos_files:
        n_s, j_s = evaluate_file(f)
        nexus_pos_peaks.append(n_s)
        jarvis_pos_peaks.append(j_s)
    
    n_rec = sum(1 for s in nexus_pos_peaks if s >= 0.68)
    j_fa = sum(1 for s in jarvis_pos_peaks if s >= 0.50)
    print(f"   • Personal NEXUS Model     : {n_rec}/{len(pos_files)} Recalled @ 0.68 ({n_rec/len(pos_files):.2%}) | Median: {np.median(nexus_pos_peaks)*100:.2f}% | Mean: {np.mean(nexus_pos_peaks)*100:.2f}%")
    print(f"   • Official Hey Jarvis Model: {j_fa}/{len(pos_files)} Triggers on 'NEXUS' ({j_fa/len(pos_files):.2%}) | Avg Peak: {np.mean(jarvis_pos_peaks)*100:.2f}%")

# Benchmark 5: Latency benchmark (1000 chunks)
print("\n6. Inference Latency (1,000 Chunks on CPU):")
dummy_inp = np.random.randn(1, 16, 96).astype(np.float32)

t0 = time.perf_counter()
for _ in range(1000):
    nexus_sess.run(None, {nexus_in: dummy_inp})
t_nexus = (time.perf_counter() - t0) / 1000.0 * 1000.0

t0 = time.perf_counter()
for _ in range(1000):
    jarvis_sess.run(None, {jarvis_in: dummy_inp})
t_jarvis = (time.perf_counter() - t0) / 1000.0 * 1000.0

print(f"   • Personal NEXUS Model Inference Latency     : {t_nexus:.3f} ms / 80ms chunk")
print(f"   • Official Hey Jarvis Model Inference Latency: {t_jarvis:.3f} ms / 80ms chunk")
print("=" * 76)

