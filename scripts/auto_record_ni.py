"""
NEXUS Wake Word Auto-Recorder — Non-Interactive
Records clips automatically with countdown beeps.
No keypresses needed — just run and speak when you hear the high beep.

Usage:
  python scripts/auto_record_ni.py
  python scripts/auto_record_ni.py --count 5
  python scripts/auto_record_ni.py --category normal
"""
import sounddevice as sd
import numpy as np
import scipy.io.wavfile as wav
import os
import time

OUT_DIR = "nexus_real_samples"
SAMPLE_RATE = 16000
DURATION = 2.0
COUNTDOWN = 3

ALL_CATEGORIES = [
    ("nexus_normal", 20, "Say 'NEXUS' at NORMAL volume"),
    ("nexus_quiet", 10, "Say 'NEXUS' QUIETLY or WHISPERED"),
    ("nexus_loud", 10, "Say 'NEXUS' LOUDLY"),
    ("nexus_distant", 5, "Say 'NEXUS' from 3 METERS away"),
    ("hey_nexus", 5, "Say 'HEY NEXUS' at normal volume"),
]

def beep(freq=800, duration=0.15):
    try:
        sd.play(np.sin(2 * np.pi * freq * np.linspace(0, duration, int(SAMPLE_RATE * duration))), SAMPLE_RATE)
        sd.wait()
    except:
        pass

def record_clip(duration=DURATION, sr=SAMPLE_RATE):
    for i in range(COUNTDOWN, 0, -1):
        print(f"  {i}...", end="", flush=True)
        beep(600, 0.1)
        time.sleep(0.9)
    print(" RECORDING NOW! Say NEXUS!", flush=True)
    beep(1000, 0.2)
    audio = sd.rec(int(duration * sr), samplerate=sr, channels=1, dtype=np.int16)
    sd.wait()
    rms = np.sqrt(np.mean((audio.astype(np.float32) / 32767) ** 2))
    return audio, rms

def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--category", type=str, default=None)
    parser.add_argument("--count", type=int, default=None)
    parser.add_argument("--all", action="store_true", help="Record all 50 clips")
    args = parser.parse_args()
    
    os.makedirs(OUT_DIR, exist_ok=True)
    
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
    elif args.all:
        cats = ALL_CATEGORIES
    else:
        # Default: record 5 normal clips as a test
        cats = [("nexus_normal", 5, "Say 'NEXUS' at NORMAL volume")]
    
    total = sum(c for _, c, _ in cats)
    
    print("=" * 60)
    print("NEXUS Wake Word Auto-Recorder (Non-Interactive)")
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
    print()
    print("Starting in 5 seconds... get ready!")
    for i in range(5, 0, -1):
        print(f"  {i}...", end="", flush=True)
        time.sleep(1)
    print("GO!")
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
            
            time.sleep(0.5)
    
    print(f"\n{'=' * 60}")
    print(f"Recording complete!")
    print(f"{'=' * 60}")
    print(f"Recorded: {recorded}")
    print(f"Skipped (too quiet): {skipped}")
    total_files = len([f for f in os.listdir(OUT_DIR) if f.endswith('.wav')])
    print(f"Total files in {OUT_DIR}/: {total_files}")
    print()
    if total_files >= 50:
        print("All 50 clips recorded! Ready for training.")
        print(f"Zip: zip -r nexus_real_samples.zip {OUT_DIR}/")
    else:
        print(f"Recorded {total_files}/50 clips. Run again with --all to complete.")

if __name__ == "__main__":
    main()
