#!/usr/bin/env python3
"""
NEXUS Local Wake Word Trainer & Optimizer
Extracts ONNX embeddings from real laptop recordings with multi-device acoustic augmentations:
  1. Built-in Laptop Array (80-160Hz HPF + 113.3Hz fan resonance)
  2. Studio USB Condenser (Full-spectrum flat response)
  3. Narrowband Bluetooth Headset / Telecom (300Hz-3400Hz bandpass)
  4. Whisper / Distance Attenuation (0.35x RMS with AGC stress)
  5. Close Proximity / Saturation (1.8x RMS with soft tanh)
  6. Additive Background Noise (ESC-50 / MS-SNSD / MUSAN profile mix)

Trains a robust 128-dim KWS classifier with BCEWithLogitsLoss(pos_weight=8.0) and exports ONNX.
"""

import os
import sys
import glob
import random
from pathlib import Path
import numpy as np

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

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
        self.sigmoid = nn.Sigmoid()

    def forward(self, x):
        # x shape: [batch, 16, 96]
        batch_size = x.shape[0]
        view = x.view(batch_size, -1)
        
        linear = self.layer1(view)
        norm1 = self.layernorm1(linear)
        relu = self.relu1(norm1)
        
        linear_1 = self.layer2(relu)
        norm2 = self.layernorm2(linear_1)
        relu_1 = self.relu2(norm2)
        
        linear_2 = self.last_layer(relu_1)
        return self.sigmoid(linear_2)


def butter_bandpass(lowcut, highcut, fs=SR, order=3):
    nyq = 0.5 * fs
    low = max(lowcut / nyq, 0.01)
    high = min(highcut / nyq, 0.99)
    b, a = butter(order, [low, high], btype='band')
    return b, a


def extract_windows_from_audio(audio, mel_sess, emb_sess, names, highpass_cutoff=80.0):
    """Filter and extract all valid 16-frame embedding windows from an audio array."""
    # Highpass filter (configurable for acoustic profile)
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
    
    for i in range(0, len(audio) - CHUNK + 1, CHUNK):
        chunk = audio[i:i + CHUNK].copy()
        rms = float(np.sqrt(np.mean(chunk ** 2)))
        
        if rms < SILENCE_RMS:
            # Advance embedding buffer with comfort embedding so time progresses continuously
            emb_buf = (emb_buf + [comfort_emb])[-EMB_FRAMES:]
            continue
            
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
        
        # Collect full 16-frame embedding context
        stacked = np.stack(emb_buf).astype(np.float32)
        windows.append(stacked)
        
    return windows


