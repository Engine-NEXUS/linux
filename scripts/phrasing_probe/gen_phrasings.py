"""Phrasing-space model for Ghost Mode commands.

Utterance = [wake] [filler] [frame] VERB [det] OBJECT [qualifier] [suffix]
Counts the surface-form space per action and samples it (realistic-weighted + uniform stress),
then applies an ASR-channel noise model. Output: JSONL for tests/phrasing_probe.rs + a counts table.
"""
import itertools, json, random, sys, os

random.seed(7)
OUT = sys.argv[1]

WAKE = ["", "nexus", "hey nexus", "okay nexus", "nexus,"]
FILLER = ["", "so", "and", "ok", "okay", "um", "uh", "now", "then", "also", "just", "alright", "well"]
FRAME = ["", "can you", "could you", "would you", "will you", "can you please", "please",
         "i want you to", "i need you to", "i'd like you to", "go ahead and", "let's", "you should", "try to"]
QUAL = ["", "please", "for me", "right now", "quickly", "now", "sir", "again", "in the browser", "here"]
# realistic weights: probability of picking the empty option for each optional layer
P_EMPTY = {"wake": 0.65, "filler": 0.75, "frame": 0.62, "qual": 0.78}

APPS = ["brave", "chrome", "notepad", "spotify", "calculator", "discord", "vs code", "file explorer",
        "task manager", "whatsapp", "word", "excel"]
QUERIES = ["almonds", "cats", "python tutorial", "weather today", "best laptops", "react hooks"]
TEXTS = ["hello world", "see you tomorrow", "meeting at five", "thank you so much", "okay sounds good"]
ELEMENTS = ["send", "search", "submit", "ok", "cancel", "login", "next", "save", "the first result",
            "the search bar", "close", "settings", "play", "download", "sign in", "add"]
KEYS = ["enter", "escape", "tab", "space", "backspace", "delete", "down", "up", "left", "right", "home", "end"]
NUMS = [("1", "one", "first"), ("2", "two", "second"), ("3", "three", "third"), ("4", "four", "fourth"),
        ("5", "five", "fifth"), ("6", "six", "sixth")]

# action -> dict(verbs, dets, objs, expect=set of acceptable labels (filled after calibration), needs=slot kind)
A = {}
def act(id_, verbs, dets, objs, kind=None, expect=None, group="existing"):
    A[id_] = dict(verbs=verbs, dets=dets, objs=objs, kind=kind, expect=expect or {id_}, group=group)

act("open_app", ["open", "launch", "start", "run", "fire up", "bring up", "pull up", "show me", "go to", "load"],
    ["", "the", "my"], ["{app}", "{app} app", "{app} application"], "app", {"open_app", "open_settings", "open_url", "focus_app"})
act("close_app", ["close", "quit", "exit", "kill", "shut down", "terminate", "end"],
    ["", "the"], ["{app}", "{app} app"], "app", {"close_app"})
act("new_tab", ["open", "create", "make", "start", "add", "give me", "get me", "launch", "open up"],
    ["", "a", "the", "another", "one more"], ["new tab", "tab", "blank tab", "fresh tab", "empty tab", "new browser tab"],
    None, {"browser_new_tab"}, "keyboard")
act("close_tab", ["close", "kill", "shut", "exit", "get rid of", "remove", "dismiss"],
    ["", "this", "the", "current"], ["tab", "current tab", "this tab", "page", "browser tab"], None, {"close_tab", "browser_close_tab"}, "keyboard")
act("reopen_tab", ["reopen", "restore", "bring back", "undo closing of", "open again"],
    ["", "the", "my"], ["last tab", "closed tab", "last closed tab", "tab i just closed", "previous tab i closed"], None,
    {"reopen_tab", "browser_reopen_tab"}, "keyboard")
act("next_tab", ["go to", "switch to", "move to", "jump to", "show", "open"],
    ["", "the"], ["next tab", "tab to the right", "right tab", "following tab"], None, {"next_tab", "browser_next_tab", "browser_tab"}, "keyboard")
