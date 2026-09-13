#!/usr/bin/env python3
"""
NEXUS NLU — Interactive Voice Sample Collector

Prompts you with phrases to speak, records your voice, transcribes via
the STT server (Groq or Moonshine), and saves the real transcript as
training data for BERT-Mini. This captures:
  - Real human speech (not TTS)
  - Your actual microphone characteristics
  - Real room acoustics
  - STT distortions and mishearings
  - Pronunciation variants
  - Filler words and natural phrasing

Usage:
  python scripts/collect_nlu_samples.py                    # full collection (all intents)
  python scripts/collect_nlu_samples.py --intent type_text  # specific intent only
  python scripts/collect_nlu_samples.py --count 20         # 20 per intent (default 10)
  python scripts/collect_nlu_samples.py --list              # list all intents
  python scripts/collect_nlu_samples.py --text-only          # type instead of speak (no mic)

Output:
  server/admin/data/collected_samples.jsonl  — real transcripts + intents
  (merge_and_train.py reads this and adds to dataset.json)

Requirements:
  pip install sounddevice numpy scipy
  STT server running on port 39217 (nexus start runs it automatically)
  Or set GROQ_API_KEY env var for cloud transcription
"""

import os
import sys
import json
import time
import random
import argparse
import requests
from pathlib import Path

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# ─── Paths ──────────────────────────────────────────────────────────────────
SCRIPT_DIR = Path(__file__).parent
ROOT_DIR = SCRIPT_DIR.parent
OUTPUT_PATH = ROOT_DIR / "server" / "admin" / "data" / "collected_samples.jsonl"
STT_PORT = 39217
STT_URL = f"http://127.0.0.1:{STT_PORT}"

# ─── Audio settings ─────────────────────────────────────────────────────────
SAMPLE_RATE = 16000
RECORD_SECONDS = 2  # 2 seconds per utterance (continuous, no Enter needed)

# ─── Groq API key auto-load ─────────────────────────────────────────────────

