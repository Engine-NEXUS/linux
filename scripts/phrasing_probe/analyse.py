import json, collections, sys
S = sys.argv[1]
inp = {}
for l in open(f"{S}/probe_in.jsonl", encoding="utf-8"):
    r = json.loads(l); inp[r["id"]] = r
out = {}
for l in open(f"{S}/probe_out.jsonl", encoding="utf-8"):
    r = json.loads(l); out[r["id"]] = r
meta = json.load(open(f"{S}/meta.json"))

# Expected labels after calibration against canonical phrases (what the parser calls each action today).
EXPECT = {
 "open_app": {"open_app", "open_settings", "open_url", "focus_app"}, "close_app": {"close_app"},
 "new_tab": {"browser_new_tab"}, "close_tab": {"browser_close_tab"}, "goto_tab_n": {"browser_tab"},
 "search_web": {"search"}, "address_bar": {"browser_search_focus", "system_click_element"},
 "type_text": {"type_text"}, "press_key": {"press_key", "press_hotkey"},
 "minimize": {"system_minimize_window"}, "maximize": {"system_maximize_window"},
 "click_element": {"system_click_element"}, "ghost_enter": {"enter_ghost_control"}, "ghost_exit": {"exit_ghost_control"},
 "stop_cancel": {"cancel_action", "media_stop"}, "confirm_send": {"confirm_send"},
 "media": {"media_play_pause", "media_next", "media_previous", "media_stop"},
}
# Actions with NO intent in the parser today (any non-"none" parse is a wrong parse).
NO_INTENT = [a for a in meta["counts"] if a not in EXPECT]
APPS = ["brave","chrome","notepad","spotify","calculator","discord","vs code","file explorer","task manager","whatsapp","word","excel"]

def verdict(r):
    a = inp[r["id"]]["action"]; lab = out[r["id"]]["label"]
    if a in EXPECT:
        if lab == "none": return "miss"
        return "ok" if lab in EXPECT[a] else "wrong"
    return "gap" if lab == "none" else "wrong"   # no intent exists

def pct(n, d): return f"{100*n/d:5.1f}%" if d else "  n/a"
def tally(rows):
    c = collections.Counter(verdict(r) for r in rows); n = len(rows); return c, n

by_action = collections.defaultdict(list)
for i, r in inp.items():
    if r["mode"] in ("real", "uniform", "noise"): by_action[r["action"]].append(r)

print("=== A. PER-ACTION, existing-intent actions (clean realistic / clean uniform-stress / noisy) ===")
print(f"{'action':14s} {'space(core)':>11s} {'real ok':>8s} {'uni ok':>8s} {'noise ok':>8s} | {'real miss/wrong':>15s} {'noise wrong':>11s}")
tot = collections.Counter(); totn = collections.Counter()
for a in EXPECT:
    rl = [r for r in by_action[a] if r["mode"] == "real"]; un = [r for r in by_action[a] if r["mode"] == "uniform"]
    nz = [r for r in by_action[a] if r["mode"] == "noise"]
    cr, nr = tally(rl); cu, nu = tally(un); cn, nn = tally(nz)
    print(f"{a:14s} {meta['counts'][a]['core']:>11d} {pct(cr['ok'],nr):>8s} {pct(cu['ok'],nu):>8s} {pct(cn['ok'],nn):>8s} | {cr['miss']:>6d}/{cr['wrong']:<6d} {pct(cn['wrong'],nn):>11s}")
    for k, v in [("real", (cr, nr)), ("uni", (cu, nu)), ("noise", (cn, nn))]:
        tot[(k, "ok")] += v[0]["ok"]; tot[(k, "miss")] += v[0]["miss"]; tot[(k, "wrong")] += v[0]["wrong"]; totn[k] += v[1]
for k in ("real", "uni", "noise"):
    print(f"  TOTAL {k:6s}: ok {pct(tot[(k,'ok')],totn[k])}  miss {pct(tot[(k,'miss')],totn[k])}  WRONG {pct(tot[(k,'wrong')],totn[k])}  (n={totn[k]})")

print("\n=== B. ACTIONS WITH NO INTENT TODAY (what happens when the user says them) ===")
print(f"{'action':14s} {'clean: clean-miss':>18s} {'clean: WRONG parse':>19s}   examples of wrong parse")
for a in NO_INTENT:
    rows = [r for r in by_action[a] if r["mode"] in ("real", "uniform")]
    c, n = tally(rows)
    ex = [(inp[r["id"]]["text"], out[r["id"]]["label"]) for r in rows if verdict(r) == "wrong"][:2]
    print(f"{a:14s} {pct(c['gap'],n):>18s} {pct(c['wrong'],n):>19s}   {ex}")