act("prev_tab", ["go to", "switch to", "move to", "jump to", "show", "open", "go back to"],
    ["", "the"], ["previous tab", "last tab", "tab to the left", "left tab"], None, {"prev_tab", "browser_prev_tab", "browser_tab"}, "keyboard")
act("goto_tab_n", ["switch to", "go to", "open", "jump to", "move to", "show", "shift to"],
    ["", "the"], ["tab {n}", "tab number {n}", "{ord} tab", "{w} tab"], "num", {"browser_tab", "goto_tab", "switch_tab"}, "keyboard")
act("refresh", ["refresh", "reload", "update", "hit refresh on", "reload"],
    ["", "the", "this"], ["", "page", "tab", "site", "website"], None, {"refresh", "browser_refresh", "press_hotkey", "press_key"}, "keyboard")
act("go_back", ["go back", "back", "navigate back", "take me back", "go to the previous page", "previous page", "go backward"],
    [""], [""], None, {"go_back", "browser_back", "press_hotkey", "press_key"}, "keyboard")
act("go_forward", ["go forward", "forward", "navigate forward", "go to the next page", "next page", "go ahead one page"],
    [""], [""], None, {"go_forward", "browser_forward", "press_hotkey", "press_key"}, "keyboard")
act("search_web", ["search", "search for", "google", "look up", "look for", "search up", "search the web for", "find", "bing"],
    [""], ["{q}"], "query", {"search", "browser_search", "browser_navigate"}, "existing")
act("address_bar", ["focus", "select", "click", "go to", "click on", "highlight"],
    ["", "the"], ["address bar", "url bar", "search bar", "location bar", "omnibox"], None, {"browser_search_focus", "address_bar", "click_element"}, "keyboard")
act("find_in_page", ["find", "search", "look for", "locate"],
    [""], ["{q} on this page", "{q} in the page", "{q} on the page", "{q} here"], "query", {"find_in_page", "browser_find"}, "keyboard")
act("type_text", ["type", "write", "input", "dictate", "enter the text", "type out", "type in"],
    [""], ["{t}"], "text", {"type_text"}, "existing")
act("press_key", ["press", "hit", "tap", "push", "press the", "hit the", "press down"],
    [""], ["{k}", "{k} key"], "key", {"press_key", "press_hotkey"}, "existing")
for nm, objs in [("save", ["", "it", "this", "the file", "the document", "changes", "my work", "this file"]),
                 ("copy", ["", "it", "this", "that", "the text", "selection", "the selected text"]),
                 ("paste", ["", "it", "here", "that", "the text", "clipboard"]),
                 ("cut", ["", "it", "this", "that", "the selection"]),
                 ("undo", ["", "it", "that", "the last action", "last change", "the last thing"]),
                 ("redo", ["", "it", "that", "the last action"]),
                 ("select_all", ["", "everything", "all", "all text", "the whole page", "the whole document"])]:
    verbs = {"save": ["save", "store", "keep", "write out"], "copy": ["copy", "duplicate", "copy over"],
             "paste": ["paste", "put in", "insert"], "cut": ["cut", "remove and copy"],
             "undo": ["undo", "revert", "take back", "go back on"], "redo": ["redo", "repeat", "do again"],
             "select_all": ["select", "highlight", "mark", "choose"]}[nm]
    act(nm, verbs, [""], objs, None, {nm, "press_hotkey", "press_key"}, "keyboard")
act("scroll_down", ["scroll down", "scroll", "page down", "go down", "move down", "scroll down a bit", "scroll down a little", "scroll further down"],
    [""], [""], None, {"scroll_down", "scroll"}, "keyboard")
act("scroll_up", ["scroll up", "page up", "go up", "move up", "scroll up a bit", "scroll back up", "scroll to the top"],
    [""], [""], None, {"scroll_up", "scroll"}, "keyboard")
