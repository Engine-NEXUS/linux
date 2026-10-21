"""Case-bank retrieval for the Main Center validity/CBR path.

Adapted from Memento-Teams/Memento @ 42fbbca (memory/np_memory.py):
load_jsonl + extract_pairs taken verbatim (stdlib-only). The torch/
transformers neural retriever was replaced with a stdlib token-overlap
scorer below — same (question, plan, score) contract, zero heavy deps.
See docs/features/79-cloned-agent-subsystem-integration.md.
"""

from __future__ import annotations

import json
import re
import sys
from typing import List, Tuple


def load_jsonl(path: str) -> List[dict]:
    items = []
    with open(path, "r", encoding="utf-8") as f:
        for ln, line in enumerate(f, 1):
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
                items.append(obj)
            except Exception as e:
                print(f"[WARN] Failed to parse line {ln}, skipped: {e}", file=sys.stderr)
    return items


def extract_pairs(items: List[dict], key_field: str, value_field: str) -> List[Tuple[str, object, int]]:
    pairs = []
    for i, obj in enumerate(items):
        if key_field in obj and value_field in obj:
            pairs.append((str(obj[key_field]), obj[value_field], i))
        elif len(obj) == 2:
            ks = list(obj.keys())
            pairs.append((str(obj[ks[0]]), obj[ks[1]], i))
        else:
            pass
    return pairs


_TOKEN_RE = re.compile(r"[a-z0-9]+")


def _tokens(text: str) -> set:
    return set(_TOKEN_RE.findall(text.lower()))


def retrieve(
    task: str,
    pairs: List[Tuple[str, object, int]],
    top_k: int = 5,
) -> List[dict]:
    """Stdlib token-overlap retrieval (Jaccard). Same return contract as
    the torch version: [{rank, score, question, plan, line_index}]."""
    if not pairs:
        return []
    qtok = _tokens(task)
    scored = []
    for key, value, line_index in pairs:
        ktok = _tokens(key)
        union = qtok | ktok
        score = (len(qtok & ktok) / len(union)) if union else 0.0
        scored.append((score, key, value, line_index))
    scored.sort(key=lambda r: r[0], reverse=True)
    results = []
    for rank, (score, key, value, line_index) in enumerate(scored[: max(top_k, 0)], 1):
        results.append(
            {
                "rank": rank,
                "score": round(float(score), 6),
                "question": key,
                "plan": value,
                "line_index": line_index,
            }
        )
    return results
