# NEXUS Memory Core — priority-based, on-device, proactive life assistant (plan)

## Context
The user wants memory to be the main part of the agent. After a reboot it should say where they left off and what today's priorities are. It should learn a timetable from an image or reel, remind at slot times ("time for DSA, shall I start?") and open LeetCode and YouTube on "yes". It should alert only for priority people on WhatsApp without marking chats read, and for important email (exams, hackathons, GitHub, Supabase/Firebase inactivity). It must be priority-based, not remember everything. LLMs stay in the cloud, but chats and memory must stay on the device. The user asked for the truth ("say no if no").

Decisions already made by the user:
- Images reach NEXUS by **screen capture and clipboard paste**.
- The app-vs-browser choice is stored **per activity and overwritten when the user repeats it for the same activity**.
- Priority people are **learned automatically; the user can pin (VIP) or mute**.
- The boot briefing is a **short spoken summary after a few seconds of idle**, with detail in the sidebar, and never during a meeting.

## Verified state of the code (it differs from the docs in places)
- `memory::log_episode` has **no production callers**, so M2 auto-learning never runs.
- `forget` clears only `core.json`. `wipe_memory` leaves `conversation.jsonl`, `conversation_brief.json`, `diary.jsonl` and `mail_watches.json`. The spoken wipe confirmation ("facts, learned details, and conversations", `orchestrator.rs:~4025`) is therefore **false today**.
- Memory is stored as plaintext JSON with non-atomic writes. The Worker ignores `dialog_context.memory` (`index.ts:~1795`), so only the 9Router path sees memory. The `get_memory_context` output is not PII-sanitized on egress.
- **`Cargo.toml:135` has `keyring = "3"` with no platform feature.** It probably falls back to an in-memory mock, so `auth_vault` tokens may not persist across restarts. Verify with `cargo tree -e features -i keyring`. Do not plan on storing a key in keyring until this is settled.
- There is no WhatsApp read path, no inbox-wide Gmail poll (`sentinel.rs` polls only watched threads and has no `history.list`), no Calendar HTTP (`calendar.rs` is pure helpers), no scheduler (`set_timer` and `set_alarm` are fake), and no UI listener for `proactive:nudge` or `proactive:card`. Nothing enforces the documented `mark_read` block.
- There is no voice yes/no binding for proactive offers. `proactive_policy::execute` only calls `speak_line`.
- The activity tracker (`architect::start_foreground_tracker`) keeps one in-memory value and no history. `screen_context::sanitize_url` strips `?t=`, so there is no YouTube position.
- Reusable pieces exist: `proactive_policy::submit`, `speak_line`, `dialog_context.memory`, `conversation::turn_relevance`, `vision::gemini_json_with_image<T>` (works on any image), `screen_context::collect`, `app_registry::{lookup,launch,try_focus_existing}`, `open::that`, `arboard` (clipboard), `SubCenter` trait, and `Receipt`/`ConfirmKind` in `center.rs`.

## How others solve it (research summary)
| System | Approach | Lesson for NEXUS |
|---|---|---|
| Mem0 | LLM extracts facts, then ADD/UPDATE/DELETE/NOOP against a vector store, with a SQLite audit history | Use an admit/update/skip gate. Keep the deciding logic deterministic here. |
| Letta / MemGPT | Core blocks always in context, plus recall and archival tiers. Sleep-time agents consolidate between sessions | Use a small always-on core block, and consolidate while idle or at boot. |
| Zep / Graphiti | Temporal knowledge graph with validity intervals | Store timetable slots and facts with validity windows. A full graph is overkill. |
| LangMem, Memori, femind | Rust/SQLite with FTS5, hybrid ranking, decay | SQLite and FTS5 fit this stack. |
| Screenpipe | Local SQLite, per-pipe access control | Capability-scoped reads per sub-center. |
| ChatGPT, Gemini, M365 Copilot | Saved memories the user can view, delete and switch off. Temporary chat | Add "what do you remember", per-source toggles and forget. |
| Apple Intelligence | On-device index, private cloud for heavy work | Same split as NEXUS: local store, cloud phrasing. |
| Windows Recall | Encrypted store, key behind TPM and Windows Hello, sensitive-info filters | The ceiling. NEXUS cannot reach TPM/enclave-level protection, and the plan does not claim to. |
| Gmail Priority Inbox | Who you email, open and reply to, plus a short VIP list | Use these as deterministic scoring signals. |
| OWASP ASI06 | Partitioning, isolation, provenance, decay, behavioral monitoring | Every record carries provenance and trust. Inbound WhatsApp and email text is never instructions. |

