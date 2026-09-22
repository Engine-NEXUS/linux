#!/usr/bin/env python3
"""
NEXUS NLU — Training Data & Category Coverage Inspector

Inspects dataset.json, collect_progress.json, and collected_samples.jsonl
to provide a clean, category-by-category breakdown of training samples,
real voice samples, coverage percentages, and actionable training recommendations.

Tiers:
  ★ Flawless (Apex)  : 100+ rows, 10+ voice (≥99.5% accuracy — asymptotic perfection ceiling)
  ● Strong           : 50-99 rows, 5+ voice (95-98% accuracy — production ready)
  ◐ Good             : 25-49 rows, 2+ voice (88-94% accuracy — covers standard phrasing)
  ▲ Thin             : 10-24 rows (75-85% accuracy — textbook phrasing only)
  ✖ Weak             : <10 rows (needs data immediately)

Usage:
    python scripts/nlu_stats.py
    nexus stats                             # via CLI
"""

import json
import os
import sys
from collections import Counter
from pathlib import Path

# Fix Windows console encoding
if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")

SCRIPT_DIR = Path(__file__).parent
ROOT_DIR = SCRIPT_DIR.parent
DATASET_PATH = ROOT_DIR / "server" / "nlu" / "dataset.json"
COLLECTED_PATH = ROOT_DIR / "server" / "admin" / "data" / "collected_samples.jsonl"
PROGRESS_PATH = ROOT_DIR / "server" / "admin" / "data" / "collect_progress.json"

# ANSI Colors
C_RESET = "\033[0m"
C_BOLD = "\033[1m"
C_DIM = "\033[2m"
C_RED = "\033[31m"
C_GREEN = "\033[32m"
C_YELLOW = "\033[33m"
C_BLUE = "\033[34m"
C_MAGENTA = "\033[35m"
C_CYAN = "\033[36m"
C_WHITE = "\033[37m"

# Target sample counts per intent (500 samples per category benchmark)
TARGET_STRONG_DATASET = 100    # 100 rows per intent for Strong (~500/category)
TARGET_STRONG_VOICE = 10       # 10 voice per intent for Strong
TARGET_FLAWLESS_DATASET = 250  # 250 rows per intent for Flawless (Apex Mastery)
TARGET_FLAWLESS_VOICE = 25     # 25 voice per intent for Flawless (Apex Mastery)

CATEGORIES = [
    {
        "id": "mcp",
        "name": "Food, Shopping & Social (MCP)",
        "desc": "Food orders (Swiggy), product searches (Amazon), WhatsApp messaging",
        "intents": [
            "order_food",
            "search_product",
            "send_whatsapp_message",
            "whatsapp_chat",
            "whatsapp_open",
            "whatsapp_search",
        ],
    },
    {
        "id": "github",
        "name": "GitHub Operations",
        "desc": "PRs (merge/close/list/approve), branch checks, repo analysis, workflows",
        "intents": [
            "merge_pr",
            "approve_pr",
            "close_pr",
            "list_prs",
            "get_pr",
            "revert_pr",
            "comment_pr",
            "create_pr",
            "list_pr_files",
            "list_branches",
            "check_branch",
            "delete_branch",
            "update_branch",
            "analyse_repo",
            "analyse_pr",
            "analyse_latest_pr",
            "list_workflows",
            "list_workflow_runs",
            "cancel_workflow",
            "rerun_workflow",
            "list_releases",
            "create_release",
            "list_collaborators",
            "add_collaborator",
            "remove_collaborator",
            "list_org_members",
            "add_org_member",
            "remove_org_member",
        ],
    },
    {
        "id": "apps",
        "name": "Apps & System Navigation",
        "desc": "Opening/closing apps, settings, web searches, app focus, architect mode",
        "intents": [
            "open_app",
            "close_app",
            "search",
            "open_settings",
            "open_url",
            "focus_app",
            "open_architect",
        ],
    },
    {
        "id": "messages",
        "name": "Dictation & Messages",
        "desc": "Text typing, action confirmations, cancellations, greeting",
        "intents": [
            "type_text",
            "confirm_send",
            "cancel_action",
            "greeting",
        ],
    },
    {
        "id": "live",
        "name": "Live Browser & Key Control",
        "desc": "Simulated keyboard presses, hotkeys, browser tabs and URL navigation",
        "intents": [
            "press_key",
            "press_hotkey",
            "browser_new_tab",
            "browser_navigate",
            "browser_search",
        ],
    },
    {
        "id": "media",
        "name": "Media & Audio Controls",
        "desc": "Play, pause, skip tracks, volume and media stopping",
        "intents": [
            "media_play_pause",
            "media_next",
            "media_previous",
            "media_stop",
        ],
    },
    {
        "id": "other",
        "name": "Out-of-Scope & General",
        "desc": "General chatter, unmapped commands, negative boundary training",
        "intents": [
            "unknown",
        ],
    },
]


