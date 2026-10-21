# Ghost Research 02 — Grounding Options, Perfection Limits, Costs

Sources: UI-TARS (arXiv:2501.12326, open weights), OmniParser
(Microsoft, arXiv:2408.00203, MIT), OSWorld/ScreenSpot, Anthropic
Computer Use. Researched 2026-09-25.

## 1. Options for "where is the search box" (ranked by fit)

1. **UIA bounds (Windows)** — exact rectangles from the OS, millisecond
   latency, zero RAM/model cost. Covers Win32/WPF/WinUI + many browser
   chrome elements. Fails on canvas/games/Citrix/Electron-custom.
2. **OmniParser sidecar** — icon detector + Florence-2 captioner + OCR →
   labeled boxes any VLM (our Worker LLM) can pick. MIT, fits our
   lazy-start/kill-idle sidecar doctrine. Cost: ~1-3GB RAM resident,
   5-30s/shot CPU-only.
3. **UI-TARS grounding** — SOTA (94.2 ScreenSpot-V2, 61.6 ScreenSpot-Pro,
   OSWorld 42.5). 7B weights (~14GB) don't fit family hardware → API
   hosting = per-shot cost + seconds of latency.
4. **Vendor Computer Use API** — pixel endpoint like Clicky's; per-call
   cost, best accuracy-per-dollar when UIA/OmniParser fail.

Doctrine: UIA first (deterministic, free) → OmniParser where UIA is
blind → hosted grounding only as last resort. Measure, don't assume:
50-screenshot local eval decides.

## 2. Perfection analysis (benchmark truth)

Routine UIs ground at ~19/20 (SOTA); dense professional UIs at ~3/5
(ScreenSpot-Pro 61.6); end-to-end multi-step tasks below half (OSWorld
42.5). "Perfectly" is unavailable at any price — the engineering answer
is the verify step (re-check the landing, retry once, report): convert
residual error into retries, and bound misses with confirm gates +
credential refusal.

## 3. Cost ledger (nothing is free)

| Path | Extra RAM | Latency/shot | Money |
|---|---|---|---|
| UIA bounds | ~0 | ms | free |
| OmniParser local | ~1-3GB while resident | 5-30s CPU | free (killed when idle) |
| Hosted grounding | 0 | 2-6s | ~1-2k vision tokens/shot |
| Planner per step | 0 | 1-4s × steps (cap 10) | neuron budget × steps |
| Ring theater | ~0 (stage DOM) | 0 | free |

On an 8GB laptop a resident vision sidecar doubles NEXUS's footprint —
hence lazy-start/kill-idle and UIA-first. Per-task honest totals: UIA
path ≈ free, ~2-4s; vision path ≈ GBs or tokens + 5-30s.

## 4. Hard OS limits (no model fixes these)

UIPI (can't click elevated windows), exclusive-fullscreen games,
credential-field refusal (by design), DPI math (the #1 DIY failure
mode — needs per-machine calibration), dynamic UIs (screenshot age vs
act time: capture and act within the same second, re-verify after).
