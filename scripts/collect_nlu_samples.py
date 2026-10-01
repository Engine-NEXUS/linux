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
    # NOTE: "list_prs" appears ONCE here (merged 2026-09-18 — a duplicate
    # key used to silently drop 21 phrases; Python keeps only the last).
    "list_prs": [
        "list prs",
        "list pull requests",
        "show prs",
        "show pull requests",
        "list the prs",
        "list the pull requests",
        "show the prs",
        "show the pull requests",
        "show me the pull requests",
        "show me the pull requests and all",
        "give me all the pull requests",
        "pull up the pull request list",
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
        "list open prs in nexus",
        "what prs are open in shopkart",
        "list all prs in ledger-ai",
        "show the prs in meet",
        "list pull requests in congi",
        "fetch pr list for servx",
        "display open prs in zync",
        "show me the prs in nexus",
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
    # ─── MCP commerce + messaging ──────────────────────────────────
    "order_food": [
        "order pizza",
        "order biryani",
        "order pizza from dominos",
        "order biryani from paradise",
        "get me a burger",
        "i want dosa",
        "i am craving sushi",
        "bring me noodles",
        "fetch pasta from oven story",
        "order food from swiggy",
        "get food",
        "order lunch",
    ],
    "search_product": [
        "search for headphones on amazon",
        "find a laptop on amazon",
        "amazon search for keyboard",
        "search amazon for mouse",
        "check the price of a monitor on amazon",
        "buy a webcam on amazon",
        "look up an ssd on amazon",
        "hunt down a backpack on amazon",
        "search for earbuds on amazone",
        "find a power bank on amazon",
        "search amazon for desk lamp",
        "look for a notebook on amazon",
    ],
    "send_whatsapp_message": [
        "send mom a whatsapp message saying i will be late",
        "whatsapp dad saying on my way",
        "message lakshya on whatsapp saying happy birthday",
        "tell prem good luck on whatsapp",
        "text eesha saying call me back",
        "ping brother saying almost there",
        "send mom a whatsup message saying hello",
        "whatsapp mommy saying good night",
        "forward this to dad saying found it",
        "send a message to mom saying take care",
        "message amma on whatsapp saying reached home",
        "tell sister happy diwali on whatsapp",
    ],
    "whatsapp_chat": [
        "open chat with mom",
        "chat with dad",
        "message lakshya",
        "open chat with prem on whatsapp",
        "talk to eesha on whatsapp",
        "ping brother on whatsapp",
        "chat with mom",
        "message dad on whatsapp",
        "open chat with sister",
        "talk to mom",
        "get me dad on whatsapp",
        "chat with boss",
    ],
    # ─── GitHub operations ─────────────────────────────────────────
    "merge_pr": [
        "merge pr 42 in servx",
        "merge pull request 7 in zync",
        "squash merge pr 15 in nexus",
        "approve and merge pr 3 in shopkart",
        "merge pr 99 in ledger-ai",
        "merge the pull request 21 in servx",
        "rebase merge pr 8 in zync",
        "merge pr 12",
        "please merge pr 5 in nexus",
        "merge pull request 30 in owner project",
        "merge pr 18 in meet",
        "merge pr 66 in congi",
    ],
    "approve_pr": [
        "approve pr 42 in servx",
        "approve pull request 7 in zync",
        "sign off on pr 15 in nexus",
        "approve pr 3",
        "give approval for pr 99 in shopkart",
        "approve the pull request 21 in servx",
        "greenlight pr 8 in zync",
        "approve pr 12 in meet",
        "please approve pr 5",
        "accept pr 30 in owner project",
        "approve pr 18 in congi",
        "approve pull request 66 in ledger-ai",
    ],
    "close_pr": [
        "close pr 42 in servx",
        "close pull request 7 in zync",
        "decline pr 15 in nexus",
        "close pr 3",
        "reject pr 99 in shopkart",
        "close the pull request 21 in servx",
        "shut pr 8 in zync",
        "close pr 12 in meet",
        "please close pr 5",
        "abandon pr 30 in owner project",
        "close pr 18 in congi",
        "close pull request 66 in ledger-ai",
    ],
    "get_pr": [
        "get pr 42 in servx",
        "show pull request 7 in zync",
        "describe pr 15 in nexus",
        "get pr 3",
        "fetch pr 99 in shopkart",
        "tell me about pr 21 in servx",
        "open pr 8 in zync",
        "get pull request 12 in meet",
        "show me pr 5",
        "details of pr 30 in owner project",
        "get pr 18 in congi",
        "what is pr 66 in ledger-ai",
    ],
    "list_branches": [
        "list branches in servx",
        "show all branches of zync",
        "branch list for nexus",
        "what branches exist in shopkart",
        "show branches in ledger-ai",
        "list all branches in meet",
        "branches in congi",
        "display branches of servx",
        "fetch branch list for zync",
        "show me branches in nexus",
        "all branches in shopkart",
        "list the branches in ledger-ai",
    ],
    # ─── GitHub PR operations ──────────────────────────────────────────
    "create_pr": [
        "create a pull request in servx with title add auth and head feature to base main",
        "create pr in zync title fix memory leak from dev to main",
        "open a pr in nexus called update styling from branch feature-ui to main",
        "create a pr for shopkart titled improve checkout",
        "open pull request in ledger-ai from dev to master",
        "make a pr in meet with title fix connection timeout",
        "create pr in servx",
        "create a new pull request in zync",
        "open pr in repo nexus titled refactor router",
        "create pull request for congi from patch-1 to main",
        "new pr in shopkart titled support apple pay",
        "create pr with title fix bug in zync",
    ],
    "comment_pr": [
        "comment on pr 42 in servx saying looks good to me",
        "add a comment on pull request 15 in nexus saying please run tests",
        "leave a comment on pr 8 in zync with message need more details",
        "comment on pr 3 saying approved from my side",
        "post a comment on pr 99 in shopkart saying check lint errors",
        "comment on pr 12 in meet saying verified on staging",
        "write a comment on pr 21 in servx saying let's merge this",
        "comment on pull request 66 in ledger-ai saying nice catch",
    ],
    "revert_pr": [
        "revert pr 42 in servx",
        "revert pull request 15 in nexus",
        "revert pr 8 in zync",
        "undo pr 3 in shopkart",
        "roll back pull request 21 in servx",
        "revert pr 99",
        "create a revert for pr 12 in meet",
        "revert pull request 66 in ledger-ai",
    ],
    "list_pr_files": [
        "list files in pr 42 in servx",
        "show files changed in pr 15 in nexus",
        "what files are modified in pull request 8 in zync",
        "files in pr 3 in shopkart",
        "view changed files for pr 21 in servx",
        "list pr files for pr 99",
        "show files for pull request 12 in meet",
        "get list of files in pr 66 in ledger-ai",
    ],
    "update_branch": [
        "update branch for pr 42 in servx",
        "update pr 15 in nexus with latest main",
        "sync branch for pull request 8 in zync",
        "update branch for pr 3",
        "bring pr 21 in servx up to date",
        "update pull request 99 in shopkart",
        "sync pr 12 in meet with master",
        "update branch in ledger-ai for pr 66",
    ],
    # ─── GitHub Analysis ──────────────────────────────────────────────
    "analyse_pr": [
        "analyse pr 42 in servx",
        "analyze pull request 15 in nexus",
        "review pr 8 in zync",
        "deep analyse pr 3 in shopkart",
        "analyse pr 21 in lakshya/servx",
        "review pull request 99 in chitkul/shopkart",
        "inspect pr 12 in meet",
        "analyse pr 66 in ledger-ai",
    ],
    "analyse_latest_pr": [
        "analyse the latest pr in servx",
        "analyze latest pull request in nexus",
        "review recent pr in zync",
        "analyse latest pr by chitkul in shopkart",
        "check the newest pr in meet",
        "inspect the latest pull request in ledger-ai",
        "analyse latest pr in servx",
    ],
    "check_branch": [
        "check branch feature-login in servx",
        "check the dev branch in nexus",
        "inspect branch release-v2 in zync",
        "status of branch main in shopkart",
        "check branch staging in meet",
        "verify branch patch-1 in ledger-ai",
        "check branch test in servx",
    ],
    "delete_branch": [
        "delete branch feature-login in servx",
        "remove branch old-ui in nexus",
        "delete the branch temp in zync",
        "delete branch fix-typo in shopkart",
        "remove dev-test branch in meet",
        "delete branch bugfix in ledger-ai",
    ],
    # ─── GitHub Releases & Workflows ──────────────────────────────────
    "create_release": [
        "create release v1.0.0 in servx",
        "publish release v2.1 in nexus",
        "create new release v0.5.0 in zync",
        "make release 1.2.3 in shopkart",
        "tag release v3.0 in meet",
        "draft release v1.1 in ledger-ai",
    ],
    "list_releases": [
        "list releases in servx",
        "show all releases of nexus",
        "get releases for zync",
        "view release history in shopkart",
        "what releases exist in meet",
        "fetch releases for ledger-ai",
    ],
    "list_workflows": [
        "list workflows in servx",
        "show all workflows of nexus",
        "get github actions workflows in zync",
        "view workflows in shopkart",
        "actions list for meet",
        "list workflows in ledger-ai",
    ],
    "list_workflow_runs": [
        "list workflow runs in servx",
        "show workflow history in nexus",
        "view action runs for zync",
        "recent workflow runs in shopkart",
        "show ci runs in meet",
        "list workflow runs for ledger-ai",
    ],
    "rerun_workflow": [
        "rerun workflow 12345 in servx",
        "retry workflow run 67890 in nexus",
        "re-run action 54321 in zync",
        "restart workflow 98765 in shopkart",
        "rerun workflow 11223 in meet",
        "retry failed workflow 44556 in ledger-ai",
    ],
    "cancel_workflow": [
        "cancel workflow 12345 in servx",
        "stop workflow run 67890 in nexus",
        "cancel action 54321 in zync",
        "abort workflow 98765 in shopkart",
        "cancel ci run 11223 in meet",
        "kill workflow 44556 in ledger-ai",
    ],
    # ─── GitHub Collaborators & Org Members ───────────────────────────
    "add_collaborator": [
        "add collaborator john to servx",
        "invite alice to collaborate on nexus",
        "add collaborator bob in zync with write access",
        "add user charlie as collaborator in shopkart",
        "invite developer dave to meet",
        "add collaborator eve in ledger-ai",
    ],
    "remove_collaborator": [
        "remove collaborator john from servx",
        "revoke collaboration for alice in nexus",
        "remove user bob from zync",
        "delete collaborator charlie from shopkart",
        "remove dave from meet repository",
        "drop collaborator eve in ledger-ai",
    ],
    "list_collaborators": [
        "list collaborators in servx",
        "show all collaborators of nexus",
        "who has access to zync",
        "collaborator list for shopkart",
        "view collaborators in meet",
        "show members with access to ledger-ai",
    ],
    "add_org_member": [
        "add org member john to acme",
        "invite alice to organization nexus-team",
        "add user bob to org myorg with admin role",
        "add org member charlie to devcorp",
        "invite dave to org techstack",
    ],
    "remove_org_member": [
        "remove org member john from acme",
        "remove alice from organization nexus-team",
        "delete user bob from org myorg",
        "remove charlie from org devcorp",
        "drop member dave from org techstack",
    ],
    "list_org_members": [
        "list org members in acme",
        "show all members of organization nexus-team",
        "who is in org myorg",
        "member list for organization devcorp",
        "view all members in org techstack",
    ],
    # ─── Navigation & Modes ───────────────────────────────────────────
    "open_url": [
        "open https://github.com",
        "navigate to https://news.ycombinator.com",
        "open url google.com",
        "open website https://docs.rs",
        "browse to https://crates.io",
        "open link https://wikipedia.org",
    ],
    "open_architect": [
        "open architect",
        "launch architect mode",
        "open the architect",
        "show architecture mapper",
        "switch to architect mode",
        "open code architect",
    ],
    # ─── Modes (Ghostwriter / Echo entry + exit) ───────────────────
    "ghostwriter_start": [
        "ghostwriter",
        "ghost writer",
        "ghost mode",
        "open ghostwriter mode",
        "open the ghost mode",
        "start ghostwriter",
        "take a letter",
        "write this down",
        "scribe mode",
        "take dictation",
        "ghostwriter for mom",
        "start writing",
    ],
    "ghostwriter_stop": [
        "exit ghost mode",
        "exit the mode",
        "stop ghostwriter",
        "close ghostwriter",
        "end dictation",
        "command mode",
        "done writing",
        "quit ghost mode",
        "stop listening",
        "that's all",
        "all done",
        "exit the ghost mode",
    ],
}

