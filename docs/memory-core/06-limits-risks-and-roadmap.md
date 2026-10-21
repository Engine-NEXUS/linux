# 06 - Limits, risks and roadmap

## Honest NOs
- Exact YouTube resume position (query params are stripped; needs a browser extension).
- Greeting before login (autostart runs at logon).
- Downloading Instagram reels (only a paused frame / screenshot can be analysed).
- Controlling WhatsApp read state when you reply from the phone.
- Stopping same-user malware from reading the memory (see encryption scope).

## Risks
- WhatsApp: unofficial clients can get a number banned; session re-pair ~20 days;
  read-receipt behaviour of the bridge unknown until tested.
- Google: unverified-app warning and (in Testing mode) 7-day token expiry.
- Spoken text (alert names, optional message text) is sent to the cloud voice service.
- Retrieval is lexical; priority scores and thresholds are hypotheses.

## Open items
- Rebuild (`nexus build`) so the installed binary has the Photos-scope removal
  and the needs-sign-in notice.
- API keys duplicated in keychain and plaintext `settings.json` (decision pending).
- Unexplained startup stderr registry-key error.
- Not built: mark-read-after-reply, call-frequency signal, WhatsApp previews in the briefing.

## Roadmap
- **P7:** routine learning - miners propose schedule changes as suggestions only
  (never silent edits), and friend-mode check-ins.
- Possible: local-only voice for private text, browser extension for exact YouTube resume.
