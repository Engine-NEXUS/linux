#!/usr/bin/env python3

import argparse
import json
from collections import defaultdict
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DATASET_PATH = SCRIPT_DIR / "dataset.json"
LABELS_PATH = SCRIPT_DIR / "model" / "labels.json"

CORRUPT_ASR_TEXTS = {
    "dit van vox.",
    "right. thanks for your help.",
    "type this, the meeting.",
    "hello, bird.",
    "hi, please review this code.",
    "great hallo world.",
    "right please review this code",
    "right, thanks a lot.",
    "right, thanks for your help.",
    "let's meet at ibm.",
    "спасибо, леван.",
    "yes, sir.",
    "this max space.",
    "пресс-хаб.",
    "this is backspace.",
    "let's backspace.",
}

LIVE_TEST_CASES = {
    "type_text": [
        ("could you type deployment finished", {"text": "deployment finished"}),
        ("write this down review the latest changes", {"text": "review the latest changes"}),
        ("please enter hello from nexus", {"text": "hello from nexus"}),
        ("type in the meeting starts at nine", {"text": "the meeting starts at nine"}),
        ("put this text there build completed successfully", {"text": "build completed successfully"}),
    ],
    "press_key": [
        ("could you press escape", {"key": "escape"}),
        ("hit the return key", {"key": "return"}),
        ("tap backspace once", {"key": "backspace"}),
        ("press page down", {"key": "page down"}),
        ("please hit f ten", {"key": "f ten"}),
    ],
    "press_hotkey": [
        ("hold control and press c", {"keys": ["control", "c"]}),
        ("use alt and tab together", {"keys": ["alt", "tab"]}),
        ("press control shift escape", {"keys": ["control", "shift", "escape"]}),
        ("hit command shift p", {"keys": ["command", "shift", "p"]}),
        ("use windows key and l", {"keys": ["windows", "l"]}),
    ],
    "confirm_send": [
        ("yes submit that now", {}),
        ("that looks right send it", {}),
        ("confirm and continue", {}),
        ("okay deliver the message", {}),
        ("proceed with sending", {}),
    ],
    "cancel_action": [
        ("actually cancel the current action", {}),
        ("wait do not send that", {}),
        ("abort this operation", {}),
        ("hold on I changed my mind", {}),
        ("stop before doing that", {}),
    ],
    "browser_new_tab": [
        ("give me another browser tab", {}),
        ("create one fresh tab", {}),
        ("start a blank browser tab", {}),
        ("please add another tab", {}),
        ("I need a separate browser window", {}),
    ],
    "browser_navigate": [
        ("load github dot com in this tab", {"url": "github dot com"}),
        ("take this browser to docs dot rs", {"url": "docs dot rs"}),
        ("navigate over to crates dot io", {"url": "crates dot io"}),
        ("visit hugging face dot co here", {"url": "hugging face dot co"}),
        ("browse to stack overflow dot com", {"url": "stack overflow dot com"}),
    ],
    "browser_search": [
        ("look up rust ownership in this browser", {"query": "rust ownership"}),
        ("search the web for tauri commands", {"query": "tauri commands"}),
        ("google how to resolve a merge conflict here", {"query": "how to resolve a merge conflict"}),
        ("find on the web typescript generics", {"query": "typescript generics"}),
        ("browser search for github workflow syntax", {"query": "github workflow syntax"}),
    ],
    "whatsapp_open": [
        ("bring up my whatsapp application", {}),
        ("start the whatsapp desktop client", {}),
        ("show me whatsapp please", {}),
        ("launch my whatsapp now", {}),
        ("open the messaging app whatsapp", {}),
    ],
    "whatsapp_search": [
        ("locate priya in whatsapp", {"contact": "priya"}),
        ("search my whatsapp chats for james", {"contact": "james"}),
        ("find the conversation with john on whatsapp", {"contact": "john"}),
        ("show the whatsapp chat with sarah", {"contact": "sarah"}),
        ("look for dad in my whatsapp contacts", {"contact": "dad"}),
    ],
    "focus_app": [
        ("bring visual studio code into focus", {"target": "visual studio code"}),
        ("switch over to the terminal window", {"target": "terminal"}),
        ("put chrome in the foreground", {"target": "chrome"}),
        ("activate my notepad window", {"target": "notepad"}),
        ("return focus to firefox", {"target": "firefox"}),
    ],
}


def normalize(text):
    return " ".join(text.lower().strip().split())


