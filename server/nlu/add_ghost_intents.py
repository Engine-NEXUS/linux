#!/usr/bin/env python3
"""
NEXUS NLU — Add Ghost Mode, Dictation, Browser Navigation, and Email Watch
training examples to dataset.json and synchronize labels.json and nlu_server.py.
"""

import json
import random
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
LABELS_PATH = SCRIPT_DIR / "model" / "labels.json"
RESOURCES_LABELS_PATH = SCRIPT_DIR.parent.parent / "src-tauri" / "resources" / "server" / "nlu" / "model" / "labels.json"
NLU_SERVER_PATH = SCRIPT_DIR.parent.parent / "src-tauri" / "resources" / "server" / "nlu_server.py"

# Comprehensive intent datasets
NEW_INTENTS_DATA = {
    "ghost_mode_enter": [
        "ghost mode",
        "enter ghost mode",
        "start ghost mode",
        "turn on ghost mode",
        "activate ghost mode",
        "switch to ghost mode",
        "enable ghost mode",
        "go into ghost mode",
        "nexus ghost mode",
        "the ghost mode",
        "post mode",
        "the post mode",
        "host mode",
        "coast mode",
        "take over",
        "take control",
        "take over the screen",
        "take the wheel",
        "ghost control",
        "ghost mode please",
        "can you enter ghost mode",
        "please turn on ghost mode",
        "initiate ghost mode",
        "switch to ghost control",
        "launch ghost mode",
        "open ghost mode",
        "put it in ghost mode",
        "start ghost control",
        "ghost takeover",
        "hands on mode",
        "assist me in ghost mode",
        "jump into ghost mode",
        "nexus enter ghost mode",
        "nexus start ghost mode",
        "nexus activate ghost mode",
        "hey nexus enter ghost mode",
        "take over my computer",
        "take over control",
        "enter into ghost mode",
        "begin ghost mode",
        "run ghost mode",
        "i need ghost mode",
        "give me ghost mode",
        "turn ghost mode on",
        "switch ghost mode on",
        "start takeover",
        "activate ghost",
        "ghost mode now",
        "please start ghost mode",
        "ghost mode right now"
    ],
    "ghost_mode_exit": [
        "exit ghost mode",
        "stop ghost mode",
        "turn off ghost mode",
        "quit ghost mode",
        "leave ghost mode",
        "disable ghost mode",
        "end ghost mode",
        "cancel ghost mode",
        "the ghost mode exit",
        "exit the ghost mode",
        "release control",
        "hands off",
        "stop taking control",
        "back to normal mode",
        "return to normal mode",
        "give me back control",
        "i have control now",
        "stop ghost control",
        "nexus exit ghost mode",
        "nexus stop ghost mode",
        "turn ghost mode off",
        "switch ghost mode off",
        "deactivate ghost mode",
        "terminate ghost mode",
        "abort ghost mode",
        "stop takeover",
        "close ghost mode",
        "get out of ghost mode",
        "done with ghost mode",
        "finish ghost mode",
        "take back control",
        "stop controlling",
        "back to normal",
        "exit ghost",
        "leave ghost",
        "stop ghost",
        "normal mode please",
        "please exit ghost mode",
        "nexus turn off ghost mode",
        "shut off ghost mode"
    ],
    "start_dictation": [
        "type whatever i say",
        "start typing",
        "type line by line",
        "start dictation",
        "begin dictation",
        "start dictating",
        "begin typing",
        "take dictation",
        "write what i say",
        "type what i say",
        "listen and type",
        "transcribe what i say",
        "dictation mode",
        "enter dictation mode",
        "turn on dictation",
        "start transcribing",
        "type as i speak",
        "write down what i say",
        "please start typing",
        "type my words",
        "take down what i say",
        "start voice typing",
        "voice dictation on",
        "enable dictation",
        "can you type whatever i say",
        "type this line by line",
        "record and type",
        "start taking notes",
        "start typing text",
        "type line by line please"
    ],
    "stop_dictation": [
        "stop typing",
        "stop dictation",
        "stop dictating",
        "finish dictation",
        "end dictation",
        "end typing",
        "done typing",
        "stop typing now",
        "pause dictation",
        "quit dictation",
        "cancel dictation",
        "disable dictation",
        "turn off dictation",
        "close dictation",
        "exit dictation mode",
        "done with dictation",
        "stop voice typing",
        "finish typing",
        "stop writing",
        "done with typing",
        "nexus stop typing",
        "nexus stop dictation",
        "please stop typing",
        "stop taking notes",
        "enough typing",
        "stop typing please"
    ],
    "watch_screen_email": [
        "watch this email",
        "track deadline changes in this email",
        "update me whenever there is any update on the deadline",
        "notify me when this email updates",
        "track this email",
        "watch deadline in this email",
        "monitor this email",
        "keep an eye on this email",
        "watch for email updates",
        "track the deadline for this email",
        "update me on this email deadline",
        "notify me if the deadline changes",
        "watch this thread",
        "monitor this email thread",
        "track updates to this email",
        "track changes in this email",
        "let me know if this deadline moves",
        "watch email deadline",
        "monitor deadline for this email",
        "track email deadline changes",
        "keep track of this email",
        "watch for replies to this email",
        "notify me about this email",
        "alert me if deadline changes in this email",
        "watch this email for me",
        "nexus watch this email",
        "nexus track this email deadline",
        "please watch this email thread",
        "alert me on email updates",
        "follow this email thread"
    ],
    "browser_tab": [
        "tab 1", "tab 2", "tab 3", "tab 4", "tab 5",
        "switch to tab 1", "switch to tab 2", "switch to tab 3",
        "go to tab 1", "go to tab 2", "go to tab 3",
        "next tab", "previous tab", "first tab", "second tab",
        "move to tab 2", "switch to second tab", "go to first tab",
        "jump to tab 3", "switch tab to 4", "open tab 2",
        "switch to the next tab", "switch to the previous tab"
    ],
    "browser_close_tab": [
        "close tab", "close this tab", "close the tab",
        "close current tab", "shut tab", "kill this tab",
        "exit tab", "close active tab", "dismiss this tab",
        "close tab 1", "close tab 2", "close tab 3"
    ],
    "browser_search_focus": [
        "search", "search bar", "focus address bar",
        "focus search bar", "go to search bar", "click search bar",
        "activate search bar", "open search bar", "focus the address bar",
        "address bar please", "jump to address bar", "ready to search",
        "highlight address bar", "focus omnibox", "go to omnibox"
    ]
}


