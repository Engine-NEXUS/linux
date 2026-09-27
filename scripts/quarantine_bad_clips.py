"""
Quarantine poisoned near-silent positive clips from the wake word dataset.
These clips have RMS < 0.005 — they are essentially silence with a tiny
amount of noise, which teach the model that silence = NEXUS (poisoning).
"""
import shutil
from pathlib import Path
import sys
sys.stdout.reconfigure(encoding="utf-8", errors="replace")
import numpy as np
from scipy.io import wavfile

pos_dir = Path("wake_word_data/positive")
quarantine = Path("wake_word_data/quarantined_silent")
quarantine.mkdir(exist_ok=True)

# Find all clips with RMS below threshold — these are near-silence
SILENCE_RMS_THRESHOLD = 0.006  # Anything quieter than this is suspect noise
clips = sorted(pos_dir.glob("*.wav"))
moved = 0

print(f"Scanning {len(clips)} positive clips for poisoned/silent samples...")
for p in clips:
    sr, data = wavfile.read(str(p))
    if data.ndim > 1:
        data = data.mean(axis=1)
    audio = data.astype(np.float32) / 32768.0
    rms = float(np.sqrt(np.mean(audio ** 2)))
    if rms < SILENCE_RMS_THRESHOLD:
        shutil.move(str(p), str(quarantine / p.name))
        print(f"  Quarantined: {p.name} (RMS={rms:.4f})")
        moved += 1

remaining = len(list(pos_dir.glob("*.wav")))
print(f"\nMoved {moved} poisoned clips to wake_word_data/quarantined_silent/ (safe, not deleted)")
print(f"Remaining clean positive clips: {remaining}")