# ─── Collection tiers (stratified scheduler) ──────────────────────────────
# Quitting early must still leave every tier covered: the scheduler walks
# tiers round-robin, always picking the least-covered intent inside the tier.
TIERS = {
    "mcp": ["order_food", "search_product", "send_whatsapp_message",
            "whatsapp_chat", "whatsapp_open", "whatsapp_search"],
    "github": ["merge_pr", "approve_pr", "close_pr", "list_prs", "get_pr",
               "create_pr", "comment_pr", "revert_pr", "list_pr_files", "update_branch",
               "analyse_repo", "analyse_pr", "analyse_latest_pr", "check_branch",
               "delete_branch", "create_release", "list_releases", "list_workflows",
               "list_workflow_runs", "rerun_workflow", "cancel_workflow",
               "add_collaborator", "remove_collaborator", "list_collaborators",
               "add_org_member", "remove_org_member", "list_org_members", "list_branches"],
    "messages": ["type_text", "confirm_send", "cancel_action", "greeting"],
    "live": ["press_key", "press_hotkey", "browser_new_tab", "browser_navigate",
             "browser_search", "focus_app"],
    "apps": ["open_app", "close_app", "search", "open_settings", "open_url", "open_architect"],
    "media": ["media_play_pause", "media_next", "media_previous", "media_stop"],
    "modes": ["ghostwriter_start", "ghostwriter_stop"],
    "other": ["unknown"],
}

