"""Record AMBIENT audio locally so false-execution-per-hour can be measured (docs/features/103 Q2).

What it does: records the default microphone in 30-second WAV chunks into a local folder, for N minutes, while you do
what you normally do with Ghost Mode idle in the background (TV on, calls, chatting, typing). Later, the chunks are
run through the real STT + gate chain and every command that WOULD have executed is counted.

PRIVACY: this records whatever the microphone hears, including other people. It is OFF unless you run it, asks for an
explicit typed confirmation, writes ONLY to the folder you choose on this PC, never uploads anything, and the folder
can be deleted at any time. Do not run it while others who haven't agreed are speaking nearby.

Usage:  python scripts/phrasing_probe/record_ambient.py --minutes 120 [--out %LOCALAPPDATA%/NEXUS/ambient]
Requires: pip install sounddevice numpy scipy   (the same packages `nexus collect` installs)
"""
import argparse, os, sys, time


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--minutes", type=float, default=120)
    ap.add_argument("--chunk-seconds", type=int, default=30)
    ap.add_argument("--out", default=os.path.expandvars(r"%LOCALAPPDATA%/NEXUS/ambient"))
    a = ap.parse_args()

    print(__doc__)
    ans = input(f'Type RECORD to record {a.minutes:.0f} min of microphone audio into "{a.out}": ').strip()
    if ans != "RECORD":
        print("cancelled — nothing recorded")
        return 1
    try:
        import numpy as np
        import sounddevice as sd
        from scipy.io import wavfile
    except ImportError as e:
        print(f"missing dependency: {e}\n  pip install sounddevice numpy scipy")
        return 2

    os.makedirs(a.out, exist_ok=True)
    sr = 16000
    end = time.time() + a.minutes * 60
    i = 0
    print("recording… press Ctrl+C to stop early")
    try:
        while time.time() < end:
            buf = sd.rec(int(a.chunk_seconds * sr), samplerate=sr, channels=1, dtype="int16")
            sd.wait()
            path = os.path.join(a.out, f"ambient_{int(time.time())}_{i:04d}.wav")
            wavfile.write(path, sr, np.asarray(buf).reshape(-1))
            i += 1
            if i % 10 == 0:
                print(f"  {i * a.chunk_seconds / 60:.0f} min recorded")
    except KeyboardInterrupt:
        print("\nstopped")
    print(f"{i} chunks ({i * a.chunk_seconds / 60:.1f} min) in {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
