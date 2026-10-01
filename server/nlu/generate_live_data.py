#!/usr/bin/env python3
"""
NEXUS NLU — Generate live-mode training examples for BERT-Mini.

Generates training data for the 11 new live-mode intents:
  type_text, press_key, press_hotkey, confirm_send, cancel_action,
  browser_new_tab, browser_navigate, browser_search,
  whatsapp_open, whatsapp_search, focus_app

Also generates additional examples for existing intents that need
more coverage (open_app, close_app, search, etc.).

Output: new_examples.json (merged into dataset.json by merge_and_train.py)

Usage:
  cd server/nlu
  python generate_live_data.py
  python merge_and_train.py --new-data new_examples.json
  python train.py
"""

import json
import random
import os
from pathlib import Path

random.seed(42)

OUTPUT_PATH = Path(__file__).parent / "new_examples.json"

# ─── Vocabulary ────────────────────────────────────────────────────────────

APPS = [
    "notepad", "chrome", "brave", "firefox", "vscode", "visual studio code",
    "terminal", "powershell", "file explorer", "calculator", "spotify",
    "whatsapp", "discord", "slack", "zoom", "teams", "outlook", "word",
    "excel", "obsidian", "github desktop", "paint", "snipping tool",
    "edge", "safari", "notion", "figma", "photoshop", "illustrator",
    "blender", "unity", "unreal engine", "steam", "epic games",
    "task manager", "registry editor", "command prompt", "git bash",
    "android studio", "xcode", "vim", "emacs", "sublime text",
    "intellij idea", "pycharm", "webstorm", "rust rover", "cursor",
    "after effects", "premiere pro", "audacity", "obs studio",
    "torrent", "qbittorrent", "utorrent", "vlc", "kodi",
    "settings", "control panel", "clock", "calendar", "mail",
    "photos", "camera", "maps", "weather", "news",
    "store", "app store", "play store",
]

KEYS = [
    "enter", "escape", "tab", "space", "backspace", "delete",
    "up", "down", "left", "right", "home", "end",
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "page up", "page down", "insert", "print screen", "scroll lock", "pause",
    "a", "b", "c", "d", "e", "enter", "space",
]

HOTKEYS = [
    ["ctrl", "a"], ["ctrl", "c"], ["ctrl", "v"], ["ctrl", "x"],
    ["ctrl", "z"], ["ctrl", "y"], ["ctrl", "s"], ["ctrl", "f"],
    ["ctrl", "t"], ["ctrl", "w"], ["ctrl", "tab"],
    ["ctrl", "shift", "tab"], ["alt", "f4"],
    ["ctrl", "shift", "n"], ["win", "d"],
    ["ctrl", "shift", "esc"], ["ctrl", "alt", "del"],
    ["shift", "f5"], ["ctrl", "shift", "t"],
    ["ctrl", "shift", "o"], ["ctrl", "n"], ["ctrl", "o"],
    ["ctrl", "p"], ["ctrl", "q"], ["ctrl", "r"],
    ["ctrl", "plus"], ["ctrl", "minus"], ["ctrl", "0"],
    ["ctrl", "shift", "z"], ["ctrl", "shift", "s"],
    ["ctrl", "shift", "c"], ["ctrl", "shift", "v"],
    ["alt", "tab"], ["alt", "enter"], ["alt", "space"],
    ["win", "e"], ["win", "r"], ["win", "i"], ["win", "l"],
    ["win", "tab"], ["win", "left"], ["win", "right"],
    ["ctrl", "win", "d"], ["ctrl", "win", "f4"],
    ["shift", "tab"], ["ctrl", "alt", "tab"],
    ["ctrl", "shift", "enter"], ["ctrl", "enter"],
    ["ctrl", "shift", "w"], ["ctrl", "shift", "p"],
    ["ctrl", "alt", "s"], ["ctrl", "shift", "f"],
    ["ctrl", "k"], ["ctrl", "l"], ["ctrl", "j"],
    ["ctrl", "u"], ["ctrl", "i"], ["ctrl", "b"],
    ["ctrl", "shift", "7"], ["ctrl", "shift", "5"],
]

TEXTS = [
    "hello world", "hi what are u doing", "hey mummy", "good morning",
    "i'll be there in 5 minutes", "thanks a lot", "see you tomorrow",
    "happy birthday", "call you back", "running late", "on my way",
    "let's meet at 3pm", "the quick brown fox", "nexus is the best",
    "i love coding", "what time is it", "how are you", "good night",
    "talk later", "please review this", "hey can we talk", "sure thing",
    "just got home", "where are you", "miss you", "love you",
    "get well soon", "congratulations", "well done", "no worries",
    "happy new year", "merry christmas", "happy diwali",
    "see you soon", "take care", "have a good day", "sweet dreams",
    "good afternoon", "good evening", "welcome back",
    "i'm sorry", "my bad", "no problem", "don't worry about it",
    "let me check", "i'll get back to you", "sounds good",
    "can we reschedule", "i'm available", "let me think about it",
    "that's awesome", "great job", "keep it up", "proud of you",
    "thinking of you", "check this out", "you won't believe it",
    "lol", "haha", "omg", "wtf", "btw", "fyi", "asap", "tbh",
    "the meeting is at 2pm", "deadline is tomorrow",
    "please send the report", "attached is the document",
    "thanks for your help", "could you help me", "i need a favor",
    "happy anniversary", "congrats on the new job",
    "see you at the party", "bring some snacks",
    "the password is hunter2", "don't forget the meeting",
    "reminder pay the bills", "pick up milk on the way home",
    "the project is done", "code review please",
    "fix the bug in line 42", "deploy to production",
    "merge the pull request", "close the issue",
    "update the documentation", "refactor the function",
    "the build is failing", "tests are passing",
    "can you review my code", "i found a bug",
    "this feature is ready", "ship it",
    "rollback the deployment", "the server is down",
    "restart the service", "check the logs",
    "cpu usage is high", "memory leak detected",
    "the api is slow", "timeout error",
    "database connection failed", "retry the request",
    "clear the cache", "flush the queue",
    "the migration is complete", "backup is done",
    "restore from backup", "the index is rebuilt",
    "deploy the hotfix", "the patch is ready",
    "version 2.0 is live", "changelog is updated",
]

CONTACTS = [
    "mom", "dad", "mummy", "lakshya", "john", "sarah", "prem",
    "eesha", "mike", "anna", "boss", "team", "bro", "sis",
    "grandma", "grandpa", "uncle", "aunt",
    "priya", "rahul", "arjun", "kavya", "rohit", "neha",
    "alex", "emma", "liam", "olivia", "noah", "ava",
    "sophie", "luca", "mia", "leo", "ella", "max",
    "david", "james", "maria", "carlos", "yuki", "chen",
    "the group", "family", "work group", "project team",
    "client", "lawyer", "doctor", "landlord", "tenant",
]

URLS = [
    "github.com", "google.com", "youtube.com", "wikipedia.org",
    "stackoverflow.com", "reddit.com", "twitter.com", "linkedin.com",
    "gmail.com", "amazon.com", "netflix.com", "spotify.com",
    "docs.rs", "crates.io", "arxiv.org",
    "huggingface.co", "kaggle.com", "leetcode.com",
    "hackerrank.com", "codepen.io", "jsfiddle.net",
    "replit.com", "codesandbox.io", "vercel.com",
    "netlify.com", "cloudflare.com", "aws.amazon.com",
    "azure.microsoft.com", "cloud.google.com", "digitalocean.com",
    "figma.com", "notion.so", "linear.app", "jira.com",
    "trello.com", "asana.com", "slack.com", "discord.com",
    "zoom.us", "teams.microsoft.com", "meet.google.com",
    "openai.com", "anthropic.com", "mistral.ai", "groq.com",
    "developer.mozilla.org", "w3schools.com", "freecodecamp.org",
    "udemy.com", "coursera.org", "edx.org", "khanacademy.org",
    "medium.com", "dev.to", "hashnode.com",
    "news.ycombinator.com", "producthunt.com",
    "npmjs.com", "pypi.org", "rubygems.org", "maven.org",
    "docker.com", "kubernetes.io", "prometheus.io",
    "grafana.com", "elastic.co", "splunk.com",
]

