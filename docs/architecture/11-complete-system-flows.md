# NEXUS Complete System Flows & Architecture Catalog

> **Status:** Production Reference (September 2026)  
> **Master Architecture:** Tauri v2 Thin Client + Central Orchestrator + Multi-Subsystem Hybrid Brain.  
> Every end-to-end path from speech input → intent parsing → orchestrator routing → safety barriers → backend execution → UI/voice output.

---

## Table of Contents

1. [Flow 1: WhatsApp MCP Messaging Flow (Write with Safety & Auto-Resume)](#flow-1-whatsapp-mcp-messaging-flow)
2. [Flow 2: WhatsApp Desktop Chat & Partial Prompt Flow](#flow-2-whatsapp-desktop-chat--partial-prompt-flow)
3. [Flow 3: Swiggy Food & Instamart Commerce Flow (OAuth PKCE)](#flow-3-swiggy-food--instamart-commerce-flow)
4. [Flow 4: Amazon E-Commerce Product Search Flow](#flow-4-amazon-e-commerce-product-search-flow)
5. [Flow 5: GitHub PR Code Analysis Flow (GLM-4.7/5.3 AI Review)](#flow-5-github-pr-code-analysis-flow)
6. [Flow 6: GitHub Sub-Command CLI Operations Flow (28 Typed Actions)](#flow-6-github-sub-command-cli-operations-flow)
7. [Flow 7: 9Router Ultra-Low Latency AI Cascade Flow](#flow-7-9router-ultra-low-latency-ai-cascade-flow)
8. [Flow 8: Command Center Multi-Step Compound Workflow](#flow-8-command-center-multi-step-compound-workflow)
9. [Flow 9: Local Desktop App Launching & Resolution Flow](#flow-9-local-desktop-app-launching--resolution-flow)
10. [Flow 10: Native OS Media & System Control Flow](#flow-10-native-os-media--system-control-flow)
11. [Flow 11: Always-Listening Wake Word & Audio Capture Flow](#flow-11-always-listening-wake-word--audio-capture-flow)
12. [Flow 12: Speech-to-Text & Phonetic Hotword Correction Flow](#flow-12-speech-to-text--phonetic-hotword-correction-flow)
13. [Flow 13: Local BERT-Mini NLU & Cloudflare R2 OTA Update Flow](#flow-13-local-bert-mini-nlu--cloudflare-r2-ota-update-flow)
14. [Flow 14: Admin Qwen Brain & Continuous Training Loop](#flow-14-admin-qwen-brain--continuous-training-loop)
15. [Flow 15: Meeting Privacy Protection & Stream Suppression Flow](#flow-15-meeting-privacy-protection--stream-suppression-flow)
16. [Flow 16: Ghostwriter Dictation Mode Flow](#flow-16-ghostwriter-dictation-mode-flow)

---

## Flow 1: WhatsApp MCP Messaging Flow

Handles real-world conversational messaging requests via the local WhatsApp MCP bridge with safety confirmation, live QR sidebar rendering, and auto-resume on pairing.

```text
User says: "send hi to mummy in whatsapp" (or "send a whatsapp message to dad saying on my way")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::SendWhatsAppMessage { contact: "mummy", message: "hi" }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::Mcp ➔ McpServer::WhatsApp ➔ tool: "send_message"
                          │
                          ▼
             Confirmation Gate (mcp_client.rs)
   ➔ Emits OrchestratorEvent::Confirm ("Send WhatsApp message to mummy: \"hi\"?")
                          │
                          ▼
             User Approves / Voice Confirms
                          │
                          ▼
       Orchestrator calls http://127.0.0.1:8765/mcp
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
      [Bridge Ready]             [Bridge Disconnected / Unpaired / Expired]
            │                           │
            │                           ├─ 1. Speaks Voice Guidance:
            │                           │     "WhatsApp isn't connected, sir — I've opened
            │                           │      the setup card with the QR. Scan it with your
            │                           │      phone and I'll confirm."
            │                           │
            │                           ├─ 2. Shows Sidebar Overlay:
            │                           │     Displays Connect Card with live rotating QR code,
            │                           │     numbered steps, and direct browser link.
            │                           │
            │                           ├─ 3. Stashes Failed Task:
            │                           │     Saved in PENDING_MCP_RETRY.
            │                           │
            │                           ├─ 4. Background Ready Monitor (spawn_ready_monitor):
            │                           │     Polls bridge every 5s; auto-refreshes QR payload.
            │                           │
            │                           ▼
            │               User Scans QR Code with Phone
            │                           │
            │                           ├─ 5. Bridge saves fresh Signal session into local whatsapp.db.
            │                           ├─ 6. Monitor detects `McpConnectState::Ready`.
            │                           ├─ 7. Sidebar card updates to "Connected".
            │                           ├─ 8. Speaks: "WhatsApp is connected, sir."
            │                           └─ 9. Auto-resumes & sends original message ("hi" to mummy)!
            │
            ▼
   Message Sent Successfully!
```

---

## Flow 2: WhatsApp Desktop Chat & Partial Prompt Flow

Distinguishes between app-opening shortcuts and incomplete message requests.

```text
User says: "open chat with mom" OR "send a message on whatsapp"
                          │
                          ▼
            Intent Parser (intent_parser.rs)
                          │
            ┌─────────────┴───────────────────────────┐
            ▼                                         ▼
   [Chat Open Phrase]                       [Partial Message Phrase]
"open chat with mom" / "message mom"   "send a message on whatsapp" / "send message to mummy"
            │                                         │
            ▼                                         ▼
ParsedIntent::WhatsappChat { contact }       ParsedIntent::NeedMoreInfo { prompt }
            │                                         │
            ▼                                         ▼
  Central Orchestrator                      Central Orchestrator
➔ Subsystem::LocalCommand                  ➔ Speaks prompt via Web Speech TTS:
➔ command_executor::open_whatsapp_chat()      • "What should I say to mummy?" OR
➔ Opens WhatsApp desktop app/URI              • "Who should I message on WhatsApp,
  (whatsapp://send?phone=... / web)              and what should I say?"
            │                                         │
            ▼                                         ▼
  Chat Window Focused                         Awaits Next User Turn
```

---

## Flow 3: Swiggy Food & Instamart Commerce Flow

Manages food and grocery ordering via Swiggy MCP servers, handling OAuth 2.1 PKCE authorization, restaurant search, cart operations, and confirmation-gated order placement.

```text
User says: "order a pizza from dominos" (or "order biryani on swiggy")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::OrderFood { query: "pizza", restaurant: Some("dominos") }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::Mcp ➔ McpServer::SwiggyFood ➔ tool: "search_restaurants"
                          │
                          ▼
             Pre-Flight Credential Check
   ➔ Checks auth_vault.rs for service "swiggy"
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
      [Token Live]             [Token Missing / Expired]
            │                           │
            │                           ├─ 1. Speaks: "Your Swiggy login expired, sir —
            │                           │     reconnect it in Settings, Connections tab."
            │                           ├─ 2. Opens Sidebar Connect Card with Login Button.
            │                           ├─ 3. User clicks Login ➔ Browser OAuth PKCE.
            │                           ├─ 4. Cloudflare Worker exchanges code ➔ D1 database.
            │                           └─ 5. Auth Vault fetches fresh token ➔ Retries search.
            │
            ▼
   Call McpServer::SwiggyFood ("search_restaurants")
   ➔ Returns matching Dominos outlets + menu items
                          │
                          ▼
   User says: "add Margherita pizza to cart and place order"
                          │
                          ▼
   Write Action Confirmation Barrier:
   ➔ Requires confirmation for "update_food_cart" and "place_food_order"
   ➔ Emits OrchestratorEvent::Confirm:
      "This will perform an irreversible action on swiggy-food. Tool: place_food_order. Proceed?"
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
       [Approved]                  [Rejected]
            │                           │
            ▼                           ▼
   Orchestrator calls              Cancels order & speaks:
   SwiggyFood "place_food_order"   "Order cancelled, sir."
   ➔ Returns Order Tracking ID & ETA
   ➔ Speaks: "Your order from Dominos has been placed, sir."
```

---

## Flow 4: Amazon E-Commerce Product Search Flow

Processes shopping search queries through the Amazon MCP scraper / browser bridge.

```text
User says: "search for sony headphones on amazon" (or "buy laptop on amazon")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::SearchProduct { query: "sony headphones" }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::Mcp ➔ McpServer::Amazon
   ➔ Tool: "amazon_search" (params: { query: "sony headphones", max_results: 5 })
                          │
                          ▼
            Execute MCP HTTP JSON-RPC
   ➔ Calls http://127.0.0.1:8766/mcp
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
       [Success]                    [Bridge Down]
            │                           │
            │                           ├─ Speaks: "Amazon search bridge is offline, sir."
            │                           └─ Opens Connect Card in Sidebar with setup guide.
            ▼
   Parse Product Items:
   ➔ Extracts title, price, rating, prime eligibility, and URL
                          │
                          ▼
   Orchestrator Emits Result:
   1. Speaks: "Found top results for Sony headphones on Amazon, starting at 4,990 rupees."
   2. Response Sidebar slides in:
      Streams product cards with prices, ratings, and one-click purchase links.
```

---

## Flow 5: GitHub PR Code Analysis Flow

Voice-triggered senior engineer PR code review using Cloudflare Workers AI with streaming UI animation and immediate audio feedback.

```text
User says: "analyse PR 5 in servx"
                          │
                          ▼
         Audio Recorder & Phonetic Normalizer
   ➔ Mishearing correction: "cervix" ➔ "servx", "pf5" ➔ "PR 5"
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::AnalysePr { owner: None, repo: "servx", pr_number: 5 }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::WorkerBackend
   ➔ Immediate Long-Running Query Detection (<1ms)
                          │
                          ├─ 1. Speaks Instant Ack: "On it, sir."
                          ├─ 2. Hides Floating Orb (no awkward waiting state).
                          └─ 3. Dispatches payload to Cloudflare Worker.
                          │
                          ▼
               Cloudflare Worker Backend
   1. Resolves short repo name "servx" against user's GitHub repos via OAuth.
   2. Fetches PR files, unified diffs, commits, comments via GitHub REST API.
   3. Selects AI Model:
      • Normal Context (<520k chars): @cf/zai-org/glm-4.7-flash
      • Deep Review (>520k chars): @cf/zai-org/glm-5.3-flash
   4. Executes Senior Review Prompt (Risk Assessment, Code Quality, Edge Cases, Security).
                          │
                          ▼
               Worker Returns Result JSON
   { "text": "Here is the analysis of PR 5, sir.", "analysis": "# Code Review PR #5..." }
                          │
                          ▼
            Client Receives Result Event
   1. Shows Response Sidebar Window (show_sidebar_with_analysis).
   2. Renders Markdown Streaming Word Animation (wordFadeIn, natural spacing).
   3. Orb reappears briefly to speak: "Here is the analysis of PR 5, sir."
   4. Orb transitions back to Idle.
```

---

## Flow 6: GitHub Sub-Command CLI Operations Flow

28 typed sub-commands executed directly on device via `octocrab` with conflict pre-checks and centralized destructive confirmations.

```text
User says: "merge PR 23 in ultron" (or "approve PR 12", "create issue bug in ultron")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::GitHub(GitHubCommand::MergePr { repo: "ultron", pr_number: 23 })
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::GitHub ➔ github_cmd::process()
                          │
                          ▼
   1. Token Fetch: Retrieves cached GitHub OAuth token (or queries Worker).
   2. Pre-Check: Checks mergeability, CI status, and branch protection rules.
   3. Destructive Action Check:
      • Read commands (list_prs, show_pr) execute immediately.
      • Destructive commands (merge, close, delete_branch) trigger confirmation:
        "Merge PR #23 in ultron? Merge method: merge commit."
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
       [Approved]                  [Rejected]
            │                           │
            ▼                           ▼
   Execute Octocrab API Call       Cancels action & speaks:
   ➔ octocrab.pulls().merge()      "Merge cancelled, sir."
   ➔ Success Response
            │
            ▼
   Orchestrator Emits Result:
   ➔ Speaks: "PR 23 in ultron has been merged successfully, sir."
```

---

## Flow 7: 9Router Ultra-Low Latency AI Cascade Flow

Direct localhost → Cloud Free LLM cascade that bypasses Worker round-trips for general Q&A with 3–7x lower latency (~240ms).

```text
User says: "what is the capital of France?" (or general conversational query)
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::Unknown { text: "what is the capital of France?" }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::WorkerBackend ➔ dispatch_to_worker()
   ➔ 9Router Routing Gate: router::can_route(transcript) == true
                          │
                          ▼
               9Router Cascade (router.rs)
                          │
            ┌─────────────┼─────────────┬─────────────┐
            ▼             ▼             ▼             ▼
       1. Cerebras    2. Groq      3. Gemini     4. Worker Fallback
       Llama 3.3 70B  gpt-oss-120b gemini-flash  Cloudflare Neurons
       (~80ms)        (~120ms)     (~400ms)      (~1200ms)
            │             │             │             │
            └─────────────┴──────┬──────┴─────────────┘
                                 │
                                 ▼
                     Provider Returns Answer
            "The capital of France is Paris, sir."
                                 │
                                 ▼
         TTS speaks answer aloud ➔ Orb transitions to Idle
```

---

## Flow 8: Command Center Multi-Step Compound Workflow

Splits multi-step utterances (*"X then Y"*) and executes sub-tasks across different subsystems with stateful pause/resume.

```text
User says: "open chrome then search for rust tutorials on amazon"
                          │
                          ▼
         Command Center (command_center.rs)
   ➔ split_compound() ➔ ["open chrome", "search for rust tutorials on amazon"]
   ➔ build_plan() ➔ TaskPlan with 2 sequential PlanSteps:
      • Step 1: Subsystem::LocalCommand (OpenApp "chrome")
      • Step 2: Subsystem::Mcp (SearchProduct "rust tutorials")
                          │
                          ▼
                execute_plan() - Step 1
   ➔ command_executor::execute_command(OpenApp "chrome")
   ➔ Result: "Opened Chrome, sir."
                          │
                          ▼
                execute_plan() - Step 2
   ➔ Checks if Step 1 was cancelled (barge-in guard)
   ➔ Calls McpServer::Amazon ("amazon_search")
   ➔ Result: "Found Rust tutorials on Amazon."
                          │
                          ▼
                  Merge Step Results
   ➔ Combines responses: "Opened Chrome, sir. Found Rust tutorials on Amazon."
   ➔ Speaks merged outcome aloud.
```

---

## Flow 9: Local Desktop App Launching & Resolution Flow

Instant (<1ms) application and PWA launching using pre-indexed OS shortcut caches and fuzzy phrase matching.

```text
User says: "open spotify" (or "launch vs code", "open whatsapp")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::OpenApp { target: "spotify" }
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::LocalCommand ➔ command_executor::execute_command()
                          │
                          ▼
            App Registry (app_registry.rs)
   1. Checks in-memory Resolution Cache (remembers past user selections).
   2. O(1) HashMap lookup across pre-indexed Start Menu / Applications / PWAs.
   3. Fuzzy Levenshtein match fallback.
                          │
                          ▼
             Launch Target Resolved:
   • Windows: ShellExecuteW to "shell:AppsFolder\Spotify..." OR exe path
   • macOS: `open -b com.spotify.client`
   • Linux: XDG .desktop exec
                          │
                          ▼
   Orb speaks: "Opening Spotify, sir." ➔ Overlay hides
```

---

## Flow 10: Native OS Media & System Control Flow

Hardware media key and system volume manipulation without external API dependencies.

```text
User says: "pause music" (or "next track", "volume up", "mute")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::MediaPlayPause / MediaNext / MediaPrevious / MediaStop
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   ➔ Subsystem::LocalCommand ➔ command_executor.rs
                          │
                          ▼
            Native OS Dispatch:
   • Windows: SendInput with VK_MEDIA_PLAY_PAUSE / VK_VOLUME_UP
   • macOS: CoreMedia / IOKit media key events
   • Linux: D-Bus MPRIS `org.mpris.MediaPlayer2.Player.PlayPause`
                          │
                          ▼
   Orb speaks brief confirmation ("Paused, sir.") ➔ Resets to Idle
```

---

## Flow 11: Always-Listening Wake Word & Audio Capture Flow

Continuous keyword spotting (KWS) in pure Rust at <1.5% CPU using openWakeWord with zero audio leaving the local device.

```text
User speaks in room: "...nexus what time is it..."
                          │
                          ▼
               Audio Capture via `cpal`
   ➔ 16 kHz Mono PCM audio stream (continuous)
                          │
                          ▼
         openWakeWord Engine (wakeword_oww.rs)
   Evaluates 80ms chunks (1280 samples) via tract-onnx:
   1. melspectrogram.onnx (Audio ➔ Mel spectrogram)
   2. embedding_model.onnx (Spectrogram ➔ Feature embeddings)
   3. custom nexus.onnx (Embeddings ➔ Probability score)
                          │
                          ▼
             Probability Threshold Gate:
   • Score > 0.5 AND refractory period (>2000ms) expired?
                          │
                          ▼
         Wake Event Emitted (Tauri IPC)
   1. Triggers window.__NEXUS_WAKE__() in Webview.
   2. Frontend Zustand store transitions: Idle ➔ Listening.
   3. Rive Vector Orb lights up.
   4. Activates Silero Neural VAD for user speech capture.
```

---

## Flow 12: Speech-to-Text & Phonetic Hotword Correction Flow

Local neural transcription with dynamic vocabulary biasing and regex mishearing corrections.

```text
User speaks command into microphone
                          │
                          ▼
      Webview Audio Capture (ScriptProcessorNode)
   ➔ Silero VAD detects end of speech (silence boundary)
   ➔ Audio buffer downsampled to 16 kHz mono WAV
                          │
                          ▼
      Local STT Server (server/stt_server.py)
   ➔ Model: Moonshine Medium v2 / faster-whisper on localhost:39217
   ➔ Hotword Injection: Loads %APPDATA%\...\stt_hotwords.txt (repo names, brands)
   ➔ Returns raw transcript
                          │
                          ▼
      Phonetic Correction Layer (recorder.ts)
   Regex fixes common acoustic mishearings:
   • "cervix" / "service" / "serve x" ➔ "servx"
   • "unless" / "analyze" ➔ "analyse"
   • "pf5" / "pe5" / "pr5" ➔ "PR 5"
   • "sync" / "zinc" ➔ "zync"
                          │
                          ▼
   Cleaned transcript passed to Central Orchestrator
```

---

## Flow 13: Local BERT-Mini NLU & Cloudflare R2 OTA Update Flow

Sub-millisecond intent and slot parsing with automatic over-the-air model updates on startup.

```text
NEXUS Client Starts Up
                          │
                          ▼
      OTA Model Update Check (nlu_update.rs)
   1. At startup +15s, queries GET /models/nlu/latest on Cloudflare Worker.
   2. Compares local manifest version with Cloudflare KV manifest.
   3. If new model available:
      • Streams ONNX blobs from R2 bucket (`nexus-models`).
      • Verifies sha256 checksums per file.
      • Saves to %APPDATA%\com.nexus.assistant\nlu_model\.
      • Calls POST http://127.0.0.1:39218/reload_model for zero-downtime swap.
                          │
                          ▼
      Runtime Transcript Classification
   ➔ Post transcript to http://127.0.0.1:39218/parse
   ➔ BERT-Mini ONNX extracts Intent + BIO Slot Sequence
   ➔ Calibrated Confidence Score (>0.85 threshold)
   ➔ Returns typed ParsedIntent to Orchestrator
```

---

## Flow 14: Admin Qwen Brain & Continuous Training Loop

Admin-only local reasoning and continuous dataset improvement.

```text
Transcript misses deterministic regex & NLU confidence < 0.85
                          │
                          ▼
         Admin Brain Gate (admin_config.rs)
   Is device Admin (`is_admin: true` + `features = ["admin-brain"]`)?
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
        [Admin]                    [Family User]
            │                           │
            ▼                           ▼
   Local Qwen Brain               Routes to 9Router
   (Qwen2.5-0.5B GGUF :39219)     or Cloudflare Worker
   ➔ Semantic routing &           (zero local LLM RAM)
      compound step planning
                          │
                          ▼
   Continuous Self-Improvement:
   1. Brain logs ambiguous turns to `approved_phrasings.jsonl`.
   2. Admin runs `nexus train` to retrain BERT-Mini ONNX.
   3. Admin publishes updated model to Cloudflare R2 via `publish_nlu.py`.
   4. Family devices pull updated model over the air automatically!
```

---

## Flow 15: Meeting Privacy Protection & Stream Suppression Flow

Real-time WASAPI / CoreAudio call detection to eliminate mic feedback and accidental wake-ups during meetings.

```text
Background Meeting Detector (meeting_detector.rs)
                          │
                          ▼
   1. Scans active audio capture sessions via Windows WASAPI / macOS CoreAudio.
   2. Monitors active process names:
      • Zoom.exe, Teams.exe, Google Meet (Chrome/Edge audio tabs), Webex.exe
                          │
                          ▼
            Meeting Active Detected?
                          │
            ┌─────────────┴─────────────┐
            ▼                           ▼
        [Meeting ON]               [Meeting OFF]
            │                           │
            ▼                           ▼
   Activates Privacy Mode:         Restores Normal Mode:
   • Mutes wake-word KWS           • Re-arms wake-word listener
   • Mutes local TTS audio         • Re-enables voice synthesis
   • Updates Tray icon status
```

---

## Flow 16: Ghostwriter Dictation Mode Flow

Hands-free continuous dictation room with real-time text typing and clipboard injection.

```text
User says: "ghostwriter mode" (or "take dictation", "type for me")
                          │
                          ▼
            Intent Parser (intent_parser.rs)
   ➔ ParsedIntent::GhostwriterEntry
                          │
                          ▼
         Central Orchestrator (orchestrator.rs)
   1. Opens dedicated Ghostwriter persistent overlay.
   2. Speaks: "Ghostwriter active, sir. Speak freely."
                          │
                          ▼
            Continuous Speech Loop:
   ➔ Streams audio chunks → Local STT.
   ➔ Converts speech → text in real time.
   ➔ Injects typed text directly into active application window via OS keystrokes.
                          │
                          ▼
User says: "exit ghostwriter" (or "stop writing", "close ghostwriter")
   ➔ Closes Ghostwriter room and returns to normal Assistant mode.
```