def load_groq_key():
    """Load Groq API key from env, then from NEXUS settings.json."""
    import os
    key = os.environ.get("GROQ_API_KEY")
    if key:
        return key

    # Try NEXUS settings.json
    settings_paths = [
        Path(os.environ.get("APPDATA", "")) / "com.nexus.assistant" / "settings.json",
        Path.home() / ".config" / "com.nexus.assistant" / "settings.json",
        ROOT_DIR / "server" / "admin" / "settings.json",
    ]
    for path in settings_paths:
        if path.exists():
            try:
                with open(path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                key = data.get("groqApiKey") or data.get("groq_api_key")
                if key:
                    return key
            except (json.JSONDecodeError, OSError):
                continue
    return None

# ─── Phrase catalog by intent ──────────────────────────────────────────────
# These are the phrases the script will prompt you to say.
# Each intent has a list of phrases. The script picks randomly.
PHRASES = {
    "type_text": [
        "type hello world",
        "type i will be there in 5 minutes",
        "type thanks a lot",
        "type see you tomorrow",
        "type happy birthday",
        "type the quick brown fox",
        "type i love coding",
        "type good night",
        "type let's meet at 3pm",
        "write hello world",
        "write thanks for your help",
        "enter hello world",
        "type this the meeting is at 2pm",
        "type the password is hunter2",
        "type please review this code",
    ],
    "press_key": [
        "press enter",
        "press escape",
        "press tab",
        "press space",
        "press backspace",
        "press delete",
        "press up",
        "press down",
        "press left",
        "press right",
        "press home",
        "press end",
        "press f5",
        "press f11",
        "press f12",
        "hit enter",
        "tap escape",
        "press the enter key",
        "press the escape key",
        "hit the tab key",
    ],
    "press_hotkey": [
        "press ctrl a",
        "press ctrl c",
        "press ctrl v",
        "press ctrl x",
        "press ctrl z",
        "press ctrl y",
        "press ctrl s",
        "press ctrl f",
        "press ctrl t",
        "press ctrl w",
        "press ctrl shift tab",
        "press alt f4",
        "press ctrl shift n",
        "press win d",
        "press ctrl shift esc",
        "press ctrl alt del",
        "press shift f5",
        "press ctrl shift t",
        "press ctrl plus",
        "press ctrl minus",
    ],
    "confirm_send": [
        "send",
        "send it",
        "send message",
        "send the message",
        "send now",
        "send it now",
        "go ahead and send",
        "please send",
        "send please",
        "go ahead please",
        "ship it",
        "fire it off",
        "do it",
        "confirm",
        "yes send",
        "just send it",
        "send it please",
        "go ahead",
        "confirm send",
        "yes go ahead",
        "okay send",
        "send it for me",
        "please go ahead",
        "yes confirm",
    ],
    "cancel_action": [
        "stop",
        "cancel that",
        "stop that",
        "don't send",
        "don't do it",
        "don't send it",
        "wait don't",
        "wait",
        "hold on",
        "wait a second",
        "hold on a minute",
        "scratch that",
        "abort",
        "nevermind",
        "cancel",
        "cancel it",
        "never mind",
        "forget it",
        "no",
        "nope",
        "stop it",
        "stop now",
        "halt",
        "abort it",
        "back off",
    ],
    "browser_new_tab": [
        "new tab",
        "open new tab",
        "open a new tab",
        "new browser tab",
        "open a new tab please",
        "can you open a new tab",
        "tab",
        "another tab",
        "one more tab",
        "start a new tab",
        "new window",
        "open new window",
        "open a new window",
        "create new tab",
        "make a new tab",
        "open another tab",
        "open one more tab",
        "open a new window please",
        "new browser window",
        "open a fresh tab",
    ],
    "browser_navigate": [
        "go to github.com",
        "go to google.com",
        "go to youtube.com",
        "navigate to github.com",
        "navigate to google.com",
        "browse to stackoverflow.com",
        "visit reddit.com",
        "take me to github.com",
        "jump to youtube.com",
        "head to google.com",
        "go to wikipedia.org",
        "go to docs.rs",
        "go to crates.io",
        "navigate to twitter.com",
        "go to linkedin.com",
        "go to gmail.com",
        "go to amazon.com",
        "go to netflix.com",
        "go to spotify.com",
        "go to huggingface.co",
    ],
    "browser_search": [
        "search for rust programming in browser",
        "search for cats in browser",
        "google rust programming in browser",
        "search for how to tie a tie in browser",
        "look up weather today here",
        "find python tutorial on the web",
        "search for machine learning basics online",
        "google world cup 2026 here",
        "search for best laptops 2026 in browser",
        "search for python vs rust in browser",
        "search for docker vs kubernetes in browser",
        "search for git rebase tutorial in browser",
        "search for regex tester in browser",
        "search for css flexbox guide in browser",
        "search for typescript generics in browser",
    ],
    "whatsapp_open": [
        "open whatsapp",
        "launch whatsapp",
        "start whatsapp",
        "whatsapp",
        "bring up whatsapp",
        "show whatsapp",
        "open whatsapp please",
        "can you open whatsapp",
        "go to whatsapp",
        "take me to whatsapp",
        "fire up whatsapp",
        "pull up whatsapp",
        "open the whatsapp",
        "open whatsapp app",
        "switch to whatsapp",
        "focus whatsapp",
        "bring whatsapp to front",
        "open whatsapp desktop",
        "open whatsapp web",
        "open my whatsapp",
    ],
    "whatsapp_search": [
        "search for mom in whatsapp",
        "find mom in whatsapp",
        "open chat with mom in whatsapp",
        "chat with mom on whatsapp",
        "go to mom chat",
        "go to chat with mom",
        "find mom",
        "look for mom",
        "search mom",
        "search for dad in whatsapp",
        "find dad in whatsapp",
        "chat with dad on whatsapp",
        "search for lakshya in whatsapp",
        "find lakshya in whatsapp",
        "chat with lakshya on whatsapp",
        "search for john in whatsapp",
        "find john in whatsapp",
        "chat with john on whatsapp",
        "search for sarah in whatsapp",
        "find sarah in whatsapp",
    ],
    "focus_app": [
        "focus chrome",
        "bring chrome to front",
        "bring chrome forward",
        "switch to chrome",
        "switch over to chrome",
        "switch to chrome window",
        "go to chrome",
        "bring up chrome",
        "show chrome",
        "maximize chrome",
        "focus chrome window",
        "bring chrome window to front",
        "focus vscode",
        "switch to vscode",
        "bring vscode to front",
        "focus notepad",
        "switch to notepad",
        "focus terminal",
        "switch to terminal",
        "focus whatsapp",
    ],
    "open_app": [
        "open chrome",
        "launch chrome",
        "start chrome",
        "open notepad",
        "launch notepad",
        "open vscode",
        "launch vscode",
        "open terminal",
        "launch terminal",
        "open calculator",
        "open file explorer",
        "open spotify",
        "launch spotify",
        "open discord",
        "launch discord",
        "open slack",
        "open zoom",
        "open teams",
        "open outlook",
        "open word",
    ],
    "close_app": [
        "close chrome",
        "quit chrome",
        "exit chrome",
        "kill chrome",
        "close notepad",
        "quit notepad",
        "close vscode",
        "quit vscode",
        "close terminal",
        "close calculator",
        "close spotify",
        "close discord",
        "close slack",
        "close zoom",
        "close teams",
        "close outlook",
        "close word",
        "close excel",
        "close whatsapp",
        "quit whatsapp",
    ],
    "search": [
        "search for cats",
        "search cats",
        "google cats",
        "look up cats",
        "find cats",
        "find me cats",
        "look for cats",
        "search for rust programming",
        "google rust programming",
        "look up rust programming",
        "search for how to tie a tie",
        "google how to tie a tie",
        "search for weather today",
        "search for python tutorial",
        "search for machine learning basics",
        "search for world cup 2026",
        "search for best laptops 2026",
        "search for python vs rust",
        "search for docker vs kubernetes",
        "search for git rebase tutorial",
    ],
    "open_settings": [
        "open settings",
        "open command center",
        "show preferences",
        "configure nexus",
        "open the settings",
        "show settings",
        "open the command center",
        "show the settings",
        "open configuration",
        "show configuration",
        "open the configuration",
        "show the preferences",
        "open the preferences",
        "open settings please",
        "show settings please",
        "open the command center please",
        "show me the settings",
        "show me the command center",
        "show me the preferences",
        "display settings",
    ],
    "media_play_pause": [
        "play",
        "pause",
        "play pause",
        "play music",
        "pause music",
        "play the music",
        "pause the music",
        "play the song",
        "pause the song",
        "resume",
        "resume music",
        "resume the music",
        "resume playback",
        "play media",
        "pause media",
        "play the media",
        "pause the media",
        "toggle play",
        "toggle pause",
        "play or pause",
    ],
    "media_next": [
        "next",
        "next song",
        "next track",
        "skip",
        "skip song",
        "skip track",
        "next please",
        "go next",
        "go to next",
        "go to the next song",
        "go to the next track",
        "play the next song",
        "play the next track",
        "move to next",
        "move to the next song",
        "move to the next track",
        "advance",
        "advance to next",
        "advance to the next song",
        "advance to the next track",
    ],
    "media_previous": [
        "previous",
        "previous song",
        "previous track",
        "go back",
        "go back a song",
        "go back a track",
        "previous please",
        "go previous",
        "go to previous",
        "go to the previous song",
        "go to the previous track",
        "play the previous song",
        "play the previous track",
        "move to previous",
        "move to the previous song",
        "move to the previous track",
        "rewind",
        "rewind a song",
        "rewind a track",
    ],
    "media_stop": [
        "stop music",
        "stop the music",
        "stop playing",
        "stop the song",
        "stop the track",
        "stop playback",
        "stop the playback",
        "stop audio",
        "stop the audio",
        "halt music",
        "halt the music",
        "halt playback",
        "halt the playback",
        "cease music",
        "cease the music",
        "cease playback",
        "cease the playback",
        "end music",
        "end the music",
        "end playback",
    ],
    "greeting": [
        "hello",
        "hi",
        "hey",
        "hey nexus",
        "hello nexus",
        "hi nexus",
        "good morning",
        "good afternoon",
        "good evening",
        "good night",
        "hey there",
        "hi there",
        "hello there",
        "howdy",
        "greetings",
        "yo",
        "sup",
        "what's up",
        "how are you",
        "how are you doing",
    ],
    "analyse_repo": [
        "analyse the repo zync",
        "analyse repo zync",
        "analyse zync",
        "analyze the repo zync",
        "analyze zync",
        "analyse the repository zync",
        "analyse repository zync",
        "analyse the zync repo",
        "analyse the zync repository",
        "analyse the repo servx",
        "analyse servx",
        "analyse the repo github",
        "analyse the repo nexus",
        "analyse the repo ultron",
        "analyse the repo octocat hello world",
        "analyse the repo octocat hello world",
        "analyse the repo prem servx",
        "analyse the repo eesha zync",
        "analyse the repo lakshya ultron",
        "analyse the repo octocat hello world",
    ],
    "list_prs": [
        "list prs",
        "list pull requests",
        "show prs",
        "show pull requests",
        "list the prs",
        "list the pull requests",
        "show the prs",
        "show the pull requests",
        "list prs in zync",
        "list pull requests in zync",
        "show prs in zync",
        "show pull requests in zync",
        "list prs in servx",
        "list pull requests in servx",
        "show prs in servx",
        "show the prs in zync",
        "show the pull requests in zync",
        "list the prs in servx",
        "list the pull requests in servx",
        "show the prs in servx",
    ],
    "unknown": [
        "what is the meaning of life",
        "tell me a joke",
        "who are you",
        "what can you do",
        "what time is it",
        "what's the weather",
        "how old are you",
        "where do you live",
        "what is your name",
        "do you like pizza",
        "can you sing",
        "tell me a story",
        "what is ai",
        "explain quantum computing",
        "what is the speed of light",
        "how tall is mount everest",
        "what is the capital of france",
        "who won the world cup",
        "what is blockchain",
        "explain relativity",
    ],
}

# ─── Audio recording ────────────────────────────────────────────────────────

def record_audio(duration=RECORD_SECONDS):
    """Record audio from the microphone. Returns numpy array or None."""
    try:
        import sounddevice as sd
        import numpy as np
    except ImportError:
        print("  [ERROR] sounddevice not installed. Run: pip install sounddevice numpy scipy")
        return None

    print(f"  Recording {duration}s... ", end="", flush=True)
    audio = sd.rec(int(duration * SAMPLE_RATE), samplerate=SAMPLE_RATE,
                   channels=1, dtype=np.float32)
    sd.wait()
    rms = float(np.sqrt(np.mean(audio**2)))
    print(f"RMS={rms:.4f}")

    if rms < 0.005:
        print("  [WARN] Too quiet — did you speak? Skipping.")
        return None

    return audio


def audio_to_wav_bytes(audio):
    """Convert numpy float32 audio to WAV bytes (16kHz mono 16-bit PCM)."""
    import io
    import scipy.io.wavfile as wav
    import numpy as np

    # Convert float32 [-1, 1] to int16 [-32768, 32767]
    audio_int16 = (audio * 32767).astype(np.int16)
    buf = io.BytesIO()
    wav.write(buf, SAMPLE_RATE, audio_int16)
    return buf.getvalue()


def transcribe_via_stt_server(audio):
    """Send audio to the local STT server for transcription."""
    wav_bytes = audio_to_wav_bytes(audio)
    try:
        response = requests.post(
            f"{STT_URL}/transcribe",
            files={"audio": ("audio.wav", wav_bytes, "audio/wav")},
            timeout=10,
        )
        if response.status_code == 200:
            return response.json().get("text", "").strip()
        else:
            return None
    except (requests.exceptions.ConnectionError, Exception):
        return None


def transcribe_via_groq(audio):
    """Send audio to Groq for transcription (cloud, auto-loads key from NEXUS settings)."""
    api_key = load_groq_key()
    if not api_key:
        return None

    wav_bytes = audio_to_wav_bytes(audio)
    try:
        response = requests.post(
            "https://api.groq.com/openai/v1/audio/transcriptions",
            headers={"Authorization": f"Bearer {api_key}"},
            files={"file": ("audio.wav", wav_bytes, "audio/wav")},
            data={"model": "whisper-large-v3-turbo"},
            timeout=10,
        )
        if response.status_code == 200:
            return response.json().get("text", "").strip()
        else:
            print(f"  [ERROR] Groq returned {response.status_code}: {response.text[:100]}")
            return None
    except Exception as e:
        print(f"  [ERROR] Groq request failed: {e}")
        return None


def transcribe(audio):
    """Transcribe audio — try Groq first (cloud, auto-key), then STT server."""
    # Try Groq cloud first (auto-loads key from NEXUS settings)
    text = transcribe_via_groq(audio)
    if text:
        return text

    # Fall back to local STT server
    text = transcribe_via_stt_server(audio)
    if text:
        return text

    return None


# ─── Collection logic ──────────────────────────────────────────────────────

def save_sample(text, intent, slots, source="voice"):
    """Save a collected sample to the JSONL file."""
    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)

    sample = {
        "text": text,
        "intent": intent,
        "slots": slots,
        "source": source,
        "timestamp": time.time(),
    }

    with open(OUTPUT_PATH, "a", encoding="utf-8") as f:
        f.write(json.dumps(sample, ensure_ascii=False) + "\n")