SEARCH_QUERIES = [
    "cats", "rust programming", "how to tie a tie", "weather today",
    "best restaurants near me", "what is quantum computing",
    "python tutorial", "machine learning basics", "world cup 2026",
    "stock market today", "news headlines", "movie showtimes",
    "flights to london", "hotel deals", "recipe for pasta",
    "how to learn guitar", "best laptops 2026", "iphone 17 review",
    "python vs rust", "react vs vue", "docker vs kubernetes",
    "how to fix blue screen", "windows 11 tips", "mac shortcuts",
    "linux commands cheat sheet", "git rebase tutorial",
    "sql join types", "regex tester", "css flexbox guide",
    "typescript generics", "async await explained",
    "binary search algorithm", "quick sort vs merge sort",
    "rest api design", "graphql vs rest", "websocket tutorial",
    "redis cache", "postgresql vs mysql", "mongodb tutorial",
    "nginx config", "ssl certificate", "https redirect",
    "ci cd pipeline", "github actions", "docker compose",
    "terraform basics", "ansible playbook", "linux bash script",
    "ai news", "chatgpt alternatives", "stable diffusion guide",
    "midjourney prompts", "dalle 3 vs midjourney",
    "crypto prices", "bitcoin halving", "ethereum gas fees",
    "nft marketplace", "web3 tutorial", "solidity smart contract",
    "game development", "unity vs unreal", "godot engine",
    "blender tutorial", "photoshop shortcuts", "video editing tips",
    "music production", "audacity noise reduction",
    "how to cook rice", "best pizza recipe", "gym workout plan",
    "yoga for beginners", "meditation guide", "sleep better tips",
    "productivity apps", "time management", "habit tracking",
    "book recommendations", "best movies 2026", "tv shows to binge",
    "travel destinations", "cheap flights", "visa requirements",
    "passport renewal", "tax filing", "insurance comparison",
    "car maintenance", "driving test tips", "parking near me",
    "electrician near me", "plumber near me", "doctor appointment",
    "covid symptoms", "flu remedies", "headache relief",
    "back pain exercises", "posture correction", "eye strain relief",
]

# ─── Phrasing Templates ────────────────────────────────────────────────────

def gen_type_text():
    """Generate type_text examples."""
    examples = []
    patterns = [
        "type {text}",
        "type in {text}",
        "type out {text}",
        "write {text}",
        "write out {text}",
        "write in {text}",
        "enter {text}",
        "input {text}",
        "insert {text}",
        "put in {text}",
        "type this {text}",
        "type that {text}",
        "type {text} for me",
        "write {text} for me",
        "please type {text}",
        "can you type {text}",
        "so type {text}",
        "and type {text}",
        "now type {text}",
        "typed {text}",           # STT mishearing
        "right {text}",            # STT: write→right
        "i want to type {text}",
        "i want to write {text}",
        "let me type {text}",
        "let me write {text}",
        "start typing {text}",
        "start writing {text}",
        "go ahead and type {text}",
        "go ahead and write {text}",
        "type {text} now",
        "type {text} please",
        "type {text} right now",
        "i need to type {text}",
        "i need to write {text}",
        "just type {text}",
        "just write {text}",
        "simply type {text}",
        "simply write {text}",
        "type the following {text}",
        "type the text {text}",
        "write the text {text}",
        "type the message {text}",
        "write the message {text}",
        "type the word {text}",
        "type the words {text}",
        "dictate {text}",
        "transcribe {text}",
        "key in {text}",
        "punch in {text}",
        "fill in {text}",
        "fill out {text}",
        "type {text} into the box",
        "type {text} in the field",
        "type {text} in the chat",
        "type {text} in the message",
        "type {text} in the search",
        "type {text} in the url bar",
        "type {text} in the address bar",
        "type {text} in the terminal",
        "type {text} in the editor",
        "type {text} in notepad",
        "type {text} in vscode",
        "type {text} in word",
        "type {text} in the document",
        "type {text} in the file",
        "type {text} in the input",
        "type {text} in the form",
        "type {text} in the box",
        # STT distortions
        "tiepe {text}",            # STT: type→tiepe
        "tight {text}",            # STT: type→tight
        "tap {text}",              # STT: type→tap (short)
        "right in {text}",         # STT: write→right
        "wright {text}",           # STT: write→wright
        "rite {text}",             # STT: write→rite
        "inter {text}",            # STT: enter→inter
        "inset {text}",            # STT: insert→inset
        "tiped {text}",            # STT: typed→tiped
        # Filler words
        "um type {text}",
        "uh type {text}",
        "like type {text}",
        "so um type {text}",
        "hey nexus type {text}",
        "nexus type {text}",
        "ok type {text}",
        "okay type {text}",
        "well type {text}",
        "hmm type {text}",
        "actually type {text}",
        "just um type {text}",
    ]
    for text in TEXTS:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(text=text),
                "intent": "type_text",
                "slots": {"text": text},
            })
    return examples


def gen_press_key():
    """Generate press_key examples."""
    examples = []
    patterns = [
        "press {key}",
        "hit {key}",
        "tap {key}",
        "strike {key}",
        "press the {key}",
        "hit the {key}",
        "tap the {key}",
        "send {key}",
        "press {key} please",
        "can you press {key}",
        "so press {key}",
        "and press {key}",
        "now press {key}",
        "pasture {key}",           # STT: press→pasture
        "pressed {key}",           # STT past tense
        "press {key} now",
        "press {key} key",
        "press the {key} key",
        "hit the {key} key",
        "tap the {key} key",
        "press {key} for me",
        "please press {key}",
        "go ahead and press {key}",
        "i need to press {key}",
        "just press {key}",
        "simply press {key}",
        "press {key} on the keyboard",
        "press {key} on keyboard",
        "press the {key} button",
        "hit the {key} button",
        "tap the {key} button",
        "press {key} button",
        "send the {key} key",
        "trigger {key}",
        "activate {key}",
        "press {key} once",
        "press {key} twice",
        "press {key} three times",
        "double press {key}",
        "press {key} and enter",
        "press {key} then enter",
        # STT distortions
        "best {key}",              # STT: press→best
        "passed {key}",            # STT: press→passed
        "past your {key}",         # STT: press→pasture
        "perish {key}",            # STT: press→perish
        "pest {key}",              # STT: press→pest
        "pierce {key}",            # STT: press→pierce
        # Filler words
        "um press {key}",
        "uh press {key}",
        "hey nexus press {key}",
        "nexus press {key}",
        "ok press {key}",
        "okay press {key}",
        "like press {key}",
        "well press {key}",
        "hmm press {key}",
        "actually press {key}",
    ]
    for key in KEYS:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(key=key),
                "intent": "press_key",
                "slots": {"key": key},
            })
    return examples