# ─── User-facing collection categories ──────────────────────────────────
# Menu shown on bare `nexus collect`: pick ONE category to train, or random
# mix across all tiers (the default stratified scheduler). Each category maps
# onto TIERS above — no category may name intents that don't exist.
CATEGORY_MENU = [
    ("github", "GitHub — PRs (merge/approve/close/list/create/comment), branch, actions, repo analysis",
     ["github"]),
    ("mcp", "Food / shopping / chat — order food, search Amazon, WhatsApp messages",
     ["mcp"]),
    ("apps", "Apps & system — open/close apps, settings, search, URL, architect",
     ["apps"]),
    ("messages", "Messages & dictation — type text, confirmations, ghostwriter",
     ["messages", "modes"]),
    ("live", "Live control — keys, hotkeys, browser tabs, URL navigation, focus",
     ["live"]),
    ("media", "Media & audio — play, pause, next, previous, stop",
     ["media"]),
    ("random", "Random mix — stratified across every tier (default)",
     None),
]


def category_intents(choice):
    """Resolve a menu choice (name or number string) to intent list or None.

    Returns None for random/all (caller uses every PHRASES intent).
    Raises KeyError on unknown choice.
    """
    for i, (name, _desc, tiers) in enumerate(CATEGORY_MENU, start=1):
        if choice == name or choice == str(i):
            if tiers is None:
                return None
            intents = [i for t in tiers for i in TIERS.get(t, [])]
            return [i for i in intents if i in PHRASES]
    raise KeyError(f"unknown category: {choice}")

