import json, collections, sys, random
exec(open('analyse.py',encoding='utf-8').read().split("def pct")[0].replace("S = sys.argv[1]","S = r'%s'" % sys.argv[1]))
def pct(n,d): return f"{100*n/d:5.1f}%" if d else "  n/a"
LAYERS=("wake","filler","frame","qual")
rows=[r for r in inp.values() if r["mode"] in("real","uniform") and r["action"] in EXPECT]
def nl(r): return sum(1 for l in LAYERS if r[l])
print("=== accuracy by number of extra layers (wake/filler/frame/qual) present, existing-intent actions ===")
for k in range(0,5):
    sub=[r for r in rows if nl(r)==k]
    c=collections.Counter(verdict(r) for r in sub); n=len(sub)
    if n: print(f" {k} layers: n={n:5d}  ok {pct(c['ok'],n)}  miss {pct(c['miss'],n)}  wrong {pct(c['wrong'],n)}")
print("\n=== exactly ONE layer present: ok-rate by layer ===")
for l in LAYERS:
    sub=[r for r in rows if nl(r)==1 and r[l]]
    c=collections.Counter(verdict(r) for r in sub); n=len(sub)
    print(f" {l:7s} n={n:4d}  ok {pct(c['ok'],n)}")
print("\n=== CORE ONLY (0 layers): per action ok-rate, with a few failing examples ===")
tot=collections.Counter()
for a in EXPECT:
    sub=[r for r in rows if r["action"]==a and nl(r)==0]
    c=collections.Counter(verdict(r) for r in sub); n=len(sub)
    tot['n']+=n; tot['ok']+=c['ok']; tot['wrong']+=c['wrong']
    fails=[(r['text'],out[r['id']]['label']) for r in sub if verdict(r)!='ok']
    random.seed(1); ex=random.sample(fails,min(3,len(fails)))
    print(f" {a:13s} n={n:3d} ok {pct(c['ok'],n)} wrong {c['wrong']:2d}  e.g. {ex}")
print(f" TOTAL core-only: n={tot['n']}  ok {pct(tot['ok'],tot['n'])}  wrong {pct(tot['wrong'],tot['n'])}")
print("\n=== polite-frame ONLY (single layer) examples ===")
sub=[r for r in rows if nl(r)==1 and r['frame']][:12]
for r in sub: print("  ",repr(r['text']),'->',out[r['id']]['label'], verdict(r))
