#!/usr/bin/env python3
"""P4 SLO board collector â€” assembles the ring-promotion board from
on-device artifacts. Read-only; never modifies data.

Sources:
  - %APPDATA%/com.nexus.assistant/missed_intents.jsonl  (parse misses)
  - %APPDATA%/com.nexus.assistant/suggested_phrases.json (weekly top-10)
  - %APPDATA%/com.nexus.assistant/diary.jsonl           (wake/quota/fail events)
  - %APPDATA%/com.nexus.assistant/nexus_unified.log     (watchdog pokes,
    hallucination filter hits, silence-recovery restarts, quota denials)
  - P0 fixtures: printed reminder (run via cargo test / soak script)

Usage: python scripts/collect_slo.py [--days N]
"""
import argparse
import json
import re
import sys
from collections import Counter
from datetime import datetime, timedelta
from pathlib import Path

APPDATA = Path.home() / "AppData" / "Roaming" / "com.nexus.assistant"


def read_tail(path, nbytes=2_000_000):
    try:
        with open(path, "rb") as f:
            f.seek(0, 2)
            size = f.tell()
            f.seek(max(0, size - nbytes))
            return f.read().decode("utf-8", "replace")
    except OSError:
        return ""


def within_days(iso_or_ms, days, now):
    try:
        ts = float(iso_or_ms)
        if ts > 1e12:  # ms epoch
            ts /= 1000.0
        return (now - datetime.fromtimestamp(ts)).days <= days
    except (ValueError, TypeError, OSError, OverflowError):
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--days", type=int, default=7)
    args = ap.parse_args()
    now = datetime.now()
    cutoff = now - timedelta(days=args.days)

    print("=" * 68)
    print(f"  NEXUS SLO BOARD (ring gate) â€” window: last {args.days} days")
    print(f"  generated {now:%Y-%m-%d %H:%M}")
    print("=" * 68)

    # 1. Parse-miss volume + trend (missed_intents.jsonl)
    misses_recent = 0
    misses_total = 0
    lines = read_tail(APPDATA / "missed_intents.jsonl", 5_000_000)
    for line in lines.splitlines():
        if not line.strip():
            continue
        try:
            rec = json.loads(line)
        except json.JSONDecodeError:
            continue
        misses_total += 1
        ts = rec.get("ts") or rec.get("timestamp")
        if ts is not None and within_days(ts, args.days, now):
            misses_recent += 1
    print(f"\n[Parse misses] total={misses_total}  last {args.days}d={misses_recent}")
    print("  (trend down week-over-week = healthy; SLA: mine â†’ phrase candidate â‰¤ 7d)")

    # 2. Weekly top-10 promotion candidates
    sugg = APPDATA / "suggested_phrases.json"
    try:
        clusters = json.loads(sugg.read_text("utf-8"))
    except (OSError, json.JSONDecodeError):
        clusters = []
    print(f"\n[Weekly top-{min(10, len(clusters))} promotion candidates]")
    for c in clusters[:10]:
        print(f"  {c.get('count', 0):>3}Ã— {c.get('example', '')[:60]}")

    # 3. Runtime health from the unified log
    log = read_tail(APPDATA / "nexus_unified.log")
    if log:
        pokes = len(re.findall(r"watchdog poke", log))
        recovery = len(re.findall(r"silence-recovery #\d+", log))
        hallu = len(re.findall(r"filtered hallucination", log))
        quota = len(re.findall(r"quota denial|quota_exceeded", log))
        wake_rej = len(re.findall(r"REJECTED by speaker verification", log))
        print(f"\n[Runtime signals, log tail]")
        print(f"  watchdog pokes (deaf-session heals): {pokes}   gate: rare, explained")
        print(f"  silence-recovery restarts:           {recovery}   gate: < 12/day sustained")
        print(f"  hallucination filter hits:           {hallu}   (rate vs 1.5% of captures)")
        print(f"  quota denials surfaced:              {quota}   gate: every one spoken")
        print(f"  speaker-verification rejects:        {wake_rej}")
    else:
        print("\n[Runtime signals] nexus_unified.log not found â€” run from the app machine")

    # 4. Release gates that need tooling (printed, not fabricated)
    print("\n[Release gates â€” run on this machine]")
    print("  cargo test --lib -- --test-threads=1        (expect 656/656)")
    print("  cargo test --test e2e_voice_fixture          (expect 49/50, accent 15/15)")
    print("  python scripts/soak_wake_fa.py --full        (gate < 1 FA / 8h; known: necess_0003 FA)")
    print("  node ../scripts/check-turn-ends.mjs          (expect OK)")
    print("  npm test (server/worker)                     (expect 53/53)")

    print("\n" + "=" * 68)
    return 0


if __name__ == "__main__":
    sys.exit(main())

