# Departments: Amazon Bridge + Long-Tail Token Services (2026-09-20)

## 1. Amazon bridge (Playwright scraper, `:8766`)

Same card system as WhatsApp (§03), different pairing step:

1. Failure classes: down (TCP refused) / session-invalid (sign-in
   lapsed) / never-configured.
2. Card: status + steps — start bridge program (managed child, log-view
   button) → **"Sign in" button that `shell.open`s the bridge's local
   auth URL** (today's guidance string already promises *"sign in when
   its browser window opens"* — make it real) → poll session endpoint
   until valid → `ready` + voice confirm + auto-retry.
3. Scope stated on the card, as today: read-only (search, details,
   reviews). No write pairing to design.
4. Voice: *"Amazon bridge needs sign-in, sir — the card will walk you
   through."*

## 2. Spotify / Vercel / Render (vault paste services)

Full OAuth is unjustified until usage demands it. Upgrade in place:

1. Keep paste-token Save/Delete + badges.
2. Add per-service **"Get token" button** → `shell.open` the exact
   provider page (Spotify Dashboard, Vercel/Render personal-token page).
3. Numbered 3-step instructions in Telegram style (the only card that
   has them today — `telegram.rs` + Connections hint naming @BotFather /
   @userinfobot): where to click, what to paste, how to revoke.
4. Token hygiene per Zapier rule (§01): paste fields are
   password-inputs; tokens never enter URLs, logs, or the audit trail.

## 3. Telegram (reference implementation — change nothing)

Two-field form (bot token + chat id), exact-source steps, save+restart
note, silent-until-configured, rotation re-read + double-401 self-heal
(`telegram.rs:133-215`). New cards copy this shape.
