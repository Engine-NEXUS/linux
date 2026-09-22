# NEXUS TTS Voice — Research & Plan: ElevenLabs vs Fish Audio

> **Question:** Which TTS provider is "free forever" and can be used permanently for NEXUS?
> **Date:** 2026-08-30
> **Status:** Research complete, awaiting decision

---

## TL;DR — Neither is truly "free forever"

| Provider | Free tier | Is it permanent? | Verdict |
|----------|-----------|-------------------|---------|
| **ElevenLabs** | 10,000 credits/month (~10K chars) | ✅ Permanent free tier, but **no API access on free plan** | Useless for NEXUS — API is locked behind $5/mo Starter |
| **Fish Audio** | `s2.1-pro-free` model, unlimited under Fair Use | ⚠️ **Extended through Aug 31, 2026** — they keep extending it, but it's not contractually permanent | **Best option** — free API, same quality as paid, ~100ms TTFA |

**Recommendation: Fish Audio `s2.1-pro-free`** is the best choice for NEXUS right now. It's free, has API access, ~100ms time-to-first-audio, supports voice cloning, and 83 languages. The free period has been extended twice (June → July → August) and Fish Audio has stated they'll "communicate any changes with advance notice." If they ever end it, switching to their paid tier is $15/M UTF-8 bytes (~$0.015 per 1K chars).

ElevenLabs free tier is **not viable** because the API is locked — you can only use their web editor, not programmatic TTS. The minimum plan with API access is Starter at $5/month (30K chars).

---

## 1. ElevenLabs — Detailed Analysis

### Free Plan (Permanent)

| Feature | Free | Starter ($5/mo) | Creator ($22/mo) |
|---------|------|-----------------|------------------|
| Credits/month | 10,000 | 30,000 | 121,000 |
| **API access** | ❌ **Locked** | ✅ | ✅ |
| Commercial license | ❌ | ✅ | ✅ |
| Voice cloning | 3 voices (Voice Design only) | 10 voices | 30 voices |
| Standard voices | 29 | 29 | 29 |
| Audio quality | 128 kbps MP3 | 128 kbps MP3 | 192 kbps MP3 |
| Priority queue | ❌ | ✅ | ✅ |
| Credit card required | No | Yes | Yes |

### Why ElevenLabs Free doesn't work for NEXUS

1. **API is locked on the free plan.** NEXUS needs programmatic TTS — we call the API from the frontend (`ttsPlayer.ts:playElevenLabs`). Without API access, the free tier is useless.
2. **10K credits/month is tiny.** A typical NEXUS session might speak 500-2000 chars per response. At 10 responses/day, that's 5K-20K chars/day — you'd burn through 10K credits in less than 2 days.
3. **No commercial license.** NEXUS is a product (not personal use). The free plan explicitly prohibits commercial use.
4. **Minimum viable plan: Starter ($5/mo)** — gives 30K chars/month + API access + commercial license. But 30K chars is still only ~15-20 NEXUS responses per day.

### ElevenLabs API pricing (pay-as-you-go)

| Model | Cost per 1M chars |
|-------|-------------------|
| eleven_turbo_v2_5 (multilingual) | ~$300/1M (varies by credit conversion) |
| eleven_v2 (multilingual) | ~$300/1M |
| eleven_flash_v2_5 (discounted) | ~$150/1M |

ElevenLabs is **expensive** compared to Fish Audio ($15/1M).

### ElevenLabs Startup Grant (12 months free)

ElevenLabs offers a "Labs Grant" for startups:
- 33M characters (33 million!)
- 12 months free
- High concurrency limits
- Improved support

**Requirements:** You need to apply and be accepted. It's for "new products or startups." If NEXUS qualifies, this is the best option — 33M chars is enough for ~16K NEXUS responses per month for a year. But it's not "free forever" — it's 12 months.

---

## 2. Fish Audio — Detailed Analysis

### Free Tier (`s2.1-pro-free`)