def main():
    print("=" * 65)
    print("  NEXUS MULTI-DEVICE ACOUSTIC WAKE WORD TRAINING")
    print("=" * 65)
    
    mel = ort.InferenceSession(str(OWW_DIR / 'melspectrogram.onnx'))
    emb = ort.InferenceSession(str(OWW_DIR / 'embedding_model.onnx'))
    names = (mel.get_inputs()[0].name, emb.get_inputs()[0].name)
    
    X_pos = []
    X_neg = []
    
    # 1. Load background noise pool for acoustic mixing
    bg_files = sorted(glob.glob(str(DATA_DIR / "background" / "*.wav")))
    bg_cache = []
    for p in bg_files[:200]:
        sr, d = wavfile.read(p)
        if sr == 16000:
            if d.ndim > 1:
                d = d.mean(axis=1)
            bg_cache.append(d.astype(np.float32) / 32768.0)

    # Telecom / Bluetooth filter
    b_telecom, a_telecom = butter_bandpass(300, 3400)

    print("\n1. Extracting multi-device augmented features from POSITIVE recordings...")
    pos_files = sorted(glob.glob(str(DATA_DIR / "positive" / "*.wav")))
    
    for p in pos_files:
        sr, data = wavfile.read(p)
        if sr != 16000:
            continue
        if data.ndim > 1:
            data = data.mean(axis=1)
        raw_audio = data.astype(np.float32) / 32768.0

        # Augmentation 1: Native Studio / Laptop Clean
        wins_clean = extract_windows_from_audio(raw_audio, mel, emb, names, highpass_cutoff=80.0)
        if wins_clean:
            X_pos.extend(wins_clean[-3:])

        # Augmentation 2: Narrowband Bluetooth / Telecom bandpass (300-3400Hz)
        audio_telecom = lfilter(b_telecom, a_telecom, raw_audio).astype(np.float32)
        wins_telecom = extract_windows_from_audio(audio_telecom, mel, emb, names, highpass_cutoff=80.0)
        if wins_telecom:
            X_pos.extend(wins_telecom[-2:])

        # Augmentation 3a: Laptop Fan Rumble + Unfiltered 80Hz HPF (Simulates uncalibrated default)
        t = np.linspace(0, len(raw_audio) / 16000.0, len(raw_audio), endpoint=False)
        for fund_hz in [113.3, 72.0, 145.0]:
            fan_hum = (0.020 * np.sin(2 * np.pi * fund_hz * t) + 0.010 * np.sin(2 * np.pi * fund_hz * 2 * t)).astype(np.float32)
            audio_laptop_raw = np.clip(raw_audio * 0.65 + fan_hum, -1.0, 1.0)
            wins_laptop_raw = extract_windows_from_audio(audio_laptop_raw, mel, emb, names, highpass_cutoff=80.0)
            if wins_laptop_raw:
                X_pos.extend(wins_laptop_raw[-2:])

        # Augmentation 3b: Laptop Fan Rumble + Calibrated 128Hz HPF
        fan_hum_cal = (0.020 * np.sin(2 * np.pi * 113.3 * t)).astype(np.float32)
        audio_laptop_cal = np.clip(raw_audio * 0.70 + fan_hum_cal, -1.0, 1.0)
        wins_laptop_cal = extract_windows_from_audio(audio_laptop_cal, mel, emb, names, highpass_cutoff=128.0)
        if wins_laptop_cal:
            X_pos.extend(wins_laptop_cal[-2:])

        # Augmentation 4: Quiet Whisper / Distance Attenuation (0.25x - 0.40x gain)
        for q_gain in [0.25, 0.40]:
            audio_quiet = (raw_audio * q_gain).astype(np.float32)
            wins_quiet = extract_windows_from_audio(audio_quiet, mel, emb, names, highpass_cutoff=80.0)
            if wins_quiet:
                X_pos.extend(wins_quiet[-2:])

        # Augmentation 5: Additive Ambient Noise (random slice of background pool at 15-25dB SNR)
        if bg_cache:
            for _ in range(2):
                bg_slice = random.choice(bg_cache)
                if len(bg_slice) < len(raw_audio):
                    bg_slice = np.tile(bg_slice, int(np.ceil(len(raw_audio) / len(bg_slice))))
                bg_sub = bg_slice[:len(raw_audio)] * random.uniform(0.10, 0.25)
                audio_noisy = np.clip(raw_audio + bg_sub, -1.0, 1.0)
                wins_noisy = extract_windows_from_audio(audio_noisy, mel, emb, names, highpass_cutoff=80.0)
                if wins_noisy:
                    X_pos.extend(wins_noisy[-2:])

    print(f"   Collected {len(X_pos)} multi-device positive training windows from {len(pos_files)} base clips.")

    print("\n2. Extracting features from NEGATIVE soundalike recordings...")
    neg_files = sorted(glob.glob(str(DATA_DIR / "negative" / "*.wav")))
    for p in neg_files:
        sr, data = wavfile.read(p)
        if sr != 16000:
            continue
        if data.ndim > 1:
            data = data.mean(axis=1)
        raw_audio = data.astype(np.float32) / 32768.0
        wins = extract_windows_from_audio(raw_audio, mel, emb, names, highpass_cutoff=80.0)
        if wins:
            X_neg.extend(wins)
    print(f"   Collected {len(X_neg)} negative soundalike windows from {len(neg_files)} clips.")

    print("\n3. Extracting features from BACKGROUND noise recordings...")
    for p in bg_files:
        sr, data = wavfile.read(p)
        if sr != 16000:
            continue
        if data.ndim > 1:
            data = data.mean(axis=1)
        raw_audio = data.astype(np.float32) / 32768.0
        wins = extract_windows_from_audio(raw_audio, mel, emb, names, highpass_cutoff=80.0)
        if wins:
            X_neg.extend(wins)
    print(f"   Total negative windows (Soundalikes + Noise): {len(X_neg)} from {len(neg_files) + len(bg_files)} clips.")

    print("\n3b. Adding comfort-noise and sparse-transient negative windows to X_neg...")
    comfort_path = OWW_DIR / "comfort_embedding.npy"
    if comfort_path.exists():
        comfort_emb = np.load(str(comfort_path)).astype(np.float32)
        # 300 pure comfort noise windows
        for _ in range(300):
            noise_jitter = np.random.normal(0, 0.05, (EMB_FRAMES, 96)).astype(np.float32)
            pure_comfort = np.tile(comfort_emb, (EMB_FRAMES, 1)) + noise_jitter
            X_neg.append(pure_comfort)
        # 300 sparse transient bursts (15 comfort frames + 1 transient noise frame)
        for _ in range(300):
            sparse_win = np.tile(comfort_emb, (EMB_FRAMES, 1)).astype(np.float32)
            burst_idx = random.randint(0, EMB_FRAMES - 1)
            sparse_win[burst_idx] = np.random.normal(0, 1.5, 96).astype(np.float32)
            X_neg.append(sparse_win)
        print(f"   Added 600 synthetic comfort/transient negative windows to eliminate post-reset spikes.")

    X_pos = np.array(X_pos, dtype=np.float32)
    y_pos = np.ones((len(X_pos), 1), dtype=np.float32)
    
    X_neg = np.array(X_neg, dtype=np.float32)
    y_neg = np.zeros((len(X_neg), 1), dtype=np.float32)
    
    # Shuffle and split into Train & Validation (80/20)
    np.random.seed(42)
    torch.manual_seed(42)
    
    idx_pos = np.random.permutation(len(X_pos))
    split_pos = int(0.8 * len(X_pos))
    X_pos_train, X_pos_val = X_pos[idx_pos[:split_pos]], X_pos[idx_pos[split_pos:]]
    y_pos_train, y_pos_val = y_pos[idx_pos[:split_pos]], y_pos[idx_pos[split_pos:]]
    
    idx_neg = np.random.permutation(len(X_neg))
    split_neg = int(0.8 * len(X_neg))
    X_neg_train, X_neg_val = X_neg[idx_neg[:split_neg]], X_neg[idx_neg[split_neg:]]
    y_neg_train, y_neg_val = y_neg[idx_neg[:split_neg]], y_neg[idx_neg[split_neg:]]
    
    X_train = np.concatenate([X_pos_train, X_neg_train], axis=0)
    y_train = np.concatenate([y_pos_train, y_neg_train], axis=0)
    
    X_val = np.concatenate([X_pos_val, X_neg_val], axis=0)
    y_val = np.concatenate([y_pos_val, y_neg_val], axis=0)
    
    train_dataset = TensorDataset(torch.from_numpy(X_train), torch.from_numpy(y_train))
    train_loader = DataLoader(train_dataset, batch_size=64, shuffle=True)
    
    print(f"\nTraining Dataset: {len(X_train)} samples ({len(X_pos_train)} pos, {len(X_neg_train)} neg)")
    print(f"Validation Dataset: {len(X_val)} samples ({len(X_pos_val)} pos, {len(X_neg_val)} neg)")
    
    # Initialize Model & Balanced Loss (pos_weight=1.2 balances false alarms vs misses)
    # Previously pos_weight=8.0 biased the network heavily towards triggering on ambiguous noise/speech
    model = OwwClassifier()
    pos_weight = torch.tensor([1.2])
    criterion = nn.BCEWithLogitsLoss(pos_weight=pos_weight)
    optimizer = optim.AdamW(model.parameters(), lr=0.001, weight_decay=1e-4)
    scheduler = optim.lr_scheduler.CosineAnnealingLR(optimizer, T_max=60)
    
    # Wrap model to add sigmoid for ONNX export
    class SigmoidWrapper(nn.Module):
        def __init__(self, base):
            super().__init__()
            self.base = base
        def forward(self, x):
            return torch.sigmoid(self.base(x))
    
    # Patch last layer to output logits during training
    model.sigmoid = nn.Identity()

    print("\n4. Training Neural Classifier with Device-Invariance Optimization...")
    best_val_loss = float('inf')
    best_weights = None
    THRESHOLD = 0.50
    
    for epoch in range(1, 61):
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
            val_preds = model(torch.from_numpy(X_val)).numpy()
            val_loss = criterion(torch.from_numpy(val_preds), torch.from_numpy(y_val)).item()
            
            all_sigmoid = torch.sigmoid(torch.from_numpy(val_preds)).numpy()
            pos_sigmoid = all_sigmoid[:len(X_pos_val)]
            neg_sigmoid = all_sigmoid[len(X_pos_val):]
            
            recall = np.mean(pos_sigmoid >= THRESHOLD)
            fa_rate = np.mean(neg_sigmoid >= THRESHOLD)
            
            if val_loss < best_val_loss:
                best_val_loss = val_loss
                best_weights = model.state_dict().copy()
                
            if epoch % 5 == 0 or epoch == 1:
                print(f"   Epoch {epoch:2d}/60 — Loss: {total_loss/len(X_train):.4f} | Val Loss: {val_loss:.4f} | Recall: {recall:6.1%} | False Alarm: {fa_rate:6.1%}")

    print("\n5. Restoring best model & exporting to ONNX...")
    model.load_state_dict(best_weights)
    model.eval()
    
    export_model = SigmoidWrapper(model)
    export_model.eval()
    
    dummy_input = torch.randn(1, 16, 96, dtype=torch.float32)
    output_onnx = str(ROOT / "src-tauri" / "resources" / "oww" / "nexus.onnx")
    
    backup_onnx = str(ROOT / "src-tauri" / "resources" / "oww" / "nexus.onnx.bak")
    if os.path.exists(output_onnx):
        import shutil
        shutil.copy2(output_onnx, backup_onnx)
        
    torch.onnx.export(
        export_model,
        dummy_input,
        output_onnx,
        input_names=["x"],
        output_names=["sigmoid"],
        dynamic_axes={"x": {0: "batch_size"}, "sigmoid": {0: "batch_size"}},
        opset_version=14,
        dynamo=False,
    )
    
    print(f"   Saved optimized ONNX model to: {output_onnx}")
    print("=" * 65)
    print("  TRAINING COMPLETE! READY FOR VERIFICATION")
    print("=" * 65)


if __name__ == "__main__":
    main()
