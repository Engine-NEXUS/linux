# Copy-Paste Research Prompt — Same Voice Offline + Agent Task-Watch (my idea, for my own research)

> Paste everything below the line into Claude / Antigravity / opencode / Devin
> (or any agent) to redo and extend this research yourself.

---

## Research brief: one assistant voice online AND offline, plus an agent-completion watcher that costs ~zero API

### 1. Problem statement (my idea, in my words)

I run a voice assistant on my Windows laptop. It speaks with a cloud TTS voice
when the internet is available and falls back to a different-sounding local voice
when offline — the voice swap mid-sentence sounds broken. I want:

1. **One identical voice online and offline.** When I change the assistant's voice,
   the same voice must be downloaded/prepared locally so that with no internet the
   assistant still speaks in exactly the voice I chose.
2. **Screen stays awake automatically** while the assistant is speaking or working,
   so the laptop never sleeps mid-task.
3. **"Tell me when it's done" watching.** I give prompts to coding agents (Claude,
   Antigravity, opencode, DevinAI) and wait. I want the assistant to observe the
   agent's window, keep the screen awake meanwhile, and notify me the moment a
   response arrives — **automatically, without burning through API key limits**.

### 2. What prior research already found (verify and extend this, don't take it on faith)

- **Cloud TTS voices (e.g. Microsoft Edge voices) are server-side weights — there
  is no "download this voice" endpoint.** "Same voice offline" therefore means
  *reproducing the timbre locally*. Three routes were compared:
  - **R1 — zero-shot synthesis (XTTS-v2):** clone from a ~6-second clip and
    synthesize directly. Found: ~4 GB disk with torch, ~2–4 GB RAM, slow on CPU
    (seconds per sentence), and the Coqui Public Model License restricts
    commercial use. Tentative verdict: REJECTED (too heavy/slow/license-hostile).
  - **R2 — convert, don't synthesize (OpenVoice V2):** keep a small fast local
    TTS as the base speaker and run a tone-color converter to the enrolled voice.
    Found: MIT licensed, free for commercial use, feed-forward (fast, no
    autoregression — paper claims 12× real-time on an A10G). Tentative verdict:
    RECOMMENDED. **Verify: CPU speed on a laptop, model/footprint sizes, and
    speaker-similarity scores vs a cloud reference.**
  - **R3 — cache-and-replay:** record cloud utterances while online, replay exact
    audio offline. Exact but only for previously heard sentences. Complement, not
    a solution.
- **Legal red line found:** cloning Microsoft's voice talent (Ava, Ryan, …) from
  Edge output has no public license grant (Microsoft Q&A threads explicitly give
  no confirmation; Azure's own custom-voice path requires talent consent). So
  enrollment should be the **user's own voice** (read a ~30 s script) or a
  royalty-free reference — clips local-only, never redistributed. **Verify this
  legal reading independently; I am not a lawyer.**
- **Keep-awake is trivial:** Win32 `SetThreadExecutionState(ES_CONTINUOUS |
  ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)` while speaking/working, cleared on
  idle. Zero API cost.
- **Task watching without API burn:** detect completion with LOCAL OS signals
  only (window-title change, process CPU decay, output-file mtime, UIA
  accessibility events); call cloud vision/LLM at most once to confirm a
  suspected completion. Math to check: naive 30-second screenshot polling ≈ 960
  vision calls per 8-hour run — over typical free tiers (≈500/day) — while the
  event-driven design uses ~1–3 calls per task.
- **Prior art to mine:** UFO (Microsoft UI agent), OSWorld / WindowsAgentArena
  (task-success detectors), ScreenSpot / OmniParser / Ferret-UI (grounding),
  VALL-E / VALLE-X / VoiceBox / HierSpeech++ (SOTA cloning, likely too heavy),
  kNN-VC / FreeVC (fallback converters), Azure Custom Neural Voice (the
  consent-gated legal shape to copy).

### 3. Sources to start from (re-verify each)

- https://huggingface.co/coqui/XTTS-v2
- https://github.com/idiap/coqui-ai-TTS
- https://docs.coqui.ai/en/latest/models/xtts.html
- https://arxiv.org/html/2312.01479v2 (OpenVoice paper)
- https://docs.myshell.ai/technology/openvoice
- https://huggingface.co/myshell-ai/OpenVoiceV2
- https://research.myshell.ai/open-voice
- https://learn.microsoft.com/en-us/azure/ai-foundry/responsible-ai/speech-service/text-to-speech/data-privacy-security
- https://learn.microsoft.com/en-us/legal/cognitive-services/speech-service/text-to-speech/transparency-note
- https://learn.microsoft.com/en-us/answers/questions/5925556/commercial-use-of-edge-read-aloud-voices-via-edge
- https://learn.microsoft.com/en-us/answers/questions/6007729/commercial-use-of-microsoft-edge-text-to-speech-vi

### 4. Questions for your own research (answer with numbers, tables, and citations)

1. **Voice-quality shootout:** for ONE fixed reference voice, compare R1 vs R2 vs
   a fine-tuned small model on (a) speaker-similarity (SIM / MOS-style scores),
   (b) CPU seconds-per-sentence on a mid laptop, (c) disk + idle RAM. Declare a
   winner with a table.
2. **Licensing:** can output of a cloud TTS voice be used as a cloning reference
   for personal offline use? Distinguish personal use vs redistribution, and cite
   actual terms — flag every uncertainty instead of guessing.
3. **Watcher design:** for Claude / Antigravity / opencode / DevinAI specifically,
   which local completion signals exist per app (window title patterns? process
   behavior? output files?) and which need a fallback? Rank by reliability.
4. **API budget:** price the watcher per 8-hour run on free tiers (Groq, Gemini)
   and confirm the "≈zero" claim or refute it with numbers.
5. **Failure modes:** what makes each approach announce a false "done" (streaming
   partials? spinners? rate-limit pages?) and what single confirmation step kills
   each false positive?
6. Deliverable: a ranked recommendation (what to build first) with effort
   estimates and a test plan I can run on my own laptop.
