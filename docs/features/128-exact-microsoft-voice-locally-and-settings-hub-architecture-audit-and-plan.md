# 128 — The EXACT same voice locally (Ava / Ryan) + how the Settings hub really works and why it drifted (RESEARCH + AUDIT + PLAN, nothing built)

Date: 2026-10-16. Supersedes the recommendation in `127-one-voice-everywhere-...md` (user rejected "a lookalike": *"if it doesn't sound the same, no — I want a carbon copy"*). The bug findings of 127 (prosody changes per line, the "Unstop"->"stop" Urgent bug, no engine switching) still stand and are folded in below.

---

# PART A — Where the exact voice can be downloaded

## A.1 Verified facts (sources at the bottom)

1. **Microsoft publishes the Azure neural TTS voices as Docker images.** Registry listing (fetched 2026-10-16) shows the tags
   `mcr.microsoft.com/azure-cognitive-services/speechservices/neural-text-to-speech:3.13.1-amd64-en-us-avaneural` (also `...avamultilingualneural`)
   and `...:3.13.1-amd64-en-gb-ryanneural` (34 Ryan versions, 1.5.0 -> 3.13.1). So **both of your voices exist as a downloadable, local model.**
2. Microsoft's docs say SSML behaviour is "consistent for each text to speech voice both in the Azure cloud and the container". Edge's "Ava/Ryan Online (Natural)" is the same Azure voice family the app already uses through `tts_edge.rs`. This is the only way to run **the same voice** locally (same voice model, not a lookalike).
3. Hardware (docs): neural TTS container **min 6 cores / 12 GB, recommended 8 cores / 16 GB**, each core >= 2.6 GHz, host **must support AVX2**, Docker with **Linux containers** on Windows.
4. **Billing/connection rule (important):** a normal ("connected") container needs an Azure **Speech resource** (the free **F0** tier is allowed; F0 = 0.5 M characters/month) and reports usage to Azure about every 10-15 minutes. If it cannot reach the billing endpoint it keeps running but stops serving, retries 10 times, then **stops serving requests** => roughly **1.7-2.5 hours of internet-free operation**, then it is silent until the link returns.
5. **Fully offline (days/weeks) = "disconnected container"**: needs Microsoft's **request form + approval** and a **commitment-tier plan** (charged up-front, calendar-year). Microsoft's stated eligibility is "strategic customer or partner" with a no-connectivity use case; approval is by email within ~10 business days. **For an individual this is likely to be refused (unverified for you).**
6. **Windows 11 Narrator "natural" voices are on-device** (Settings > Accessibility > Narrator > Add natural voices). The list includes **Ryan (UK male)** but **not Ava** (Aria, Guy, Jenny, Ryan, Sonia, ...). Windows does not expose them to other apps; a third-party MIT project (`NaturalVoiceSAPIAdapter`) hacks them into SAPI using keys extracted from system files; its own page says Microsoft does not allow third-party use, it can break after any update, and newer Windows 11 voices reportedly stopped working with it. Your build is 10.0.26300 (very new). Whether Narrator-Ryan is bit-identical to cloud Ryan is **unverified** (same voice talent; the on-device model may differ).

## A.2 Options, honest ranking (exact voice only)

