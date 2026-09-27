"""Quick live mic test — records 5s and reports RMS, peak, and frequency content."""
import sounddevice as sd
import numpy as np
import sys

SR = 16000
DURATION = 5.0

print(f"Recording {DURATION}s at {SR}Hz mono...")
print("Say 'NEXUS' a few times during the recording.\n")

audio = sd.rec(int(DURATION * SR), samplerate=SR, channels=1, dtype=np.float32)
sd.wait()

rms = np.sqrt(np.mean(audio**2))
peak = np.max(np.abs(audio))
zero_crossings = np.sum(np.abs(np.diff(np.sign(audio.flatten()))) > 0)

# Check for silence (Intel SST driver issue)
silence_pct = np.mean(np.abs(audio.flatten()) < 0.0001) * 100

# FFT to see frequency content
fft = np.fft.rfft(audio.flatten())
freqs = np.fft.rfftfreq(len(audio), 1/SR)
magnitudes = np.abs(fft)
top_5_idx = np.argsort(magnitudes)[-5:][::-1]

print(f"RMS:          {rms:.6f}")
print(f"Peak:         {peak:.6f}")
print(f"Silence %:    {silence_pct:.1f}%  (>95% = Intel SST driver issue)")
print(f"Zero crosses: {zero_crossings}")
print(f"\nTop 5 frequencies:")
for i in top_5_idx:
    if freqs[i] > 0:
        print(f"  {freqs[i]:.0f} Hz  (magnitude: {magnitudes[i]:.1f})")

if silence_pct > 95:
    print("\n*** INTEL SST SILENCE DETECTED ***")
    print("The mic is producing mostly silence. The driver may need a restart.")
    print("Try: Restart-Service -Name 'Audiosrv' -Force")
elif rms < 0.001:
    print("\n*** VERY LOW AUDIO ***")
    print("Mic is working but audio is very quiet. Check mic gain settings.")
elif rms > 0.001:
    print("\n*** MIC IS WORKING ***")
    print(f"Audio levels look healthy (RMS={rms:.4f})")