def gen_press_hotkey():
    """Generate press_hotkey examples."""
    examples = []
    patterns = [
        "press {keys}",
        "hit {keys}",
        "press {keys_plus}",
        "press {keys_and}",
        "do {keys}",
        "send {keys}",
        "press {keys} please",
        "so press {keys}",
        "and press {keys}",
        "press {keys} now",
        "press {keys} together",
        "press {keys} at the same time",
        "press {keys} simultaneously",
        "press {keys_plus} together",
        "press {keys_and} at the same time",
        "hit {keys_plus}",
        "hit {keys_and}",
        "tap {keys}",
        "tap {keys_plus}",
        "press the {keys} combo",
        "press the {keys} combination",
        "press the {keys_plus} combo",
        "press {keys} shortcut",
        "press {keys} hotkey",
        "press {keys} hotkeys",
        "press the {keys} shortcut",
        "press the {keys} hotkey",
        "press {keys} keys together",
        "press {keys} keys at once",
        "press {keys} keys simultaneously",
        "do the {keys} combo",
        "do the {keys_plus} combo",
        "send {keys_plus}",
        "send {keys_and}",
        "trigger {keys}",
        "trigger {keys_plus}",
        "activate {keys}",
        "activate {keys_plus}",
        "press {keys} for me",
        "please press {keys}",
        "can you press {keys}",
        "go ahead and press {keys}",
        "i need to press {keys}",
        "just press {keys}",
        "press {keys} on the keyboard",
        "press {keys} on keyboard",
        # STT distortions
        "best {keys}",             # STT: press→best
        "passed {keys}",           # STT: press→passed
        "past your {keys}",         # STT: press→pasture
        # Filler words
        "um press {keys}",
        "hey nexus press {keys}",
        "nexus press {keys}",
        "ok press {keys}",
        "okay press {keys}",
    ]
    for combo in HOTKEYS:
        keys_str = " ".join(combo)
        keys_plus = " plus ".join(combo)
        keys_and = " and ".join(combo)
        for pattern in patterns:
            examples.append({
                "text": pattern.format(
                    keys=keys_str,
                    keys_plus=keys_plus,
                    keys_and=keys_and,
                ),
                "intent": "press_hotkey",
                "slots": {"keys": combo},
            })
    return examples


def gen_confirm_send():
    """Generate confirm_send examples."""
    examples = []
    texts = [
        "send", "send it", "send message", "send the message",
        "send now", "send it now", "go ahead and send",
        "please send", "send please", "go ahead please",
        "ship it", "fire it off", "do it", "confirm", "yes send",
        "hit enter to send", "just send it", "send it please",
        "go ahead", "confirm send", "yes go ahead", "okay send",
        "send it for me", "please go ahead", "yes confirm",
        "send that", "send that message", "send that now",
        "send the text", "send the text now", "send the text please",
        "yes", "yep", "yeah", "sure", "ok", "okay", "go for it",
        "do it now", "do it please", "do it for me",
        "confirm it", "confirm that", "confirm please",
        "approve", "approve it", "approved",
        "submit", "submit it", "submit now",
        "fire away", "fire it", "let it rip", "let her rip",
        "send away", "send it away", "send it out",
        "post it", "post it now", "post the message",
        "publish it", "publish the message",
        "deliver it", "deliver the message",
        "transmit it", "transmit the message",
        "dispatch it", "dispatch the message",
        "execute", "execute it", "execute send",
        "proceed", "proceed with send", "proceed please",
        "continue", "continue with send",
        "go", "go now", "go send", "go send it",
        "done", "done send", "make it so",
        "affirmative", "roger that", "copy that",
        "ten four", "acknowledge", "acknowledged",
        # STT distortions
        "sand it",                 # STT: send→sand
        "sent it",                 # STT: send→sent (past tense)
        "cent it",                 # STT: send→cent
        "sinned",                  # STT: send→sinned
        "go ahead and sand",       # STT
        "go head",                 # STT: ahead→head
        # Filler words
        "um send it", "uh send", "like send it",
        "hey nexus send it", "nexus send",
        "ok send it", "okay go ahead",
    ]
    for text in texts:
        examples.append({
            "text": text,
            "intent": "confirm_send",
            "slots": {},
        })
    return examples


def gen_cancel_action():
    """Generate cancel_action examples."""
    examples = []
    texts = [
        "stop", "cancel that", "stop that",
        "don't send", "don't do it", "don't send it", "wait don't",
        "wait", "hold on", "wait a second", "hold on a minute",
        "stoop",                     # STT: stop→stoop
        "scratch that", "abort", "nevermind",  # casual
        "cancel", "cancel it", "cancel everything",
        "cancel that please", "please cancel",
        "never mind", "never mind that",
        "forget it", "forget that", "forget about it",
        "no", "nope", "nah", "no way", "not now",
        "don't", "do not", "do not send",
        "stop it", "stop now", "stop please",
        "halt", "halt it", "halt now",
        "abort it", "abort now", "abort please",
        "terminate", "terminate it", "terminate now",
        "discontinue", "discontinue that",
        "cease", "cease and desist",
        "cut it", "cut it out", "cut that out",
        "knock it off", "knock it off now",
        "quit it", "quit that", "quit now",
        "back off", "back away",
        "hold up", "hold everything",
        "wait a minute", "wait a moment", "wait a bit",
        "hold on a second", "hold on a moment",
        "give me a second", "give me a minute",
        "not yet", "not now", "not right now",
        "hold your horses", "hold tight",
        "pause it", "pause that", "pause everything",
        "undo it", "undo that", "undo the last",
        "revert it", "revert that",
        "rollback", "rollback that",
        "stop sending", "stop typing", "stop everything",
        "don't type that", "don't press that",
        "not that", "anything but that",
        "no don't", "no wait", "no stop",
        "actually no", "actually cancel", "actually stop",
        "on second thought", "on second thought no",
        "changed my mind", "i changed my mind",
        # STT distortions
        "stomp",                    # STT: stop→stomp
        "stow",                     # STT: stop→stow
        "stoop it",                 # STT
        "cancel that for real",
        "scratch", "scratch all that",
        "nix it", "nix that", "veto",
        "belay that", "belay",
        "stand down", "stand by",
        # Filler words
        "um stop", "uh cancel", "like stop",
        "hey nexus stop", "nexus cancel",
        "ok stop", "okay wait",
    ]
    for text in texts:
        examples.append({
            "text": text,
            "intent": "cancel_action",
            "slots": {},
        })
    return examples


def gen_browser_new_tab():
    """Generate browser_new_tab examples."""
    examples = []
    texts = [
        "new tab", "open new tab", "open a new tab", "new browser tab",
        "open a new tab please", "can you open a new tab",
        "tab", "another tab", "one more tab", "start a new tab",
        "new window", "open new window", "open a new window",
        "new tap",                   # STT: tab→tap
        "open new tap",              # STT
        "create new tab", "make a new tab",
        "open another tab", "open one more tab",
        "new browser window", "open a new browser window",
        "open a fresh tab", "open a fresh window",
        "spawn new tab", "spawn a new tab",
        "launch new tab", "launch a new tab",
        "start new tab", "start a new tab",
        "begin new tab", "begin a new tab",
        "add a new tab", "add new tab",
        "give me a new tab", "give me a new window",
        "i need a new tab", "i need a new window",
        "let's open a new tab", "let's open a new window",
        "open a new tab for me", "open a new window for me",
        "please open a new tab", "please open a new window",
        "can you open a new window", "could you open a new tab",
        "open a new tab now", "open a new window now",
        "just open a new tab", "just open a new window",
        "open a blank tab", "open a blank window",
        "open a private tab", "open a private window",
        "open an incognito tab", "open an incognito window",
        "new incognito tab", "new private window",
        "open a new incognito", "open a new private",
        "new browser session", "open a new session",
        "start a new session",
        # STT distortions
        "new tad",                  # STT: tab→tad
        "new tan",                  # STT: tab→tan
        "open new tad",             # STT
        "open a new tad",           # STT
        "new windo",                # STT: window→windo
        "open new windo",           # STT
        # Filler words
        "um new tab", "uh open a new tab",
        "hey nexus new tab", "nexus new tab",
        "ok new tab", "okay open a new tab",
        "so open a new tab", "and open a new tab",
        "now open a new tab", "like open a new tab",
    ]
    for text in texts:
        examples.append({
            "text": text,
            "intent": "browser_new_tab",
            "slots": {},
        })
    return examples


