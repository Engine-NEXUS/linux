#!/usr/bin/env python3
"""
NEXUS Multilingual Synthetic Negative Generator
Generates negative speech clips across English, Hindi, and Telugu voices
to prevent false triggers on common conversational phrases.
Target directory: wake_word_data/negative (same dir the trainer reads)
"""

import os
import asyncio
import subprocess
from pathlib import Path

# ── Conversational phrases that should NEVER trigger the wake word ──────────────

ENGLISH_WORDS = [
    # Generic greetings / closings
    "hi", "hello", "hey there", "bye", "goodbye", "see you later", "good night",
    "good morning", "good evening", "morning", "evening",
    # Affirmations / negations
    "yes", "no", "okay", "nope", "yep", "sure", "fine", "alright",
    # Other assistants (phonetically test confusion)
    "alexa", "siri", "cortana", "jarvis", "google", "computer", "assistant",
    # Commands that sound like wake words
    "next", "nexus focus", "next step", "next slide", "next song",
    "texas", "lexus", "excess", "mexico", "vexus", "index us",
    # Common conversation starters
    "what", "who", "where", "when", "why", "how", "what's up", "how are you",
    "can you", "please", "thank you", "thanks", "sorry", "excuse me",
    # Common commands
    "stop", "play", "pause", "open", "close", "cancel", "back", "go", "start",
    "search for", "find", "call", "text", "send", "show me",
    # Numbers
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    # Random speech
    "the weather today", "what time is it", "set a timer", "remind me",
    "turn on", "turn off", "volume up", "volume down", "play music",
    "I need help", "let me think", "hang on a second", "wait",
    # Cough-like / throat clear sounds
    "hmm", "uh", "um", "ah", "oh", "err",
]

# Hindi phrases the model will encounter in an Indian home
HINDI_WORDS = [
    "haan", "nahin", "theek hai", "acha", "bilkul", "shukriya", "namaste",
    "kya hua", "bolo", "sunao", "ruko", "jao", "aao", "bhaiya", "didi",
    "mummy", "papa", "chai lao", "paani lao", "khaana ready hai",
    "kya kar rahe ho", "kab aoge", "thoda ruko", "suno suno",
    "ek minute", "abhi aata hoon", "theek hai ji",
    "gaana bajao", "band karo", "chalu karo",
    "arre", "arrey yaar", "yaar sun", "bhai",
]

# Telugu phrases
TELUGU_WORDS = [
    "emi", "cheppandi", "avunu", "kaadu", "sare", "okay cheyandi",
    "ikkade raa", "ela unnaru", "enti vishayam", "chudandi",
    "manchi ga undi", "nenu vastanu", "abbaa", "ayyo",
    "koncham aapu", "vivaraalu cheppandi", "telugu lo matladu",
]

# ── Voices per language ──────────────────────────────────────────────────────────

ENGLISH_VOICES = [
    "en-US-AriaNeural", "en-US-GuyNeural", "en-US-JennyNeural",
    "en-US-DavisNeural", "en-US-MonicaNeural",
    "en-GB-RyanNeural", "en-GB-SoniaNeural",
    "en-AU-NatashaNeural", "en-AU-WilliamNeural",
    "en-IN-NeerjaNeural", "en-IN-PrabhatNeural",
    "en-CA-ClaraNeural",
]

HINDI_VOICES = [
    "hi-IN-MadhurNeural",   # Male
    "hi-IN-SwaraNeural",    # Female
]

TELUGU_VOICES = [
    "te-IN-MohanNeural",    # Male
    "te-IN-ShrutiNeural",   # Female
]

# ── Output directory (same dir train_local_wakeword.py reads from) ───────────────
ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "wake_word_data" / "negative"
DATA_DIR.mkdir(parents=True, exist_ok=True)

sem = asyncio.Semaphore(25)  # Max concurrent TTS + ffmpeg calls

async def generate_clip(word: str, voice: str) -> bool:
    """Generate a single clip. Returns True if successfully created."""
    safe_word = word.replace(" ", "_").replace("'", "").replace("?", "").replace(",", "")
    base = f"synth_{voice}_{safe_word}"
    mp3 = DATA_DIR / f"{base}.mp3"
    wav = DATA_DIR / f"{base}.wav"
    
    if wav.exists():
        return False  # Already done
    
    async with sem:
        cmd = f'edge-tts --text "{word}" --voice {voice} --write-media "{mp3}"'
        proc = await asyncio.create_subprocess_shell(
            cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
        )
        await proc.communicate()
        
        if mp3.exists():
            ff = f'ffmpeg -y -i "{mp3}" -ar 16000 -ac 1 "{wav}" -loglevel quiet'
            ff_proc = await asyncio.create_subprocess_shell(ff)
            await ff_proc.communicate()
            try:
                mp3.unlink()
            except Exception:
                pass
            return wav.exists()
        return False


async def main():
    tasks = []
    
    # English: all words × all English voices
    for word in ENGLISH_WORDS:
        for voice in ENGLISH_VOICES:
            tasks.append(generate_clip(word, voice))
    
    # Hindi: all Hindi words × Hindi voices
    for word in HINDI_WORDS:
        for voice in HINDI_VOICES:
            tasks.append(generate_clip(word, voice))
    
    # Telugu: all Telugu words × Telugu voices
    for word in TELUGU_WORDS:
        for voice in TELUGU_VOICES:
            tasks.append(generate_clip(word, voice))
    
    total = len(tasks)
    print(f"Generating {total} multilingual negative clips...")
    print(f"  English:  {len(ENGLISH_WORDS) * len(ENGLISH_VOICES)} clips")
    print(f"  Hindi:    {len(HINDI_WORDS) * len(HINDI_VOICES)} clips")
    print(f"  Telugu:   {len(TELUGU_WORDS) * len(TELUGU_VOICES)} clips")
    print(f"  Saved to: {DATA_DIR}\n")
    
    done = 0
    for coro in asyncio.as_completed(tasks):
        created = await coro
        done += 1
        if done % 100 == 0 or done == total:
            print(f"  Progress: {done}/{total} clips processed...")
    
    existing = list(DATA_DIR.glob("synth_*.wav"))
    print(f"\nDone! {len(existing)} synthetic negative clips in {DATA_DIR}")


if __name__ == "__main__":
    asyncio.run(main())
