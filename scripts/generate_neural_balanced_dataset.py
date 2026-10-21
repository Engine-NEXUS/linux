#!/usr/bin/env python3
"""
Generate a diverse, neural multi-voice dataset of "NEXUS" and "Hey NEXUS" positives.
Uses Microsoft Edge TTS neural voices with multi-rate and multi-pitch variations,
resampled to 16kHz mono PCM, verified with Whisper.
"""

import asyncio
import os
import sys
from pathlib import Path
import numpy as np
import scipy.io.wavfile as wavfile
from faster_whisper import WhisperModel
import edge_tts
import io

ROOT = Path(__file__).resolve().parent.parent
POS_DIR = ROOT / "wake_word_data" / "positive"
POS_DIR.mkdir(parents=True, exist_ok=True)

VOICES = [
    "en-US-AvaNeural",
    "en-US-AndrewNeural",
    "en-US-EmmaNeural",
    "en-US-BrianNeural",
    "en-US-ChristopherNeural",
    "en-US-JennyNeural",
    "en-US-GuyNeural",
    "en-US-AriaNeural",
    "en-US-EricNeural",
    "en-GB-SoniaNeural",
    "en-GB-RyanNeural",
    "en-IN-NeerjaNeural",
    "en-IN-PrabhatNeural",
]

PHRASES = [
    # Standalone 2-syllable NEXUS
    ("nexus", "nexus"),
    ("NEXUS", "nexus"),
    ("Nexus", "nexus"),
    # 3-syllable Hey NEXUS
    ("hey nexus", "hey nexus"),
    ("Hey NEXUS", "hey nexus"),
    ("Hey, Nexus", "hey nexus"),
    # Okay NEXUS
    ("okay nexus", "okay nexus"),
    ("OK NEXUS", "okay nexus"),
]

RATES = ["-20%", "-10%", "+0%", "+10%", "+20%"]
PITCHES = ["-5Hz", "+0Hz", "+5Hz"]

async def synthesize_clip(text: str, voice: str, rate: str, pitch: str) -> bytes:
    communicate = edge_tts.Communicate(text, voice, rate=rate, pitch=pitch)
    data = bytearray()
    async for chunk in communicate.stream():
        if chunk["type"] == "audio":
            data.extend(chunk["data"])
    return bytes(data)

def decode_mp3_to_wav16k(mp3_bytes: bytes) -> np.ndarray:
    """Decode MP3 bytes into 16kHz mono int16 array using subprocess ffmpeg or soundfile."""
    import subprocess
    cmd = [
        "ffmpeg", "-y", "-i", "pipe:0",
        "-ar", "16000", "-ac", "1", "-f", "s16le", "pipe:1"
    ]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    out, _ = proc.communicate(input=mp3_bytes)
    audio = np.frombuffer(out, dtype=np.int16)
    return audio

async def main():
    print("=" * 70)
    print("  NEURAL MULTI-VOICE WAKE WORD DATASET GENERATOR")
    print("=" * 70)

    # Clean existing neural synthetic files if any
    for old_f in POS_DIR.glob("neural_pos_*.wav"):
        old_f.unlink()

    whisper = WhisperModel("tiny.en", device="cpu", compute_type="int8")
    print("Loaded Whisper validator.\n")

    saved_count = 0
    target_count = 250
    tasks = []

    print(f"Generating balanced neural positives across {len(VOICES)} voices...")

    clip_id = 0
    for voice in VOICES:
        for text, category in PHRASES:
            for rate in RATES:
                pitch = np.random.choice(PITCHES)
                clip_id += 1
                try:
                    mp3_data = await synthesize_clip(text, voice, rate, pitch)
                    if not mp3_data:
                        continue
                    audio16 = decode_mp3_to_wav16k(mp3_data)
                    if len(audio16) < 1600:  # < 100ms
                        continue

                    # Pad to 2.0s (32000 samples)
                    target_len = 32000
                    if len(audio16) < target_len:
                        pad_before = (target_len - len(audio16)) // 3
                        pad_after = target_len - len(audio16) - pad_before
                        audio16 = np.pad(audio16, (pad_before, pad_after), mode="constant")
                    else:
                        audio16 = audio16[:target_len]

                    # Verify with Whisper
                    audio_f32 = audio16.astype(np.float32) / 32768.0
                    segs, _ = whisper.transcribe(audio_f32, beam_size=1)
                    transcription = " ".join(s.text for s in segs).lower().strip()

                    if "nexus" in transcription or "nexis" in transcription or "lexus" in transcription:
                        out_path = POS_DIR / f"neural_pos_{saved_count+1:04d}.wav"
                        wavfile.write(str(out_path), 16000, audio16)
                        saved_count += 1
                        if saved_count % 25 == 0:
                            print(f"  [Progress] {saved_count} verified neural positives generated... (latest: '{transcription}' by {voice})")
                        if saved_count >= target_count:
                            break
                except Exception as e:
                    # Ignore single voice/clip network blips
                    pass
            if saved_count >= target_count:
                break
        if saved_count >= target_count:
            break

    print(f"\nSuccessfully generated & verified {saved_count} neural samples in {POS_DIR}")
    print(f"Total positive dataset is now {len(list(POS_DIR.glob('*.wav')))} samples.")

if __name__ == "__main__":
    asyncio.run(main())