def gen_browser_navigate():
    """Generate browser_navigate examples."""
    examples = []
    patterns = [
        "go to {url}",
        "navigate to {url}",
        "browse to {url}",
        "visit {url}",
        "take me to {url}",
        "jump to {url}",
        "head to {url}",
        "point to {url}",
        "open {url} in this tab",
        "open {url} here",
        "open {url}",
        "open the site {url}",
        "open the website {url}",
        "open the page {url}",
        "go to the site {url}",
        "go to the website {url}",
        "go to the page {url}",
        "navigate to the site {url}",
        "navigate to the website {url}",
        "navigate to the page {url}",
        "browse to the site {url}",
        "browse to the website {url}",
        "visit the site {url}",
        "visit the website {url}",
        "visit the page {url}",
        "take me to the site {url}",
        "take me to the website {url}",
        "take me to the page {url}",
        "head over to {url}",
        "head on over to {url}",
        "make your way to {url}",
        "find your way to {url}",
        "direct me to {url}",
        "direct me to the site {url}",
        "direct me to the website {url}",
        "bring me to {url}",
        "bring me to the site {url}",
        "bring me to the website {url}",
        "load {url}",
        "load the page {url}",
        "load the site {url}",
        "load the website {url}",
        "pull up {url}",
        "pull up the site {url}",
        "pull up the website {url}",
        "pull up the page {url}",
        "show me {url}",
        "show me the site {url}",
        "show me the website {url}",
        "show me the page {url}",
        "display {url}",
        "display the site {url}",
        "display the website {url}",
        "render {url}",
        "render the page {url}",
        "fetch {url}",
        "fetch the page {url}",
        "fetch the site {url}",
        "request {url}",
        "request the page {url}",
        "connect to {url}",
        "connect to the site {url}",
        "connect to the website {url}",
        "link to {url}",
        "link me to {url}",
        "route me to {url}",
        "redirect me to {url}",
        "send me to {url}",
        "transport me to {url}",
        "travel to {url}",
        "go to {url} please",
        "go to {url} now",
        "go to {url} for me",
        "please go to {url}",
        "can you go to {url}",
        "could you go to {url}",
        "would you go to {url}",
        "just go to {url}",
        "simply go to {url}",
        "go to {url} in the browser",
        "go to {url} in this browser",
        "go to {url} in the current tab",
        "go to {url} in the active tab",
        "go to {url} in chrome",
        "go to {url} in firefox",
        "go to {url} in edge",
        "go to {url} in brave",
        "navigate to {url} in the browser",
        "navigate to {url} in chrome",
        "open {url} in the browser",
        "open {url} in chrome",
        "open {url} in firefox",
        "open {url} in edge",
        "open {url} in brave",
        "open {url} in a new tab",
        "open {url} in a new window",
        # STT distortions
        "go too {url}",             # STT: to→too
        "go to {url} dot com",       # STT adds dot com
        "navigate too {url}",        # STT
        # Filler words
        "um go to {url}", "uh go to {url}",
        "hey nexus go to {url}", "nexus go to {url}",
        "ok go to {url}", "okay go to {url}",
        "so go to {url}", "and go to {url}",
        "now go to {url}", "like go to {url}",
        "actually go to {url}", "hmm go to {url}",
    ]
    for url in URLS:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(url=url),
                "intent": "browser_navigate",
                "slots": {"url": url},
            })
    return examples


def gen_browser_search():
    """Generate browser_search examples."""
    examples = []
    patterns = [
        "search {query} in browser",
        "search for {query} in browser",
        "google {query} in browser",
        "search {query} in this tab",
        "look up {query} here",
        "find {query} on the web",
        "search {query} online",
        "google {query} here",
        "search for {query} on the web",
        "search for {query} online",
        "search for {query} on google",
        "search for {query} on the internet",
        "search for {query} on the net",
        "search for {query} on the web please",
        "google {query}",
        "google for {query}",
        "google search {query}",
        "google search for {query}",
        "search the web for {query}",
        "search the internet for {query}",
        "search the net for {query}",
        "search online for {query}",
        "search google for {query}",
        "search bing for {query}",
        "search duckduckgo for {query}",
        "search yahoo for {query}",
        "search youtube for {query}",
        "search stack overflow for {query}",
        "search stackoverflow for {query}",
        "search github for {query}",
        "search reddit for {query}",
        "search wikipedia for {query}",
        "search amazon for {query}",
        "search the web for {query} please",
        "search for {query} please",
        "can you search for {query}",
        "could you search for {query}",
        "please search for {query}",
        "search for {query} now",
        "search for {query} for me",
        "just search for {query}",
        "simply search for {query}",
        "go ahead and search for {query}",
        "i want to search for {query}",
        "i need to search for {query}",
        "let me search for {query}",
        "let's search for {query}",
        "help me search for {query}",
        "find {query} on google",
        "find {query} on the internet",
        "find {query} on the web",
        "find {query} online",
        "find me {query} on the web",
        "find me {query} online",
        "find me {query} on google",
        "look up {query} on google",
        "look up {query} on the web",
        "look up {query} on the internet",
        "look up {query} online",
        "look {query} up",
        "look {query} up on google",
        "look {query} up on the web",
        "look {query} up online",
        "google what is {query}",
        "google how to {query}",
        "google why is {query}",
        "search what is {query}",
        "search how to {query}",
        "search why is {query}",
        "bing {query}",
        "bing search {query}",
        "duckduckgo {query}",
        "yahoo {query}",
        "search {query} in chrome",
        "search {query} in firefox",
        "search {query} in edge",
        "search {query} in brave",
        "search for {query} in chrome",
        "search for {query} in firefox",
        "search for {query} in edge",
        "search for {query} in brave",
        "google {query} in chrome",
        "google {query} in firefox",
        "google {query} in edge",
        "google {query} in brave",
        # STT distortions
        "search four {query}",        # STT: for→four
        "look app {query}",           # STT: up→app
        "search for {query} on the web",
        "goggle {query}",             # STT: google→goggle
        "goggle search {query}",      # STT
        "search for {query} on the internet",
        # Filler words
        "um search for {query}", "uh search for {query}",
        "hey nexus search for {query}", "nexus search for {query}",
        "ok search for {query}", "okay search for {query}",
        "so search for {query}", "and search for {query}",
        "now search for {query}", "like search for {query}",
        "actually search for {query}", "hmm search for {query}",
    ]
    for query in SEARCH_QUERIES:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(query=query),
                "intent": "browser_search",
                "slots": {"query": query},
            })
    return examples