act("zoom_in", ["zoom in", "make it bigger", "enlarge", "increase zoom", "magnify", "make the text bigger"], [""], [""], None, {"zoom_in"}, "keyboard")
act("zoom_out", ["zoom out", "make it smaller", "shrink", "decrease zoom", "make the text smaller"], [""], [""], None, {"zoom_out"}, "keyboard")
act("minimize", ["minimize", "minimise", "hide", "shrink", "send to taskbar"], ["", "the", "this"], ["", "window", "app"], None, {"minimize_window", "minimize"}, "keyboard")
act("maximize", ["maximize", "maximise", "make fullscreen", "go fullscreen", "enlarge"], ["", "the", "this"], ["", "window", "app"], None, {"maximize_window", "maximize"}, "keyboard")
act("switch_window", ["switch window", "switch to the other window", "alt tab", "go to the other window", "next window", "switch apps", "switch to the previous app"],
    [""], [""], None, {"switch_window", "press_hotkey"}, "keyboard")
act("click_element", ["click", "click on", "press", "tap", "hit", "select", "choose", "push", "tap on"],
    ["", "the"], ["{e}", "{e} button"], "element", {"click_element", "click", "ghost_click"}, "mouse")
act("ghost_enter", ["ghost mode", "enter ghost mode", "activate ghost mode", "start ghost mode", "turn on ghost mode",
                    "go into ghost mode", "switch to ghost mode", "enable ghost mode", "take control", "ghost control"],
    [""], [""], None, {"enter_ghost_control"}, "existing")
act("ghost_exit", ["exit ghost mode", "stop ghost mode", "leave ghost mode", "turn off ghost mode", "close ghost mode",
                   "end ghost mode", "disable ghost mode", "cancel ghost mode", "quit ghost mode", "get out of ghost mode"],
    [""], [""], None, {"exit_ghost_control"}, "existing")
act("stop_cancel", ["stop", "cancel", "never mind", "abort", "halt", "stand down", "forget it", "stop that", "cancel that", "wait"],
    [""], [""], None, {"cancel_action", "media_stop", "greeting"}, "existing")
act("confirm_send", ["send", "send it", "send message", "yes send it", "go ahead and send", "send that", "fire it off"],
    [""], [""], None, {"confirm_send"}, "existing")
act("media", ["play", "pause", "resume", "next song", "skip", "previous song", "skip this song", "play the next song", "stop the music"],
    [""], [""], None, {"media_play_pause", "media_next", "media_previous", "media_stop"}, "existing")

def fill(tmpl, kind):
    if kind == "app":   return tmpl.format(app=random.choice(APPS))
    if kind == "query": return tmpl.format(q=random.choice(QUERIES))
    if kind == "text":  return tmpl.format(t=random.choice(TEXTS))
    if kind == "key":   return tmpl.format(k=random.choice(KEYS))
    if kind == "element": return tmpl.format(e=random.choice(ELEMENTS))
    if kind == "num":
        d, w, o = random.choice(NUMS)
        return tmpl.format(n=random.choice([d, w]), ord=o, w=w)
    return tmpl

def slot_count(kind):
    return {"app": len(APPS), "query": len(QUERIES), "text": len(TEXTS), "key": len(KEYS),
            "element": len(ELEMENTS), "num": len(NUMS) * 2}.get(kind, 1)

def space_size(a):
    core = len(a["verbs"]) * len(a["dets"]) * len(a["objs"]) * slot_count(a["kind"])
    return core, core * len(WAKE) * len(FILLER) * len(FRAME) * len(QUAL)

def pick(opts, key, weighted):
    if weighted and random.random() < P_EMPTY[key]:
        return ""
    return random.choice(opts)

LAST = {}
def make(aid, weighted):
    a = A[aid]
    verb, det = random.choice(a['verbs']), random.choice(a['dets'])
    obj = fill(random.choice(a['objs']), a['kind'])
    # grammaticality: no determiner before an object that already carries one, or before nothing
    if (not obj) or obj.split()[0] in ("this","current","the","last","next","previous","my","a","another","one","tab","right","left","following"):
        det = "" if (not obj or obj.split()[0] in ("this","current","the","my","a","another","one")) else det
    if det == "the" and obj.split()[0] in ("tab","right","left","following","last","next","previous"):
        pass
    core = f"{verb} {det} {obj}"
    wk, fl, fr, ql = pick(WAKE, "wake", weighted), pick(FILLER, "filler", weighted), pick(FRAME, "frame", weighted), pick(QUAL, "qual", weighted)
    LAST.clear(); LAST.update(wake=wk, filler=fl, frame=fr, verb=verb, qual=ql)
    parts = [wk, fl, fr, core, ql]
    return " ".join(" ".join(p for p in parts if p).split())

