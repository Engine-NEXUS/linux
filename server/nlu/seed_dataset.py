#!/usr/bin/env python3
"""
NEXUS NLU — Seed dataset generator for the expanded 46-intent schema.

Generates ~50 examples per intent across all 46 intents, including:
  - Base phrasings (multiple verbs, word orders)
  - STT mishearing variants (Whisper/faster-whisper common errors)
  - Filler word variants (and, so, but, then, now, please, hey, ok)
  - Conversational variants

Output: dataset.json with balanced classes (~2300 total examples)

Usage:
  cd server/nlu
  python seed_dataset.py
"""

import json
import random
from pathlib import Path

random.seed(42)

OUTPUT_PATH = Path(__file__).parent / "dataset.json"

# ─── Helpers ────────────────────────────────────────────────────────────────

FILLERS = ["and", "so", "but", "then", "now", "please", "hey", "ok", "well", "also"]

def with_fillers(examples, max_variants=3):
    """Add filler-word variants to a list of example dicts.
    
    Each example is {"text": ..., "intent": ..., "slots": ...}.
    Adds variants like "and open whatsapp" with the same intent/slots.
    """
    result = list(examples)
    seen_texts = {e["text"] for e in result}
    for ex in examples:
        added = 0
        for filler in FILLERS:
            if added >= max_variants:
                break
            new_text = f"{filler} {ex['text']}"
            if new_text not in seen_texts:
                result.append({
                    "text": new_text,
                    "intent": ex["intent"],
                    "slots": ex["slots"],
                })
                seen_texts.add(new_text)
                added += 1
    return result

def with_mishearings(examples, mishearing_map, max_variants=5):
    """Add STT mishearing variants to example dicts.
    
    mishearing_map: dict of {correct_word: [misheard1, misheard2, ...]}
    """
    result = list(examples)
    seen_texts = {e["text"] for e in result}
    for ex in examples:
        added = 0
        for correct, misheard_list in mishearing_map.items():
            if correct in ex["text"] and added < max_variants:
                for misheard in misheard_list:
                    new_text = ex["text"].replace(correct, misheard)
                    if new_text not in seen_texts:
                        new_ex = {"text": new_text, "intent": ex["intent"], "slots": dict(ex["slots"])}
                        # Update slot values if the correct word was in a slot
                        for k, v in ex["slots"].items():
                            if v == correct:
                                new_ex["slots"][k] = misheard
                        result.append(new_ex)
                        seen_texts.add(new_text)
                        added += 1
                        if added >= max_variants:
                            break
    return result

# ─── Intent generators ──────────────────────────────────────────────────────

def gen_open_app():
    """open_app — launch applications."""
    apps = [
        "whatsapp", "chrome", "spotify", "notepad", "calculator", "vscode",
        "visual studio code", "terminal", "powershell", "settings",
        "file explorer", "explorer", "task manager", "control panel",
        "discord", "slack", "notion", "figma", "chatgpt", "claude",
        "youtube", "github", "gmail", "outlook", "word", "excel",
        "powerpoint", "steam", "zoom", "teams", "telegram", "edge",
        "firefox", "brave", "paint", "blender", "unity", "docker",
        "postman", "obs", "vlc", "netflix", "amazon", "twitter",
        "reddit", "linkedin", "instagram", "facebook", "twitch",
    ]
    verbs = ["open", "launch", "start", "run", "show", "pull up", "bring up", "fire up", "go to"]
    suffixes = ["", "", "", "app", "application", "for me", "please"]
    
    examples = []
    for app in apps:
        for verb in verbs[:5]:  # 5 verbs per app
            suffix = random.choice(suffixes)
            text = f"{verb} {app}" + (f" {suffix}" if suffix else "")
            examples.append({"text": text, "intent": "open_app", "slots": {"app_name": app}})
    
    # STT mishearings
    mishearings = {
        "whatsapp": ["whats app", "what's app", "whatsap", "whats appp"],
        "gemini": ["gem ini", "gemmine", "jam ini"],
        "youtube": ["you tube", "u tube", "you tub"],
        "github": ["git hub", "get hub", "gitHub"],
        "chatgpt": ["chat gpt", "chat g p t", "chatgpt"],
        "vscode": ["vs code", "v s code", "vscode"],
        "powerpoint": ["power point", "powerpoint"],
        "notepad": ["note pad", "not pad"],
        "firefox": ["fire fox", "firefoxx"],
        "file explorer": ["file explorer", "fileexplorer"],
    }
    for app in ["whatsapp", "gemini", "youtube", "github", "chatgpt", "vscode", "notepad", "firefox"]:
        for misheard in mishearings.get(app, [])[:2]:
            examples.append({"text": f"open {misheard}", "intent": "open_app", "slots": {"app_name": misheard}})
    
    # Add filler variants for a subset
    examples.extend(with_fillers(examples[:30], max_variants=2))
    
    return examples[:55]