| Feature | Value |
|---------|-------|
| Model | `s2.1-pro-free` (same model as paid `s2.1-pro`) |
| Cost | $0 / M UTF-8 bytes |
| Usage cap | No hard cap (subject to Fair Use Policy) |
| API access | ✅ Full API (same endpoint as paid) |
| Commercial use | ✅ (but >$1M ARR needs to contact them) |
| Voice cloning | ✅ Included |
| Languages | 83 |
| Time to first audio | ~100ms (with streaming) |
| Credit card required | No |
| SLA / TTFA guarantee | ❌ No SLA on free tier |
| Data retention | Requests may be used to improve model quality |

### Free period status (as of August 2026)

- **Original end date:** July 24, 2026
- **Extended (June update):** Through end of July 2026
- **Extended (July update):** Through **August 31, 2026**
- Fish Audio's statement: *"We'll communicate any changes with advance notice"*
- Their blog explains the free tier is **economically viable** due to a 4× efficiency improvement (4 H200s → 1 H200), not a subsidy. This suggests it could continue indefinitely, but there's no contractual guarantee.

### Fair Use Policy

Fish Audio reserves the right to throttle/limit usage that "looks like abuse rather than development." For NEXUS:
- Typical usage: ~20-100 TTS calls/day (one per assistant response)
- This is well within "development and smaller businesses" use
- No risk of throttling for normal NEXUS usage

### Fish Audio paid tier (for comparison)

| Model | Price |
|-------|-------|
| `s2.1-pro` | $15.00 / M UTF-8 bytes |
| `s2.1-pro-free` | $0.00 / M UTF-8 bytes |
| `s2-pro` | $15.00 / M UTF-8 bytes |
| `s1` | $15.00 / M UTF-8 bytes |

If the free tier ends, switching to paid is a **one-line code change** (change `model: "s2.1-pro-free"` to `model: "s2.1-pro"`). At $15/M bytes, a typical NEXUS response (~500 chars = ~500 bytes) costs $0.0075 — less than a cent per response.

### Concurrency limits

| Tier | Spending threshold | Concurrent requests |
|------|-------------------|---------------------|
| Starter | < $100 paid | 5 |
| Elevated | ≥ $100 paid | 15 |
| High Volume | ≥ $1,000 paid | 50 |

On the free tier, you start at Starter (5 concurrent). This is fine for NEXUS — we only do one TTS call at a time.

### Fish Audio streaming (low latency)

Fish Audio supports two streaming modes:

1. **HTTP streaming** (`tts.stream`): You have the full text, audio chunks arrive as they generate. Time-to-first-audio ~100ms.
2. **WebSocket** (`tts.stream_websocket`): Text arrives token-by-token (from an LLM). Audio starts before the sentence is finished. Ideal for NEXUS + Worker AI streaming.