PROGRESS_PATH = ROOT_DIR / "server" / "admin" / "data" / "collect_progress.json"

# ─── Audio recording ────────────────────────────────────────────────────────

def record_duration_for(phrase, wpm=140, margin=1.2, minimum=2.0, maximum=10.0):
    """Seconds needed to speak `phrase`, from its length.

    Conversational speech runs ~140 words/min (~2.3 words/s). Duration =
    words / rate + margin for breath + mic onset, clamped so short phrases
    don't clip and long ones don't idle. A 3-word phrase gets ~2.5s, a
    12-word phrase ~6.3s — instead of a fixed 2s that clips long reads
    and wastes time on short ones.
    """
    words = max(1, len(str(phrase).split()))
    secs = words / (wpm / 60.0) + margin
    return round(min(maximum, max(minimum, secs)), 1)


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
            data={
                "model": "whisper-large-v3-turbo",
                "language": "en",
                "temperature": "0.0",
                "prompt": "NEXUS, WhatsApp, Biryani, Dosa, Ghostwriter, VS Code, PR, GitHub, Terminal, Spotify",
            },
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
    elif intent == "order_food":
        # "order pizza from dominos" → food_item + restaurant
        if " from " in phrase:
            head, rest = phrase.split(" from ", 1)
            for prefix in ["order ", "get me ", "get ", "i want ", "i am craving ",
                           "bring me ", "fetch "]:
                if head.startswith(prefix):
                    head = head[len(prefix):]
                    break
            return {"food_item": head.strip(), "restaurant": rest.strip()}
        for prefix in ["order ", "get me some ", "get me ", "get ", "i want ",
                       "i am craving ", "bring me ", "fetch ", "order some "]:
            if phrase.startswith(prefix):
                return {"food_item": phrase[len(prefix):].strip()}
        return {}
    elif intent == "search_product":
        # strip amazon wrappers, keep the product as query
        rest = phrase
        for prefix in ["search for ", "find ", "amazon search for ",
                       "search amazon for ", "check the price of ",
                       "buy ", "look up ", "hunt down ", "look for "]:
            if rest.startswith(prefix):
                rest = rest[len(prefix):]
                break
        for suffix in [" on amazon", " on amazone"]:
            if rest.endswith(suffix):
                rest = rest[:-len(suffix)]
        return {"query": rest.strip()}
    elif intent == "send_whatsapp_message":
        # "... saying <message>" → contact + message
        if " saying " in phrase:
            head, message = phrase.split(" saying ", 1)
            contact = head
            for cut in ["send ", "whatsapp ", "message ", "a whatsapp message ",
                        "a message ", " on whatsapp", " to ", "tell ", "text ",
                        "ping ", "forward this to ", "send a message to "]:
                contact = contact.replace(cut, " ")
            contact = " ".join(contact.split())
            return {"contact": contact, "message": message.strip()}
        # "tell <contact> <message>" (no "saying") → first word contact
        rest = phrase
        for prefix in ["tell ", "text ", "ping "]:
            if rest.startswith(prefix):
                rest = rest[len(prefix):]
                break
        for suffix in [" on whatsapp", " whatsapp"]:
            if rest.endswith(suffix):
                rest = rest[:-len(suffix)]
        parts = rest.strip().split(None, 1)
        if len(parts) == 2:
            return {"contact": parts[0], "message": parts[1]}
        return {}
    elif intent in ("merge_pr", "approve_pr", "close_pr", "get_pr", "revert_pr", "list_pr_files", "update_branch"):
        # "merge pr 42 in servx" → pr_number + repo
        import re as _re
        m = _re.search(r"(?:pr|pull request)\s+#?(\d+)", phrase)
        pr = m.group(1) if m else ""
        repo = ""
        m2 = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)\s*$", phrase)
        if m2:
            repo = m2.group(1)
        out = {}
        if pr:
            out["pr_number"] = pr
        if repo:
            out["repo"] = repo
        return out
    elif intent == "create_pr":
        # "create pr in servx titled fix auth from dev to main"
        import re as _re
        repo, title, head, base = "", "", "", ""
        m_repo = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)", phrase)
        if m_repo:
            repo = m_repo.group(1)
        m_title = _re.search(r"(?:title|titled|called|with title)\s+([a-z0-9_\- ]+?)(?:\s+from|\s+to|\s+in|$)", phrase)
        if m_title:
            title = m_title.group(1).strip()
        m_head = _re.search(r"from\s+([a-z0-9_\-/]+)", phrase)
        if m_head:
            head = m_head.group(1)
        m_base = _re.search(r"to\s+([a-z0-9_\-/]+)", phrase)
        if m_base:
            base = m_base.group(1)
        out = {}
        if repo: out["repo"] = repo
        if title: out["title"] = title
        if head: out["head"] = head
        if base: out["base"] = base
        return out
    elif intent == "comment_pr":
        import re as _re
        m_pr = _re.search(r"(?:pr|pull request)\s+#?(\d+)", phrase)
        pr = m_pr.group(1) if m_pr else ""
        m_repo = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)", phrase)
        repo = m_repo.group(1) if m_repo else ""
        body = ""
        if " saying " in phrase:
            body = phrase.split(" saying ", 1)[1].strip()
        elif " with message " in phrase:
            body = phrase.split(" with message ", 1)[1].strip()
        out = {}
        if pr: out["pr_number"] = pr
        if repo: out["repo"] = repo
        if body: out["body"] = body
        return out
    elif intent in ("analyse_repo", "analyse_pr", "analyse_latest_pr", "check_branch"):
        import re as _re
        m_pr = _re.search(r"(?:pr|pull request)\s+#?(\d+)", phrase)
        pr = m_pr.group(1) if m_pr else ""
        m_repo = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)", phrase)
        repo = m_repo.group(1) if m_repo else ""
        m_author = _re.search(r"by\s+([a-z0-9_\-]+)", phrase)
        author = m_author.group(1) if m_author else ""
        out = {}
        if repo: out["repo"] = repo
        if pr: out["pr_number"] = pr
        if author: out["author"] = author
        return out
    elif intent in ("add_collaborator", "remove_collaborator", "list_collaborators"):
        import re as _re
        m_repo = _re.search(r"(?:in|to|from|for|of)\s+([a-z0-9_\-/]+)\s*$", phrase)
        repo = m_repo.group(1) if m_repo else ""
        m_user = _re.search(r"(?:collaborator|user|developer)\s+([a-z0-9_\-]+)", phrase)
        username = m_user.group(1) if m_user else ""
        permission = "admin" if "admin" in phrase else ("write" if "write" in phrase else ("read" if "read" in phrase else ""))
        out = {}
        if repo: out["repo"] = repo
        if username: out["username"] = username
        if permission: out["permission"] = permission
        return out
    elif intent in ("add_org_member", "remove_org_member", "list_org_members"):
        import re as _re
        m_org = _re.search(r"(?:org|organization)\s+([a-z0-9_\-]+)", phrase)
        org = m_org.group(1) if m_org else ""
        m_user = _re.search(r"(?:member|user)\s+([a-z0-9_\-]+)", phrase)
        username = m_user.group(1) if m_user else ""
        role = "admin" if "admin" in phrase else ("member" if "member" in phrase else "")
        out = {}
        if org: out["org"] = org
        if username: out["username"] = username
        if role: out["role"] = role
        return out
    elif intent in ("create_release", "list_releases", "delete_branch",
                    "list_workflows", "list_workflow_runs", "rerun_workflow", "cancel_workflow"):
        import re as _re
        m_repo = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)", phrase)
        repo = m_repo.group(1) if m_repo else ""
        m_tag = _re.search(r"(?:release|tag)\s+(v?[0-9]+(?:\.[0-9]+)*)", phrase)
        release_tag = m_tag.group(1) if m_tag else ""
        m_branch = _re.search(r"branch\s+([a-z0-9_\-/]+)", phrase)
        branch = m_branch.group(1) if m_branch else ""
        m_id = _re.search(r"(?:workflow|run|action)\s+(\d+)", phrase)
        workflow_id = m_id.group(1) if m_id else ""
        out = {}
        if repo: out["repo"] = repo
        if release_tag: out["release_tag"] = release_tag
        if branch: out["branch"] = branch
        if workflow_id: out["workflow_id"] = workflow_id
        return out
    elif intent == "list_prs":
        import re as _re
        m = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)\s*$", phrase)
        state = "open" if "open" in phrase else ("all" if "all" in phrase else "")
        out = {}
        if m:
            out["repo"] = m.group(1)
        if state:
            out["state"] = state
        return out
    elif intent == "list_branches":
        import re as _re
        m = _re.search(r"(?:in|of|for)\s+([a-z0-9_\-/]+)\s*$", phrase)
        return {"repo": m.group(1)} if m else {}
    elif intent == "open_url":
        import re as _re
        m = _re.search(r"(https?://\S+|[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})", phrase)
        return {"url": m.group(1)} if m else {}
    elif intent in ("ghostwriter_start", "ghostwriter_stop", "open_architect"):
        return {}
    return {}


