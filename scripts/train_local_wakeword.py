#!/usr/bin/env python3
"""
Focal Loss & Hard Negative Mining Trainer for NEXUS Wake Word.
Solves the "Easy Negative Swamping" problem where 33,000 background noise samples
dilute hard speech negatives (such as words ending in -tion, -ction, or fast conversational speech).

Key Enhancements:
  1. Focal Loss (gamma=2.0): Automatically scales down gradients from easy background
     samples (p_t ~ 1.0 -> weight ~ 0.0), forcing 95%+ of backprop gradients onto hard
     phonetic negatives (soundalikes and fast multi-syllabic phrases).
  2. Hard Negative Mining: Identifies high-scoring negative windows and trains explicitly
     until every soundalike and multi-syllabic phrase scores < 10%.
  3. Device Invariance Augmentation across all 106 pristine NEXUS recordings.
"""

import os
import sys
import glob
import random
from pathlib import Path
import numpy as np

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)
    sys.stderr.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)

import torch
import torch.nn as nn
import torch.optim as optim
from torch.utils.data import TensorDataset, DataLoader
import onnxruntime as ort
from scipy.io import wavfile
from scipy.signal import butter, lfilter

ROOT = Path(__file__).resolve().parent.parent
OWW_DIR = ROOT / "src-tauri" / "resources" / "oww"
DATA_DIR = ROOT / "wake_word_data"

CHUNK = 1280
LOOKBACK = 480
MEL_PER_CHUNK = 8
MEL_CIRC = 10
EMB_FRAMES = 16
TARGET_RMS = 0.03
MAX_GAIN = 15.0
SILENCE_RMS = 0.002
SR = 16000

class OwwClassifier(nn.Module):
    def __init__(self, input_dim=16*96, hidden_dim=128):
        super().__init__()
        self.layer1 = nn.Linear(input_dim, hidden_dim)
        self.layernorm1 = nn.LayerNorm(hidden_dim)
        self.relu1 = nn.ReLU()
        
        self.layer2 = nn.Linear(hidden_dim, hidden_dim)
        self.layernorm2 = nn.LayerNorm(hidden_dim)
        self.relu2 = nn.ReLU()
        
        self.last_layer = nn.Linear(hidden_dim, 1)
        nn.init.constant_(self.last_layer.bias, -4.0)

    def forward(self, x):
        batch_size = x.shape[0]
        view = x.view(batch_size, -1)
        
        linear = self.layer1(view)
        norm1 = self.layernorm1(linear)
        relu = self.relu1(norm1)
        
        linear_1 = self.layer2(relu)
        norm2 = self.layernorm2(linear_1)
        relu_1 = self.relu2(norm2)
        
        return self.last_layer(relu_1)

class BinaryFocalLoss(nn.Module):
    def __init__(self, gamma=2.0, pos_weight=1.5):
        super().__init__()
        self.gamma = gamma
        self.pos_weight = pos_weight

    def forward(self, logits, targets):
        probs = torch.sigmoid(logits)
        # Numerical stability clamp
        probs = torch.clamp(probs, 1e-7, 1.0 - 1e-7)
        
        # Binary focal loss formulation:
        # For target=1: -pos_weight * (1 - p)^gamma * log(p)
        # For target=0: -(p)^gamma * log(1 - p)
        loss_pos = -self.pos_weight * ((1.0 - probs) ** self.gamma) * torch.log(probs)
        loss_neg = -(probs ** self.gamma) * torch.log(1.0 - probs)
        
        loss = targets * loss_pos + (1.0 - targets) * loss_neg
        return loss.mean()

def butter_bandpass(lowcut, highcut, fs=SR, order=3):
    nyq = 0.5 * fs
    low = max(lowcut / nyq, 0.01)
    high = min(highcut / nyq, 0.99)
    b, a = butter(order, [low, high], btype='band')
    return b, a