def canonical_intent(text, intents):
    labels = set(intents)
    if len(labels) == 1:
        return next(iter(labels))
    if labels == {"browser_navigate", "open_url"}:
        return "browser_navigate"
    if text == "stop" and labels == {"cancel_action", "media_stop"}:
        return "cancel_action"
    if text == "stop playing" and labels == {"media_play_pause", "media_stop"}:
        return "media_stop"
    if text == "pause it" and labels == {"cancel_action", "media_play_pause"}:
        return "media_play_pause"
    if labels == {"greeting", "unknown"}:
        return "unknown"
    if labels == {"confirm_send", "greeting"}:
        return "confirm_send"
    if labels == {"open_app", "open_settings"}:
        return "open_settings"
    if labels == {"whatsapp_chat", "whatsapp_search"}:
        return "whatsapp_chat"
    if "whatsapp_open" in labels:
        if text.startswith(("focus ", "switch ", "bring ", "activate ", "restore ")) and "focus_app" in labels:
            return "focus_app"
        return "whatsapp_open"
    raise ValueError(f"No canonical policy for {text!r}: {sorted(labels)}")


def repair(dataset_path, apply):
    data = json.loads(dataset_path.read_text(encoding="utf-8"))
    labels = json.loads(LABELS_PATH.read_text(encoding="utf-8"))
    allowed_slots = {tag[2:] for tag in labels["slots"] if tag.startswith("B-")}
    train = data.get("train", [])
    test = list(data.get("test", []))
    before_test = len(test)
    stats = defaultdict(int)

    repaired_train = []
    for row in train:
        text = normalize(row.get("text", ""))
        if text in CORRUPT_ASR_TEXTS:
            stats["corrupt_asr_removed"] += 1
            continue
        slots = row.get("slots") if isinstance(row.get("slots"), dict) else {}
        invalid = [slot for slot in slots if slot not in allowed_slots]
        for slot in invalid:
            del slots[slot]
            stats["invalid_slots_removed"] += 1
        if text == "press ctrl shift d." and row.get("intent") == "press_hotkey":
            slots["keys"] = ["ctrl", "shift", "d"]
            stats["incorrect_slots_corrected"] += 1
        row["slots"] = slots
        repaired_train.append(row)

    by_text = defaultdict(list)
    for row in repaired_train:
        by_text[normalize(row["text"])].append(row)
    conflict_resolution = {}
    for text, rows in by_text.items():
        intents = {row["intent"] for row in rows}
        if len(intents) > 1:
            conflict_resolution[text] = canonical_intent(text, intents)

    resolved_train = []
    for row in repaired_train:
        text = normalize(row["text"])
        canonical = conflict_resolution.get(text)
        if canonical and row["intent"] != canonical:
            stats["conflicting_rows_removed"] += 1
            continue
        resolved_train.append(row)

    existing_test = {(normalize(row["text"]), row["intent"]) for row in test}
    for intent, cases in LIVE_TEST_CASES.items():
        for text, slots in cases:
            key = (normalize(text), intent)
            if key not in existing_test:
                test.append({"text": text, "intent": intent, "slots": slots})
                existing_test.add(key)
                stats["live_test_examples_added"] += 1

    test_texts = {normalize(row["text"]) for row in test}
    no_leak_train = []
    for row in resolved_train:
        if normalize(row["text"]) in test_texts:
            stats["train_test_leaks_removed"] += 1
            continue
        no_leak_train.append(row)

    seen = set()
    deduped_train = []
    for row in no_leak_train:
        key = (normalize(row["text"]), row["intent"], json.dumps(row.get("slots", {}), sort_keys=True))
        if key in seen:
            stats["duplicate_rows_removed"] += 1
            continue
        seen.add(key)
        deduped_train.append(row)

    output = dict(data)
    output["train"] = deduped_train
    output["test"] = test
    if apply:
        dataset_path.write_text(json.dumps(output, indent=2, ensure_ascii=False), encoding="utf-8")
    return {
        "applied": apply,
        "before_train": len(train),
        "after_train": len(deduped_train),
        "before_test": before_test,
        "after_test": len(test),
        **dict(stats),
    }


def main():
    parser = argparse.ArgumentParser(description="Repair confirmed NEXUS NLU dataset defects")
    parser.add_argument("--apply", action="store_true", help="Write repairs to dataset.json")
    parser.add_argument("--dataset", type=Path, default=DATASET_PATH)
    args = parser.parse_args()
    result = repair(args.dataset, args.apply)
    print(json.dumps(result, indent=2))
    if not args.apply:
        print("Dry run only. Re-run with --apply to write changes.")


if __name__ == "__main__":
    main()