def load_existing_count():
    """Count existing samples in the output file."""
    if not OUTPUT_PATH.exists():
        return 0
    count = 0
    with open(OUTPUT_PATH, "r", encoding="utf-8") as f:
        for _ in f:
            count += 1
    return count


def check_stt_server():
    """Check if the STT server is running."""
    try:
        response = requests.get(f"{STT_URL}/health", timeout=2)
        return response.status_code == 200
    except Exception:
        return False


def get_slots_for_intent(intent, phrase):
    """Extract slots from the phrase based on the intent."""
    # For most intents, the slots are embedded in the phrase
    # The deterministic parser will handle slot extraction during training
    # Here we just provide minimal slot info
    if intent == "type_text":
        # "type <text>" → text is everything after "type "
        if phrase.startswith("type "):
            return {"text": phrase[5:]}
        elif phrase.startswith("write "):
            return {"text": phrase[6:]}
        elif phrase.startswith("enter "):
            return {"text": phrase[6:]}
        return {"text": phrase}
    elif intent == "press_key":
        # "press <key>" → key is the last word
        words = phrase.split()
        if len(words) >= 2:
            return {"key": words[-1]}
        return {"key": ""}
    elif intent == "press_hotkey":
        # "press ctrl a" → keys = ["ctrl", "a"]
        words = phrase.split()
        if words[0] == "press":
            return {"keys": words[1:]}
        return {"keys": []}
    elif intent == "browser_navigate":
        # "go to github.com" → url = "github.com"
        for url_marker in ["go to ", "navigate to ", "browse to ", "visit ", "take me to ", "jump to ", "head to "]:
            if phrase.startswith(url_marker):
                return {"url": phrase[len(url_marker):]}
        return {"url": ""}
    elif intent == "browser_search":
        # "search for X in browser" → query = "X"
        for prefix in ["search for ", "google ", "look up ", "find ", "search "]:
            if phrase.startswith(prefix):
                rest = phrase[len(prefix):]
                # Remove trailing " in browser", " on the web", etc.
                for suffix in [" in browser", " on the web", " online", " here", " in this tab"]:
                    if rest.endswith(suffix):
                        rest = rest[:-len(suffix)]
                return {"query": rest}
        return {"query": ""}
    elif intent in ("whatsapp_search",):
        # "find mom in whatsapp" → contact = "mom"
        for prefix in ["search for ", "find ", "look for ", "search ", "chat with ", "open chat with ", "go to ", "message "]:
            if phrase.startswith(prefix):
                rest = phrase[len(prefix):]
                for suffix in [" in whatsapp", " on whatsapp", " chat", " in the whatsapp"]:
                    if rest.endswith(suffix):
                        rest = rest[:-len(suffix)]
                return {"contact": rest}
        return {"contact": ""}
    elif intent == "focus_app":
        # "focus chrome" → target = "chrome"
        for prefix in ["focus ", "bring ", "switch to ", "go to ", "bring up ", "show ", "maximize "]:
            if phrase.startswith(prefix):
                rest = phrase[len(prefix):]
                for suffix in [" to front", " forward", " window", " to the front", " window to front"]:
                    if rest.endswith(suffix):
                        rest = rest[:-len(suffix)]
                return {"target": rest}
        return {"target": ""}
    elif intent in ("open_app", "close_app"):
        # "open chrome" → app_name = "chrome"
        for prefix in ["open ", "launch ", "start ", "close ", "quit ", "exit ", "kill ", "run ", "fire up ", "bring up "]:
            if phrase.startswith(prefix):
                rest = phrase[len(prefix):]
                for suffix in [" app", " application", " the", " please", " for me"]:
                    if rest.endswith(suffix):
                        rest = rest[:-len(suffix)]
                return {"app_name": rest}
        return {"app_name": ""}
    elif intent == "search":
        for prefix in ["search for ", "google ", "look up ", "find ", "search ", "look for "]:
            if phrase.startswith(prefix):
                return {"query": phrase[len(prefix):]}
        return {"query": ""}
    return {}