def gen_open_url():
    """open_url — open URLs in browser."""
    urls = [
        "google.com", "github.com", "stackoverflow.com", "youtube.com",
        "gmail.com", "reddit.com", "twitter.com", "linkedin.com",
        "amazon.com", "netflix.com", "wikipedia.org", "docs.python.org",
        "developer.mozilla.org", "rust-lang.org", "crates.io",
    ]
    verbs = ["open", "go to", "visit", "browse to", "navigate to"]
    
    examples = []
    for url in urls:
        for verb in verbs:
            examples.append({"text": f"{verb} {url}", "intent": "open_url", "slots": {"url": url}})
    
    # "in browser" variants
    sites = ["gmail", "youtube", "github", "twitter", "reddit", "spotify", "netflix"]
    for site in sites:
        examples.append({"text": f"open {site} in browser", "intent": "open_url", "slots": {"url": site}})
        examples.append({"text": f"open {site} website", "intent": "open_url", "slots": {"url": site}})
        examples.append({"text": f"open {site} on the web", "intent": "open_url", "slots": {"url": site}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_close_app():
    """close_app — close/quit applications."""
    apps = ["whatsapp", "chrome", "spotify", "notepad", "calculator", "vscode",
            "discord", "slack", "terminal", "powershell", "firefox", "edge",
            "teams", "zoom", "telegram", "notion", "figma", "blender", "unity"]
    verbs = ["close", "quit", "exit", "kill", "shut down", "shut"]
    
    examples = []
    for app in apps:
        for verb in verbs:
            examples.append({"text": f"{verb} {app}", "intent": "close_app", "slots": {"app_name": app}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_whatsapp_chat():
    """whatsapp_chat — open chat with a contact."""
    contacts = ["mom", "dad", "lakshya", "john", "alice", "bob", "sarah", "prem",
                "eesha", "arjun", "priya", "rahul", "sam", "alex", "emma"]
    patterns = [
        "chat with {c}",
        "open chat with {c}",
        "open my chat with {c}",
        "message {c}",
        "message {c} on whatsapp",
        "send message to {c}",
        "send whatsapp to {c}",
        "whatsapp {c}",
        "open whatsapp chat with {c}",
        "chat with {c} on whatsapp",
    ]
    
    examples = []
    for contact in contacts:
        for pattern in patterns:
            text = pattern.format(c=contact)
            examples.append({"text": text, "intent": "whatsapp_chat", "slots": {"contact": contact}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_open_architect():
    """open_architect — open the architecture mapper window."""
    base = [
        "open architecture mapper", "open the architecture mapper",
        "open architect", "open the architect",
        "show architecture mapper", "show the architecture mapper",
        "launch architecture mapper", "launch the architecture mapper",
        "start architecture mapper", "bring up architecture mapper",
        "bring up the architecture mapper", "pull up architecture mapper",
        "pull up the architecture mapper", "show me the architecture",
        "show me architecture mapper", "give me the architecture",
        "open architecture map", "open architecture window",
        "open architecture diagram", "open architecture graph",
        "open architecture viewer", "open architecture explorer",
        "open codebase mapper", "open dependency mapper",
        "show architecture", "show the architecture", "display architecture",
        "open the architecture", "open codebase", "open codebase map",
        "open dependency map", "open dependencies mapper",
        "open map", "open the map", "show map", "show the map",
    ]
    # STT fuzzy mishearings
    fuzzy = [
        "open octach at mapper", "open arcade mapper", "open arch at mapper",
        "open arch mapper", "open architecture at mapper", "open architect mapper",
        "open are cat mapper", "open our cat mapper", "open ark mapper",
        "open art at mapper", "open art mapper", "open a cat mapper",
        "open acat mapper", "open architecture remember", "open architecture member",
        "open architecture december", "open architecture mac", "open architecture mad",
        "open architecture matter", "open architecture master",
        "open up and remember", "open up and member", "open up and december",
        "open are cat map", "open our cat map", "open ark map",
        "open art map", "open a cat map",
    ]
    
    all_phrases = base + fuzzy
    examples = [{"text": p, "intent": "open_architect", "slots": {}} for p in all_phrases]
    return with_fillers(examples, max_variants=2)[:55]

def gen_search():
    """search — web search queries."""
    queries = [
        "cats", "rust async programming", "how to make pasta", "weather today",
        "best restaurants near me", "capital of france", "python decorators",
        "docker compose tutorial", "react hooks vs classes", "typescript generics",
        "how to fix a leaky faucet", "best laptop for programming 2025",
        "rust vs go performance", "tauri vs electron", "cloudflare workers pricing",
        "github actions ci cd", "postgresql vs mysql", "redis cache strategies",
        "what is kubernetes", "how to learn rust",
    ]
    verbs = ["search for", "search", "google", "look up", "find me", "find", "look for"]
    
    examples = []
    for query in queries:
        for verb in verbs:
            examples.append({"text": f"{verb} {query}", "intent": "search", "slots": {"query": query}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_media_play_pause():
    """media_play_pause — play/pause media."""
    phrases = [
        "pause", "play", "resume", "play pause", "play/pause",
        "toggle media", "pause music", "pause media", "resume music",
        "play music", "pause playback", "resume playback",
        "pause the music", "play the music", "resume the music",
        "pause the song", "play the song", "resume the song",
        "pause this", "play this", "resume this",
        "pause audio", "play audio", "resume audio",
        "stop playing", "start playing", "toggle play",
        "pause it", "play it", "resume it",
        "hit pause", "hit play",
    ]
    examples = [{"text": p, "intent": "media_play_pause", "slots": {}} for p in phrases]
    return with_fillers(examples, max_variants=2)[:55]

def gen_media_next():
    """media_next — next track."""
    phrases = [
        "next", "next song", "next track", "skip", "skip song",
        "skip track", "next one", "go forward", "forward",
        "next please", "skip this", "skip this song",
        "move to next", "go to next song", "go to next track",
        "advance track", "advance song", "next audio",
        "skip to next", "jump forward", "next track please",
    ]
    examples = [{"text": p, "intent": "media_next", "slots": {}} for p in phrases]
    return with_fillers(examples, max_variants=2)[:55]

def gen_media_previous():
    """media_previous — previous track."""
    phrases = [
        "previous", "previous song", "previous track", "prev",
        "prev song", "go back", "go back a song", "previous one",
        "last song", "last track", "back", "rewind",
        "previous please", "go to previous", "go to previous song",
        "go to previous track", "previous audio", "back one track",
        "play the last song", "play previous", "step back",
    ]
    examples = [{"text": p, "intent": "media_previous", "slots": {}} for p in phrases]
    return with_fillers(examples, max_variants=2)[:55]

def gen_media_stop():
    """media_stop — stop media playback."""
    phrases = [
        "stop music", "stop media", "stop playback", "stop playing",
        "stop the music", "stop the song", "stop audio",
        "stop", "halt music", "halt playback", "stop it",
        "stop playing music", "stop playing the song",
        "cease playback", "stop the audio", "stop everything",
        "stop all music", "stop all audio", "stop all playback",
        "silence the music", "quiet the music",
    ]
    examples = [{"text": p, "intent": "media_stop", "slots": {}} for p in phrases]
    return with_fillers(examples, max_variants=2)[:55]

def gen_greeting():
    """greeting — conversational greetings, thanks, farewells."""
    phrases = [
        # Hello
        "hello", "hi", "hey", "yo", "sup", "what's up", "howdy",
        "greetings", "hiya", "hey there", "hello nexus", "hi nexus",
        "hey nexus", "good day", "hello there",
        # How are you
        "how are you", "how's it going", "how are things",
        "how are you doing", "what's going on", "how do you do",
        # Bye
        "bye", "goodbye", "see you", "farewell", "bye nexus",
        "goodbye nexus", "see you later", "catch you later",
        # Thanks
        "thanks", "thank you", "thx", "ty", "thanks nexus",
        "appreciate it", "thank you nexus", "much appreciated",
        # Identity
        "what's your name", "who are you", "who is nexus",
        "what are you",
        # Capabilities
        "what can you do", "help me", "help", "what do you do",
        # Time of day
        "good morning", "good afternoon", "good evening", "good night",
        "morning", "evening",
        # Affirmative
        "yes", "yeah", "yep", "yup", "sure", "ok", "okay",
        "alright", "sounds good", "got it", "understood",
        "roger", "affirmative", "will do",
        # Negative
        "no", "nope", "nah", "no thanks", "never mind",
        "forget it", "cancel", "disregard", "nevermind",
    ]
    examples = [{"text": p, "intent": "greeting", "slots": {}} for p in phrases]
    return examples[:55]

def gen_analyse_repo():
    """analyse_repo — analyse a repository."""
    repos = ["servx", "zync", "congi", "eesh264", "nexus-agent", "ultron", "myrepo"]
    owners = ["zync-meet", "eesh264", "myorg"]
    
    examples = []
    for repo in repos:
        examples.append({"text": f"analyse {repo}", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyse {repo} repo", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the repo {repo}", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyse repo {repo}", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the {repo} repo", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyse {repo} repository", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyze {repo}", "intent": "analyse_repo", "slots": {"repo": repo}})
        examples.append({"text": f"analyze {repo} repo", "intent": "analyse_repo", "slots": {"repo": repo}})
    
    # owner/repo format
    for owner in owners:
        for repo in repos[:4]:
            examples.append({
                "text": f"analyse {owner}/{repo}",
                "intent": "analyse_repo",
                "slots": {"owner": owner, "repo": repo},
            })
            examples.append({
                "text": f"analyze {owner}/{repo}",
                "intent": "analyse_repo",
                "slots": {"owner": owner, "repo": repo},
            })
    
    # STT fuzzy repo names
    fuzzy_repos = {
        "zync": ["zink", "zinc", "zinck", "zinkk"],
        "servx": ["cervix", "servex", "serviks", "survex"],
    }
    for correct, misheard_list in fuzzy_repos.items():
        for misheard in misheard_list:
            examples.append({"text": f"analyse {misheard}", "intent": "analyse_repo", "slots": {"repo": correct}})
            examples.append({"text": f"analyse {misheard} repo", "intent": "analyse_repo", "slots": {"repo": correct}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_analyse_pr():
    """analyse_pr — analyse a specific PR."""
    repos = ["servx", "zync", "congi", "eesh264", "nexus-agent"]
    owners = ["zync-meet", "eesh264"]
    pr_numbers = [1, 5, 10, 23, 42, 99, 100, 254]
    
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"analyse PR {pr} {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse PR {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse pr {pr} {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyze PR {pr} {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse pull request {pr} {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse pull request {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse the pr {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse the pr {pr} {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analyse the pull request {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
    
    # owner/repo format
    for owner in owners:
        for repo in repos[:3]:
            for pr in [5, 23, 254]:
                examples.append({
                    "text": f"analyse PR {pr} {owner}/{repo}",
                    "intent": "analyse_pr",
                    "slots": {"pr_number": str(pr), "owner": owner, "repo": repo},
                })
    
    # Deep analysis variants
    for repo in repos[:3]:
        for pr in [24, 42]:
            examples.append({"text": f"deep analysis PR {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"deep analyse PR {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"deep analyze PR {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
            examples.append({"text": f"analysis PR {pr} in {repo}", "intent": "analyse_pr", "slots": {"pr_number": str(pr), "repo": repo}})
    
    # STT fuzzy
    examples.append({"text": "analyse pr 254 in zink", "intent": "analyse_pr", "slots": {"pr_number": "254", "repo": "zync"}})
    examples.append({"text": "analyse pr 254 in zinc", "intent": "analyse_pr", "slots": {"pr_number": "254", "repo": "zync"}})
    examples.append({"text": "analyse pr 254 in cervix", "intent": "analyse_pr", "slots": {"pr_number": "254", "repo": "servx"}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_analyse_latest_pr():
    """analyse_latest_pr — analyse the latest/newest PR."""
    repos = ["zync", "servx", "congi", "nexus-agent"]
    authors = ["prem", "eesha", "arjun", "lakshya"]
    
    examples = []
    for repo in repos:
        examples.append({"text": f"analyse the pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the pr of {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse latest pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the latest pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the pull request in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse latest pull request in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse the latest pull request in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse newest pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse recent pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"analyse current pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"the pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
        examples.append({"text": f"latest pr in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo}})
    
    # With author
    for repo in repos:
        for author in authors:
            examples.append({"text": f"analyse the pr by {author} in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"analyse the pr of {author} in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"analyse the pr from {author} in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"analyse latest pr by {author} in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"analyse the latest pr by {author} in {repo}", "intent": "analyse_latest_pr", "slots": {"repo": repo, "author": author}})
    
    return with_fillers(examples, max_variants=2)[:55]

def gen_check_branch():
    """check_branch — check latest branch of a repo."""
    repos = ["servx", "zync", "congi", "nexus-agent"]
    authors = ["eesha", "prem", "arjun", "lakshya"]
    
    examples = []
    for repo in repos:
        examples.append({"text": f"check the latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check the latest branch in {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"show the latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"show latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"what is the latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"what's the latest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check the latest branch of {repo} created", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check the newest branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check the recent branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
        examples.append({"text": f"check the new branch of {repo}", "intent": "check_branch", "slots": {"repo": repo}})
    
    # With author
    for repo in repos:
        for author in authors:
            examples.append({"text": f"check the latest branch of {repo} created by {author}", "intent": "check_branch", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"check latest branch by {author} in {repo}", "intent": "check_branch", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"show the latest branch of {repo} by {author}", "intent": "check_branch", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"what is the latest branch of {repo} created by {author}", "intent": "check_branch", "slots": {"repo": repo, "author": author}})
            examples.append({"text": f"check the latest branch by {author} of {repo}", "intent": "check_branch", "slots": {"repo": repo, "author": author}})
    
    return with_fillers(examples, max_variants=2)[:55]

# ─── GitHub command generators ──────────────────────────────────────────────

def gen_merge_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    pr_numbers = [5, 10, 23, 42, 99]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"merge pr {pr} in {repo}", "intent": "merge_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"merge pull request {pr} in {repo}", "intent": "merge_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"squash merge pr {pr} in {repo}", "intent": "merge_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"rebase merge pr {pr} in {repo}", "intent": "merge_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"merge pr {pr} for {repo}", "intent": "merge_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_approve_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    pr_numbers = [5, 10, 23, 42, 99]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"approve pr {pr} in {repo}", "intent": "approve_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"approve pull request {pr} in {repo}", "intent": "approve_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"approve the pr {pr} in {repo}", "intent": "approve_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_close_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    pr_numbers = [5, 10, 23, 42, 99]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"close pr {pr} in {repo}", "intent": "close_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"close pull request {pr} in {repo}", "intent": "close_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"close the pr {pr} in {repo}", "intent": "close_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"shut down pr {pr} in {repo}", "intent": "close_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_prs():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    # --- Repository-specific forms ---
    for repo in repos:
        examples.append({"text": f"list prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"list open prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"list closed prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"list all prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"list merged prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show open prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show closed prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show all prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show merged prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"list pull requests in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show pull requests in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"get prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"fetch prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"display prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"what prs are open in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"what are the open prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"open the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"open pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show me the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"view the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"view pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"give me the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"pull up the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"bring up the pr list in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"latest prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"active prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"show live prs in {repo}", "intent": "list_prs", "slots": {"repo": repo}})
        examples.append({"text": f"for {repo}", "intent": "list_prs", "slots": {"repo": repo}})

    # --- Account-wide / no-repository forms ---
    # These are critical: "open the pr list" without a repo should still
    # classify as list_prs, not open_app. The deterministic parser handles
    # these, but BERT-Mini needs to know them too for fallback.
    no_repo = [
        "open the pr list",
        "open pr list",
        "show the pr list",
        "show pr list",
        "show me the pr list",
        "show me pr list",
        "give me the pr list",
        "give me pr list",
        "view the pr list",
        "view pr list",
        "get the pr list",
        "get pr list",
        "pull up the pr list",
        "pull up pr list",
        "bring up the pr list",
        "bring up pr list",
        "list prs",
        "list open prs",
        "list closed prs",
        "list all prs",
        "list merged prs",
        "list pull requests",
        "list all pull requests",
        "show prs",
        "show open prs",
        "show closed prs",
        "show all prs",
        "show merged prs",
        "show pull requests",
        "show all pull requests",
        "get prs",
        "fetch prs",
        "display prs",
        "what prs are open",
        "what are the open prs",
        "what prs are closed",
        "what are the closed prs",
        "latest prs",
        "active prs",
        "show live prs",
        "give me prs",
        "tell me the pr list",
        "tell me pr list",
        "open the pull request list",
        "show the pull request list",
        "view the pull request list",
    ]
    for text in no_repo:
        examples.append({"text": text, "intent": "list_prs", "slots": {"repo": ""}})

    # STT mishearing variants — faster-whisper/Whisper commonly mishears
    # short technical words like "pr", "prs", and "list". These variants
    # ensure BERT-Mini recognizes the user's intent even when STT produces
    # phonetically similar but wrong transcripts.
    pr_mishearings = {
        "pr list": [
            "pee are list", "p r list", "pea arr list", "prr list",
            "per list", "p are list", "p r l list",
        ],
        "prs": [
            "pee ars", "p r s", "pea arrs", "prrs", "pers",
            "p r", "pea are", "p are",
        ],
        "pull request": [
            "pull reques", "pool request", "pull re quest",
            "pull reque", "pool reques",
        ],
        "pull requests": [
            "pull requess", "pool requests", "pull re quests",
            "pool requess",
        ],
        "list prs": [
            "lis prs", "lass prs", "leest prs",
            "lis pee ars", "lass pee ars",
        ],
        "show prs": [
            "so prs", "show pers", "show pee ars",
            "so pee ars", "show p r s",
        ],
        "latest prs": [
            "latest pers", "latest pee ars", "latest p r s",
            "lates prs", "latest p r",
        ],
        "open prs": [
            "open pers", "open pee ars", "open p r s",
        ],
    }
    for correct, misheard_list in pr_mishearings.items():
        for misheard in misheard_list:
            examples.append({
                "text": misheard,
                "intent": "list_prs",
                "slots": {"repo": ""},
            })
            # Also add "show me the {misheard}" form
            examples.append({
                "text": f"show me the {misheard}",
                "intent": "list_prs",
                "slots": {"repo": ""},
            })

    return with_fillers(examples, max_variants=2)

def gen_get_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    pr_numbers = [5, 10, 23, 42, 99]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"get pr {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"show pr {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"tell me about pr {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"get pull request {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"show pull request {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"info about pr {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"details of pr {pr} in {repo}", "intent": "get_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_create_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    examples = []
    for repo in repos:
        examples.append({"text": f"create pr fix header from feature-branch to main in {repo}", "intent": "create_pr", "slots": {"repo": repo, "title": "fix header", "head": "feature-branch", "base": "main"}})
        examples.append({"text": f"create pr add tests from dev to main in {repo}", "intent": "create_pr", "slots": {"repo": repo, "title": "add tests", "head": "dev", "base": "main"}})
        examples.append({"text": f"create pull request update docs from feature to master in {repo}", "intent": "create_pr", "slots": {"repo": repo, "title": "update docs", "head": "feature", "base": "master"}})
        examples.append({"text": f"create pr fix bug from hotfix to main in {repo}", "intent": "create_pr", "slots": {"repo": repo, "title": "fix bug", "head": "hotfix", "base": "main"}})
        examples.append({"text": f"create pr new feature from feature-x to develop in {repo}", "intent": "create_pr", "slots": {"repo": repo, "title": "new feature", "head": "feature-x", "base": "develop"}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_update_branch():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    pr_numbers = [5, 10, 23, 42]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"update branch for pr {pr} in {repo}", "intent": "update_branch", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"update branch for {pr} in {repo}", "intent": "update_branch", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"update the branch for pr {pr} in {repo}", "intent": "update_branch", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"sync branch for pr {pr} in {repo}", "intent": "update_branch", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_revert_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    pr_numbers = [5, 10, 23, 42, 99]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"revert pr {pr} in {repo}", "intent": "revert_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"revert pull request {pr} in {repo}", "intent": "revert_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"revert the pr {pr} in {repo}", "intent": "revert_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"undo pr {pr} in {repo}", "intent": "revert_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"rollback pr {pr} in {repo}", "intent": "revert_pr", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_pr_files():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    pr_numbers = [5, 10, 23, 42]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            examples.append({"text": f"list pr files for pr {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"list pr files for {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"show pr files for pr {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"show files for pr {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"what files changed in pr {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
            examples.append({"text": f"list files in pr {pr} in {repo}", "intent": "list_pr_files", "slots": {"repo": repo, "pr_number": str(pr)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_comment_pr():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    pr_numbers = [5, 10, 23, 42]
    comments = ["looks good to me", "needs work", "approved", "please fix the tests", "great work", "can you update the docs"]
    examples = []
    for repo in repos:
        for pr in pr_numbers:
            for comment in comments:
                examples.append({"text": f"comment on pr {pr} in {repo}: {comment}", "intent": "comment_pr", "slots": {"repo": repo, "pr_number": str(pr), "body": comment}})
                examples.append({"text": f"comment pr {pr} in {repo}: {comment}", "intent": "comment_pr", "slots": {"repo": repo, "pr_number": str(pr), "body": comment}})
    return examples[:55]

def gen_add_collaborator():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    usernames = ["user1", "johndoe", "alice", "bob", "devuser"]
    permissions = ["admin", "push", "pull", "triage", "maintain"]
    examples = []
    for repo in repos:
        for username in usernames:
            examples.append({"text": f"add {username} as collaborator to {repo}", "intent": "add_collaborator", "slots": {"repo": repo, "username": username}})
            for perm in permissions:
                examples.append({"text": f"add {username} as {perm} collaborator to {repo}", "intent": "add_collaborator", "slots": {"repo": repo, "username": username}})
    return with_fillers(examples, max_variants=1)[:55]

def gen_remove_collaborator():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    usernames = ["user1", "johndoe", "alice", "bob", "devuser"]
    examples = []
    for repo in repos:
        for username in usernames:
            examples.append({"text": f"remove {username} as collaborator from {repo}", "intent": "remove_collaborator", "slots": {"repo": repo, "username": username}})
            examples.append({"text": f"remove {username} collaborator from {repo}", "intent": "remove_collaborator", "slots": {"repo": repo, "username": username}})
            examples.append({"text": f"remove {username} from {repo}", "intent": "remove_collaborator", "slots": {"repo": repo, "username": username}})
            examples.append({"text": f"delete {username} as collaborator from {repo}", "intent": "remove_collaborator", "slots": {"repo": repo, "username": username}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_collaborators():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    for repo in repos:
        examples.append({"text": f"list collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
        examples.append({"text": f"show collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
        examples.append({"text": f"get collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
        examples.append({"text": f"who are the collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
        examples.append({"text": f"list all collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
        examples.append({"text": f"show all collaborators in {repo}", "intent": "list_collaborators", "slots": {"repo": repo}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_add_org_member():
    orgs = ["myorg", "zync-meet", "eesh264"]
    usernames = ["user1", "johndoe", "alice", "bob", "devuser"]
    examples = []
    for org in orgs:
        for username in usernames:
            examples.append({"text": f"add {username} to org {org}", "intent": "add_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"add {username} as admin to org {org}", "intent": "add_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"add {username} as member to org {org}", "intent": "add_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"add {username} to organization {org}", "intent": "add_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"invite {username} to org {org}", "intent": "add_org_member", "slots": {"org": org, "username": username}})
    return with_fillers(examples, max_variants=1)[:55]

def gen_remove_org_member():
    orgs = ["myorg", "zync-meet", "eesh264"]
    usernames = ["user1", "johndoe", "alice", "bob", "devuser"]
    examples = []
    for org in orgs:
        for username in usernames:
            examples.append({"text": f"remove {username} from org {org}", "intent": "remove_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"remove {username} from organization {org}", "intent": "remove_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"delete {username} from org {org}", "intent": "remove_org_member", "slots": {"org": org, "username": username}})
            examples.append({"text": f"kick {username} from org {org}", "intent": "remove_org_member", "slots": {"org": org, "username": username}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_org_members():
    orgs = ["myorg", "zync-meet", "eesh264", "bigorg"]
    examples = []
    for org in orgs:
        examples.append({"text": f"list members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"list members of organization {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"show members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"get members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"who are the members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"list all members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"show all members of org {org}", "intent": "list_org_members", "slots": {"org": org}})
        examples.append({"text": f"list org members {org}", "intent": "list_org_members", "slots": {"org": org}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_delete_branch():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    branches = ["feature", "hotfix", "dev", "test-branch", "old-feature"]
    examples = []
    for repo in repos:
        for branch in branches:
            examples.append({"text": f"delete branch {branch} in {repo}", "intent": "delete_branch", "slots": {"repo": repo, "branch": branch}})
            examples.append({"text": f"remove branch {branch} in {repo}", "intent": "delete_branch", "slots": {"repo": repo, "branch": branch}})
            examples.append({"text": f"delete the branch {branch} in {repo}", "intent": "delete_branch", "slots": {"repo": repo, "branch": branch}})
            examples.append({"text": f"remove the branch {branch} in {repo}", "intent": "delete_branch", "slots": {"repo": repo, "branch": branch}})
            examples.append({"text": f"destroy branch {branch} in {repo}", "intent": "delete_branch", "slots": {"repo": repo, "branch": branch}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_branches():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    for repo in repos:
        examples.append({"text": f"list branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"show branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"get branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"what branches are in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"list all branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"show all branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
        examples.append({"text": f"list the branches in {repo}", "intent": "list_branches", "slots": {"repo": repo}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_create_release():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    tags = ["v1.0", "v2.0", "v1.5.0", "v3.0.0-beta", "v0.9.0"]
    examples = []
    for repo in repos:
        for tag in tags:
            examples.append({"text": f"create release {tag} in {repo}", "intent": "create_release", "slots": {"repo": repo, "release_tag": tag}})
            examples.append({"text": f"create a release {tag} in {repo}", "intent": "create_release", "slots": {"repo": repo, "release_tag": tag}})
            examples.append({"text": f"publish release {tag} in {repo}", "intent": "create_release", "slots": {"repo": repo, "release_tag": tag}})
            examples.append({"text": f"make release {tag} in {repo}", "intent": "create_release", "slots": {"repo": repo, "release_tag": tag}})
            examples.append({"text": f"new release {tag} in {repo}", "intent": "create_release", "slots": {"repo": repo, "release_tag": tag}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_releases():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    for repo in repos:
        examples.append({"text": f"list releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"show releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"get releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"what releases are in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"list all releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"show all releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
        examples.append({"text": f"list the releases in {repo}", "intent": "list_releases", "slots": {"repo": repo}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_workflows():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    for repo in repos:
        examples.append({"text": f"list workflows in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"show workflows in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"get workflows in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"what workflows are in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"list all workflows in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"show all workflows in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"list github actions in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
        examples.append({"text": f"show actions in {repo}", "intent": "list_workflows", "slots": {"repo": repo}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_list_workflow_runs():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi", "myorg/myrepo"]
    examples = []
    for repo in repos:
        examples.append({"text": f"list workflow runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"show workflow runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"get workflow runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"list runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"show runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"what are the recent workflow runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"list action runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
        examples.append({"text": f"show action runs in {repo}", "intent": "list_workflow_runs", "slots": {"repo": repo}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_rerun_workflow():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    run_ids = [123, 456, 789, 1001, 9999]
    examples = []
    for repo in repos:
        for run_id in run_ids:
            examples.append({"text": f"rerun workflow {run_id} in {repo}", "intent": "rerun_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"rerun the workflow {run_id} in {repo}", "intent": "rerun_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"retry workflow {run_id} in {repo}", "intent": "rerun_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"rerun action {run_id} in {repo}", "intent": "rerun_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"restart workflow {run_id} in {repo}", "intent": "rerun_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_cancel_workflow():
    repos = ["owner/repo", "zync-meet/zync", "eesh264/congi"]
    run_ids = [123, 456, 789, 1001, 9999]
    examples = []
    for repo in repos:
        for run_id in run_ids:
            examples.append({"text": f"cancel workflow {run_id} in {repo}", "intent": "cancel_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"cancel the workflow {run_id} in {repo}", "intent": "cancel_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"stop workflow {run_id} in {repo}", "intent": "cancel_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"abort workflow {run_id} in {repo}", "intent": "cancel_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
            examples.append({"text": f"halt workflow {run_id} in {repo}", "intent": "cancel_workflow", "slots": {"repo": repo, "workflow_id": str(run_id)}})
    return with_fillers(examples, max_variants=2)[:55]

def gen_unknown():
    """unknown — queries that don't match any command."""
    phrases = [
        "what is the weather like", "tell me a joke", "how are you today",
        "what time is it", "remind me to call mom", "set an alarm for 7 am",
        "what is the capital of france", "write an email to john",
        "translate hello to spanish", "send a message to alice",
        "what can you do", "who are you", "explain quantum computing",
        "what is the meaning of life", "tell me about yourself",
        "what's the news today", "how tall is mount everest",
        "convert 100 dollars to euros", "what is 2 plus 2",
        "tell me a fun fact", "what movies are playing",
        "how do i learn python", "what is machine learning",
        "recommend a good book", "what should i eat for dinner",
        "tell me a story", "sing me a song", "what is the temperature",
        "how far is the moon", "what is dark matter",
        "explain relativity", "what is blockchain",
        "how does wifi work", "what is quantum entanglement",
        "tell me about world war 2", "who won the world cup",
        "what is the speed of light", "how old is the earth",
        "what is the largest planet", "who wrote hamlet",
        "what is artificial intelligence", "how do computers work",
        "what is the internet", "explain neural networks",
        "what is a black hole", "how do vaccines work",
        "what is climate change", "who invented the telephone",
        "what is the stock market", "how does gps work",
    ]
    return [{"text": p, "intent": "unknown", "slots": {}} for p in phrases[:55]]

# ─── Main ───────────────────────────────────────────────────────────────────

def main():
    generators = [
        ("open_app", gen_open_app),
        ("open_url", gen_open_url),
        ("close_app", gen_close_app),
        ("whatsapp_chat", gen_whatsapp_chat),
        ("open_architect", gen_open_architect),
        ("search", gen_search),
        ("media_play_pause", gen_media_play_pause),
        ("media_next", gen_media_next),
        ("media_previous", gen_media_previous),
        ("media_stop", gen_media_stop),
        ("greeting", gen_greeting),
        ("analyse_repo", gen_analyse_repo),
        ("analyse_pr", gen_analyse_pr),
        ("analyse_latest_pr", gen_analyse_latest_pr),
        ("check_branch", gen_check_branch),
        ("merge_pr", gen_merge_pr),
        ("approve_pr", gen_approve_pr),
        ("close_pr", gen_close_pr),
        ("list_prs", gen_list_prs),
        ("get_pr", gen_get_pr),
        ("create_pr", gen_create_pr),
        ("update_branch", gen_update_branch),
        ("revert_pr", gen_revert_pr),
        ("list_pr_files", gen_list_pr_files),
        ("comment_pr", gen_comment_pr),
        ("add_collaborator", gen_add_collaborator),
        ("remove_collaborator", gen_remove_collaborator),
        ("list_collaborators", gen_list_collaborators),
        ("add_org_member", gen_add_org_member),
        ("remove_org_member", gen_remove_org_member),
        ("list_org_members", gen_list_org_members),
        ("delete_branch", gen_delete_branch),
        ("list_branches", gen_list_branches),
        ("create_release", gen_create_release),
        ("list_releases", gen_list_releases),
        ("list_workflows", gen_list_workflows),
        ("list_workflow_runs", gen_list_workflow_runs),
        ("rerun_workflow", gen_rerun_workflow),
        ("cancel_workflow", gen_cancel_workflow),
        ("unknown", gen_unknown),
    ]
    
    all_examples = []
    for intent_name, gen_func in generators:
        examples = gen_func()
        print(f"  {intent_name:25s}: {len(examples):3d} examples")
        all_examples.extend(examples)
    
    # Shuffle and split 85/15
    random.shuffle(all_examples)
    split = int(len(all_examples) * 0.85)
    train = all_examples[:split]
    test = all_examples[split:]
    
    data = {"train": train, "test": test}
    
    with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
    
    print(f"\nTotal: {len(all_examples)} examples")
    print(f"  Train: {len(train)}")
    print(f"  Test:  {len(test)}")
    print(f"Saved to {OUTPUT_PATH}")
    
    # Print per-intent distribution
    from collections import Counter
    train_dist = Counter(e["intent"] for e in train)
    print(f"\nTrain distribution:")
    for intent, count in sorted(train_dist.items()):
        print(f"  {intent:25s}: {count:3d}")

if __name__ == "__main__":
    main()