# ---- ASR channel noise model ----
HOMO = {"to": ["two", "too"], "for": ["four"], "tab": ["tap", "tub", "tag"], "brave": ["wave", "rave", "grave", "bravo"],
        "open": ["opened", "oven", "opens"], "new": ["knew", "nu"], "close": ["clothes", "chose"], "whatsapp": ["what's up", "watts app"],
        "search": ["surge", "sir"], "scroll": ["scroll", "stroll"], "click": ["clique", "clip"], "save": ["safe"], "copy": ["coffee"],
        "paste": ["based", "taste"], "undo": ["and do"], "refresh": ["a fresh"], "next": ["nest", "text"], "back": ["bag", "pack"],
        "type": ["tight", "tie"], "mode": ["mold", "mood"], "ghost": ["goes", "post", "coast"], "hub": ["hop", "hup"], "stop": ["stuff"]}
def noise(text, kind):
    w = text.split()
    if kind == "case_punct": return (text[:1].upper() + text[1:] + random.choice([".", "?", "!", ""]))
    if kind == "trunc_last" and len(w) > 1: return " ".join(w[:-1])
    if kind == "head_clip" and len(w) > 1: return " ".join(w[1:])
    if kind == "homophone":
        idx = [i for i, x in enumerate(w) if x in HOMO]
        if idx:
            i = random.choice(idx); w[i] = random.choice(HOMO[w[i]])
        return " ".join(w)
    if kind == "mid_filler" and len(w) > 1:
        w.insert(random.randint(1, len(w) - 1), random.choice(["uh", "um", "like", "you know"])); return " ".join(w)
    if kind == "stutter":
        i = random.randrange(len(w)); w.insert(i, w[i]); return " ".join(w)
    if kind == "sentence_split" and len(w) > 2:
        i = random.randint(1, len(w) - 1); return (" ".join(w[:i]) + ". " + " ".join(w[i:])).capitalize()
    return text
NOISES = ["clean", "case_punct", "trunc_last", "head_clip", "homophone", "mid_filler", "stutter", "sentence_split"]

N_REAL, N_UNI, N_NOISE = 120, 200, 60
rows, counts = [], {}
for aid, a in A.items():
    core, full = space_size(a)
    counts[aid] = dict(core=core, full=full, group=a["group"])
    for i in range(N_REAL):  t=make(aid, True); rows.append(dict(id=f"{aid}|real|{i}", action=aid, mode="real", noise="clean", text=t, **LAST))
    for i in range(N_UNI):   t=make(aid, False); rows.append(dict(id=f"{aid}|uniform|{i}", action=aid, mode="uniform", noise="clean", text=t, **LAST))
    for nz in NOISES[1:]:
        for i in range(N_NOISE):
            base = make(aid, True)
            rows.append(dict(id=f"{aid}|noise-{nz}|{i}", action=aid, mode="noise", noise=nz, text=noise(base, nz)))
# canonical (calibration) phrases: shortest verb x first obj, no layers
for aid, a in A.items():
    rows.append(dict(id=f"{aid}|canon|0", action=aid, mode="canon", noise="clean", text=" ".join(
        f"{a['verbs'][0]} {a['dets'][0]} {fill(a['objs'][0], a['kind'])}".split())))

with open(os.path.join(OUT, "probe_in.jsonl"), "w", encoding="utf-8") as f:
    for r in rows: f.write(json.dumps(r) + "\n")
json.dump(dict(counts=counts, expect={k: sorted(v["expect"]) for k, v in A.items()},
               layers=dict(wake=len(WAKE), filler=len(FILLER), frame=len(FRAME), qual=len(QUAL))),
          open(os.path.join(OUT, "meta.json"), "w"), indent=1)
print(len(rows), "rows;", len(A), "actions")
tot_core = sum(c["core"] for c in counts.values()); tot_full = sum(c["full"] for c in counts.values())
print("sum core space", tot_core, " sum full space", tot_full)