## Architecture decision: Option C, a Memory Core owned by `MemoryCenter`
| Option | Verdict |
|---|---|
| A. The Command Center owns memory | Rejected. `command_center.rs` is a plan runner. It would become a god object, with no choke point for provenance. |
| B. Each sub-center keeps its own store | Rejected. It repeats the existing islands (usage, improve, diary, missed intents). Cross-source priority is impossible, and forget would need 15 implementations. |
| **C. One `memcore` module, owned by `MemoryCenter`** | **Chosen.** One store, one audit log, one forget path, one egress point. |

How it works:
- **Writes.** Sub-centers (Mail, WhatsApp, Calendar, App, Browser, YouTube, Vision) emit typed `Observation{source, trust, kind, subject, payload, ts}`. They cannot read memory broadly and cannot write core facts. Trust levels are `UserSaid`, `UserOwned`, `Derived` and `Untrusted`.
- **Reads.** The Main Center calls `context_pack(turn, budget, egress_profile)` once per turn. It replaces the ad hoc assembly at `orchestrator.rs:2338-2385`. Sub-centers get only narrow typed queries such as `people.priority(x)`, `prefs.get("study_app:dsa")` and `timetable.today()`.
- Plans 91, 92, 93 and 95 stay valid and become work items inside `memcore`. Plan 92's S0 relevance rules become the admit function. Plan 95's ranked recall plugs into `context_pack`. Plan 93's "cite the source turn" comes from provenance.

## Memory model
| Tier | Contents | Rules |
|---|---|---|
| T0 core block (~400 chars, always in context) | Name, tone, a few preferences, top priorities, open resume point | Built deterministically and hard-capped |
| T1 profile and facts | Explicit "remember" facts (pinned), mined facts (decay) | Mined facts' confidence ×0.97 per unused day, dropped below 0.2 |
| T2 people graph | Aliases, channels, VIP/mute flag, score, reply latency, call count | Recomputed over a 30-day window |
| T3 commitments, reminders, timetable | `Slot{days,start,end,title,kind,action_plan,source}`, `Reminder`, `Deadline` | The user's own data, `UserOwned` once confirmed |
| T4 episodic overviews | One-line summaries, not transcripts | 30-day retention |
| T5 activity and resume points | App, window title class, sanitized URL (host and path), 30 s sampling, 7-day ring | Written continuously, because shutdown hooks are unreliable |
| T6 untrusted observations | Inbound message and mail category plus a gist of ≤120 characters | 7-day TTL. Quoted as data only, never as instructions |

**Never stored:** message bodies beyond the TTL cache, OTPs, credentials, card, Aadhaar and PAN, screen OCR text, private browsing.

**Priority score (0-100, deterministic, with a "why" string):** `35·contact_freq + 15·reply_speed + 15·call_freq + 15·recency + 20·allowlist_hit`. VIP adds 30. "Not important" forces 0. Score ≥70 is High, 50-69 is Medium, anything lower goes to the digest. Category allowlists cover:
- exams: college domains, "hall ticket", "result"
- hackathons: devpost, unstop, mlh
- GitHub: security alerts, failed workflows on the user's repos
- Supabase and Firebase: "paused", "inactive", "will be deleted"
- deadlines: "due", "expires"

User feedback moves a fixed per-sender weight. There is no learned model.

