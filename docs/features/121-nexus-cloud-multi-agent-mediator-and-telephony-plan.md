# Feature 121: NEXUS Cloud — Multi-Friend Agent Mediator + Free SIM Telephony

**Status: RESEARCH / VISION ONLY. Nothing in this document is built, approved, or
scheduled.** The user asked for this to be written down "for planning in the
future," explicitly distinct from a build plan. Treat every section as a
starting point for a real planning pass later, not a spec to implement from.

**Date:** 2026-10-14
**Hardware this is sized against:** a dedicated server (not the user's laptop),
12GB VRAM + 64GB DDR4 RAM, running 24/7.
**Companion document:** `docs/features/122-agent-completion-watcher-and-keep-awake-architecture.md`
describes the feature that was actually built the same day — the "laptop hands"
half of this vision already exists in miniature (see §2).

---

## 1. The vision, in the user's own words (paraphrased across two messages)

A server (separate box, not the laptop) sits between the user's friends and the
coding-agent CLIs they each already use (Claude, Gemini, etc.):

- The server is **not a dumb relay** — it's a mediator that understands a
  friend's intent ("complete this by tomorrow morning"), writes the actual
  prompts for that friend's own AI agent, manages the run, tests the output,
  and analyzes whether it's actually done.
- It does this with **each friend's own credentials and API keys** — never a
  shared pool billed to the admin.
- The actual task *execution* still happens on each friend's own laptop
  (because the agent's own file access / local project context lives there),
  not on the server. The server does the "thinking" (planning, prompt-writing,
  reviewing), the laptop does the "hands" (running the CLI against the real
  project).
- The server runs a **local model** for this "manager" role specifically —
  not a thin classifier, something good enough to actually plan and critique.
- The server tracks whether each friend's laptop is online or offline, and can
  tell the user about it.
- The server **learns from how each friend interacts** and improves how it
  manages their tasks over time — the same spirit as this project's existing
  admin-only `brain_monitor.rs` self-improvement loop, done per-friend instead
  of per-admin.
- Access is gated: only people the admin explicitly enrolls get in, via an
  admin-issued key **plus** Google Authenticator (TOTP). The admin wants to be
  able to revoke someone's access by voice too.
- Separately: the user wants their own already-paid-for Jio SIM wired into the
  server so friends can **call** NEXUS and NEXUS can **call them back** with
  real voice updates — and this must cost **$0 recurring**, because they're
  already paying for the SIM's plan and don't want a second bill on top of it.
- This whole thing is explicitly **team-only** — an admin-gated allowlist, not
  something every random user gets, partly because the server's RAM/VRAM is a
  real, finite, shared resource.

## 2. The one piece that's already half-built

Read `docs/features/122-agent-completion-watcher-and-keep-awake-architecture.md`
before planning this feature — it is directly reusable, not a separate thing to
design from scratch.

Today, on a single laptop, saying "ask codex to fix the bug" already: parses
the intent, dispatches it to the `agent_watch` sidecar (which wraps
`pty-manager` + `coding-agent-adapters`, covering Claude/Gemini/Codex/Aider/
Hermes/opencode), runs it using whatever credentials are configured *on that
machine*, polls for completion, and reports back via voice. **That is the
exact "laptop hands" behavior this bigger vision needs on every friend's
machine** — it just needs to be triggerable by a message arriving from the
server instead of only by the local mic.

Concretely, the missing link is a transport: each friend's NEXUS instance needs
a channel to receive "do X" instructions from the server and push "it's done /
it failed / it needs login" back. Everything after that handoff point
(`agent_watch::start_watch`, the keep-awake lease, the completion poll, the
spoken report) is **already written**.

## 3. Architecture sketch

```
                         ┌─────────────────────────────────┐
                         │         NEXUS SERVER              │
                         │  (12GB VRAM / 64GB RAM, 24/7)      │
                         │                                    │
  Friend A laptop  ◄────►│  ┌──────────────────────────────┐ │
  (own NEXUS,            │  │  Manager model (local LLM)     │ │
   own Claude/Gemini     │  │  - parses intent                │ │
   API keys)             │  │  - writes the prompt for the    │ │
         ▲                │  │    friend's own agent            │ │
         │ persistent      │  │  - reviews/tests the result      │ │
         │ connection       │  │  - learns per-friend patterns     │ │
         │ (WS/poll)        │  └──────────────────────────────┘ │
         │                │              │                     │
         ▼                │              ▼                     │
  agent_watch sidecar      │  ┌──────────────────────────────┐ │
  (ALREADY BUILT,          │  │ Profile/credential store        │ │
   feature 122)            │  │  - per-friend: enrollment key,  │ │
         │                │  │    TOTP secret, device tokens   │ │
         ▼                │  │  - NEVER a shared API key        │ │
  Claude/Codex/etc CLI     │  │  - presence: last-seen per device│ │
  runs on A's OWN laptop,  │  └──────────────────────────────┘ │
  using A's OWN API key    │              │                     │
         │                │              ▼                     │
         └────── result ──┼───►  Telephony bridge (§5)          │
                         │        (own Jio SIM, free)           │
                         └─────────────────────────────────────┘
```

## 4. Access control

### 4a. Human enrollment (solved pattern — confirmed against real prior art)

Admin-issued key + TOTP (Google Authenticator) for *who gets an account* is
exactly the shape bring-your-own-key (BYOK) multi-tenant platforms already use:
register the credential, bind it to a tenant ID, never let it leak into logs
or cross tenants. Nothing novel here, just build it carefully.

### 4b. Device-level trust — the gap the human-enrollment key doesn't cover

The admin key secures *who the person is*; it does not by itself secure *which
laptop is allowed to execute what the server sends it*. Plan for a **second,
separate credential per device**: when a friend's NEXUS first connects, issue
it a short-lived, rotating device token distinct from their account key. This
is what lets the admin kill one specific compromised/lost laptop without
touching the friend's account at all.

### 4c. Voice-triggered revocation — flagged, not rejected

The idea ("as the main admin I want to revoke access by my voice too") is
reasonable as a *trigger*. The flag: this project's own speaker verification
is explicitly documented as **fail-open** (AGENTS.md: "fail-open on any
error... Toggle: `speakerVerification` in settings.json (default false)") —
a deliberate choice appropriate for a single-user desktop assistant where a
false accept costs little. Revoking a friend's remote execution access is a
much higher-stakes action than anything that fail-open check currently guards.
**Recommendation:** voice phrase arms the revoke, a second factor (the same
TOTP, or a spoken repeat-back confirm) actually executes it — the same
two-step pattern this project already uses for WhatsApp sends and GitHub
merges. Voice-alone for this specific action is the one place worth pushing
back on if it comes up again during real planning.

### 4d. Team-only gating

This falls out of 4a for free — if enrollment requires an admin-issued key,
"only my team" is the *default* state, not an extra feature. No separate work
needed beyond building 4a correctly.

## 5. Telephony — free SIM calling

**The constraint that matters: $0 recurring**, because the user already pays
monthly for the Jio SIM and explicitly does not want a second bill.

### 5a. What was actually checked (not assumed)

Three real options were researched, not guessed:

| Option | Cost | Reliability | Why / why not |
|---|---|---|---|
| **Cloud telephony DID (Exotel/Twilio) + call-forwarding** | Paid — per-minute + DID rental | High — this is what every current open-source voice-agent framework (Pipecat, LiveKit Agents) is built around | **Ruled out by the $0 constraint**, not by capability. If a small recurring cost ever becomes acceptable, this is the easy, production-grade path — keep in back pocket. |
| **Spare Android phone as a GSM↔SIP bridge** (`gsm2sip`, `Android-sip-gateway`) | Free (one-time phone, if not already owned) | **Unreliable by design, not by luck** | Android deliberately does not expose raw call audio to third-party apps — this isn't a missing feature, it's an intentional OS privacy boundary. Working gateway apps require **root**, or a device-specific HAL match that may simply not exist for a given phone model. Real two-way audio injection into a live cellular call fighting this restriction is a per-device gamble, confirmed by current (Oct 2026) write-ups still describing this as unsolved for unrooted phones. |
| **USB GSM/3G modem + Asterisk `chan_dongle`** | Free beyond a ~$10-20 one-time modem (e.g. Huawei E173) | Works, with real caveats | A modem chip is *designed* to expose its voice stream programmatically — sidesteps Android's restriction entirely. Calls ride the Jio SIM's existing plan minutes, zero new recurring cost. **Caveats, stated plainly:** one call at a time (one SIM = one radio), 2G/3G voice quality (not HD), most documentation for this exact setup is from 2013-2015 (the tooling is old, not dead — it still works, but expect real setup friction: voice is often disabled by default on these modems and needs an AT-command unlock, and device support varies model to model). |

**Recommendation for the $0 constraint as stated: the USB modem + `chan_dongle` +
Asterisk path.** It's the only option that is both actually free and not
fighting an OS-level restriction baked in on purpose.

### 5b. eSIM — a dead end for this specific use case, stated honestly

Jio's eSIM conversion (MyJio app → OTP to the Jio number → the *target
device's* EID → a QR code, confirmed against Jio's own current documented
process) is built to move a number onto a **phone or tablet's** built-in eUICC
chip. There is no consumer-priced "server eSIM slot" product, and the process
is tied to a specific device's EID, not a generic server. The server-side
equivalent of what eSIM would give a phone is just the USB-modem path in 5a —
eSIM doesn't change the server-side story at all. Don't spend more research
time chasing an eSIM-on-server angle; it doesn't exist at this price point.

### 5c. What the server actually needs to run, once the modem bridges a call in

Asterisk hands the modem's audio to whatever is listening on the other end of
the dialplan. That "other end" is where the AI voice agent lives:
- STT: route to the same free-tier cloud service this project's desktop app
  already uses (Groq Whisper, free tier) rather than running it locally —
  reuses a proven pattern, costs nothing extra, frees the GPU.
- TTS: same idea — route to edge-tts (free), the desktop app's existing
  primary voice path.
- The actual conversational reasoning (what to *say*) is the one thing that
  should be local: the same "manager" model from §6, reused for live-call
  turns, not a second model.

This means the telephony feature's *only* GPU cost is reusing the manager
model's reasoning for one more purpose — not a separate voice-specific model
to budget for.

## 6. The local "manager" model — capacity plan

### 6a. Correcting an earlier undersell

An earlier pass of this research described the server-side orchestration as
"lightweight, no GPU needed" because each friend's actual coding work runs via
*their own* cloud API key (compute happens at Anthropic/OpenAI, not on this
box). That's still true for the raw task execution — but the user clarified
the "manager" role is not a thin router: it needs to **understand intent,
write the actual prompts, and review/test what came back.** That's a real
reasoning workload and does need real GPU budget. The capacity plan below
corrects for that.

### 6b. What fits in 12GB (confirmed against current benchmarks, not assumed)

- **Qwen3 14B at 4-bit quantization is the realistic ceiling** for a 12GB card
  — fits with headroom, runs at a usable ~10-20 tok/s on consumer-tier GPUs
  per current benchmarks. Capable enough to plan a task, write a structured
  prompt, and do a first-pass review of an agent's output.
- A Q8 13B-class model is *tight* at 12GB (roughly 13-14GB just for weights at
  that quant) — Q4 is the practical choice at this VRAM tier, not Q8.
- 64GB system RAM is comfortable headroom for everything else running
  alongside the GPU work: the orchestration server process itself, Asterisk,
  a profile/credential database, multiple concurrent friend sessions' *state*
  (not their model weights — those run on the friends' own cloud API calls).

### 6c. The concurrency problem, and the fix that solves it for free

If the manager model also has to run STT+TTS for a live phone call *at the
same time* it's reviewing a friend's coding task, that's real contention on
the same 12GB. **Fix: route STT/TTS to free cloud APIs (§5c) so the GPU only
ever runs one thing — the manager model's text reasoning** — for both
"plan this coding task" and "decide what to say on this call." A call and a
coding-task review happening at the exact same instant becomes a queue, not a
VRAM crisis, and queueing is an acceptable cost for a small, admin-gated
friend group.

### 6d. Honest residual risk

This has not been measured on real hardware — these are benchmark figures
from current public sources, not a measurement on *this* server. Before
committing to Qwen3 14B as the actual pick, the real planning pass should
benchmark time-to-first-token and sustained tok/s on the actual box, under the
actual expected load (one manager session + occasionally one live call),
the same "measure before promising" discipline this project already applies
to its voice-unification research (Pillar 1, feature 122 §9/doc voice-tts-01).

## 7. Presence tracking (laptop online/offline)

Not researched because it didn't need to be — this is a standard, solved
pattern: each friend's NEXUS instance holds a persistent, auto-reconnecting
connection (WebSocket, or a short-interval poll if a WebSocket proves
troublesome through home NAT) to the server; the server records a
last-seen-at timestamp per device and can answer "is Prem's laptop on right
now" instantly. No new research needed here when real planning starts — just
build it as part of the transport layer from §2/§3.

## 8. Per-friend learning loop

This project already has the shape of what's being asked for:
`brain_monitor.rs`'s continuous self-improvement loop for the admin's own
local model (observes usage → `approved_phrasings.jsonl` → retrains). The
multi-friend version is the same idea, scaled out.

**The one thing worth flagging now, before it's accidentally discovered
later:** in a multi-tenant version, each friend's interaction history must be
kept **isolated** when it feeds back into how the manager writes prompts —
friend A's coding style, project conventions, or task-phrasing patterns must
never silently leak into how the manager prompts on friend B's behalf. This is
a correctness and privacy requirement, not a nice-to-have, and it's the kind
of thing that's easy to get wrong if the learning store is built as one shared
pool "for simplicity" early on.

## 9. Security considerations — surfaced unprompted, worth taking seriously

A 24/7 server, reachable by phone, dispatching tasks to agents with "full
control" on multiple friends' laptops, is a materially larger attack surface
than anything else in this codebase. Specific risks worth designing against
from the start, not bolting on later:

- **Phone-channel scope limiting.** A phone call is an open mic with no
  wake-word discipline and a caller who can't be fully verified (caller ID is
  trivially spoofable). This project already has hard-won experience here —
  the `directed.rs` gate exists specifically because an open mic picks up
  ambient/injected commands. **Recommendation:** scope what the phone channel
  itself can trigger (status queries, task submission with a mandatory
  repeat-back confirmation) separately from what an enrolled laptop's
  authenticated session can do. Do not give phone callers the same "full
  control" as an enrolled device.
- **Device-token revocation, not just account revocation** (§4b) — the
  mechanism that lets a stolen/compromised laptop be cut off independently of
  the person's account.
- **Credential storage** — per-friend API keys must never be stored or logged
  in plaintext anywhere the admin (or anyone else) can casually read them;
  this project already has a `pub(crate) mod proactive_policy`-style discipline
  around keychain-first secret storage (see `auth_vault.rs` precedent) — the
  same discipline must extend to *friends'* keys, not just the admin's own.
- **Audit logging** — every prompt the server writes and dispatches on a
  friend's behalf should be logged somewhere the admin can review, both for
  debugging ("why did it do that") and for trust ("what has it been doing on
  my friend's machine while I wasn't watching").

## 10. Open questions for the real planning pass

These are not answered here on purpose — they're exactly the kind of decision
that needs the user's input before a real build plan can be written:

1. Transport for server → laptop dispatch: WebSocket (lower latency, more
   moving parts, needs reconnect/backoff logic) vs. short-interval polling
   (simpler, slightly higher latency, more server load per idle friend)?
2. What exactly counts as "done, tested, and analyzed" for the manager model
   to report success — does it re-run the friend's test suite itself, or trust
   the coding agent's own self-report, or something in between?
3. Real benchmark numbers on the actual server hardware (§6d) before
   committing to a specific manager model size/quant.
4. Whether the GSM-modem telephony path (§5a) is worth the real setup time
   given its dated tooling, or whether "wait and revisit Exotel's cheap-not-
   free tier later" is actually the better trade for a feature this early.
5. Exact shape of per-friend credential storage and the device-token scheme
   (§4b) — this needs its own focused design pass, not a one-line answer.

## 11. Sources (fetched during this research, not from memory)

**Open-source voice-agent frameworks / telephony architecture:**
- [Open-source voice agent frameworks comparison (Pipecat, LiveKit, Bolna, Vocode)](https://micdrop.dev/blog/open-source-voice-agent-frameworks)
- [Exotel integration docs (ElevenLabs)](https://elevenlabs.io/docs/eleven-agents/phone-numbers/telephony/exotel)
- [Migrating Voice Applications from Twilio to Exotel](https://exotel.com/blog/migrate-twilio-to-exotel-voice-ai-india/)

**GSM/SIM bridging:**
- [asterisk-dongle-setup](https://github.com/grwlf/asterisk-dongle-setup)
- [Using a Huawei E173 USB Modem for Calls (chan_dongle)](https://docs.mikopbx.com/mikopbx/english/faq/voip-gateways/calls-through-huawei-e173)
- [Asterisk Manager Interface (AMI): The Complete Developer Guide](https://dev.to/gamlin/asterisk-manager-interface-ami-the-complete-developer-guide-4d8e)
- [GSM-to-SIP Android gateway projects (gsm2sip, Android-sip-gateway)](https://github.com/asadcr/gsm2sip)
- [Turning an Unrooted Android Phone into a Cellular SIP Gateway with Shizuku (Oct 2026)](https://blog.sparktour.me/en/posts/2026/10/10/android-shizuku-cellular-sip-gateway/)
- [Android Telecom / InCallService official docs](https://developer.android.com/develop/connectivity/telecom)

**eSIM (India/Jio specific):**
- [Jio eSIM: Buy, Activate & Convert Physical SIM to eSIM Online](https://www.jio.com/esim/)
- [How To Convert Jio SIM to eSIM](https://www.bajajfinserv.in/how-to-convert-jio-sim-to-esim)

**Multi-tenant BYOK architecture:**
- [Bring Your Own Key access for AI providers](https://www.yottalabs.ai/post/bring-your-own-key-access-for-ai-providers)
- [BYOK: Your Keys, Your Control](https://xabcstack.com/en/docs/concepts/byok)
- [TencentCloud Octop: self-hosted multi-user AI assistant](https://aitoolly.com/ai-news/article/2026-09-20-tencentcloud-releases-octop-a-smarter-self-hosted-ai-assistant-featuring-multi-user-and-multi-agent)

**Local model capacity:**
- [Local LLM VRAM & RAM Requirements 2026](https://aicybr.com/blog/local-llm-vram-ram-requirements-2026)
- [RTX 4060 Ti vs RTX 4070 for Local LLM Inference in 2026](https://www.kunalganglani.com/blog/rtx-4060-vs-rtx-4070-for-ai)

---

## 12. One-paragraph summary, for a future skim

Reuse feature 122's `agent_watch` pipeline as the "laptop hands" for every
enrolled friend; build a thin server that authenticates friends (admin key +
TOTP + a separate revocable device token), runs one local manager model
(Qwen3-14B-Q4-class, STT/TTS kept on free cloud APIs to avoid GPU contention)
that plans/prompts/reviews each friend's task using *their own* API key, and
tracks presence via a simple heartbeat. For free telephony, skip Android
entirely (fighting an intentional OS restriction) and use a cheap USB GSM
modem + Asterisk `chan_dongle` against the existing Jio SIM, accepting one
call at a time and 2G/3G voice quality as the real cost of $0 recurring.
Scope the phone channel's capabilities down from "full control" on purpose.
Nothing here is built; this is the starting map for when real planning begins.
