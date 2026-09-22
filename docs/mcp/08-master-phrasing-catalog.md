# Master Phrasing Catalog Plan — every way every command can be said (2026-09-18)

Rule: coverage = families × verbs × fillers × entities × confusables.
Target 60–100 rows/intent, triggers ≥60 rows with recall ≥0.95.

## A. Modes (must never misfire)

| Intent | Sayable forms ((triggers)) | Confusables | Target |
|---|---|---|---|
| ghostwriter_start | ghostwriter, ghost writer, start ghostwriting, take dictation, take a letter, write this down, start writing, scribe, note this down, type for me (10) | go writer, ghost rider, coast writer | 70 |
| echo_start | echo, echo mode, read it back, read that out, read my messages, what came in (6) | eco, ecko, ecco, ego | 60 |
| ghostwriter/send_it | send it, ok/perfect/yeah/so + send it, shoot it, fire it off (8) | scent it | 40 |
| scratch_that | scratch that, delete that, undo that, cut that, drop that (5) | — | 30 |

## B. Local (12 intents)

| Intent | Verbs × pattern | Entities | Confusables | Target |
|---|---|---|---|---|
| open_app | open/launch/start/run/fire up/bring up/show/pull up/go to/visit (10) × [please/hey/nexus] | 200 apps | chrome/crome, edge/age | 100 |
| close_app | close/quit/exit/kill/shut down/end (6) | same | — | 60 |
| open_url | open/go to/visit/browse to + site/domain (6) | 20 sites + any domain | — | 50 |
| whatsapp_chat | open chat with/chat with/message X on whatsapp (5) | contacts | — | 50 |
| search | search/google/look up/find/look for (5) | open vocabulary | — | 60 |
| media ×4 | play/pause/resume/next/skip/previous/stop × music/song/media (12) | — | pause/paws, next/necks | 40 each |
| greeting | hi/hello/hey/thanks/bye/good morning/who are you/help (10+) | — | — | 50 |
| open_settings/architect | open/show/command center/settings/architecture/mapper (8) | — | architect/octach | 50 each |

## C. Analysis (4) + GitHub (28)

- analyse_repo/pr/latest/check_branch: analyse/analyze/deep analyse/scan/map/review × repo forms (bare, owner/repo, "X repo", PR #n, latest/by author) = 6 verbs × 5 repo forms = 30 families → 60 each.
- 24 live GitHub ops: one family per regex branch (list/get/merge/approve/close/comment/collab×3/org×3/branch/release/workflow×4) + number/repo/username/org variations → 40 each.
- 4 dead ops: regexes first, then 30 each (delete_release, branch protection, outside-collab ×2).

## D. Live (11) — healthy, add fallbacks + confusables

type/press/hotkey/confirm/cancel/new-tab/navigate/bsearch/wopen/wsearch/focus:
add deterministic fallback for the 5 NLU-only ones; confusables
(type/tight, tab/tap, press/present). Hold counts, +15 phonetic rows each.

## E. MCP — current 3 + new 12

| Intent | Forms | Slots variations | Target |
|---|---|---|---|
| order_food | order/get/i want/craving/bring/fetch/hungry × dish × [from restaurant] (8) | 50 dishes × 100 restaurants | 100+ |
| search_product | search/find/look for/price of/buy × product × [on amazon] (8) | open products | 100+ |
| send_whatsapp_message | send/tell/text/ping/forward/message × contact × saying + message (8) | 200 contacts | 100+ |
| open_meeting_from_chat | open/join + meet/link/call × from X/recent/mummy sent (8) | — | 60 |
| play_on_spotify | play/put on/queue × song × on spotify (5) | open music | 60 |
| search_youtube | search/find/play video/summarize × query (5) | open | 50 |
| create_meet | schedule/set up/create meeting with X (5) | contacts | 50 |
| linkedin_msg/post/search | message/search/post × who/what (6) | open | 50 |
| vercel/render_status+deploy | status/redeploy/rollback × project/service (6) | project names | 40 each |
| gmail_send/draft | send/mail/draft email to X saying Y (6) | contacts | 80 |
| calendar_create | schedule/add/remind × event × time (6) | open | 60 |

## F. OOS negatives (anti-perfection insurance)

Per tier: `order a book` (not food), `send the file` (not whatsapp),
`bank branch` (not git branch), `menu` without food (not order),
`send it` outside Ghostwriter (not a send). 300–500 total.

## Totals

55 intents + 4 mode intents + ~15 new MCP intents ≈ 74 labels × ~65 avg ≈
**~4,800 rows** (from 2,900 today). Gap ≈ 1,900 rows, generated in
phase9 (MCP verbs), phase10 (thin top-up + confusables), phase11 (dead ops),
phase12 (modes + new-MCP).

## Verification per batch (test twice)

1. Family-dedup vs frozen test-452 (zero overlap).
2. MITL conf90 filter (keep only model-leaning rows).
3. 20-phrasing spot test per intent ≥0.85, triggers ≥0.95.
4. Retrain → audit gates → OTA. Roll back on any regression.
