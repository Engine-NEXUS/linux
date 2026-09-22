"""
NEXUS Wake Word Auto-Recorder — Phase E
Non-interactive version with countdown timers.
Just speak 'NEXUS' when you see 'RECORDING NOW'.

Usage:
  python scripts/auto_record_samples.py
  python scripts/auto_record_samples.py --category normal
  python scripts/auto_record_samples.py --count 10
"""
import sounddevice as sd
import numpy as np
import scipy.io.wavfile as wav
import os
import sys
import time

OUT_DIR = "nexus_real_samples"
SAMPLE_RATE = 16000
DURATION = 2.0  # seconds per clip
COUNTDOWN = 3   # seconds countdown before each recording

# Recording categories: (prefix, count, instructions)
ALL_CATEGORIES = [
    ("nexus_normal", 20, "Say 'NEXUS' at NORMAL volume"),
    ("nexus_quiet", 10, "Say 'NEXUS' QUIETLY or WHISPERED"),
    ("nexus_loud", 10, "Say 'NEXUS' LOUDLY"),
    ("nexus_distant", 5, "Say 'NEXUS' from 3 METERS away"),
    ("hey_nexus", 5, "Say 'HEY NEXUS' at normal volume"),
]

def beep(freq=800, duration=0.15):
    """Play a short beep to signal recording start."""
    try:
        sd.play(np.sin(2 * np.pi * freq * np.linspace(0, duration, int(SAMPLE_RATE * duration))), SAMPLE_RATE)
        sd.wait()
    except:
        pass

def record_clip(duration=DURATION, sr=SAMPLE_RATE):
    """Record a single clip with countdown."""
    # Countdown
    for i in range(COUNTDOWN, 0, -1):
        print(f"  {i}...", end="", flush=True)
        beep(600, 0.1)
        time.sleep(0.9)
    print(" RECORDING NOW!", flush=True)
    beep(1000, 0.2)  # Higher beep = start recording
    
    # Record
    audio = sd.rec(int(duration * sr), samplerate=sr, channels=1, dtype=np.int16)
    sd.wait()
    
    # Compute RMS
    rms = np.sqrt(np.mean((audio.astype(np.float32) / 32767) ** 2))
    return audio, rms

def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--category", type=str, default=None,
                        help="Record only this category (normal, quiet, loud, distant, hey_nexus)")
    parser.add_argument("--count", type=int, default=None,
                        help="Override clip count for the category")
    args = parser.parse_args()
    
    os.makedirs(OUT_DIR, exist_ok=True)
    
    # Select categories
    if args.category:
        prefix = f"nexus_{args.category}" if args.category != "hey_nexus" else "hey_nexus"
        cats = [(p, c, i) for p, c, i in ALL_CATEGORIES if p == prefix]
        if not cats:
            print(f"Unknown category: {args.category}")
            print(f"Available: normal, quiet, loud, distant, hey_nexus")
            return
        if args.count:
            p, c, i = cats[0]
            cats = [(p, args.count, i)]
    else:
        cats = ALL_CATEGORIES
    
    total = sum(c for _, c, _ in cats)
    
    print("=" * 60)
    print("NEXUS Wake Word Auto-Recorder")
    print("=" * 60)
    print()
    print(f"Total clips to record: {total}")
    print(f"Duration per clip: {DURATION}s")
    print(f"Countdown: {COUNTDOWN}s before each clip")
    print()
    print("INSTRUCTIONS:")
    print("  - You will hear a low beep countdown (3... 2... 1...)")
    print("  - When you hear the HIGH beep, say 'NEXUS' immediately")
    print("  - Recording stops automatically after 2 seconds")
    print("  - Sit near your laptop microphone")
    print("  - Press Ctrl+C to stop early")
    print()
    input("Press ENTER when you are ready to start...")
    print()
    
    clip_num = 0
    recorded = 0
    skipped = 0
    
    for prefix, count, instructions in cats:
        print(f"\n{'=' * 60}")
        print(f"Category: {prefix} ({count} clips)")
        print(f"Instructions: {instructions}")
        print(f"{'=' * 60}\n")
        
        existing = len([f for f in os.listdir(OUT_DIR) if f.startswith(prefix) and f.endswith(".wav")])
        start = existing + 1
        
        for i in range(start, start + count):
            clip_num += 1
            print(f"  Clip {clip_num}/{total} ({prefix}_{i:03d}.wav)")
            print(f"  {instructions}")
            
            try:
                audio, rms = record_clip()
            except KeyboardInterrupt:
                print("\n\nRecording stopped by user.")
                print(f"Recorded: {recorded}, Skipped: {skipped}")
                return
            
            if rms < 0.001:
                print(f"  SKIP (too quiet, RMS={rms:.4f}) — speak louder next time\n")
                skipped += 1
                continue
            
            fname = f"{OUT_DIR}/{prefix}_{i:03d}.wav"
            wav.write(fname, SAMPLE_RATE, audio)
            print(f"  SAVED (RMS={rms:.4f})\n")
            recorded += 1
            
            time.sleep(0.5)  # Brief pause between clips
    
    print(f"\n{'=' * 60}")
    print(f"Recording complete!")
    print(f"{'=' * 60}")
    print(f"Recorded: {recorded}")
    print(f"Skipped (too quiet): {skipped}")
    print(f"Total files in {OUT_DIR}/: {len([f for f in os.listdir(OUT_DIR) if f.endswith('.wav')])}")
    print()
    print("Next steps:")
    print(f"  1. Zip: zip -r nexus_real_samples.zip {OUT_DIR}/")
    print(f"  2. Upload to Kaggle as dataset 'nexus-real-samples'")
    print(f"  3. Attach to the training kernel")

if __name__ == "__main__":
    main()