def main():
    print(f"Loading dataset from {DATASET_PATH}...")
    with open(DATASET_PATH, "r", encoding="utf-8") as f:
        data = json.load(f)

    train_data = data.setdefault("train", [])
    val_data = data.setdefault("validation", [])
    test_data = data.setdefault("test", [])

    existing_texts = {
        split_name: {item["text"].strip().lower() for item in data[split_name]}
        for split_name in ["train", "validation", "test"]
    }

    added_counts = {intent: 0 for intent in NEW_INTENTS_DATA}

    for intent, phrases in NEW_INTENTS_DATA.items():
        random.seed(42)
        shuffled = list(phrases)
        random.shuffle(shuffled)

        n = len(shuffled)
        n_test = max(2, int(n * 0.15))
        n_val = max(2, int(n * 0.15))
        n_train = n - n_test - n_val

        splits = {
            "test": shuffled[:n_test],
            "validation": shuffled[n_test:n_test + n_val],
            "train": shuffled[n_test + n_val:]
        }

        for split_name, split_phrases in splits.items():
            target_list = data[split_name]
            for phrase in split_phrases:
                p_lower = phrase.strip().lower()
                if p_lower not in existing_texts[split_name]:
                    target_list.append({
                        "text": phrase,
                        "intent": intent,
                        "slots": {}
                    })
                    existing_texts[split_name].add(p_lower)
                    added_counts[intent] += 1

    print("Added samples per intent:")
    for intent, count in added_counts.items():
        print(f"  {intent}: +{count}")

    print(f"Saving updated dataset to {DATASET_PATH}...")
    with open(DATASET_PATH, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)

    # Synchronize labels.json
    intents_to_add = list(NEW_INTENTS_DATA.keys())
    for lp in [LABELS_PATH, RESOURCES_LABELS_PATH]:
        if lp.exists():
            with open(lp, "r", encoding="utf-8") as f:
                l_data = json.load(f)
            current_intents = l_data.get("intents", [])
            for new_intent in intents_to_add:
                if new_intent not in current_intents:
                    # Insert before 'unknown' if present, else append
                    if "unknown" in current_intents:
                        idx = current_intents.index("unknown")
                        current_intents.insert(idx, new_intent)
                    else:
                        current_intents.append(new_intent)
            l_data["intents"] = current_intents
            with open(lp, "w", encoding="utf-8") as f:
                json.dump(l_data, f, indent=2, ensure_ascii=False)
            print(f"Synchronized {lp} (total intents: {len(current_intents)})")

    print("\nDataset and labels successfully updated for Ghost Mode & Voice alignment!")


if __name__ == "__main__":
    main()
