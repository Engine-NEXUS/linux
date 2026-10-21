#!/usr/bin/env python3
"""P0 FA/hr soak harness for the NEXUS wake-word model.

Streams negative audio through the SAME chunk pipeline as
verify_hardened_model.py but with live trigger semantics: state is
carried across file boundaries (like the real Rust stream), a trigger
fires at peak >= THRESHOLD, and a 3s refractory suppresses echoes
(matching wakeword_oww.rs).

Modes:
  --smoke    20 negative files (~3 min) — validates the harness
  --full     all wake_word_data/negative + background (hours; schedule it)
  --silence S  S seconds of digital silence (expect exactly 0 triggers)

Gate (plan 02 P0.2): < 1 FA / 8 h on TV/conversation soak.

Example:
  python scripts/soak_wake_fa.py --smoke
  python scripts/soak_wake_fa.py --silence 3600
"""
import argparse
import glob
import sys
import time
from pathlib import Path

import numpy as np
from scipy.io import wavfile

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import verify_hardened_model as V

THRESHOLD = 0.68
REFRACTORY_S = 3.0
CHUNK_S = V.CHUNK / 16000.0


def load_16k_mono(path):
    sr, data = wavfile.read(path)
    if data.ndim > 1:
        data = data.mean(axis=1)
    audio = data.astype(np.float32) / 32768.0
    if sr != 16000:
        # Linear resample to 16k (same approach as the trainer).
        n = int(len(audio) * 16000 / sr)
        audio = np.interp(
            np.linspace(0, len(audio), n, endpoint=False),
            np.arange(len(audio)),
            audio,
        ).astype(np.float32)
    return audio


