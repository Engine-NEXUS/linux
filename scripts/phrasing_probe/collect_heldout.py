"""Collect HELD-OUT phrasings: how *you* would say each Ghost Mode action, in your own words.

Why: a test corpus generated from the same lexicon the grammar uses passes by construction. Honest accuracy
needs phrasings the lexicon author never saw (docs/features/103 §6). Run this once (≈10 minutes), type naturally.

Usage:  python scripts/phrasing_probe/collect_heldout.py [--out tests/fixtures/heldout_user.jsonl]
        Press Enter on an empty line to move on; type 'skip' to skip an action; Ctrl+C saves what you have.

Output rows: {"action","text","source":"user","ts"}. Plain text, stays on this machine, never uploaded.
Do NOT paste anything private: these are short commands, not messages.
"""
import argparse, json, os, sys, time

PROMPTS = [
    ("open_app", "open an application (e.g. Chrome, Notepad)"),
    ("close_app", "close an application"),
    ("new_tab", "open a new browser tab"),
    ("close_tab", "close the current browser tab"),
    ("reopen_tab", "bring back the tab you just closed"),
    ("next_tab", "go to the next browser tab"),
    ("prev_tab", "go to the previous browser tab"),
    ("goto_tab_n", "jump to tab number 3"),
    ("refresh", "reload the page"),
    ("go_back", "go back one page"),
    ("go_forward", "go forward one page"),
    ("search_web", "search the web for 'almonds'"),
    ("address_bar", "put the cursor in the address bar"),
    ("find_in_page", "find the word 'price' on this page"),
    ("type_text", "type 'hello world' into the current field"),
    ("press_key", "press the Enter key"),
    ("save", "save the current file"),
    ("copy", "copy the selected text"),
    ("paste", "paste what you copied"),
    ("cut", "cut the selected text"),
    ("undo", "undo the last thing"),
    ("redo", "redo it"),
    ("select_all", "select everything"),
    ("scroll_down", "scroll down"),
    ("scroll_up", "scroll up"),
    ("zoom_in", "make the page bigger"),
    ("zoom_out", "make the page smaller"),
    ("minimize", "minimise the current window"),
    ("maximize", "maximise / fullscreen the current window"),
    ("switch_window", "switch to the other window"),
    ("click_element", "click a button named 'Submit'"),
    ("ghost_enter", "turn on Ghost Mode"),
    ("ghost_exit", "turn off Ghost Mode"),
    ("stop_cancel", "stop / cancel what NEXUS is doing"),
    ("confirm_send", "send the message you just typed"),
    ("media", "pause / skip the music"),
]


def main():
    ap = argparse.ArgumentParser()
    here = os.path.dirname(os.path.abspath(__file__))
    ap.add_argument("--out", default=os.path.join(here, "..", "..", "src-tauri", "tests", "fixtures", "heldout_user.jsonl"))
    ap.add_argument("--per-action", type=int, default=3)
    args = ap.parse_args()
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    n = 0
    print(__doc__.split("Usage:")[0])
    try:
        with open(args.out, "a", encoding="utf-8") as f:
            for i, (aid, desc) in enumerate(PROMPTS, 1):
                print(f"\n[{i}/{len(PROMPTS)}] How would YOU tell NEXUS to: {desc}?")
                for k in range(args.per_action):
                    t = input(f"   way {k + 1}: ").strip()
                    if not t:
                        break
                    if t.lower() == "skip":
                        break
                    f.write(json.dumps(dict(action=aid, text=t, source="user", ts=int(time.time())), ensure_ascii=False) + "\n")
                    f.flush()
                    n += 1
    except (KeyboardInterrupt, EOFError):
        print("\nstopped early — saved what you entered")
    print(f"\n{n} phrasings appended to {os.path.abspath(args.out)}")


if __name__ == "__main__":
    sys.exit(main())