```typescript
// Current NEXUS code (non-streaming):
const response = await fetch("https://api.fish.audio/v1/tts", {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiKey}` },
  body: JSON.stringify({
    text,
    reference_id: referenceId,
    format: "mp3",
    latency: "normal",
    model: "s2.1-pro",  // ← change to "s2.1-pro-free"
  }),
});
const blob = await response.blob();  // ← waits for full audio
```

```typescript
// Streaming version (lower latency):
const response = await fetch("https://api.fish.audio/v1/tts", {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiKey}` },
  body: JSON.stringify({
    text,
    reference_id: referenceId,
    format: "mp3",
    latency: "balanced",  // ~300ms TTFA
    chunk_length: 200,    // smaller = sooner first audio
    model: "s2.1-pro-free",
  }),
});
// Stream chunks as they arrive instead of waiting for full blob
const reader = response.body!.getReader();
// ... feed chunks to AudioContext as they arrive
```

---

## 3. Current NEXUS TTS Implementation

NEXUS already supports Fish Audio! The code is in `frontend/src/audio/ttsPlayer.ts`:

### Current providers (in priority order):

1. **Gemini Flash TTS** (`gemini_tts`) — default, uses a hardcoded API key
2. **Fish Audio** (`fish_audio`) — uses `s2.1-pro` (paid model), requires user API key
3. **ElevenLabs** (`elevenlabs`) — requires user API key, uses `eleven_turbo_v2_5`
4. **Web Speech API** (`neural`) — free, offline, system voices (fallback)

### Current curated voices:

| Voice ID | Provider | Notes |
|----------|----------|-------|
| `gemini_flash` | Gemini TTS | Default, hardcoded API key |
| `ethan` | Fish Audio | Uses `s2.1-pro` (paid), reference_id `536d3a5e...` |
| `jarvis` | Web Speech | British butler, falls back to ElevenLabs if key set |
| `nova` | Web Speech | American female |
| `echo` | Web Speech | Australian male |
| `onyx` | Web Speech | Deep baritone |

### The problem with the current setup

1. **Gemini Flash TTS is the default** but uses a free Google AI key with strict rate limits (`<GEMINI_API_KEY>`). It will stop working eventually.
2. **Fish Audio uses `s2.1-pro` (paid)** — costs $15/M bytes. Should use `s2.1-pro-free` instead.
3. **ElevenLabs requires a paid plan** ($5/mo minimum for API access).
4. **Web Speech is the fallback** — works offline but sounds robotic.

---

## 4. Recommendation — Switch to Fish Audio `s2.1-pro-free`

### Why Fish Audio wins for NEXUS

| Criterion | ElevenLabs Free | Fish Audio Free |
|-----------|----------------|-----------------|
| API access | ❌ Locked | ✅ Full API |
| Cost | $0 (but useless) | $0 |
| Commercial use | ❌ Prohibited | ✅ Allowed (<$1M ARR) |
| Monthly limit | 10K chars (~10 responses) | No hard cap (Fair Use) |
| Voice cloning | ❌ (Free plan) | ✅ Included |
| Languages | 29 | 83 |
| Time to first audio | ~400ms (paid) | ~100ms (streaming) |
| Quality | Excellent | Excellent (same model, free vs paid) |
| Permanence | Permanent free tier | Extended through Aug 31, likely to continue |
| Switching cost if free ends | $5/mo (Starter) | $15/M bytes (~$0.0075/response) |

### What needs to change in NEXUS

**Minimal change (just switch to free model):**

In `frontend/src/audio/ttsPlayer.ts`, line 380:
```typescript
// BEFORE
model: "s2.1-pro",

// AFTER
model: "s2.1-pro-free",
```

**Full plan (make Fish Audio the default, add streaming):**

1. Change Fish Audio model from `s2.1-pro` → `s2.1-pro-free`
2. Make Fish Audio the default TTS provider (instead of Gemini Flash)
3. Add Fish Audio streaming support (HTTP chunked streaming for ~100ms TTFA)
4. Add 2-3 more curated Fish Audio voices (different reference_ids)
5. Update the Setup Wizard to prompt for Fish Audio API key (free to get)
6. Update Settings to show "Fish Audio (Free)" as the default option
7. Keep Gemini/ElevenLabs/Web Speech as fallbacks

### Fish Audio API key

Getting a Fish Audio API key is free — no credit card required:
1. Go to https://fish.audio
2. Sign up
3. Get API key from dashboard
4. Use `model: "s2.1-pro-free"` header

The user would need to sign up once and paste their API key into NEXUS settings. This is the same flow as the current ElevenLabs/Fish Audio setup.

---

## 5. Alternative: Self-Host Fish Speech (truly free forever)

Fish Audio's model is **open-weight** — you can self-host it:

- Model: Fish Speech 1.5 (open weights on HuggingFace)
- License: CC BY-NC-SA 4.0 (non-commercial)
- Hardware: 1× GPU with 8GB+ VRAM (RTX 3060 or better)
- Latency: Same as API if hosted locally

**For NEXUS:** This would give truly free, permanent TTS with no API dependency. But it requires a GPU and setup effort. Not practical for most users, but worth noting as a fallback if Fish Audio ever ends the free API.

---

## 6. Risk Mitigation

### If Fish Audio ends the free tier:

1. **Switch to paid `s2.1-pro`** — one-line change, $0.0075/response
2. **Switch to ElevenLabs Starter** — $5/mo, 30K chars
3. **Switch to Gemini TTS** — free tier available (currently used as default)
4. **Self-host Fish Speech** — open weights, truly free
5. **Fall back to Web Speech API** — always available, no cost

### If Fish Audio throttles NEXUS (Fair Use):

- Unlikely at 20-100 calls/day
- If it happens, switch to paid tier ($15/M bytes)
- Or distribute across multiple free API keys (not recommended)

---

## 7. Implementation Plan (when approved)

### Phase 1: Switch to Fish Audio Free (5 minutes)

1. In `ttsPlayer.ts:playFishAudio`, change `model: "s2.1-pro"` → `model: "s2.1-pro-free"`
2. In `ttsPlayer.ts:CURATED_VOICES`, make the Fish Audio "Ethan" voice the default
3. Change `default_tts_provider()` in `commands.rs` from `"neural"` → `"fish_audio"`
4. Change default `tts_voice` from `"jarvis"` → `"ethan"`

### Phase 2: Add streaming (30 minutes)

1. Rewrite `playFishAudio` to use HTTP chunked streaming instead of `response.blob()`
2. Feed chunks to a Web Audio API `AudioBufferSourceNode` as they arrive
3. This reduces perceived TTS latency from ~2-3s (full blob) to ~100ms (first chunk)

### Phase 3: Add more Fish Audio voices (15 minutes)

1. Find 3-4 good reference voices on Fish Audio's voice library
2. Add them to `CURATED_VOICES` with their `reference_id`s
3. Options: different accents, genders, tones (butler, casual, professional)

### Phase 4: Update Setup Wizard + Settings (20 minutes)

1. Setup Wizard: Add a "Voice" step that prompts for Fish Audio API key
2. Link to https://fish.audio — "Sign up free, get API key"
3. Settings: Show "Fish Audio (Free)" as the recommended provider
4. Voice preview: Let user test different voices before choosing

### Phase 5: Remove Gemini hardcoded key (10 minutes)

1. Remove the hardcoded `DEFAULT_GEMINI_KEY` from `ttsPlayer.ts`
2. Gemini TTS becomes an optional alternative (user provides their own key)
3. Fish Audio Free becomes the only zero-cost default

---

## 8. File Changes Summary

| File | Phase | Change |
|------|-------|--------|
| `frontend/src/audio/ttsPlayer.ts` | 1 | `model: "s2.1-pro"` → `"s2.1-pro-free"`, make Ethan default |
| `frontend/src/audio/ttsPlayer.ts` | 2 | Rewrite `playFishAudio` to use HTTP streaming |
| `frontend/src/audio/ttsPlayer.ts` | 3 | Add 3-4 more Fish Audio voices to `CURATED_VOICES` |
| `frontend/src/audio/ttsPlayer.ts` | 5 | Remove hardcoded Gemini API key |
| `src-tauri/src/commands.rs` | 1 | `default_tts_provider()` → `"fish_audio"`, `tts_voice` → `"ethan"` |
| `frontend/src/setup/SetupApp.tsx` | 4 | Add Fish Audio API key prompt + voice preview |
| `frontend/src/settings/SettingsApp.tsx` | 4 | Show "Fish Audio (Free)" as recommended |

---

## 9. Decision Matrix

| Option | Cost | Quality | Permanence | Effort | Recommended? |
|--------|------|---------|------------|--------|--------------|
| **Fish Audio `s2.1-pro-free`** | $0 | Excellent | Likely permanent (extended 2×) | Low (1 line change) | ✅ **YES** |
| Fish Audio `s2.1-pro` (paid) | $15/M bytes | Excellent | Permanent | Low | Backup |
| ElevenLabs Free | $0 | Excellent | Permanent | N/A | ❌ No API access |
| ElevenLabs Starter | $5/mo | Excellent | Permanent | Low | Alternative |
| ElevenLabs Startup Grant | $0 for 12mo | Excellent | 12 months | Medium (apply) | If eligible |
| Gemini Flash TTS | $0 | Good | Rate-limited | Already done | Current default |
| Web Speech API | $0 | Fair | Permanent | Already done | Fallback |
| Self-host Fish Speech | $0 | Excellent | Permanent | High (needs GPU) | Future option |

**Final recommendation: Switch to Fish Audio `s2.1-pro-free` as the default TTS provider. It's free, has full API access, ~100ms latency with streaming, supports voice cloning, and is the same model as their paid tier. The free period has been extended twice and Fish Audio has stated it's economically viable (not a subsidy). If it ever ends, switching to paid is a one-line change at $0.0075 per response.**