def gen_whatsapp_open():
    """Generate whatsapp_open examples."""
    examples = []
    texts = [
        "open whatsapp", "launch whatsapp", "start whatsapp",
        "whatsapp", "bring up whatsapp", "show whatsapp",
        "open whatsapp please", "can you open whatsapp",
        "open what's app",           # STT
        "open what sap",             # STT
        "open whats app",            # STT
        "go to whatsapp", "take me to whatsapp",
        "fire up whatsapp", "pull up whatsapp",
        "open the whatsapp", "open whatsapp app",
        "switch to whatsapp", "focus whatsapp",
        "bring whatsapp to front",
        "open whatsapp desktop",
        "open whatsapp web",
        "whatsapp desktop", "whatsapp web",
        "show me whatsapp", "show me the whatsapp",
        "display whatsapp", "display the whatsapp",
        "load whatsapp", "load the whatsapp",
        "start whatsapp please", "please open whatsapp",
        "could you open whatsapp", "would you open whatsapp",
        "just open whatsapp", "simply open whatsapp",
        "open whatsapp for me", "open whatsapp now",
        "open whatsapp right now",
        "i need whatsapp", "i want whatsapp",
        "let me see whatsapp", "let's open whatsapp",
        "help me open whatsapp",
        "activate whatsapp", "activate the whatsapp",
        "restore whatsapp", "restore the whatsapp",
        "resume whatsapp", "resume the whatsapp",
        "return to whatsapp", "return to the whatsapp",
        "get back to whatsapp", "get back to the whatsapp",
        "go back to whatsapp", "go back to the whatsapp",
        "back to whatsapp", "back to the whatsapp",
        "switch over to whatsapp",
        "switch to whatsapp please",
        "switch to whatsapp now",
        "bring whatsapp up", "bring up the whatsapp",
        "pop open whatsapp", "pop up whatsapp",
        "whatsapp please", "whatsapp now",
        "open the whatsapp app",
        "open the whatsapp application",
        "launch the whatsapp application",
        "start the whatsapp application",
        "open the whatsapp desktop app",
        "open the whatsapp desktop application",
        "open my whatsapp", "open my whatsapp app",
        "open my whatsapp desktop",
        "open my whatsapp web",
        # STT distortions
        "open whats up",             # STT
        "open what's up",            # STT
        "open what sap",             # STT
        "open what's app",           # STT
        "open whats app",            # STT
        "open what app",             # STT
        "open watts app",            # STT
        "open wattsap",              # STT
        "open watsap",               # STT
        "open watsapp",              # STT
        "open whatsap",              # STT
        "open whatsappp",            # STT
        "open whatsappp",            # STT
        "launch whats app",          # STT
        "start whats app",           # STT
        "go to whats app",           # STT
        "switch to whats app",       # STT
        "focus whats app",           # STT
        # Filler words
        "um open whatsapp", "uh open whatsapp",
        "hey nexus open whatsapp", "nexus open whatsapp",
        "ok open whatsapp", "okay open whatsapp",
        "so open whatsapp", "and open whatsapp",
        "now open whatsapp", "like open whatsapp",
        "actually open whatsapp", "hmm open whatsapp",
        "hey open whatsapp", "yo open whatsapp",
    ]
    for text in texts:
        examples.append({
            "text": text,
            "intent": "whatsapp_open",
            "slots": {},
        })
    return examples


def gen_whatsapp_search():
    """Generate whatsapp_search examples."""
    examples = []
    patterns = [
        "search for {contact} in whatsapp",
        "find {contact} in whatsapp",
        "open chat with {contact} in whatsapp",
        "chat with {contact} on whatsapp",
        "go to {contact} chat",
        "go to chat with {contact}",
        "find {contact}",
        "look for {contact}",
        "search {contact}",
        "search for {contact}",
        "find {contact} on whatsapp",
        "find {contact} in the whatsapp",
        "find {contact} in my whatsapp",
        "find {contact} in my contacts",
        "find {contact} in my chats",
        "find {contact} in the chats",
        "find {contact} in the chat list",
        "find {contact} in the chat",
        "find {contact} in whatsapp please",
        "find {contact} in whatsapp now",
        "find {contact} in whatsapp for me",
        "find {contact} in the whatsapp app",
        "find {contact} in the whatsapp application",
        "find {contact} in whatsapp desktop",
        "find {contact} in whatsapp web",
        "search {contact} in whatsapp",
        "search {contact} on whatsapp",
        "search {contact} in the whatsapp",
        "search {contact} in my whatsapp",
        "search {contact} in my contacts",
        "search {contact} in my chats",
        "search {contact} in the chats",
        "search {contact} in the chat list",
        "search {contact} in the chat",
        "search {contact} in whatsapp please",
        "search {contact} in whatsapp now",
        "search {contact} in whatsapp for me",
        "search for {contact} on whatsapp",
        "search for {contact} in the whatsapp",
        "search for {contact} in my whatsapp",
        "search for {contact} in my contacts",
        "search for {contact} in my chats",
        "search for {contact} in the chats",
        "search for {contact} in the chat list",
        "search for {contact} in the chat",
        "search for {contact} in whatsapp please",
        "search for {contact} in whatsapp now",
        "search for {contact} in whatsapp for me",
        "look for {contact} in whatsapp",
        "look for {contact} on whatsapp",
        "look for {contact} in the whatsapp",
        "look for {contact} in my whatsapp",
        "look for {contact} in my contacts",
        "look for {contact} in my chats",
        "look for {contact} in the chats",
        "look for {contact} in the chat list",
        "look for {contact} in the chat",
        "look for {contact} in whatsapp please",
        "look for {contact} in whatsapp now",
        "look for {contact} in whatsapp for me",
        "open chat with {contact}",
        "open chat with {contact} please",
        "open chat with {contact} now",
        "open chat with {contact} for me",
        "open chat with {contact} on whatsapp",
        "open chat with {contact} in whatsapp",
        "open the chat with {contact}",
        "open the chat with {contact} in whatsapp",
        "open the chat with {contact} on whatsapp",
        "start chat with {contact}",
        "start chat with {contact} in whatsapp",
        "start chat with {contact} on whatsapp",
        "start a chat with {contact}",
        "start a chat with {contact} in whatsapp",
        "start a chat with {contact} on whatsapp",
        "begin chat with {contact}",
        "begin chat with {contact} in whatsapp",
        "begin a chat with {contact}",
        "begin a chat with {contact} in whatsapp",
        "chat with {contact}",
        "chat with {contact} please",
        "chat with {contact} now",
        "chat with {contact} for me",
        "chat with {contact} in whatsapp",
        "chat with {contact} on whatsapp",
        "message {contact}",
        "message {contact} please",
        "message {contact} now",
        "message {contact} for me",
        "message {contact} in whatsapp",
        "message {contact} on whatsapp",
        "send a message to {contact}",
        "send a message to {contact} in whatsapp",
        "send a message to {contact} on whatsapp",
        "send message to {contact}",
        "send message to {contact} in whatsapp",
        "send message to {contact} on whatsapp",
        "text {contact}",
        "text {contact} please",
        "text {contact} now",
        "text {contact} for me",
        "text {contact} in whatsapp",
        "text {contact} on whatsapp",
        "go to {contact} in whatsapp",
        "go to {contact} on whatsapp",
        "go to {contact} in the whatsapp",
        "go to {contact} in my whatsapp",
        "go to {contact} chat please",
        "go to {contact} chat now",
        "go to {contact} chat for me",
        "go to the chat with {contact}",
        "go to the chat with {contact} in whatsapp",
        "go to the chat with {contact} on whatsapp",
        "navigate to {contact} chat",
        "navigate to {contact} in whatsapp",
        "navigate to {contact} on whatsapp",
        "navigate to the chat with {contact}",
        "navigate to the chat with {contact} in whatsapp",
        "navigate to the chat with {contact} on whatsapp",
        "jump to {contact} chat",
        "jump to {contact} in whatsapp",
        "jump to the chat with {contact}",
        "switch to {contact} chat",
        "switch to {contact} in whatsapp",
        "switch to the chat with {contact}",
        "bring up {contact} chat",
        "bring up the chat with {contact}",
        "bring up {contact} in whatsapp",
        "bring up the chat with {contact} in whatsapp",
        "pull up {contact} chat",
        "pull up the chat with {contact}",
        "pull up {contact} in whatsapp",
        "pull up the chat with {contact} in whatsapp",
        "show me {contact} chat",
        "show me the chat with {contact}",
        "show me {contact} in whatsapp",
        "show me the chat with {contact} in whatsapp",
        "show {contact} chat",
        "show the chat with {contact}",
        "show {contact} in whatsapp",
        "show the chat with {contact} in whatsapp",
        "display {contact} chat",
        "display the chat with {contact}",
        "display {contact} in whatsapp",
        "display the chat with {contact} in whatsapp",
        "find me {contact}",
        "find me {contact} in whatsapp",
        "find me {contact} on whatsapp",
        "find me the chat with {contact}",
        "find me the chat with {contact} in whatsapp",
        "find my chat with {contact}",
        "find my chat with {contact} in whatsapp",
        "find my chat with {contact} on whatsapp",
        "find the chat with {contact}",
        "find the chat with {contact} in whatsapp",
        "find the chat with {contact} on whatsapp",
        "can you find {contact}",
        "can you find {contact} in whatsapp",
        "could you find {contact}",
        "could you find {contact} in whatsapp",
        "please find {contact}",
        "please find {contact} in whatsapp",
        "please search for {contact}",
        "please search for {contact} in whatsapp",
        "can you search for {contact}",
        "can you search for {contact} in whatsapp",
        "could you search for {contact}",
        "could you search for {contact} in whatsapp",
        "just find {contact}",
        "just find {contact} in whatsapp",
        "just search for {contact}",
        "just search for {contact} in whatsapp",
        "simply find {contact}",
        "simply find {contact} in whatsapp",
        "simply search for {contact}",
        "simply search for {contact} in whatsapp",
        "go ahead and find {contact}",
        "go ahead and find {contact} in whatsapp",
        "go ahead and search for {contact}",
        "go ahead and search for {contact} in whatsapp",
        "i need to find {contact}",
        "i need to find {contact} in whatsapp",
        "i need to search for {contact}",
        "i need to search for {contact} in whatsapp",
        "i want to find {contact}",
        "i want to find {contact} in whatsapp",
        "i want to search for {contact}",
        "i want to search for {contact} in whatsapp",
        "let me find {contact}",
        "let me find {contact} in whatsapp",
        "let me search for {contact}",
        "let me search for {contact} in whatsapp",
        "let's find {contact}",
        "let's find {contact} in whatsapp",
        "let's search for {contact}",
        "let's search for {contact} in whatsapp",
        "help me find {contact}",
        "help me find {contact} in whatsapp",
        "help me search for {contact}",
        "help me search for {contact} in whatsapp",
        # STT distortions
        "search four {contact}",      # STT: for→four
        "look app {contact}",         # STT: up→app
        "find {contact} in whats app", # STT
        "find {contact} in what sap", # STT
        "find {contact} in what's app", # STT
        "search {contact} in whats app", # STT
        "chat wit {contact}",         # STT: with→wit
        "massage {contact}",          # STT: message→massage
        "open chat wit {contact}",    # STT
        "go too {contact} chat",       # STT: to→too
        # Filler words
        "um find {contact}", "uh find {contact}",
        "hey nexus find {contact}", "nexus find {contact}",
        "ok find {contact}", "okay find {contact}",
        "so find {contact}", "and find {contact}",
        "now find {contact}", "like find {contact}",
        "actually find {contact}", "hmm find {contact}",
    ]
    for contact in CONTACTS:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(contact=contact),
                "intent": "whatsapp_search",
                "slots": {"contact": contact},
            })
    return examples