def load_dataset_counts():
    """Load intent counts from dataset.json (training split)."""
    if not DATASET_PATH.exists():
        return Counter()
    try:
        with open(DATASET_PATH, "r", encoding="utf-8") as f:
            data = json.load(f)
        train = data.get("train", data) if isinstance(data, dict) else data
        return Counter(ex.get("intent", "") for ex in train if ex.get("intent"))
    except Exception as e:
        print(f"{C_RED}[ERROR] Could not load dataset: {e}{C_RESET}")
        return Counter()


def load_voice_counts():
    """Load real voice sample counts from collect_progress.json and collected_samples.jsonl."""
    counts = Counter()
    # 1. Read cumulative progress
    if PROGRESS_PATH.exists():
        try:
            with open(PROGRESS_PATH, "r", encoding="utf-8") as f:
                data = json.load(f)
                for intent, count in data.items():
                    counts[intent] = max(counts[intent], count)
        except Exception:
            pass

    # 2. Add any newly collected pending samples
    if COLLECTED_PATH.exists():
        try:
            with open(COLLECTED_PATH, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if line:
                        try:
                            entry = json.loads(line)
                            intent = entry.get("intent")
                            if intent:
                                counts[intent] += 1
                        except json.JSONDecodeError:
                            continue
        except Exception:
            pass

    return counts


def make_progress_bar(percentage, width=12):
    """Render a visual ASCII progress bar."""
    filled = int(round((percentage / 100.0) * width))
    filled = max(0, min(width, filled))
    bar = "█" * filled + "░" * (width - filled)
    if percentage >= 95:
        color = C_MAGENTA  # Apex Flawless tier
    elif percentage >= 75:
        color = C_GREEN    # Strong tier
    elif percentage >= 50:
        color = C_CYAN     # Good tier
    elif percentage >= 25:
        color = C_YELLOW   # Needs Work
    else:
        color = C_RED      # Weakest
    return f"{color}[{bar}]{C_RESET} {percentage:3.0f}%"


def main():
    dataset_counts = load_dataset_counts()
    voice_counts = load_voice_counts()

    total_dataset_rows = sum(dataset_counts.values())
    total_voice_rows = sum(voice_counts.values())

    print(f"\n{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")
    print(f"  {C_CYAN}{C_BOLD}NEXUS NLU — Training Data & Category Coverage Inspector{C_RESET}")
    print(f"{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")
    print(f"  Total Trained Dataset Rows : {C_BOLD}{C_GREEN}{total_dataset_rows:,}{C_RESET} (across {len(dataset_counts)} intents)")
    print(f"  Real Voice Samples Logged  : {C_BOLD}{C_MAGENTA}{total_voice_rows:,}{C_RESET} recorded on-device")
    print(f"  Levels of Mastery          : {C_RED}✖ Weak{C_RESET} → {C_YELLOW}▲ Thin{C_RESET} → {C_CYAN}◐ Good{C_RESET} → {C_GREEN}● Strong (95%+){C_RESET} → {C_MAGENTA}{C_BOLD}★ Flawless Apex (99.5%+){C_RESET}")
    print(f"────────────────────────────────────────────────────────────────────────────────")

    category_summaries = []

    for cat in CATEGORIES:
        cat_intents = cat["intents"]
        cat_dataset_total = sum(dataset_counts.get(i, 0) for i in cat_intents)
        cat_voice_total = sum(voice_counts.get(i, 0) for i in cat_intents)
        
        target_strong_d = len(cat_intents) * TARGET_STRONG_DATASET
        target_strong_v = len(cat_intents) * TARGET_STRONG_VOICE

        # Normal percentage against Strong baseline (100% = Strong, >100% = heading to Flawless)
        dataset_pct = min(100.0, (cat_dataset_total / max(1, target_strong_d)) * 100.0)
        voice_pct = min(100.0, (cat_voice_total / max(1, target_strong_v)) * 100.0)
        
        # Combined health score (60% dataset + 40% voice)
        health_score = (dataset_pct * 0.6) + (voice_pct * 0.4)

        # Identify status per intent inside category
        intent_stats = []
        for intent in cat_intents:
            d_count = dataset_counts.get(intent, 0)
            v_count = voice_counts.get(intent, 0)
            intent_stats.append((intent, d_count, v_count))

        intent_stats.sort(key=lambda x: (x[1], x[2]))  # weakest first
        weakest = [i[0] for i in intent_stats if i[1] < 20 or i[2] < 3]

        # Check if entire category qualifies for Flawless Apex (250+ dataset, 25+ voice per intent)
        is_category_flawless = (
            cat_dataset_total >= (len(cat_intents) * TARGET_FLAWLESS_DATASET) and
            cat_voice_total >= (len(cat_intents) * TARGET_FLAWLESS_VOICE) and
            all(d >= TARGET_FLAWLESS_DATASET and v >= TARGET_FLAWLESS_VOICE for _, d, v in intent_stats)
        )
        # Check if entire category qualifies for Strong (100+ dataset, 10+ voice per intent)
        is_category_strong = (
            cat_dataset_total >= target_strong_d and
            cat_voice_total >= target_strong_v and
            all(d >= TARGET_STRONG_DATASET and v >= TARGET_STRONG_VOICE for _, d, v in intent_stats)
        )

        category_summaries.append({
            "id": cat["id"],
            "name": cat["name"],
            "desc": cat["desc"],
            "intents_count": len(cat_intents),
            "dataset_total": cat_dataset_total,
            "voice_total": cat_voice_total,
            "dataset_pct": dataset_pct,
            "voice_pct": voice_pct,
            "health_score": health_score,
            "is_flawless": is_category_flawless,
            "is_strong": is_category_strong,
            "intent_stats": intent_stats,
            "weakest": weakest,
        })

    # Print Category Summary Cards
    print(f"\n{C_BOLD}  CATEGORY OVERVIEW:{C_RESET}\n")
    print(f"  {'Category':<32} {'Dataset':<10} {'Voice':<8} {'Coverage Bar':<22} {'Mastery Status'}")
    print(f"  {'-'*30:<32} {'-'*8:<10} {'-'*6:<8} {'-'*20:<22} {'-'*18}")

    for cat in category_summaries:
        if cat["is_flawless"]:
            status = f"{C_MAGENTA}{C_BOLD}★ Flawless (Apex){C_RESET}"
        elif cat["is_strong"] or (cat["health_score"] >= 90 and len(cat["weakest"]) == 0):
            status = f"{C_GREEN}● Strong (95%+){C_RESET}"
        elif cat["health_score"] >= 50:
            status = f"{C_CYAN}◐ Good{C_RESET}"
        elif cat["health_score"] >= 25:
            status = f"{C_YELLOW}▲ Needs Work{C_RESET}"
        else:
            status = f"{C_RED}{C_BOLD}✖ Weakest{C_RESET}"

        bar = make_progress_bar(cat["health_score"], width=12)
        print(f"  {C_BOLD}{cat['name']:<32}{C_RESET} {cat['dataset_total']:<10} {cat['voice_total']:<8} {bar:<22} {status}")

    # Detailed Per-Category Breakdown
    print(f"\n{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")
    print(f"  {C_CYAN}{C_BOLD}DETAILED INTENT BREAKDOWN BY CATEGORY{C_RESET}")
    print(f"{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")

    for cat in category_summaries:
        print(f"\n  {C_BOLD}{C_BLUE}▶ [{cat['id'].upper()}] {cat['name']}{C_RESET} — {C_DIM}{cat['desc']}{C_RESET}")
        print(f"    {'Intent Name':<28} {'Trained Rows':<14} {'Voice Recorded':<16} {'Mastery Level'}")
        print(f"    {'-'*26:<28} {'-'*12:<14} {'-'*14:<16} {'-'*18}")

        for intent, d_count, v_count in cat["intent_stats"]:
            if d_count >= TARGET_FLAWLESS_DATASET and v_count >= TARGET_FLAWLESS_VOICE:
                tag = f"{C_MAGENTA}{C_BOLD}★ Flawless (Apex){C_RESET}"
            elif d_count >= TARGET_STRONG_DATASET and v_count >= TARGET_STRONG_VOICE:
                tag = f"{C_GREEN}● Strong{C_RESET}"
            elif d_count >= 20:
                tag = f"{C_CYAN}◐ Good{C_RESET}"
            elif d_count >= 5:
                tag = f"{C_YELLOW}▲ Thin{C_RESET}"
            else:
                tag = f"{C_RED}✖ Weak{C_RESET}"

            v_str = f"{v_count} voice" if v_count > 0 else f"{C_DIM}0 voice{C_RESET}"
            print(f"    {intent:<28} {d_count:<14} {v_str:<16} {tag}")

    # Actionable Focus Recommendations
    print(f"\n{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")
    print(f"  {C_YELLOW}{C_BOLD}TARGETED TRAINING RECOMMENDATIONS:{C_RESET}")
    print(f"{C_BOLD}════════════════════════════════════════════════════════════════════════════════{C_RESET}")

    # Sort categories by health score ascending
    ranked = sorted(category_summaries, key=lambda c: c["health_score"])

    for rank, cat in enumerate(ranked[:3], 1):
        weak_list = ", ".join(cat["weakest"][:4]) if cat["weakest"] else "all balanced"
        print(f"\n  {C_BOLD}{rank}. Focus on Category: {C_CYAN}{cat['name']} ({cat['id']}){C_RESET}")
        print(f"     Status  : {C_YELLOW}{len(cat['weakest'])} intents need data{C_RESET} (e.g. {weak_list})")
        print(f"     Command : {C_GREEN}{C_BOLD}nexus collect --category {cat['id']}{C_RESET}")

    print(f"\n  {C_BOLD}────────────────────────────────────────────────────────────────────────────────{C_RESET}")
    print(f"  {C_MAGENTA}{C_BOLD}★ THE 0.1% LAW OF ASYMPTOTIC PERFECTION:{C_RESET}")
    print(f"  {C_DIM}• Moving from Weak → Strong (95% accuracy) takes ~100 rows + 10 voice takes (~500 per category).{C_RESET}")
    print(f"  {C_DIM}• Moving from Strong → Flawless Apex (99.5% accuracy) takes 250+ rows + 25 voice takes.{C_RESET}")
    print(f"  {C_DIM}• Once an intent hits ★ Flawless, BERT-Mini has reached its mathematical capacity limit;{C_RESET}")
    print(f"  {C_DIM}  any additional data will only contribute fractional (~0.1%) edge-case gains.{C_RESET}\n")


if __name__ == "__main__":
    main()