def collect_intent(intent, count, text_only=False, yes=False):
    """Collect samples for a single intent — continuous mode (no Enter needed)."""
    phrases = PHRASES.get(intent, [])
    if not phrases:
        print(f"  [ERROR] No phrases defined for intent '{intent}'")
        return 0

    # Pick random phrases (with replacement if count > len(phrases))
    selected = []
    for _ in range(count):
        selected.append(random.choice(phrases))

    collected = 0
    for i, phrase in enumerate(selected):
        print(f"\n  [{i+1}/{count}] Intent: {intent}")
        print(f"  Say: \"{phrase}\"")

        if text_only:
            # Text mode — just save the phrase directly
            text = phrase
            print(f"  Saved (text mode): \"{text}\"")
        else:
            # Voice mode — press Enter to record each sample
            try:
                input("  Press Enter when ready to speak... ")
            except EOFError:
                pass

            # Countdown on separate lines for visibility
            print("  ╔═══════╗")
            print("  ║   3   ║")
            print("  ╚═══════╝")
            time.sleep(0.8)
            print("  ╔═══════╗")
            print("  ║   2   ║")
            print("  ╚═══════╝")
            time.sleep(0.8)
            print("  ╔═══════╗")
            print("  ║   1   ║")
            print("  ╚═══════╝")
            time.sleep(0.8)
            print("  ╔═════════════╗")
            print("  ║   SPEAK!    ║")
            print("  ╚═════════════╝")

            audio = record_audio()
            if audio is None:
                print("  [skip] Too quiet")
                continue

            text = transcribe(audio)
            if not text:
                print("  [skip] No transcription")
                continue

            print(f"  Heard: \"{text}\"")
            action = input("  Enter=save, r=retry, e=edit transcript, s=skip, q=end: ").strip().lower()
            if action == "q":
                raise KeyboardInterrupt
            if action == "s":
                print("  Skipped")
                continue
            if action == "r":
                print("  Retrying this phrase")
                selected.insert(i + 1, phrase)
                continue
            if action == "e":
                corrected = input("  Correct transcript: ").strip()
                if not corrected:
                    print("  Skipped (empty correction)")
                    continue
                text = corrected

        slots = get_slots_for_intent(intent, text)
        save_sample(text, intent, slots, source="voice" if not text_only else "text")
        collected += 1
        print(f"  Saved ({collected}/{count} for {intent})")

        # Gap between samples (1.5s pause so you can breathe)
        if i + 1 < count:
            print("  ────────────────────────────────")
            time.sleep(1.5)

    return collected