gt = collections.Counter()
for a in NO_INTENT:
    rows = [r for r in by_action[a] if r["mode"] in ("real", "uniform")]
    c, n = tally(rows); gt["gap"] += c["gap"]; gt["wrong"] += c["wrong"]; gt["n"] += n
print(f"  TOTAL no-intent actions: clean-miss {pct(gt['gap'],gt['n'])}  WRONG-parse {pct(gt['wrong'],gt['n'])}  (n={gt['n']})")

print("\n=== C. ASR-CHANNEL NOISE (existing-intent actions only): ok / miss / wrong ===")
byn = collections.defaultdict(list)
for a in EXPECT:
    for r in by_action[a]:
        if r["mode"] == "noise": byn[r["noise"]].append(r)
for nz, rows in byn.items():
    c, n = tally(rows); print(f"{nz:15s} ok {pct(c['ok'],n)}  miss {pct(c['miss'],n)}  WRONG {pct(c['wrong'],n)}  (n={n})")

print("\n=== D. WHICH LAYER BREAKS THINGS (clean uniform-stress, existing-intent actions): ok-rate by value ===")
rows = [r for a in EXPECT for r in by_action[a] if r["mode"] == "uniform"]
for layer in ("wake", "filler", "frame", "qual"):
    d = collections.defaultdict(lambda: [0, 0])
    for r in rows:
        v = inp[r["id"]][layer] or "(none)"; d[v][1] += 1; d[v][0] += verdict(r) == "ok"
    print(f"  {layer}: " + "  ".join(f"{k!r}:{100*v[0]/v[1]:.0f}%" for k, v in sorted(d.items(), key=lambda kv: kv[1][0]/kv[1][1])))

print("\n=== E. WORST VERB SYNONYMS per existing action (clean uniform, ok-rate < 60%) ===")
for a in EXPECT:
    d = collections.defaultdict(lambda: [0, 0])
    for r in by_action[a]:
        if r["mode"] == "uniform": v = inp[r["id"]]["verb"]; d[v][1] += 1; d[v][0] += verdict(r) == "ok"
    bad = {k: f"{100*v[0]/v[1]:.0f}%({v[1]})" for k, v in d.items() if v[1] >= 3 and v[0]/v[1] < 0.6}
    if bad: print(f"  {a}: {bad}")

print("\n=== F. SLOT ACCURACY when label is right ===")
ok_rows = [r for a in ("open_app","close_app","search_web","type_text") for r in by_action[a] if r["mode"] in ("real","uniform") and verdict(r)=="ok"]
good = 0; bad = []
for r in ok_rows:
    t = inp[r["id"]]["text"]; it = out[r["id"]]["intent"].lower()
    a = inp[r["id"]]["action"]
    if a in ("open_app","close_app"):
        app = next((x for x in sorted(APPS, key=len, reverse=True) if x in t), None)
        hit = (app is None) or (app.replace(" ","") in it.replace(" ","")) or a == "open_app" and ("opensettings" in it.replace(" ","").lower())
    else:
        hit = True
    good += hit
    if not hit and len(bad) < 5: bad.append((t, out[r["id"]]["intent"][:60]))
print(f"  open/close slot check: {good}/{len(ok_rows)}  mismatches e.g. {bad}")

print("\n=== G. LATENCY (debug build, per parse) ===")
us = sorted(o["us"] for o in out.values()); print(f"  p50 {us[len(us)//2]}us  p95 {us[int(len(us)*.95)]}us  p99 {us[int(len(us)*.99)]}us  max {us[-1]}us")

# ---------- real log + ambient ----------
print("\n=== H. REAL LOG REPLAY (126 transcripts that missed at the time) + AMBIENT (65 adversarial non-commands) ===")
o2 = [json.loads(l) for l in open(f"{S}/probe_out2.jsonl", encoding="utf-8")]
i2 = {json.loads(l)["id"]: json.loads(l) for l in open(f"{S}/probe_in2.jsonl", encoding="utf-8")}
for grp in ("reallog", "ambient"):
    rows = [o for o in o2 if i2[o["id"]]["mode"] == grp]
    acc = [o for o in rows if o["label"] != "none"]
    print(f"  {grp}: {len(rows)} rows, parsed-as-something {len(acc)} ({pct(len(acc),len(rows))}), clean-miss {len(rows)-len(acc)}")
    for o in acc: print(f"      {o['text']!r:70s} -> {o['label']}  {o['intent'][:60]}")