# ─── Progress store + stratified scheduler ────────────────────────────────
# Quitting early must leave every tier covered: jobs walk tiers
# round-robin, always picking the least-covered intent inside the tier.
# Progress survives across sessions, so resume fills gaps, never the head.

def load_progress():
    """{intent: accepted_count} across all sessions (progress file wins)."""
    progress = {}
    if PROGRESS_PATH.exists():
        try:
            with open(PROGRESS_PATH, "r", encoding="utf-8") as f:
                progress = json.load(f)
        except (json.JSONDecodeError, OSError):
            progress = {}
    # Seed from the output file so pre-existing rows count too.
    if OUTPUT_PATH.exists():
        try:
            with open(OUTPUT_PATH, "r", encoding="utf-8") as f:
                for line in f:
                    try:
                        row = json.loads(line)
                        intent = row.get("intent", "")
                        if intent:
                            progress[intent] = max(progress.get(intent, 0), 0) + 1
                    except (json.JSONDecodeError, AttributeError):
                        continue
        except OSError:
            pass
    return progress


def save_progress(progress):
    try:
        PROGRESS_PATH.parent.mkdir(parents=True, exist_ok=True)
        with open(PROGRESS_PATH, "w", encoding="utf-8") as f:
            json.dump(progress, f, indent=2, sort_keys=True)
    except OSError:
        pass