# ─── Main ──────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="NEXUS NLU — Interactive Voice Sample Collector"
    )
    parser.add_argument("--intent", type=str, default=None,
                        help="Collect for a specific intent only")
    parser.add_argument("--count", type=int, default=10,
                        help="Number of samples per intent (default: 10)")
    parser.add_argument("--list", action="store_true",
                        help="List all available intents and exit")
    parser.add_argument("--text-only", action="store_true",
                        help="Type phrases instead of speaking (no microphone)")
    parser.add_argument("--yes", "-y", action="store_true",
                        help="Skip confirmation prompts (non-interactive mode)")
    args = parser.parse_args()

    if args.list:
        print("Available intents:")
        for intent, phrases in PHRASES.items():
            print(f"  {intent:25s}  ({len(phrases)} phrases)")
        print(f"\nTotal: {len(PHRASES)} intents")
        return

    print("\n" + "=" * 60)
    print("  NEXUS NLU — Voice Sample Collector (Continuous Mode)")
    print("=" * 60)

    if not args.text_only:
        # Check transcription availability
        groq_key = load_groq_key()
        stt_ok = check_stt_server()

        print(f"\n  Transcription:")
        print(f"    Groq cloud:  {'OK (key auto-loaded from NEXUS settings)' if groq_key else 'NOT AVAILABLE (set GROQ_API_KEY or run NEXUS)'}")
        print(f"    STT server:  {'OK (port ' + str(STT_PORT) + ')' if stt_ok else 'not running (optional)'}")

        if not groq_key and not stt_ok:
            print(f"\n  [ERROR] No transcription available!")
            print(f"  Fix: Start NEXUS first (nexus start) OR set GROQ_API_KEY env var")
            print(f"  The Groq key is auto-loaded from your NEXUS settings.json")
            return

        print(f"\n  Recording: {RECORD_SECONDS}s per utterance, {SAMPLE_RATE}Hz mono")
        print(f"  Mode: CONTINUOUS (no Enter needed — just speak when prompted)")
        print(f"  Countdown: 3...2...1...SPEAK! then auto-records {RECORD_SECONDS}s")
    else:
        print(f"\n  Text mode — phrases saved directly (no recording).")

    print(f"  Output: {OUTPUT_PATH}")
    print(f"  Existing samples: {load_existing_count()}")
    print(f"  Samples per intent: {args.count}")

    # Determine which intents to collect
    if args.intent:
        if args.intent not in PHRASES:
            print(f"\n  [ERROR] Unknown intent: {args.intent}")
            print(f"  Use --list to see available intents.")
            return
        intents = [args.intent]
    else:
        intents = list(PHRASES.keys())

    print(f"  Intents to collect: {len(intents)}")
    print(f"  Total samples to collect: {len(intents) * args.count}")
    print(f"\n  Press Ctrl+C at any time to stop. Samples are saved as you go.")
    print(f"  After stopping, run: nexus train")

    time.sleep(2)  # brief pause before starting


    total_collected = 0
    try:
        for intent in intents:
            print(f"\n{'─' * 60}")
            print(f"  Collecting: {intent}")
            print(f"{'─' * 60}")

            collected = collect_intent(intent, args.count, text_only=args.text_only, yes=args.yes)
            total_collected += collected
            print(f"\n  {intent}: {collected} samples collected")
    except KeyboardInterrupt:
        print(f"\n\n{'=' * 60}")
        print(f"  Stopped by user (Ctrl+C)")
        print(f"{'=' * 60}")

    print(f"\n{'=' * 60}")
    print(f"  Collection {'complete' if total_collected > 0 else 'finished'}!")
    print(f"{'=' * 60}")
    print(f"  Total samples collected: {total_collected}")
    print(f"  Saved to: {OUTPUT_PATH}")
    print(f"  Total in file: {load_existing_count()}")
    print(f"\n  Next steps:")
    print(f"    1. Run 'nexus train' to retrain BERT-Mini with your real voice data")
    print(f"    2. Run 'nexus build' to bundle the new model into the installer")
    print(f"{'=' * 60}\n")


if __name__ == "__main__":
    main()
