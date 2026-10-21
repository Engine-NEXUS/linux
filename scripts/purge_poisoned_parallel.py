#!/usr/bin/env python3
"""
Ultra-fast Parallel Positive Dataset Cleaner & Poison Purger.
Uses 4 parallel processes with faster-whisper to finish the audit in ~45 seconds.
"""

import os
import sys
import shutil
import glob
from pathlib import Path
import numpy as np
from scipy.io import wavfile
from faster_whisper import WhisperModel
from concurrent.futures import ProcessPoolExecutor

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)
    sys.stderr.reconfigure(encoding="utf-8", errors="replace", line_buffering=True)

ROOT = Path(__file__).resolve().parent.parent
POS_DIR = ROOT / "wake_word_data" / "positive"
NEG_DIR = ROOT / "wake_word_data" / "negative"
QUARANTINE_DIR = ROOT / "wake_word_data" / "quarantined_bad_positive"
QUARANTINE_DIR.mkdir(parents=True, exist_ok=True)
NEG_DIR.mkdir(parents=True, exist_ok=True)

MIN_RMS = 0.008
MIN_PEAK = 0.040
SOUNDALIKE_KEYWORDS = ["texas", "access", "excess", "nixes", "nixis", "next", "taxes", "focus", "bonus", "census"]

def process_file_batch(file_batch, worker_id):
    asr = WhisperModel("tiny.en", device="cpu", compute_type="int8", cpu_threads=2)
    
    kept = 0
    silent = 0
    soundalike = 0
    sentences = 0

    for idx, fpath in enumerate(file_batch, 1):
        f = Path(fpath)
        if not f.exists():
            continue
        try:
            sr, data = wavfile.read(fpath)
            if data.ndim > 1:
                data = data.mean(axis=1)
            audio = data.astype(np.float32) / 32768.0
            rms = float(np.sqrt(np.mean(audio ** 2)))
            peak = float(np.max(np.abs(audio)))

            # Step 1: Energy & silence check
            if rms < MIN_RMS or peak < MIN_PEAK:
                silent += 1
                dest = QUARANTINE_DIR / f"silent_{f.name}"
                shutil.move(fpath, dest)
                continue

            # Step 2: ASR Transcription
            segments, _ = asr.transcribe(audio, beam_size=1)
            text = " ".join(s.text for s in segments).strip().lower()
            text_clean = "".join(c for c in text if c.isalnum() or c.isspace()).strip()
            words = text_clean.split()

            if not text_clean or len(words) == 0:
                silent += 1
                dest = QUARANTINE_DIR / f"unintelligible_{f.name}"
                shutil.move(fpath, dest)
                continue

            # Step 3: Check for pure nexus or valid wake variant
            has_nexus = any(k in text_clean for k in ["nexus", "nexas", "nexis", "nexes"])

            if not has_nexus:
                is_soundalike = any(k in text_clean for k in SOUNDALIKE_KEYWORDS)
                if is_soundalike:
                    soundalike += 1
                    dest = NEG_DIR / f"soundalike_from_pos_{f.name}"
                    shutil.move(fpath, dest)
                else:
                    sentences += 1
                    dest = QUARANTINE_DIR / f"non_nexus_{f.name}"
                    shutil.move(fpath, dest)
                continue

            # Step 4: Has nexus, but is it a conversational sentence?
            if len(words) > 4:
                sentences += 1
                dest = QUARANTINE_DIR / f"long_sentence_{f.name}"
                shutil.move(fpath, dest)
                continue

            kept += 1

        except Exception as e:
            pass

        if idx % 20 == 0:
            print(f"[Worker {worker_id}] Processed {idx}/{len(file_batch)}: Kept={kept}, Silent={silent}, Soundalike={soundalike}, Sentence={sentences}", flush=True)

    print(f"[Worker {worker_id}] FINISHED {len(file_batch)}: Kept={kept}, Silent={silent}, Soundalike={soundalike}, Sentence={sentences}", flush=True)
    return {"kept": kept, "silent": silent, "soundalike": soundalike, "sentences": sentences}

def main():
    print("=" * 70, flush=True)
    print("  PARALLEL 4-WORKER POSITIVE DATASET CLEANER", flush=True)
    print("=" * 70, flush=True)

    pos_files = sorted(glob.glob(str(POS_DIR / "*.wav")))
    total = len(pos_files)
    print(f"Total positive files remaining: {total}", flush=True)

    num_workers = 4
    batch_size = int(np.ceil(total / num_workers))
    batches = [pos_files[i:i + batch_size] for i in range(0, total, batch_size)]

    print(f"Divided {total} files into {len(batches)} batches across {num_workers} processes.\n", flush=True)

    with ProcessPoolExecutor(max_workers=num_workers) as executor:
        futures = [executor.submit(process_file_batch, b, i) for i, b in enumerate(batches)]
        results = [f.result() for f in futures]

    total_kept = sum(r["kept"] for r in results)
    total_silent = sum(r["silent"] for r in results)
    total_soundalike = sum(r["soundalike"] for r in results)
    total_sentences = sum(r["sentences"] for r in results)

    print("\n" + "=" * 70, flush=True)
    print("  FINAL PARALLEL PURGE COMPLETE", flush=True)
    print("=" * 70, flush=True)
    print(f"  Pristine NEXUS Kept           : {total_kept}", flush=True)
    print(f"  Silent/Quiet Quarantined      : {total_silent}", flush=True)
    print(f"  Soundalikes Moved to Negative : {total_soundalike}", flush=True)
    print(f"  Conversational Quarantined    : {total_sentences}", flush=True)
    print(f"  Current Pristine Positives    : {len(glob.glob(str(POS_DIR / '*.wav')))}", flush=True)

if __name__ == "__main__":
    main()
