# Voice Call Control (2026-09-18)

Requirement: a dedicated agent number. Owner calls → agent answers and takes
commands. Everyone else → forwarded to the main number. Budget: ~₹200/month
(₹0 hardware — no spare phone, PC has no WWAN modem).

## Why not eSIM-in-PC / spare phone

- PC eSIM needs a WWAN modem (Settings → Network → eSIM profiles). Most
  desktops lack it; PC modems are data-only (no callable voice).
- No spare phone available. Travel eSIMs are data-only.
- Hardware chain exists (9esim/5ber card + SIM7600 USB voice modem,
  ~₹4–6K) but breaks the budget — documented here as a future option only.

## Recommended: virtual number + CLI split (₹200 Plivo, Exotel fallback)

```
Caller → virtual number → Worker /voice/incoming (CLI check, D1 whitelist)
  ├── CLI == owner → audio stream → VPS voice pipe
  │     (Groq STT → orchestrator → Edge TTS; speaker + PIN gate first)
  └── anyone else → <Dial> main number (agent never involved)
```

- Worker (free, existing): `/voice/incoming` + `/voice/status`, D1 logging.
- VPS voice box (~₹400–600/mo India, or Oracle free tier): websocket PCM →
  resample → STT → command map → TTS. Only owner calls reach it.
- India note: Twilio can't do domestic India-to-India — use Plivo
  (~₹200/mo + ₹0.38/min) or Exotel (~₹500–1,000/mo, better platform).
- At ₹200 total budget the number alone barely fits — so build the
  call-control logic locally first (simulated audio, real orchestrator +
  speaker verification) and rent the number when ready. Zero rework.

## Security (mandatory)

CLI can be spoofed: answering is not authenticating. Owner passes NEXUS
speaker verification (`voice_profile.rs`, 0.45) + spoken PIN (first call of
day). Fail → drop to main number. Money/messages never send on call without
`send it` + PIN. 8 kHz narrowband audio → ~5–10% worse STT; keep call
commands short, confirmations strict.

## Future hardware option (over budget today)

9esim/5ber card (carrier eSIM via main phone once) + SIM7600 USB dongle:
modem watches `RING`+CLIP — owner CLI → `ATA` + PCM to NEXUS; others →
reject → pre-set `AT+CCFC` forward-to-main. No internet needed. Risk: Jio is
VoLTE-only (module VoLTE config fiddly); Airtel/Vi safer for testing.