def build_job_queue(intents, per_intent, progress, seed=None):
    """Interleaved [(intent, phrase)]: round-robin over tiers, least-covered
    intent first inside each tier, random phrase + random tier order per
    session (seeded shuffle so every run differs)."""
    rnd = random.Random(seed if seed is not None else int(time.time() * 1000) % 100000)
    # Tier order shuffled per session; tiers filtered to requested intents.
    wanted = set(intents)
    tiers = [(name, [i for i in members if i in wanted and i in PHRASES])
             for name, members in TIERS.items()]
    tiers = [(n, m) for n, m in tiers if m]
    # Any requested intent outside tiers still gets covered (own bucket).
    covered = {i for _, m in tiers for i in m}
    leftovers = sorted(set(intents) - covered)
    if leftovers:
        tiers.append(("extra", leftovers))
    rnd.shuffle(tiers)

    # Per-intent need = quota minus already accepted.
    need = {i: max(0, per_intent - progress.get(i, 0)) for i in intents if i in PHRASES}
    if not any(need.values()):
        # Everything at quota — top up uniformly (fresh variation round).
        need = {i: per_intent for i in intents if i in PHRASES}

    jobs = []
    # Round-robin: each pass takes one job from each tier's neediest intent.
    while any(v > 0 for v in need.values()):
        progressed = False
        for _, members in tiers:
            # Least-covered member first (random tiebreak via shuffle).
            cands = [m for m in members if need.get(m, 0) > 0]
            if not cands:
                continue
            rnd.shuffle(cands)
            pick = min(cands, key=lambda m: progress.get(m, 0))
            phrases = PHRASES[pick]
            jobs.append((pick, rnd.choice(phrases)))
            need[pick] -= 1
            progressed = True
        if not progressed:
            break
    return jobs


def print_status(progress, per_intent=10):
    print("\n  Coverage by tier (accepted samples / quota):")
    for tier, members in TIERS.items():
        have = [m for m in members if m in PHRASES]
        if not have:
            continue
        line = ", ".join(f"{m}={progress.get(m, 0)}" for m in have)
        weak = [m for m in have if progress.get(m, 0) < per_intent]
        flag = f"  <-- weakest: {', '.join(weak)}" if weak else "  OK"
        print(f"    [{tier}] {line}{flag}")