def gen_focus_app():
    """Generate focus_app examples."""
    examples = []
    patterns = [
        "focus {app}",
        "bring {app} to front",
        "bring {app} forward",
        "switch to {app}",
        "switch over to {app}",
        "switch to {app} window",
        "go to {app}",
        "bring up {app}",
        "show {app}",
        "maximize {app}",
        "focus {app} window",
        "bring {app} window to front",
        "focus on {app}",
        "focus the {app}",
        "focus the {app} window",
        "focus on the {app}",
        "focus on the {app} window",
        "focus on {app} window",
        "bring the {app} to front",
        "bring the {app} forward",
        "bring the {app} window to front",
        "bring the {app} to the front",
        "bring the {app} window to the front",
        "bring {app} to the front",
        "bring {app} window to the front",
        "switch to the {app}",
        "switch to the {app} window",
        "switch over to the {app}",
        "switch over to the {app} window",
        "go to the {app}",
        "go to the {app} window",
        "bring up the {app}",
        "bring up the {app} window",
        "show me {app}",
        "show me the {app}",
        "show me the {app} window",
        "show the {app}",
        "show the {app} window",
        "display {app}",
        "display the {app}",
        "display the {app} window",
        "maximize the {app}",
        "maximize the {app} window",
        "maximize {app} window",
        "restore {app}",
        "restore the {app}",
        "restore the {app} window",
        "restore {app} window",
        "unminimize {app}",
        "unminimize the {app}",
        "unminimize the {app} window",
        "unminimize {app} window",
        "activate {app}",
        "activate the {app}",
        "activate the {app} window",
        "activate {app} window",
        "select {app}",
        "select the {app}",
        "select the {app} window",
        "select {app} window",
        "highlight {app}",
        "highlight the {app}",
        "highlight the {app} window",
        "highlight {app} window",
        "raise {app}",
        "raise the {app}",
        "raise the {app} window",
        "raise {app} window",
        "elevate {app}",
        "elevate the {app}",
        "elevate the {app} window",
        "elevate {app} window",
        "pop up {app}",
        "pop up the {app}",
        "pop up the {app} window",
        "pop up {app} window",
        "pop open {app}",
        "pop open the {app}",
        "pop open the {app} window",
        "pop open {app} window",
        "foreground {app}",
        "foreground the {app}",
        "foreground the {app} window",
        "foreground {app} window",
        "front {app}",
        "front the {app}",
        "front the {app} window",
        "front {app} window",
        "focus {app} please",
        "focus the {app} please",
        "focus on {app} please",
        "bring {app} to front please",
        "bring the {app} to front please",
        "switch to {app} please",
        "switch to the {app} please",
        "go to {app} please",
        "go to the {app} please",
        "bring up {app} please",
        "bring up the {app} please",
        "show {app} please",
        "show me {app} please",
        "show me the {app} please",
        "maximize {app} please",
        "maximize the {app} please",
        "focus {app} now",
        "focus the {app} now",
        "focus on {app} now",
        "bring {app} to front now",
        "switch to {app} now",
        "go to {app} now",
        "bring up {app} now",
        "show {app} now",
        "maximize {app} now",
        "focus {app} for me",
        "focus the {app} for me",
        "focus on {app} for me",
        "bring {app} to front for me",
        "switch to {app} for me",
        "go to {app} for me",
        "bring up {app} for me",
        "show {app} for me",
        "maximize {app} for me",
        "can you focus {app}",
        "can you focus the {app}",
        "can you focus on {app}",
        "can you bring {app} to front",
        "can you bring the {app} to front",
        "can you switch to {app}",
        "can you switch to the {app}",
        "can you go to {app}",
        "can you go to the {app}",
        "can you bring up {app}",
        "can you bring up the {app}",
        "can you show {app}",
        "can you show me {app}",
        "can you show me the {app}",
        "can you maximize {app}",
        "can you maximize the {app}",
        "could you focus {app}",
        "could you focus the {app}",
        "could you focus on {app}",
        "could you bring {app} to front",
        "could you bring the {app} to front",
        "could you switch to {app}",
        "could you switch to the {app}",
        "could you go to {app}",
        "could you go to the {app}",
        "could you bring up {app}",
        "could you bring up the {app}",
        "could you show {app}",
        "could you show me {app}",
        "could you show me the {app}",
        "could you maximize {app}",
        "could you maximize the {app}",
        "please focus {app}",
        "please focus the {app}",
        "please focus on {app}",
        "please bring {app} to front",
        "please bring the {app} to front",
        "please switch to {app}",
        "please switch to the {app}",
        "please go to {app}",
        "please go to the {app}",
        "please bring up {app}",
        "please bring up the {app}",
        "please show {app}",
        "please show me {app}",
        "please show me the {app}",
        "please maximize {app}",
        "please maximize the {app}",
        "just focus {app}",
        "just focus the {app}",
        "just focus on {app}",
        "just bring {app} to front",
        "just switch to {app}",
        "just go to {app}",
        "just bring up {app}",
        "just show {app}",
        "just maximize {app}",
        "simply focus {app}",
        "simply focus the {app}",
        "simply focus on {app}",
        "simply bring {app} to front",
        "simply switch to {app}",
        "simply go to {app}",
        "simply bring up {app}",
        "simply show {app}",
        "simply maximize {app}",
        "go ahead and focus {app}",
        "go ahead and focus the {app}",
        "go ahead and focus on {app}",
        "go ahead and bring {app} to front",
        "go ahead and switch to {app}",
        "go ahead and go to {app}",
        "go ahead and bring up {app}",
        "go ahead and show {app}",
        "go ahead and maximize {app}",
        "i need to focus {app}",
        "i need to focus the {app}",
        "i need to focus on {app}",
        "i need to bring {app} to front",
        "i need to switch to {app}",
        "i need to go to {app}",
        "i need to bring up {app}",
        "i need to show {app}",
        "i need to maximize {app}",
        "i want to focus {app}",
        "i want to focus the {app}",
        "i want to focus on {app}",
        "i want to bring {app} to front",
        "i want to switch to {app}",
        "i want to go to {app}",
        "i want to bring up {app}",
        "i want to show {app}",
        "i want to maximize {app}",
        "let me focus {app}",
        "let me focus the {app}",
        "let me focus on {app}",
        "let me bring {app} to front",
        "let me switch to {app}",
        "let me go to {app}",
        "let me bring up {app}",
        "let me show {app}",
        "let me maximize {app}",
        "let's focus {app}",
        "let's focus the {app}",
        "let's focus on {app}",
        "let's bring {app} to front",
        "let's switch to {app}",
        "let's go to {app}",
        "let's bring up {app}",
        "let's show {app}",
        "let's maximize {app}",
        "help me focus {app}",
        "help me focus the {app}",
        "help me focus on {app}",
        "help me bring {app} to front",
        "help me switch to {app}",
        "help me go to {app}",
        "help me bring up {app}",
        "help me show {app}",
        "help me maximize {app}",
        # STT distortions
        "switch too {app}",           # STT: to→too
        "go too {app}",               # STT: to→too
        "bring {app} to the front",   # STT adds "the"
        # Filler words
        "um focus {app}", "uh focus {app}",
        "hey nexus focus {app}", "nexus focus {app}",
        "ok focus {app}", "okay focus {app}",
        "so focus {app}", "and focus {app}",
        "now focus {app}", "like focus {app}",
        "actually focus {app}", "hmm focus {app}",
    ]
    for app in APPS[:15]:  # use subset to keep count reasonable
        for pattern in patterns:
            examples.append({
                "text": pattern.format(app=app),
                "intent": "focus_app",
                "slots": {"target": app},
            })
    return examples


