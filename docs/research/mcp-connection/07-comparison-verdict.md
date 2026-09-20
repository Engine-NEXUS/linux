# Comparison Verdict — Best Approach per Server (2026-09-20)

Option matrices scored against: our codebase reality (vault, deep-link
OAuth, pending-resume, sidebar rails all exist), industry standard
(§01), spec compliance (§02), and robustness (no scraping, bounded
retries, silent refresh).

---

## WhatsApp

| Option | Score | Reason |
|---|---|---|
| Scrape bridge stdout for QR | ✘ | Version-coupled, needs console window, fights rate limits |
| `shell.open /pair` only | △ fallback | Works, but no state, no polling, no voice confirm |
| **Poll `pairing_status`, render QR in card** | **✓ adopt** | First-party structured data; tool exists for exactly this; owns state end-to-end |
| Re-implement whatsmeow in Rust | ✘ | Months of protocol work duplicating the binary |

## Swiggy ×3

| Option | Score | Reason |
|---|---|---|
| Keep paste-token only | △ today | Works but no self-heal; every expiry = user errand |
| Bespoke Swiggy OAuth hack | ✘ | Non-standard, rots |
| **Spec-sequence OAuth on proven rails** | **✓ adopt** | `setup/oauth.ts` + Worker + vault already proven for Google/GitHub; adds discovery, metadata-doc registration, `resource`/`iss`, refresh |

## Amazon / Spotify / Vercel / Render

| Option | Score | Reason |
|---|---|---|
| Full OAuth everywhere now | ✘ | Cost without usage |
| **Amazon: launcher + sign-in trigger + poll; rest: token-page buttons + numbered steps** | **✓ adopt** | Matches each auth reality; Telegram card is the template |

## Cross-cutting decisions

- **Card-on-first-failure (not every failure):** voice always speaks;
  window opens once per server per session. (Prevents nag-spam; Cursor
  shows errors inline for the same reason.)
- **Auto-resume on completion:** Composio-validated; our machinery
  supports it. A connect flow that makes the user re-speak the command
  is a failed flow.
- **Proactive probing over reactive failure:** 20-day WhatsApp rotation
  and OAuth expiries are scheduled events — the monitor should surface
  them before the next command, per the Claude-issue lessons.
- **No tokens in URLs/logs/audit:** Zapier rule + spec hygiene; pinned
  by test.

## Build order (each phase verified twice before the next)

1. Shared card + state machine + Connections upgrades (§06).
2. WhatsApp end-to-end (§03) — the proof of the system.
3. Swiggy spec OAuth (§04).
4. Amazon + token-page buttons (§05).
5. Flip the switch: failures open the card (gated).

No phase touches inference, training data, or the NLU pipeline.