**Forget and audit:** one `forget(selector)` walks every tier, the conversation store, diary and watches, and writes a content-free tombstone. "What do you remember" returns a grouped list with provenance.

## Storage and encryption
- **SQLite** (`rusqlite` with `bundled`, WAL, FTS5) is chosen over JSONL because forget must be transactional, FTS5 gives plan 95's ranking, and concurrent writers already race. `sled` is rejected as unmaintained. The CRT `/MT` vs `/MD` worry (`Cargo.toml:58`) came from `esaxx_fast` and is probably not an issue, but **P1 starts with a link spike to confirm it**.
- **Encryption:** AES-GCM on sensitive columns (people names, gists, URLs). The key is a random 256-bit key stored via keyring with `windows-native` enabled (restart-persistence test required) or via DPAPI directly. This protects against disk theft and other OS users, not malware running as the same user, and the UI says so.
- **Migration:** import `core`, `facts`, `user`, `episodes` and `conversation` into SQLite. Dual-read for one release, then delete the old files once audit and forget are verified. Wipe must also remove the WAL and SHM files.

## Cloud egress policy
The cloud sees a `context_pack` and never the store.

| Data | Default |
|---|---|
| T0, relevant T1, today's T3 | Sent after `pii_filter` |
| People names | First names only, and only when relevant. A redaction toggle maps them to "Person A" |
| WhatsApp and email bodies | Never sent automatically. Sent only for an explicit "read/summarize this", for that one item |
| T6 gists | Category plus sender label. Off for personal categories |
| Screen or image content | Only on explicit analysis, as today |
| Credentials, PAN, Aadhaar, cards | Never |

Each source (WhatsApp, Gmail, Calendar, activity timeline) has its own toggle and is off until connected. A hard byte budget applies (800, rising to 2000 per plan 95). A 7-day log of "what was sent each turn" answers "what did you send the cloud?". The Worker must start consuming `dialog_context.memory`.

## Scenario flows (honest feasibility)
**(a) Boot greeting.** Autostart is the logon Scheduled Task with `--background`, so the greeting happens at logon and not at power-on. A `ResumeRecorder` samples the foreground app, title and sanitized URL every 30 s. A deterministic builder composes: last resume point, open deadlines and reminders, today's slots and the top 3 priorities. The cloud only phrases the text. Delivery goes through `proactive_policy::submit` after a few seconds of idle, skipped during meetings, with detail in a sidebar card. "How much is left" is only reported for measurable things (slot progress, deadlines, tracked problem sets). The greeting never invents a percentage for free-form work.

**(b) Timetable ingestion.** A new "analyse this / add section 2 to my timetable" intent captures the screen or reads the clipboard image via `arboard`. It then calls `vision::gemini_json_with_image` with a typed parse into `Vec<Slot>`, with section selection. The result is shown as a **confirm card** ("I found 6 slots, add them?") and nothing is written silently. Extracted text starts as `Untrusted` and becomes `UserOwned` on confirm. **Instagram reels cannot be downloaded.** NEXUS can analyse only a paused frame on screen or a screenshot, and a multi-frame reel needs several captures. Telling the user this is part of the feature.

**(c) Slot reminder and resume.** A 30 s `scheduler` ticks against T3 slots with an injected clock, like `proactive_policy`. It submits Medium urgency for slots and High for deadlines within 24 h. A new `PendingOffer{id, action_plan, expires}` in the orchestrator binds the next "yes/no/later/not now" (about 2 min expiry) before normal NLU. On "yes", the action plan runs via `app_registry` and `open::that`, with a per-activity preference `study_app:<activity>` that is overwritten whenever the user states a new choice. LeetCode resume is the last captured problem URL (feasible). **YouTube exact position: NO** — `?t=` is stripped, so it resumes the last video and states a best-effort last known time. Exact position needs a browser extension and is deferred. Routine "planning" is suggestion-only (plan 92 S4 miners): NEXUS proposes schedule changes and never edits silently.