# ─── Additional examples for existing intents ──────────────────────────────

def gen_additional_open_app():
    """Generate additional open_app examples with more variety."""
    examples = []
    patterns = [
        "open {app}", "launch {app}", "start {app}", "run {app}",
        "fire up {app}", "bring up {app}", "show {app}", "pull up {app}",
        "open the {app}", "launch the {app}", "start the {app}",
        "show me the {app}", "open {app} please", "please open {app}",
        "can you open {app}", "could you open {app}",
        "open {app} for me", "launch {app} for me",
        "open {app} app", "launch the {app} application",
        "so open {app}", "and open {app}", "but first open {app}",
        "let's open {app}", "nexus open {app}", "hey nexus open {app}",
    ]
    for app in APPS:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(app=app),
                "intent": "open_app",
                "slots": {"app_name": app},
            })
    return examples


def gen_additional_close_app():
    """Generate additional close_app examples."""
    examples = []
    patterns = [
        "close {app}", "quit {app}", "exit {app}", "kill {app}",
        "terminate {app}", "end {app}", "shut down {app}",
        "close the {app}", "quit the {app}", "kill the {app}",
        "close {app} please", "please close {app}",
        "close {app} for me", "quit {app} for me",
        "shut {app}", "bye {app}",
        "clothes {app}",              # STT: close→clothes
        "quite {app}",                # STT: quit→quite
    ]
    for app in APPS[:15]:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(app=app),
                "intent": "close_app",
                "slots": {"app_name": app},
            })
    return examples


def gen_additional_search():
    """Generate additional search examples."""
    examples = []
    patterns = [
        "search for {query}", "search {query}", "google {query}",
        "look up {query}", "find {query}", "find me {query}",
        "look for {query}", "search for the {query}",
        "google the {query}", "look up the {query}",
        "search for {query} please", "can you google {query}",
        "what is {query}", "who is {query}", "what's {query}",
        "tell me about {query}",
        "search for {query} online",
        "search four {query}",        # STT: for→four
        "look app {query}",           # STT: up→app
    ]
    for query in SEARCH_QUERIES:
        for pattern in patterns:
            examples.append({
                "text": pattern.format(query=query),
                "intent": "search",
                "slots": {"query": query},
            })
    return examples


# ─── Negative examples ─────────────────────────────────────────────────────

