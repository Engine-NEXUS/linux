"""
NEXUS Wake Word Sample Recorder — Phase E
Records real "nexus" utterances for training data augmentation.

Records 50 clips in 5 categories to capture real-world variation:
  1. Normal volume (20 clips) — say "NEXUS" at normal volume
  2. Quiet/whisper (10 clips) — say "NEXUS" quietly/whispered
  3. Loud (10 clips) — say "NEXUS" loudly
  4. Distant 3m (5 clips) — say "NEXUS" from 3 meters away
  5. Hey NEXUS (5 clips) — say "HEY NEXUS" at normal volume

Usage:
  python scripts/record_samples.py

Output:
  nexus_real_samples/nexus_normal_001.wav ... (20 clips)
  nexus_real_samples/nexus_quiet_001.wav ... (10 clips)
  nexus_real_samples/nexus_loud_001.wav ... (10 clips)
  nexus_real_samples/nexus_distant_001.wav ... (5 clips)
  nexus_real_samples/hey_nexus_001.wav ... (5 clips)

Then zip and upload to Kaggle as a dataset:
  zip -r nexus_real_samples.zip nexus_real_samples/

These 50 real clips are mixed into the 100,000 TTS samples during training.
The real clips provide microphone matching and voice characteristics that
TTS cannot capture.
"""
import sounddevice as sd
import numpy as np
import scipy.io.wavfile as wav
import os
import time

OUT_DIR = "nexus_real_samples"
SAMPLE_RATE = 16000
DURATION = 2.0  # seconds per clip

# Recording categories: (prefix, count, instructions)
CATEGORIES = [
    ("nexus_normal", 20, "Say 'NEXUS' at NORMAL volume (like talking to someone nearby)"),
    ("nexus_quiet", 10, "Say 'NEXUS' QUIETLY or WHISPERED (like in a library)"),
    ("nexus_loud", 10, "Say 'NEXUS' LOUDLY (like calling across a room)"),
    ("nexus_distant", 5, "Say 'NEXUS' from 3 METERS away (step back from the mic)"),
    ("hey_nexus", 5, "Say 'HEY NEXUS' at normal volume"),
]

os.makedirs(OUT_DIR, exist_ok=True)

print("=" * 60)
print("NEXUS Wake Word Sample Recorder — Phase E")
print("=" * 60)
print()
print("This records 50 real 'nexus' clips for wake word training.")
print("The clips capture your voice, microphone, and room acoustics")
print("that TTS-generated samples cannot reproduce.")
print()
print("Tips for best results:")
print("  - Use your laptop's built-in microphone")
print("  - Record in a quiet room (no music, no TV)")
print("  - Speak naturally — don't over-enunciate")
print("  - If a clip is too quiet, it will be skipped automatically")
print("  - Press Ctrl+C to stop early (partial sets are still useful)")
print()

total_clips = sum(count for _, count, _ in CATEGORIES)
print(f"Total clips to record: {total_clips}")
print(f"Duration per clip: {DURATION}s")
print()

clip_num = 0
for prefix, count, instructions in CATEGORIES:
    print(f"\n{'─' * 60}")
    print(f"Category: {prefix} ({count} clips)")
    print(f"Instructions: {instructions}")
    print(f"{'─' * 60}")

    existing = len([f for f in os.listdir(OUT_DIR) if f.startswith(prefix) and f.endswith(".wav")])
    start = existing + 1

    for i in range(start, start + count):
        clip_num += 1
        fname = f"{OUT_DIR}/{prefix}_{i:03d}.wav"
        input(f"  Clip {clip_num}/{total_clips} — press ENTER when ready...")
        print("  Recording... ", end="", flush=True)
        audio = sd.rec(int(DURATION * SAMPLE_RATE), samplerate=SAMPLE_RATE, channels=1, dtype=np.int16)
        sd.wait()
        rms = np.sqrt(np.mean((audio.astype(np.float32) / 32767) ** 2))
        if rms < 0.001:
            print("SKIP (too quiet — speak louder next time)")
            continue
        wav.write(fname, SAMPLE_RATE, audio)
        print(f"saved (RMS={rms:.4f})")

print(f"\n{'=' * 60}")
print(f"Recording complete!")
print(f"{'=' * 60}")
total_recorded = len([f for f in os.listdir(OUT_DIR) if f.endswith(".wav")])
print(f"Total clips in {OUT_DIR}/: {total_recorded}")
print()
print("Next steps:")
print(f"  1. Zip the samples:")
print(f"     zip -r nexus_real_samples.zip {OUT_DIR}/")
print(f"  2. Upload to Kaggle as a dataset:")
print(f"     - Go to https://www.kaggle.com/datasets")
print(f"     - Click 'New Dataset'")
print(f"     - Upload nexus_real_samples.zip")
print(f"     - Name it 'nexus-real-samples'")
print(f"  3. In the training notebook, attach this dataset")
print(f"  4. The notebook will automatically detect and include the real samples")
print()
print("The real samples will be mixed with 100,000 TTS samples during training,")
print("providing microphone matching and voice characteristics that TTS cannot.")