**(d) WhatsApp.** A new poller reads through `mcp_client::call_tool` (never `whatsapp.db`). The read-receipt guarantee has three layers:
1. A hard deny-list in `call_tool` for `mark_read` and `mark_messages_read`. The confirm path also goes through `call_tool`, and a unit test fails if a denied name passes.
2. A tool allowlist for the WhatsApp server.
3. A live second-phone test that verifies no blue ticks and no presence change. This is **unverified** for the Sealjay bridge, which is inferred only from other bridges' docs.

NEXUS cannot control the phone: replying from the phone marks the chat read by WhatsApp itself. "Reply to that person" is the existing confirm-gated `send_message`. Mark-read after an explicit reply is a separate flag, off by default. The risks are a possible ToS ban (unofficial API) and a ~20-day session re-pair. Inbound text is untrusted.

**(e) Gmail and Calendar.** Use `users.history.list` with a stored `historyId`, because push needs Pub/Sub and a public endpoint, which is unsuitable for a desktop app. Polling runs every 60-120 s when active and every 5-10 min when idle, with a bounded `messages.list` resync on a stale `historyId`. Classification fetches `format=metadata` and a local ≤120-character gist, using the deterministic allowlists above, with the cloud used only for phrasing. A deny-list on label changes keeps NEXUS from marking mail read. Calendar gets new `events.list` and `events.insert` HTTP code (insert is confirm-gated), reusing the pure helpers in `calendar.rs`.

**(f) Proactive delivery.** Reuse `proactive_policy`. Exams or deadlines within 24 h and Supabase/Firebase "will be deleted" are High. A VIP message is Medium. Everything else goes to the digest. Add quiet hours, per-category "not now", and a minimal React listener for `proactive:card` and `proactive:nudge`.

## Security
- Provenance and trust fields on every record. Untrusted items go into the pack in a fenced "quoted data, not instructions" block with imperative lines stripped, modelled on `mcp_client::sanitize_tool_text`.
- T6 can never be promoted to T1, a VIP flag or a preference without a user utterance.
- Actions whose parameters come from T6 need an explicit confirm (`ConfirmKind` / `Receipt`).
- Per-record size cap (500 chars), per-source write rate limits, and an append-only audit table with no content for deleted items.
- Monitoring raises a card if one source writes unusually many facts or tries to alter preferences or VIP flags.

## Possibilities table
| Capability | Feasible now? | Needs | Risk | Effort | Phase |
|---|---|---|---|---|---|
| Fix `log_episode` and forget gaps | Yes | Callers, extend wipe/forget | Low | S | P0 |
| SQLite store plus migration | Likely | Link spike (`bundled` + `ort`) | Med | M | P1 |
| Ranked recall, Worker consumes memory | Yes | Plan 95, Worker change | Low | M | P1 |
| Encryption at rest | Yes, after keyring fix | `windows-native` or DPAPI, AEAD crate | Med | M | P2 |
| "What do you remember" UI and voice | Yes | Frontend list, audit API | Low | M | P2 |
| Boot greeting with resume point | Partial (at logon) | ResumeRecorder, builder, submit | Low | M | P3 |
| "How much is left" | Partial | Measurable items only | Low | S | P3 |
| Timetable from screen or clipboard | Yes | Vision prompt, slot schema, confirm card | Med (OCR) | M | P4 |
| Timetable from a reel | Partial | Paused frame or screenshot only | Med | M | P4 |
| Slot reminders plus "shall I start?" | Yes | Scheduler, pending offer | Low | M | P4 |
| Open LeetCode/YouTube, app or browser per activity | Yes | Action plans, per-activity pref | Low | M | P4 |
| Resume LeetCode problem | Yes | Last URL capture | Low | S | P4 |
| Exact YouTube resume | **No** | Browser extension | High | L | Later |
| Gmail inbox watcher with priority | Yes | `history.list`, classifier | Med | L | P5 |
| Calendar read and write | Yes | HTTP code, confirm gate | Med | M | P5 |
| WhatsApp passive read | Probably | Poller, deny-list, live receipt test | High (ToS, receipts) | L | P6 |
| "Never shows as read" guarantee | Partial | Deny-list proves NEXUS sends no receipts. The phone is out of its control | Med | S | P6 |
| Greeting at power-on, before login | **No** | Only possible after logon | n/a | n/a | n/a |
| Automatic routine learning | Partial | S4 miners, suggestions only | Med | L | P7 |