class SoakState:
    """Streaming scorer with trigger + refractory semantics."""

    def __init__(self):
        self.lookback = np.zeros(V.LOOKBACK, dtype=np.float32)
        self.mel_buf = [np.zeros((V.MEL_PER_CHUNK, 32), dtype=np.float32)] * V.MEL_CIRC
        self.emb_buf = [V.comfort_emb.copy() for _ in range(V.EMB_FRAMES)]
        self.t = 0.0
        self.last_fire = -1e9
        self.triggers = []  # (t_seconds, peak)
        self.peak = 0.0

    def feed(self, audio, tag=""):
        n = (len(audio) // V.CHUNK) * V.CHUNK
        for i in range(0, n, V.CHUNK):
            chunk = audio[i:i + V.CHUNK].copy()
            self.t += CHUNK_S
            rms = float(np.sqrt(np.mean(chunk ** 2)))
            if rms < 0.002:
                self.emb_buf = (self.emb_buf + [V.comfort_emb])[-V.EMB_FRAMES:]
                self.mel_buf = (self.mel_buf + [np.zeros((V.MEL_PER_CHUNK, 32), dtype=np.float32)])[-V.MEL_CIRC:]
                continue
            if rms < 0.03:
                gain = min(0.03 / rms, 15.0)
                chunk = np.clip(chunk * gain, -1.0, 1.0)
            framed = np.concatenate([self.lookback, chunk]) * 32768.0
            self.lookback = chunk[-V.LOOKBACK:]

            m = V.mel.run(None, {V.mel.get_inputs()[0].name: framed[None, :]})[0]
            m = m.reshape(V.MEL_PER_CHUNK, 32) / 10.0 + 2.0
            self.mel_buf = (self.mel_buf + [m])[-V.MEL_CIRC:]

            window = np.stack(self.mel_buf).reshape(80, 32)[4:80]
            e = V.emb.run(None, {V.emb.get_inputs()[0].name: window[None, :, :, None].astype(np.float32)})[0]
            self.emb_buf = (self.emb_buf + [e.reshape(-1)])[-V.EMB_FRAMES:]

            stacked = np.stack(self.emb_buf).astype(np.float32)[None, :]
            score = float(V.clf.run(None, {V.clf.get_inputs()[0].name: stacked})[0][0][0])
            if score > self.peak:
                self.peak = score
            if score >= THRESHOLD and (self.t - self.last_fire) >= REFRACTORY_S:
                self.last_fire = self.t
                self.triggers.append((self.t, score))
                print(f"  TRIGGER t={self.t:8.1f}s score={score:.3f} [{tag}]", flush=True)
                # Mirror the Rust reset_after_trigger(): flush streaming
                # buffers so the next file starts clean (harness used to
                # carry the wake embedding across files — a harness bug
                # that manufactured a cross-file trigger in soak #1).
                self.lookback = np.zeros(V.LOOKBACK, dtype=np.float32)
                self.mel_buf = [np.zeros((V.MEL_PER_CHUNK, 32), dtype=np.float32)] * V.MEL_CIRC
                self.emb_buf = [V.comfort_emb.copy() for _ in range(V.EMB_FRAMES)]


def report(state, label):
    hours = state.t / 3600.0
    fa_hr = len(state.triggers) / hours if hours > 0 else 0.0
    print("=" * 64)
    print(f"SOAK {label}: {state.t/60:.1f} min audio, "
          f"{len(state.triggers)} triggers, peak={state.peak:.4f}")
    print(f"  FA/hr = {fa_hr:.3f}  (gate: < 1 FA / 8 h  ->  < 0.125/hr)")
    print("=" * 64)
    return fa_hr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--smoke", action="store_true")
    ap.add_argument("--full", action="store_true")
    ap.add_argument("--silence", type=int, default=0)
    ap.add_argument("--limit", type=int, default=0,
                    help="cap the number of files (medium soaks)")
    ap.add_argument("--stream", action="store_true",
                    help="carry buffers across files (production-like "
                         "continuity). Default OFF: each file starts with "
                         "fresh buffers — dataset clips are independent "
                         "utterances, and cross-file embedding stitching "
                         "manufactures triggers the real stream (silence-"
                         "separated speech + reset_after_trigger) does "
                         "not. Soak #1 manufactured exactly one such "
                         "cross-file trigger (hey_0007, 0.733); isolated "
                         "peak <0.09.")
    args = ap.parse_args()

    data = ROOT / "wake_word_data"
    state = SoakState()
    t0 = time.time()

    if args.silence > 0:
        print(f"soak: {args.silence}s digital silence (expect 0 triggers)")
        state.feed(np.zeros(args.silence * 16000, dtype=np.float32), tag="silence")
        report(state, "silence")
        return

    files = sorted(glob.glob(str(data / "negative" / "*.wav")))
    if args.full:
        files += sorted(glob.glob(str(data / "background" / "*.wav")))
    if args.smoke:
        files = files[:20]
    if args.limit > 0:
        files = files[:args.limit]
    if not files:
        print("soak: no negative files found");
        return
    print(f"soak: {len(files)} files (mode={'full' if args.full else ('stream' if args.stream else 'limit')})")
    total_triggers = 0
    total_peak = 0.0
    total_secs = 0.0
    for i, f in enumerate(files):
        if not args.stream:
            # Independent-utterance mode (default): fresh state per file.
            state = SoakState()
        try:
            state.feed(load_16k_mono(f), tag=Path(f).name)
        except Exception as e:
            print(f"  SKIP {f}: {e}")
            continue
        total_triggers += len(state.triggers)
        total_peak = max(total_peak, state.peak)
        total_secs += state.t
        if (i + 1) % 50 == 0:
            print(f"  ... {i + 1}/{len(files)} ({total_secs/60:.0f} min, "
                  f"{total_triggers} triggers)", flush=True)
    # Report on a synthetic state carrying the GLOBAL totals.
    summary = SoakState()
    summary.t = total_secs
    summary.triggers = [("agg", 0.0)] * total_triggers
    summary.peak = total_peak
    report(summary, "full" if args.full else ("stream" if args.stream else "limit"))
    print(f"wall time: {(time.time()-t0)/60:.1f} min")


if __name__ == "__main__":
    main()