| # | Option | Same voice? | Works offline? | Effort / catch |
|---|---|---|---|---|
| 1 | **Azure neural-TTS container** (Ava and/or Ryan) on your **server** or this laptop | **Yes** (same Azure voice) | ~2 h grace, then silent (connected); unlimited only if Microsoft approves disconnected | Docker + a free Azure account/Speech F0 resource (**you** must create it; I cannot sign up or enter credentials). Official, no more Edge 403/429 problems, LAN-fast. |
| 2 | **Pre-rendered clip bank** in that voice (rendered by #1 or Edge) | **Yes** | **Forever**, but only for the lines in the bank | Cheap; covers acks, greetings, confirmations, common answers. Not for arbitrary new sentences. |
| 3 | Narrator natural **Ryan** (JARVIS only) via the SAPI adapter | Probably very close, unverified | Yes | Unofficial hack; fragile on your Windows build; no Ava. Only as an experiment. |
| 4 | Edge online (today) | Yes | No | Unofficial endpoint (it was 403 for weeks, 429 on 2026-10-15). |
| 5 | Kokoro / Windows voice / any other model | **No** | Yes | Rejected by the user. |

**Bottom line:** you can download Ava and Ryan, but not as a free stand-alone offline file. Microsoft's licence ties them to an Azure billing meter. The realistic "always the same voice" design is **#1 + #2 + #4 chained, nothing else ever spoken**.

## A.3 Can THIS laptop host it?
`i7-1355U` (10 cores / 12 threads, AVX2-capable), **15.7 GB RAM**, **26 GB free disk**, WSL present, **Docker not installed**. A 12 GB container would leave ~3.7 GB for Windows + NEXUS + WebView2: too tight. **Run the container on your 24/7 server** (docs/features/121: 64 GB RAM) and let NEXUS call it over LAN/Tailscale; keep the laptop for NEXUS only. (Laptop-only is possible for a trial with the container capped at 8 GB, but not recommended.)

## A.4 Plan: "Same Voice Anywhere"

**E0 — Prove it is the same voice (0.5 day, needs you for the Azure step).** You create a free Azure account + Speech resource (F0). On a Docker host: `docker run ... neural-text-to-speech:3.13.1-amd64-en-us-avaneural Eula=accept Billing=<endpoint> ApiKey=<key>`. I render the same 30 sentences with the container and with Edge and compare: F0 median/range, MFCC distance, speaker-embedding cosine (script `scripts/voice_twin_score.py`), plus a blind ABX for you (target: you cannot beat chance, <= 12/20). **Gate: if the container is not indistinguishable, the plan stops here and we choose again.**

**E1 — `azure_container` backend (1.5 days).** New TTS backend next to `tts_edge.rs`: HTTP POST SSML to `http://<host>:5000/cognitiveservices/v1` (or the Speech WebSocket, same family of protocol the Edge client already speaks, to keep word-boundary timing for captions - **to verify** whether the container returns word boundaries over REST; otherwise use the WebSocket). Settings: host, port, key in the keychain (never in settings.json), voice name. Health: `/ready`, `/status`.

**E2 — One voice, ordered backends, no strangers (1 day).** `speak()` tries: container -> Edge online -> clip bank -> **caption + soft chime** (never another voice). Remove Kokoro and the Windows voice from the audible path (code stays dormant behind a dev flag). No engine change inside an utterance; the persona's prosody is **fixed** (no auto-emotion) and the substring bug in `pick_emotion` is fixed.

**E3 — Clip bank (1 day).** Render once with the container: the 29 stock lines + the ~300 most frequent replies/acks (derived from your own `episodes`/missed logs) + boot/shutdown/greeting lines, keyed by `persona + backend + prosody + text`, persisted. Offline = these play instantly in the exact voice.

**E4 — Operate the container (0.5 day).** `docker compose` file with `restart: always`, a watchdog line in NEXUS (`[VOICE] container=ok|down meter=ok|stale`), a spoken/visible warning when the billing link is lost for > 60 min ("voice server will pause in ~1 h"), and graceful fallback to Edge online.

**E5 — Optional: apply for disconnected containers** (form link in the sources). Only if you want days of offline dynamic speech; expect a possible refusal.

---

# PART B — How the Settings hub actually works, and why your NEXUS choice left a JARVIS value behind

## B.1 The architecture as built (verified in code)

```
                 ┌─────────────── writers (UI) ───────────────┐
 Settings window │ SettingsApp.tsx  → save_settings(WHOLE struct)  (footer "Save")
 Command Hub     │ SettingsSidebarApp.tsx → save_settings(WHOLE struct) (Save, and again on EVERY theme toggle)
 Hub pages       │ MemoryPage.tsx → save_settings(…)
 Setup wizard    │ SetupApp.tsx → save_settings(updated)
 Voice cards     │ set_voice_preference(key) → writes 5 fields at once (tts.rs)
 Tray / Rust     │ tray.rs and others edit settings.json directly
                 └────────────────────┬────────────────────────┘
                                      ▼
                 %APPDATA%\com.nexus.assistant\settings.json   (92 fields = Rust `NexusSettings`)
                                      ▲
 readers: 59 `settings.json` reads in 25 Rust files (read_edge_tts_voice, read_selected_voice, flag(), …),
 each re-reading the file on demand; the ONLY live broadcast is `settings:updated`, which only the theme listens to.
 Secrets (API keys) are merged specially (`merge_key`); server identity is kept in nexus-config.json.
```

Facts: `save_settings` (commands.rs:2229) **replaces the whole file** from the struct the window sends; it only protects API keys and identity. The window's struct is a **snapshot taken when the window opened** (`get_settings` on mount, `SettingsApp.tsx:111`) plus whatever the user edited. So it is **last-writer-wins with a stale snapshot**. The voice settings are also stored **redundantly**: `selectedVoice`, `edgeTtsVoice`, `ttsVoice`, `ttsProvider`, `offlineVoiceModel` all describe ONE fact (which persona).

## B.2 Your sequence, reconstructed from file timestamps

* 15-10 23:01:33 last cached Ryan (JARVIS) clip written -> JARVIS was active.
* **23:02:43** you picked **NEXUS** in the voice cards: IPC `set_voice_preference("nexus")` wrote `selectedVoice=nexus`, `edgeTtsVoice=Ava`, `offlineVoiceModel=af_heart`, and the offline-twin swap finished (`voices/manifest.json` = af_heart, 23:02:43). All correct.
* **23:02:44** (one second later) you pressed **Save**. The Command Hub's `equipVoice` only mirrors **two** of the fields into its React state (`edgeTtsVoice`, `selectedVoice`; `SettingsSidebarApp.tsx:907-908`). Its stale copy still held `offlineVoiceModel: "bm_george"` from the JARVIS period, and `save_settings` wrote the whole snapshot -> **`offlineVoiceModel` went back to bm_george**. Nothing has written settings.json since.
* Today's file: `selectedVoice=nexus`, cloud=Ava (correct), `offlineVoiceModel=bm_george` (stale), installed twin `af_heart` (correct). At run time the app derives the local twin from `selectedVoice` (not from `offlineVoiceModel`), so the damage is **latent, not audible today** - but the same pattern silently reverts ANY setting changed by an IPC while a window with an old snapshot is open (theme toggles in the hub re-save the whole struct every time, `SettingsSidebarApp.tsx:323,1298`).

So the voice itself is not "stuck on JARVIS"; the settings system just lets two writers disagree.

## B.3 Plan: one settings authority

**S0 — Hot-fix now (0.25 day).** `equipVoice` (hub) and `handleEquipPersona` (window) re-fetch `get_settings` after `set_voice_preference` and replace their local snapshot; theme toggles send only `themeMode`. Add the regression test that replays the 23:02 sequence.

**S1 — Backend-owned store (1 day).** A `SettingsStore` in Rust (in-memory, one mutex, atomic file write, monotonically increasing `rev`). New IPC `settings_patch({keys…, base_rev})` merges **only the fields that changed**; `save_settings(whole struct)` stays only as a deprecated shim that diffs against the current store and applies just the differences (so stale snapshots can no longer overwrite).

**S2 — One fact, one field (0.5 day).** `voicePersona` is the only stored voice setting; `edgeTtsVoice`, `ttsVoice`, `ttsProvider`, `offlineVoiceModel`, the container voice name, and the clip-bank key are **computed** from `voice_catalog` (written to the file only for old builds). Startup logs `[SETTINGS] voicePersona=nexus cloud=Ava twin=af_heart OK` and repairs drift.

**S3 — Live propagation (1 day).** The store emits `settings:changed {keys, rev}`; readers (`read_*`, `flag()`) become cache lookups instead of 59 disk reads; windows subscribe and re-render, so a change in the hub shows in the Settings window instantly.

**S4 — One frontend settings hook (0.5 day).** `useSettings()` shared by the Settings window, Command Hub, Memory page and wizard (patch-based), deleting the per-window copies that caused the mirror bug.

**S5 — Guard rails (0.25 day).** A CI test that every `NexusSettings` field is either patchable or derived; a mutation test that reverting the diff logic reproduces the 23:02 bug.

---

## Decisions needed from you
1. Are you willing to create a **free Azure account + Speech resource (F0)** so we can run the official Ava/Ryan container? (I cannot create accounts or enter credentials; you do that step, I do everything else.) Without it, the exact local voice is not obtainable.
2. Host: **your server** (recommended) or this laptop (tight on RAM)?
3. Which voice is THE voice: **Ava (NEXUS)**, **Ryan (JARVIS)**, or both as separate personas (two containers)?
4. Offline policy: accept **~2 h of dynamic speech offline + unlimited stock lines from the clip bank**, or also apply for Microsoft's disconnected licence (may be refused)?
5. Approve **S0 hot-fix first**, then S1-S5?

## Not verified
Whether the container's voice is *audibly* identical to Edge's (E0 tests it; Microsoft's docs say consistent, I have not heard it). Whether the container returns word boundaries over REST (captions). Container image size and real latency on your server. Pricing beyond the 0.5 M free characters/month (not fetched; the pricing page masked unit prices). Whether Microsoft would approve you for disconnected use. Narrator-Ryan vs cloud-Ryan identity. The Edge/Ava, Narrator and container statements come from Microsoft/GitHub pages fetched today; nothing was run or listened to.

## Sources
- Neural TTS container: https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-container-ntts
- Speech containers (requirements, billing, F0, 10-15 min meter): https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-container-howto
- Disconnected containers (approval, commitment tier): https://learn.microsoft.com/en-us/azure/ai-services/containers/disconnected-containers
- Registry tag list (Ava/Ryan tags): https://mcr.microsoft.com/v2/azure-cognitive-services/speechservices/neural-text-to-speech/tags/list
- Narrator natural voices list: https://support.microsoft.com/en-us/topic/appendix-a-supported-languages-and-voices-4486e345-7730-53da-fcfe-55cc64300f01
- NaturalVoiceSAPIAdapter (caveats): https://github.com/gexgd0419/NaturalVoiceSAPIAdapter
- Speech pricing (F0 0.5 M chars/month): https://azure.microsoft.com/en-au/pricing/details/cognitive-services/speech-services/