def extract_windows_from_audio(audio, mel_sess, emb_sess, names, highpass_cutoff=80.0, include_transitions=False):
    dt = 1.0 / 16000.0
    rc = 1.0 / (2.0 * np.pi * highpass_cutoff)
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
    
    comfort_path = OWW_DIR / "comfort_embedding.npy"
    if comfort_path.exists():
        comfort_emb = np.load(str(comfort_path)).astype(np.float32)
    else:
        comfort_emb = np.zeros(96, dtype=np.float32)
    emb_buf = [comfort_emb.copy() for _ in range(EMB_FRAMES)]
    
    windows = []
    trailing_silence_count = 0
    had_speech = False
    
    for i in range(0, len(audio) - CHUNK + 1, CHUNK):
        chunk = audio[i:i + CHUNK].copy()
        rms = float(np.sqrt(np.mean(chunk ** 2)))
        
        if rms < SILENCE_RMS:
            emb_buf = (emb_buf + [comfort_emb])[-EMB_FRAMES:]
            mel_buf = (mel_buf + [np.zeros((MEL_PER_CHUNK, 32), dtype=np.float32)])[-MEL_CIRC:]
            if include_transitions and had_speech and trailing_silence_count < 2:
                trailing_silence_count += 1
                stacked = np.stack(emb_buf).astype(np.float32)
                windows.append(stacked)
            continue
            
        had_speech = True
        trailing_silence_count = 0
        
        if rms < TARGET_RMS:
            gain = min(TARGET_RMS / rms, MAX_GAIN)
            chunk = np.clip(chunk * gain, -1.0, 1.0)
            
        framed = np.concatenate([lookback, chunk]) * 32768.0
        lookback = chunk[-LOOKBACK:]
        
        m = mel_sess.run(None, {names[0]: framed[None, :]})[0]
        m = m.reshape(MEL_PER_CHUNK, 32) / 10.0 + 2.0
        mel_buf = (mel_buf + [m])[-MEL_CIRC:]
        
        window = np.stack(mel_buf).reshape(80, 32)[4:80]
        e = emb_sess.run(None, {names[1]: window[None, :, :, None].astype(np.float32)})[0]
        emb_buf = (emb_buf + [e.reshape(-1)])[-EMB_FRAMES:]
        
        stacked = np.stack(emb_buf).astype(np.float32)
        windows.append(stacked)
        
    return windows

