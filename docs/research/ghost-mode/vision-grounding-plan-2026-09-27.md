# Vision Grounding Plan — Grid + Dual Provider + Quotas (2026-09-27)

## Research basis

- General VLMs regress coordinates poorly (OS-Atlas/UGround papers omit
  them as baselines). Specialized grounders win — but need 4-8GB RAM.
- **Axis-Grid Scaffold** (arXiv:2509.11548): edge rulers + grid overlay
  lifted Gemini models 11% → 95% on ScreenSpot-v2, zero training.
  This is the technique we adopt (no fonts needed — ticks, not labels).
- Iterative crop-refine (R-VLM, arXiv:2411.13591) doubles cost per click;
  deferred unless small-element misses dominate the diary.
- `gemini-3.5-flash-lite` confirmed live (GA Jul 2026, code
  `gemini-3.5-flash-lite`, 1M ctx, image in, 350 tok/s, Computer Use
  preview-gated — watch, don't build on).

## Architecture (UIA stays primary)

```
ghost_click step 2:
  UIA resolve_element (exact, free, ms)
    → hit: click
    → miss: vision fallback
      provider order per `visionProvider` setting (default auto = groq→gemini)
      skip quota-exhausted providers BEFORE calling
      screenshot → downscale 1024 → axis-grid overlay → JPEG → VLM
      parse 0-1000 → px → stop() re-check → click
      on provider switch: announce "Daily X limit reached, sir — falling back to Y."
```

## Where keys live

**Locally, OS keychain — never Cloudflare.** Cloudflare holds OAuth
tokens + NEXUS models; user API keys would transit the network and sit
in D1 (wrong trust boundary). A2 already routes all three keys
keychain-first with settings.json stripped. The Accounts tab fields
stay as-is; we add a keychain hint line.

## Quotas (free tier, per project, reset midnight Pacific)

| Provider | RPD | RPM | Note |
|----------|-----|-----|------|
| Groq vision | 14,400 | ~30 | primary: 28x headroom |
| Gemini 3.5 Flash-Lite | ~500 | ~15 | fallback: 250K TPM |

Paid Gemini math: ~$0.0005-0.001/grounding → ₹200/mo ≈ 2.5-4.8K calls
on top of ~15K free/mo. Typical use (dozens of vision fallbacks/day —
UIA hits cost 0) never leaves free tier.

Local tracking: `vision_usage.json` {pacific_date, groq, gemini},
buckets keyed by Pacific calendar day (US DST rule, no new deps).
Exhausted providers are skipped pre-call (no wasted 429s); a 429
mid-day marks exhausted immediately.

## Settings sidebar (Accounts tab, new "Ghost Vision" section)

- Provider order select: Auto (Groq→Gemini) / Groq only / Gemini only.
- Quota readout: "Groq 12 / 14,400 • Gemini 0 / 500 — resets midnight PT".
- Keychain hint under API Keys: "Keys stay on this device (OS keychain)."
- Gemini key field already exists; no new key UI needed.

## First-run nudge

`ghost_enter` with NEITHER vision key configured → open settings
sidebar (Accounts) + speak: "Ghost mode on, sir — add a Groq or Gemini
key in Settings → Accounts so I can see custom buttons." UIA-only
clicks still work; only vision fallback is gated.

## Files

- `vision.rs`: grid overlay, provider order, quota counters, Gemini call,
  fallback orchestrator, spoken-notice signal.
- `commands.rs`: `vision_quota_status`.
- `ghost.rs`/`mouse.rs`: hook fallback + announce + nudge.
- `SettingsSidebarApp.tsx`: provider select + quota readout + hint.
- Tests: grid geometry, Pacific date/DST edges, quota record/exhaust/
  reset, order selection, Gemini response parse.