def collect_jobs(jobs, progress, text_only=False):
    """Collect one sample per job in queue order. Returns accepted count.
    Progress is saved after every accept, so Ctrl+C loses nothing and the
    next run resumes at the least-covered intents."""
    intent_of = {}
    for intent, _ in jobs:
        intent_of[intent] = intent_of.get(intent, 0)
    collected = 0
    total = len(jobs)
    for idx, (intent, phrase) in enumerate(jobs):
        print(f"\n  [{idx+1}/{total}] Intent: {intent}  (have {progress.get(intent, 0)})")
        print(f"  Say: \"{phrase}\"")

        if text_only:
            text = phrase
            print(f"  Saved (text mode): \"{text}\"")
        else:
            try:
                input("  Press Enter when ready to speak... ")
            except EOFError:
                pass

            print("  Recording now — speak!")
            print()

            secs = record_duration_for(phrase)
            audio = record_audio(secs)
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
                jobs.insert(idx + 1, (intent, phrase))
                total += 1
                continue
            if action == "e":
                corrected = input("  Correct transcript: ").strip()
                if not corrected:
                    print("  Skipped (empty correction)")
                    continue
                text = corrected

        slots = get_slots_for_intent(intent, text)
        save_sample(text, intent, slots, source="voice" if not text_only else "text")
        progress[intent] = progress.get(intent, 0) + 1
        save_progress(progress)
        collected += 1
        print(f"  Saved ({collected} this run, {progress[intent]} total for {intent})")

        if idx + 1 < len(jobs):
            print("  ────────────────────────────────")
            time.sleep(1.5)

    return collected


def collect_intent(intent, count, text_only=False, yes=False):
    """Legacy single-intent entry (kept for --intent flows): routes through
    the same job machinery so progress accounting stays uniform."""
    progress = load_progress()
    phrases = PHRASES.get(intent, [])
    if not phrases:
        print(f"  [ERROR] No phrases defined for intent '{intent}'")
        return 0
    jobs = [(intent, random.choice(phrases)) for _ in range(count)]
    return collect_jobs(jobs, progress, text_only=text_only)