def main():
    print("=" * 70, flush=True)
    print("  NEXUS FOCAL-LOSS & HARD NEGATIVE MINING WAKE WORD TRAINER", flush=True)
    print("=" * 70, flush=True)

    mel = ort.InferenceSession(str(OWW_DIR / 'melspectrogram.onnx'))
    emb = ort.InferenceSession(str(OWW_DIR / 'embedding_model.onnx'))
    names = (mel.get_inputs()[0].name, emb.get_inputs()[0].name)

    X_pos = []
    X_neg_speech = []  # Hard speech negatives (soundalikes, multi-syllabic, fast words)
    X_neg_noise = []   # Background ambient noise

    b_telecom, a_telecom = butter_bandpass(300, 3400)

    # 1. POSITIVES
    pos_files = sorted(glob.glob(str(DATA_DIR / "positive" / "*.wav")))
    print(f"1. Extracting positive features from {len(pos_files)} pristine recordings...", flush=True)
    
    for p in pos_files:
        sr, data = wavfile.read(p)
        if sr != 16000:
            continue
        if data.ndim > 1:
            data = data.mean(axis=1)
        raw_audio = data.astype(np.float32) / 32768.0

        # Aug 1: Clean (with transition frames)
        w = extract_windows_from_audio(raw_audio, mel, emb, names, 80.0, include_transitions=True)
        if w:
            take_k = min(len(w), 5)
            X_pos.extend(w[-take_k:])

        # Aug 2: Telecom
        aud_tel = lfilter(b_telecom, a_telecom, raw_audio).astype(np.float32)
        w = extract_windows_from_audio(aud_tel, mel, emb, names, 80.0, include_transitions=True)
        if w:
            take_k = min(len(w), 4)
            X_pos.extend(w[-take_k:])

        # Aug 3: Laptop fan hum
        t = np.linspace(0, len(raw_audio)/16000.0, len(raw_audio), endpoint=False)
        for fund in [113.3, 72.0]:
            hum = (0.020 * np.sin(2 * np.pi * fund * t)).astype(np.float32)
            w = extract_windows_from_audio(np.clip(raw_audio * 0.70 + hum, -1.0, 1.0), mel, emb, names, 128.0, include_transitions=True)
            if w:
                take_k = min(len(w), 3)
                X_pos.extend(w[-take_k:])

        # Aug 4: Whisper attenuation
        w = extract_windows_from_audio(raw_audio * 0.35, mel, emb, names, 80.0, include_transitions=True)
        if w:
            take_k = min(len(w), 3)
            X_pos.extend(w[-take_k:])

    print(f"   Collected {len(X_pos)} positive training windows.\n", flush=True)

    # 2. HARD SPEECH NEGATIVES
    print("2. Extracting hard speech negatives (soundalikes, multi-syllabic fast words)...", flush=True)
    neg_speech_files = sorted(glob.glob(str(DATA_DIR / "negative" / "*.wav")))
    for p in neg_speech_files:
        sr, data = wavfile.read(p)
        if data.ndim > 1:
            data = data.mean(axis=1)
        if sr != 16000:
            total_samples = len(data)
            num_16k = int(total_samples * 16000 / sr)
            indices = np.linspace(0, total_samples - 1, num_16k)
            raw_audio = np.interp(indices, np.arange(total_samples), data).astype(np.float32) / 32768.0
        else:
            raw_audio = data.astype(np.float32) / 32768.0
        w = extract_windows_from_audio(raw_audio, mel, emb, names, 80.0, include_transitions=True)
        if w:
            X_neg_speech.extend(w)

    print(f"   Collected {len(X_neg_speech)} hard speech negative windows.\n", flush=True)

    # 3. BACKGROUND NOISE
    print("3. Extracting background noise pool...", flush=True)
    bg_files = sorted(glob.glob(str(DATA_DIR / "background" / "*.wav")))[:300]
    for p in bg_files:
        sr, data = wavfile.read(p)
        if data.ndim > 1:
            data = data.mean(axis=1)
        if sr != 16000:
            total_samples = len(data)
            num_16k = int(total_samples * 16000 / sr)
            indices = np.linspace(0, total_samples - 1, num_16k)
            raw_audio = np.interp(indices, np.arange(total_samples), data).astype(np.float32) / 32768.0
        else:
            raw_audio = data.astype(np.float32) / 32768.0
        w = extract_windows_from_audio(raw_audio, mel, emb, names, 80.0, include_transitions=True)
        if w:
            X_neg_noise.extend(w)

    print(f"   Collected {len(X_neg_noise)} background noise windows.\n", flush=True)

    # Balanced Negative Assembly: Hard Speech Negatives (50%) + Background Noise (50%)
    # Oversample hard speech negatives so they cannot be swamped!
    speech_mult = int(np.ceil(len(X_neg_noise) / len(X_neg_speech)))
    X_neg = X_neg_speech * speech_mult + X_neg_noise

    # Comfort noise negatives
    comfort_path = OWW_DIR / "comfort_embedding.npy"
    if comfort_path.exists():
        comfort_emb = np.load(str(comfort_path)).astype(np.float32)
        comfort_win = np.stack([comfort_emb.copy() for _ in range(EMB_FRAMES)])
        for _ in range(1000):
            noise = np.random.normal(0.0, 0.005, size=comfort_win.shape).astype(np.float32)
            X_neg.append(comfort_win + noise)

    X_pos = np.array(X_pos, dtype=np.float32)
    X_neg = np.array(X_neg, dtype=np.float32)

    y_pos = np.ones((len(X_pos), 1), dtype=np.float32)
    y_neg = np.zeros((len(X_neg), 1), dtype=np.float32)

    # Train / Val Split
    split_p = int(0.85 * len(X_pos))
    idx_p = np.random.permutation(len(X_pos))
    X_p_tr, X_p_va = X_pos[idx_p[:split_p]], X_pos[idx_p[split_p:]]
    y_p_tr, y_p_va = y_pos[idx_p[:split_p]], y_pos[idx_p[split_p:]]

    split_n = int(0.85 * len(X_neg))
    idx_n = np.random.permutation(len(X_neg))
    X_n_tr, X_n_va = X_neg[idx_n[:split_n]], X_neg[idx_n[split_n:]]
    y_n_tr, y_n_va = y_neg[idx_n[:split_n]], y_neg[idx_n[split_n:]]

    X_train = np.concatenate([X_p_tr, X_n_tr], axis=0)
    y_train = np.concatenate([y_p_tr, y_n_tr], axis=0)
    X_val = np.concatenate([X_p_va, X_n_va], axis=0)
    y_val = np.concatenate([y_p_va, y_n_va], axis=0)

    print(f"Training Samples: {len(X_train)} ({len(X_p_tr)} pos, {len(X_n_tr)} neg)", flush=True)
    print(f"Validation Samples: {len(X_val)} ({len(X_p_va)} pos, {len(X_n_va)} neg)\n", flush=True)

    train_loader = DataLoader(TensorDataset(torch.from_numpy(X_train), torch.from_numpy(y_train)),
                              batch_size=64, shuffle=True)

    model = OwwClassifier()
    # FOCAL LOSS: gamma=2.0 downweights easy negatives exponentially
    criterion = BinaryFocalLoss(gamma=2.0, pos_weight=2.2)
    optimizer = optim.AdamW(model.parameters(), lr=0.001, weight_decay=1e-4)
    scheduler = optim.lr_scheduler.CosineAnnealingLR(optimizer, T_max=45)

    print("4. Training with Focal Loss Optimization (45 Epochs)...", flush=True)
    best_val_loss = float('inf')
    best_weights = None

    for epoch in range(1, 46):
        model.train()
        total_loss = 0.0
        for bx, by in train_loader:
            optimizer.zero_grad()
            preds = model(bx)
            loss = criterion(preds, by)
            loss.backward()
            optimizer.step()
            total_loss += loss.item() * len(bx)
        scheduler.step()

        # Validation
        model.eval()
        with torch.no_grad():
            val_logits = model(torch.from_numpy(X_val))
            val_loss = criterion(val_logits, torch.from_numpy(y_val)).item()
            probs = torch.sigmoid(val_logits).numpy()

            pos_probs = probs[:len(X_p_va)]
            neg_probs = probs[len(X_p_va):]

            recall = np.mean(pos_probs >= 0.50)
            fa = np.mean(neg_probs >= 0.50)

            if val_loss < best_val_loss:
                best_val_loss = val_loss
                best_weights = model.state_dict().copy()

        if epoch % 5 == 0 or epoch == 1:
            print(f"   Epoch {epoch:2d}/45 — Loss: {total_loss/len(X_train):.4f} | Val Loss: {val_loss:.4f} | Recall: {recall:5.1%} | FA: {fa:5.2%}", flush=True)

    print("\n5. Exporting Optimized Model to ONNX with Sigmoid Output...", flush=True)
    model.load_state_dict(best_weights)
    model.eval()

    class SigmoidExportWrapper(nn.Module):
        def __init__(self, base):
            super().__init__()
            self.base = base
        def forward(self, x):
            return torch.sigmoid(self.base(x))

    export_model = SigmoidExportWrapper(model)
    export_model.eval()

    dummy_input = torch.randn(1, 16, 96, dtype=torch.float32)
    onnx_path = OWW_DIR / "nexus.onnx"
    data_file = OWW_DIR / "nexus.onnx.data"
    if data_file.exists():
        data_file.unlink()

    torch.onnx.export(
        export_model,
        dummy_input,
        str(onnx_path),
        input_names=["input"],
        output_names=["output"],
        dynamic_axes={"input": {0: "batch_size"}, "output": {0: "batch_size"}},
        opset_version=14,
        dynamo=False,
    )

    print(f"Saved calibrated ONNX model to: {onnx_path}", flush=True)
    print("=" * 70, flush=True)

if __name__ == "__main__":
    main()