def gen_negative_examples():
    """Generate examples that teach the model to NOT confuse commands."""
    examples = []
    # "type" is NOT an open verb — "type notepad" → type_text, not open_app
    for app in ["notepad", "chrome", "terminal", "whatsapp", "vscode", "firefox"]:
        examples.append({
            "text": f"type {app}",
            "intent": "type_text",
            "slots": {"text": app},
        })
    # "press" is NOT an open verb — "press chrome" → press_key, not open_app
    for app in ["chrome", "brave", "firefox", "edge", "safari"]:
        examples.append({
            "text": f"press {app}",
            "intent": "press_key",
            "slots": {"key": app},
        })
    # "send" alone is NOT a search
    examples.append({"text": "send cats", "intent": "confirm_send", "slots": {}})
    # "stop" alone is cancel, "stop music" is media_stop
    examples.append({"text": "stop", "intent": "cancel_action", "slots": {}})
    examples.append({"text": "stop music", "intent": "media_stop", "slots": {}})
    examples.append({"text": "stop the music", "intent": "media_stop", "slots": {}})
    examples.append({"text": "stop playing", "intent": "media_stop", "slots": {}})
    examples.append({"text": "stop the video", "intent": "media_stop", "slots": {}})
    # "new tab" is NOT an app
    examples.append({"text": "new tab", "intent": "browser_new_tab", "slots": {}})
    # "chat with" is NOT an open verb
    examples.append({
        "text": "chat with mom",
        "intent": "whatsapp_chat",
        "slots": {"contact": "mom"},
    })
    # "go to github.com" is navigate, "go to github" is open_app
    examples.append({
        "text": "go to github.com",
        "intent": "browser_navigate",
        "slots": {"url": "github.com"},
    })
    examples.append({
        "text": "go to github",
        "intent": "open_app",
        "slots": {"app_name": "github"},
    })
    # "go to google.com" is navigate, "go to google" is open_app
    examples.append({
        "text": "go to google.com",
        "intent": "browser_navigate",
        "slots": {"url": "google.com"},
    })
    examples.append({
        "text": "go to google",
        "intent": "open_app",
        "slots": {"app_name": "google"},
    })
    # "open github.com" is navigate (has .com), "open github" is open_app
    examples.append({
        "text": "open github.com",
        "intent": "browser_navigate",
        "slots": {"url": "github.com"},
    })
    examples.append({
        "text": "open github",
        "intent": "open_app",
        "slots": {"app_name": "github"},
    })
    # "search for cats in browser" is browser_search, "search for cats" is search
    examples.append({
        "text": "search for cats in browser",
        "intent": "browser_search",
        "slots": {"query": "cats"},
    })
    examples.append({
        "text": "search for cats",
        "intent": "search",
        "slots": {"query": "cats"},
    })
    # "find mom in whatsapp" is whatsapp_search, "find mom" is whatsapp_search
    examples.append({
        "text": "find mom in whatsapp",
        "intent": "whatsapp_search",
        "slots": {"contact": "mom"},
    })
    # "focus chrome" is focus_app, "open chrome" is open_app
    examples.append({
        "text": "focus chrome",
        "intent": "focus_app",
        "slots": {"target": "chrome"},
    })
    examples.append({
        "text": "open chrome",
        "intent": "open_app",
        "slots": {"app_name": "chrome"},
    })
    # "press enter" is press_key, "press ctrl enter" is press_hotkey
    examples.append({
        "text": "press enter",
        "intent": "press_key",
        "slots": {"key": "enter"},
    })
    examples.append({
        "text": "press ctrl enter",
        "intent": "press_hotkey",
        "slots": {"keys": ["ctrl", "enter"]},
    })
    # "type hello" is type_text, "press hello" is press_key (nonsense but distinct)
    examples.append({
        "text": "type hello",
        "intent": "type_text",
        "slots": {"text": "hello"},
    })
    # "send" is confirm_send, "send message to mom" is whatsapp_search
    examples.append({
        "text": "send",
        "intent": "confirm_send",
        "slots": {},
    })
    examples.append({
        "text": "send message to mom",
        "intent": "whatsapp_search",
        "slots": {"contact": "mom"},
    })
    # "cancel" is cancel_action, "close" is close_app
    examples.append({
        "text": "cancel",
        "intent": "cancel_action",
        "slots": {},
    })
    examples.append({
        "text": "close chrome",
        "intent": "close_app",
        "slots": {"app_name": "chrome"},
    })
    # "go to whatsapp" is open_app (no .com), "go to whatsapp.com" is navigate
    examples.append({
        "text": "go to whatsapp",
        "intent": "open_app",
        "slots": {"app_name": "whatsapp"},
    })
    examples.append({
        "text": "go to whatsapp.com",
        "intent": "browser_navigate",
        "slots": {"url": "whatsapp.com"},
    })
    # "new tab" is browser_new_tab, "new file" is NOT (unknown/open_app context)
    examples.append({
        "text": "new tab",
        "intent": "browser_new_tab",
        "slots": {},
    })
    # "type a" is type_text, "press a" is press_key
    examples.append({
        "text": "type a",
        "intent": "type_text",
        "slots": {"text": "a"},
    })
    examples.append({
        "text": "press a",
        "intent": "press_key",
        "slots": {"key": "a"},
    })
    # "type the password" is type_text (despite "password" being sensitive)
    examples.append({
        "text": "type the password",
        "intent": "type_text",
        "slots": {"text": "the password"},
    })
    # "press the enter key" is press_key
    examples.append({
        "text": "press the enter key",
        "intent": "press_key",
        "slots": {"key": "enter"},
    })
    # "open a new tab" is browser_new_tab, NOT open_app
    examples.append({
        "text": "open a new tab",
        "intent": "browser_new_tab",
        "slots": {},
    })
    # "open a new window" is browser_new_tab, NOT open_app
    examples.append({
        "text": "open a new window",
        "intent": "browser_new_tab",
        "slots": {},
    })
    # "switch to chrome" is focus_app, "switch to the chrome tab" is focus_app
    examples.append({
        "text": "switch to chrome",
        "intent": "focus_app",
        "slots": {"target": "chrome"},
    })
    # "bring whatsapp to front" is focus_app, NOT open_app
    examples.append({
        "text": "bring whatsapp to front",
        "intent": "focus_app",
        "slots": {"target": "whatsapp"},
    })
    # "search for rust" is search, "search rust in browser" is browser_search
    examples.append({
        "text": "search for rust",
        "intent": "search",
        "slots": {"query": "rust"},
    })
    examples.append({
        "text": "search rust in browser",
        "intent": "browser_search",
        "slots": {"query": "rust"},
    })
    # "stop sending" is cancel_action
    examples.append({
        "text": "stop sending",
        "intent": "cancel_action",
        "slots": {},
    })
    # "don't send" is cancel_action
    examples.append({
        "text": "don't send",
        "intent": "cancel_action",
        "slots": {},
    })
    # "yes send" is confirm_send
    examples.append({
        "text": "yes send",
        "intent": "confirm_send",
        "slots": {},
    })
    # "go ahead" is confirm_send
    examples.append({
        "text": "go ahead",
        "intent": "confirm_send",
        "slots": {},
    })
    # "never mind" is cancel_action
    examples.append({
        "text": "never mind",
        "intent": "cancel_action",
        "slots": {},
    })
    # "type hello world" is type_text, NOT search
    examples.append({
        "text": "type hello world",
        "intent": "type_text",
        "slots": {"text": "hello world"},
    })
    # "press ctrl c" is press_hotkey, NOT press_key (two keys)
    examples.append({
        "text": "press ctrl c",
        "intent": "press_hotkey",
        "slots": {"keys": ["ctrl", "c"]},
    })
    # "press ctrl shift s" is press_hotkey (three keys)
    examples.append({
        "text": "press ctrl shift s",
        "intent": "press_hotkey",
        "slots": {"keys": ["ctrl", "shift", "s"]},
    })
    return examples


# ─── Main ──────────────────────────────────────────────────────────────────

def main():
    all_examples = []

    # New live-mode intents
    generators = [
        ("type_text", gen_type_text),
        ("press_key", gen_press_key),
        ("press_hotkey", gen_press_hotkey),
        ("confirm_send", gen_confirm_send),
        ("cancel_action", gen_cancel_action),
        ("browser_new_tab", gen_browser_new_tab),
        ("browser_navigate", gen_browser_navigate),
        ("browser_search", gen_browser_search),
        ("whatsapp_open", gen_whatsapp_open),
        ("whatsapp_search", gen_whatsapp_search),
        ("focus_app", gen_focus_app),
    ]

    for name, gen in generators:
        examples = gen()
        # Cap at target count to avoid over-representing
        caps = {
            "type_text": 120,
            "press_key": 110,
            "press_hotkey": 110,
            "confirm_send": 70,
            "cancel_action": 70,
            "browser_new_tab": 60,
            "browser_navigate": 90,
            "browser_search": 90,
            "whatsapp_open": 60,
            "whatsapp_search": 90,
            "focus_app": 90,
        }
        cap = caps.get(name, 50)
        if len(examples) > cap:
            random.shuffle(examples)
            examples = examples[:cap]
        print(f"  {name}: {len(examples)} examples")
        all_examples.extend(examples)

    # Additional examples for existing intents
    print("\nAdditional examples for existing intents:")
    for name, gen, cap in [
        ("open_app", gen_additional_open_app, 34),
        ("close_app", gen_additional_close_app, 22),
        ("search", gen_additional_search, 23),
    ]:
        examples = gen()
        random.shuffle(examples)
        examples = examples[:cap]
        print(f"  {name}: +{len(examples)} examples")
        all_examples.extend(examples)

    # Negative examples
    neg = gen_negative_examples()
    print(f"\n  negative_examples: {len(neg)} examples")
    all_examples.extend(neg)

    # Shuffle all
    random.shuffle(all_examples)

    print(f"\nTotal new examples: {len(all_examples)}")

    # Write to JSON
    output = {"train": all_examples, "test": []}
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2, ensure_ascii=False)

    print(f"Written to {OUTPUT_PATH}")

    # Print intent distribution
    from collections import Counter
    counts = Counter(ex["intent"] for ex in all_examples)
    print("\nIntent distribution:")
    for intent, count in sorted(counts.items()):
        print(f"  {count:4d}  {intent}")


if __name__ == "__main__":
    main()
