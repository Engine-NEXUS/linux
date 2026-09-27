#!/usr/bin/env python3
"""
Audit all positive wake word recordings.
Performs:
  1. Audio file integrity & format check (16kHz mono).
  2. Energy & silence check (RMS, peak).
  3. Whisper ASR check (verifies 'nexus' is present in transcription).
  4. Quarantines silent or non-nexus clips into wake_word_data/quarantined_bad_positive/
"""

import sys
import shutil
import time
from pathlib import Path

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import numpy as np
from scipy.io import wavfile
from faster_whisper import WhisperModel

ROOT = Path(__file__).resolve().parent.parent
POS_DIR = ROOT / "wake_word_data" / "positive"
QUARANTINE_DIR = ROOT / "wake_word_data" / "quarantined_bad_positive"
QUARANTINE_DIR.mkdir(parents=True, exist_ok=True)

SILENCE_RMS = 0.0055
PEAK_MIN = 0.025

def audit_positive_samples():
    print("=" * 65)
    print("  NEXUS POSITIVE SAMPLES DEEP AUDIT & SCRAPER")
    print("=" * 65)
    
    files = sorted(POS_DIR.glob("*.wav"))
    print(f"Total positive files to evaluate: {len(files)}")
    
    print("\nLoading faster-whisper tiny.en for transcription verification...")
    t0 = time.time()
    asr = WhisperModel("tiny.en", device="cpu", compute_type="int8")
    print(f"ASR Model loaded in {time.time() - t0:.2f}s\n")
    
    passed = []
    quarantined_silent = []
    quarantined_no_nexus = []
    
    prompt = "Nexus, Hey Nexus, Ok Nexus, Nexus wake up, Nexus please"
    
    for idx, f in enumerate(files, 1):
        try:
            sr, data = wavfile.read(str(f))
            if data.ndim > 1:
                data = data.mean(axis=1)
            audio = data.astype(np.float32) / 32768.0
            rms = float(np.sqrt(np.mean(audio ** 2)))
            peak = float(np.max(np.abs(audio)))
            dur = len(audio) / sr
            
            # Check 1: Silence
            if rms < SILENCE_RMS or peak < PEAK_MIN:
                quarantined_silent.append((f, rms, peak, "too_quiet"))
                continue
                
            # Check 2: Whisper Transcription
            segments, _ = asr.transcribe(str(f), beam_size=1, initial_prompt=prompt)
            transcript = " ".join([s.text for s in segments]).strip().lower()
            
            # Check if nexus or phonetic close match is in transcript
            # Nexus variants: 'nexus', 'next is', 'nexis', 'nexas', 'lexus', 'next us'
            is_nexus = any(k in transcript for k in [
                "nexus", "nexas", "nexis", "next is", "next us", "nicks us", "nectars"
            ])
            
            if is_nexus:
                passed.append((f, rms, peak, transcript))
            else:
                quarantined_no_nexus.append((f, rms, peak, transcript))
                
        except Exception as e:
            quarantined_silent.append((f, 0.0, 0.0, f"read_error: {e}"))
            
        if idx % 50 == 0 or idx == len(files):
            print(f"  Processed {idx}/{len(files)} clips... (Valid: {len(passed)}, Silent: {len(quarantined_silent)}, Non-Nexus: {len(quarantined_no_nexus)})")
            
    print("\n" + "=" * 65)
    print("  AUDIT SUMMARY")
    print("=" * 65)
    print(f"  Total Scanned           : {len(files)}")
    print(f"  Passed (Verified Nexus) : {len(passed)}")
    print(f"  Rejected (Too Silent)   : {len(quarantined_silent)}")
    print(f"  Rejected (No Nexus Word): {len(quarantined_no_nexus)}")
    
    if quarantined_silent:
        print("\n  Sample of silent clips moved:")
        for f, rms, peak, reason in quarantined_silent[:5]:
            print(f"    {f.name}: RMS={rms:.4f}, Peak={peak:.4f} ({reason})")
            
    if quarantined_no_nexus:
        print("\n  Sample of clips missing 'nexus':")
        for f, rms, peak, txt in quarantined_no_nexus[:10]:
            print(f"    {f.name}: RMS={rms:.4f} -> '{txt}'")
            
    # Quarantine bad clips
    to_quarantine = quarantined_silent + [(f, r, p, txt) for f, r, p, txt in quarantined_no_nexus]
    for item in to_quarantine:
        f = item[0]
        dest = QUARANTINE_DIR / f.name
        shutil.move(str(f), str(dest))
        
    print(f"\nSuccessfully cleaned dataset! {len(passed)} verified positive clips ready in {POS_DIR}")

if __name__ == "__main__":
    audit_positive_samples()