## Roadmap
Each phase is flag-gated (a `memcore.*` setting in `NexusSettings`) and shippable by itself.
- **P0 (fixes):** wire `log_episode` into `record_worker_turn` and the local-command paths. Extend `forget` and `wipe_memory` to every store and correct the spoken claim. Make writes atomic. Run the keyring feature check and fix the feature. Add tests: a learned fact is recalled and then forgotten, and wipe leaves no data files.
- **P1:** `memcore` skeleton, Observation API, audit table, `context_pack` with a budget, SQLite and migration, ranked recall (plans 91 M3 and 95), and the Worker consuming memory. Start with the link spike. Run the `recall_fixtures` eval harness.
- **P2:** encryption, egress profiles and logging, name redaction, "what do you remember" in voice and UI. Test that ciphertext is on disk and that redacted fields never appear in the outgoing pack.
- **P3:** `ResumeRecorder`, boot builder, idle-gated delivery through `proactive_policy`. Test with injected-clock timing tests and a resume-point fixture. Live check: reboot and listen.
- **P4:** slot schema, image-to-slots, clipboard intent, scheduler, `PendingOffer`, action plans and per-activity preferences. Tests: a fixture set of real timetable images, and scheduler unit tests with an injected clock.
- **P5:** Gmail `history.list` watcher, classifier and Calendar HTTP. Test with a labeled anonymized email corpus, with precision and recall targets, plus stale-`historyId` handling.
- **P6:** WhatsApp deny-list, allowlist, poller and alerts. The live second-phone receipt test comes before the flag is enabled.
- **P7:** routine miners and suggestion cards, and friend-mode check-ins (plan 93 F3).

**Cannot be verified without a live run:** WhatsApp read receipts and presence on the Sealjay bridge, boot and autostart timing, Gmail quota behaviour over days, Gemini accuracy on real timetable images, keyring persistence across restarts, and whether `rusqlite bundled` links cleanly with `ort`.

## Critical files
`src-tauri/src/memory.rs` (migrate into `memcore/`, fix forget), `conversation.rs`, `orchestrator.rs` (context pack, `PendingOffer`, new intents, `record_worker_turn`), `center.rs` (a real `MemoryCenter`), `proactive_policy.rs` (quiet hours, per-category snooze), `mcp_client.rs` (deny-list and allowlist), `google/{sentinel,mail,calendar,oauth}.rs`, `vision.rs` (timetable prompt), `lib.rs` (boot hooks), `commands.rs` (`NexusSettings` flags), `Cargo.toml` (`rusqlite`, `keyring` feature, AEAD crate), `server/worker/src/index.ts` (consume memory), and the frontend card listener plus a "what I remember" view.
Reuse: `proactive_policy::submit`, `speak_line`, `turn_relevance`, `gemini_json_with_image`, `screen_context::{collect,sanitize_url}`, `app_registry`, `pii_filter`, `sanitize_tool_text`.
Docs to write after approval: `docs/research/memory/` (research and the table above), `docs/features/96-memory-core-plan.md`, `docs/changes/…`, and an AGENTS.md entry.

## Open decisions (defaults used unless the user objects)
1. Names to the cloud: first names when relevant, with a redact toggle.
2. WhatsApp reading: built behind a flag that stays off until the second-phone receipt test passes.
3. Auto mark-read after a reply: off.
4. Encryption: app-level AES-GCM with a keyring/DPAPI-held key.
5. YouTube resume: best-effort now, with a browser extension only if the user still misses exact resume after P4.