# ─── Main ──────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="NEXUS NLU — Interactive Voice Sample Collector"
    )
    parser.add_argument("-i", "--intent", type=str, default=None,
                        help="Collect for a specific intent only (e.g. --intent create_pr or --category github --intent create_pr)")
    parser.add_argument("-c", "--category", type=str, default=None,
                        help="Collect for a category: github, mcp, apps, messages, live, media, random (or 1-7)")
    parser.add_argument("--count", type=int, default=10,
                        help="Number of samples per intent (default: 10)")
    parser.add_argument("--list", action="store_true",
                        help="List all available intents and exit")
    parser.add_argument("--text-only", action="store_true",
                        help="Type phrases instead of speaking (no microphone)")
    parser.add_argument("--yes", "-y", action="store_true",
                        help="Skip confirmation prompts (non-interactive mode)")
    parser.add_argument("--status", action="store_true",
                        help="Show per-tier coverage and exit")
    args = parser.parse_args()

    progress = load_progress()

    if args.list:
        print("Available intents:")
        for intent, phrases in sorted(PHRASES.items()):
            print(f"  {intent:28s}  ({len(phrases)} phrases)")
        print(f"\nTotal: {len(PHRASES)} intents")
        print("\nCategories (--category <name>):")
        for i, (name, desc, _tiers) in enumerate(CATEGORY_MENU, start=1):
            print(f"  {i}. {name:10s} {desc}")
        return

    if args.status:
        print_status(progress, per_intent=args.count)
        print(f"\n  Output file rows: {load_existing_count()}")
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

        print(f"\n  Recording: adaptive per phrase (~140 wpm + margin, 2-10s), {SAMPLE_RATE}Hz mono")
        print(f"  Mode: CONTINUOUS (no Enter needed — just speak when prompted)")
        print(f"  Countdown: none — mic starts the moment you press Enter (adaptive per phrase)")
    else:
        print(f"\n  Text mode — phrases saved directly (no recording).")

    print(f"  Output: {OUTPUT_PATH}")
    print(f"  Existing samples: {load_existing_count()}")
    print(f"  Samples per intent: {args.count}")

    # Determine which intents to collect.
    if args.category and args.intent:
        choice = args.category.strip().lower()
        try:
            cat_intents = category_intents(choice)
        except KeyError:
            print(f"\n  [ERROR] Unknown category: {args.category}")
            print(f"  Use --list to see categories.")
            return
        target_intent = args.intent.strip().lower()
        if target_intent not in PHRASES:
            print(f"\n  [ERROR] Unknown intent: {target_intent}")
            print(f"  Use --list to see available intents.")
            return
        if cat_intents is not None and target_intent not in cat_intents:
            print(f"\n  [ERROR] Intent '{target_intent}' is not in category '{choice}'.")
            print(f"  Available intents in '{choice}': {', '.join(sorted(cat_intents))}")
            return
        intents = [target_intent]
        print(f"  Scope: category '{choice}' -> intent '{target_intent}'")
    elif args.intent:
        target_intent = args.intent.strip().lower()
        if target_intent not in PHRASES:
            print(f"\n  [ERROR] Unknown intent: {target_intent}")
            print(f"  Use --list to see available intents.")
            return
        cat_name = "general"
        for name, _desc, tiers in CATEGORY_MENU:
            if tiers:
                for t in tiers:
                    if target_intent in TIERS.get(t, []):
                        cat_name = name
                        break
        intents = [target_intent]
        print(f"  Scope: single intent '{target_intent}' (category: {cat_name})")
    elif args.category:
        choice = args.category.strip().lower()
        try:
            resolved = category_intents(choice)
        except KeyError:
            print(f"\n  [ERROR] Unknown category: {args.category}")
            print(f"  Use --list to see categories.")
            return
        intents = resolved if resolved is not None else [i for i in PHRASES.keys()]
        print(f"  Scope: category '{choice}' ({len(intents)} intents)")
    elif sys.stdin.isatty() and not args.yes:
        print("\n  What do you want to train?")
        for i, (name, desc, _tiers) in enumerate(CATEGORY_MENU, start=1):
            print(f"    {i}. {name:10s} {desc}")
        try:
            pick = input(f"  Pick [1-{len(CATEGORY_MENU)}] (Enter = random mix): ").strip().lower()
        except (EOFError, KeyboardInterrupt):
            print("\n  Cancelled.")
            return
        if not pick:
            pick = "random"
        try:
            resolved = category_intents(pick)
        except KeyError:
            print(f"  [ERROR] Unknown choice: {pick}")
            return
        if resolved is None:
            intents = [i for i in PHRASES.keys()]
            print(f"  Scope: random mix across all ({len(intents)} intents)")
        else:
            scope_name = next((n for n, _d, _t in CATEGORY_MENU if n == pick or str(_t) == pick), pick)
            print(f"\n  Category '{scope_name}' contains {len(resolved)} intents:")
            for idx, it in enumerate(resolved, start=1):
                print(f"    {idx:2d}. {it}")
            try:
                sub_pick = input(f"  Train all {scope_name} intents (Enter), or pick specific [1-{len(resolved)} / name]: ").strip().lower()
            except (EOFError, KeyboardInterrupt):
                print("\n  Cancelled.")
                return
            if not sub_pick:
                intents = resolved
                print(f"  Scope: full category '{scope_name}' ({len(intents)} intents)")
            else:
                chosen_intent = None
                if sub_pick.isdigit() and 1 <= int(sub_pick) <= len(resolved):
                    chosen_intent = resolved[int(sub_pick) - 1]
                elif sub_pick in resolved:
                    chosen_intent = sub_pick
                elif sub_pick in PHRASES:
                    chosen_intent = sub_pick

                if chosen_intent:
                    intents = [chosen_intent]
                    print(f"  Scope: category '{scope_name}' -> intent '{chosen_intent}'")
                else:
                    print(f"  [ERROR] Unknown intent: {sub_pick}")
                    return
    else:
        intents = [i for i in PHRASES.keys()]

    # Stratified job queue: tiers round-robin, least-covered first.
    # Quitting early still leaves every tier covered — resume later and
    # the scheduler picks up exactly at the gaps (progress file).
    jobs = build_job_queue(intents, args.count, progress)
    print(f"  Intents in scope: {len(intents)}")
    print(f"  Jobs queued (quota {args.count}/intent minus accepted): {len(jobs)}")
    print_status(progress, per_intent=args.count)
    print(f"\n  Press Ctrl+C at any time to stop. Progress is saved per sample;")
    print(f"  resume any time and the weakest intents come first.")
    print(f"  After stopping, run: nexus train")

    time.sleep(2)  # brief pause before starting


    progress_before = sum(progress.values())
    total_collected = 0
    try:
        total_collected = collect_jobs(jobs, progress,
                                       text_only=args.text_only)
    except KeyboardInterrupt:
        # collect_jobs was interrupted mid-sample (Ctrl+C or q): the return
        # value is lost, so recompute from progress (saved per sample).
        total_collected = sum(progress.values()) - progress_before
        print(f"\n\n{'=' * 60}")
        print(f"  Stopped by user — progress kept ({total_collected} this run)")
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
