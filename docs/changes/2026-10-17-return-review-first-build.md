# Return review and agent task reconciliation: first build

Date: 2026-10-17. Scope: the first implementation pass for feature 139. This is not a production release sign-off.

## Built

- One `return_review` coordinator consumes Windows lock, unlock, suspend, resume, logoff, and shutdown notifications. A saved local checkpoint joins events into an absence episode. The old unconditional boot greeting is disconnected. A return under 30 minutes has no greeting; at 30 minutes or more it adds a local-time greeting and the latest saved session/resume place.
- The return checks Windows session lock state and recent input before exposing task details. It saves a Command Hub card first, waits for the stage before voice, routes speech through the existing proactive/orb/caption path, and records spoken delivery only when the frontend reports speech complete. A quiet card completes a short return with no tasks.
- A bounded Memory Core task journal records task identity, provider, status, evidence type, and observation time. It excludes prompts, transcripts, terminal output, and API keys. NEXUS-owned local tasks become **unconfirmed** after restart until reconciled; terminal results are not inferred from workspace inactivity. A CLI adapter's output-pattern completion is labeled **possible response**, never a verified reply. The spoken review includes at most three task statuses.
- “Watch over this” records an existing foreground agent as unconfirmed rather than spawning an unrelated blank CLI. New NEXUS-owned CLI tasks still use the lazy sidecar. Its local HTTP control now requires a random per-run token, enforces body limits, and exits when the parent-held pipe closes (with a PID check as backup). It releases idle Node memory after about a minute and can recover a dead sidecar. Dependencies are pinned by a lockfile; installed resources are staged in writable app data.
- The Command Hub panel displays task evidence, last checked time, status, and the latest return review. Possible CLI responses use the medium-priority voice policy, which now considers actual desktop input activity before interrupting a user working elsewhere. Agent alerts accumulated during absence are merged into the return review.

## Implementation map

| Area | Main files | Record |
| --- | --- | --- |
| Return episodes and native signals | `src-tauri/src/return_review.rs`, `src-tauri/src/memcore/session.rs` | Coalesces session/power events, persists the away checkpoint, and gates return speech on presence and stage readiness. |
| Saved place and durable task state | `src-tauri/src/memcore/briefing.rs`, `src-tauri/src/memcore/agent_tasks.rs` | Supplies the resume point and bounded status/evidence records. The journal is currently stored through Memory Core metadata; the dedicated migrated table in the plan is still open. |
| Task discovery and sidecar | `src-tauri/src/agent_watch.rs`, `src-tauri/resources/agent_watch_sidecar/server.cjs`, `package-lock.json` | Keeps existing targets unconfirmed without a provider proof and hardens NEXUS-owned CLI control. |
| Routing and delivery | `src-tauri/src/orchestrator.rs`, `src-tauri/src/proactive_policy.rs`, `frontend/src/settings-sidebar/AgentWatchSection.tsx` | Routes watch requests, defers speech while away or busy, and displays the return card and evidence. |
| Packaging | `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `.gitignore` | Includes the sidecar resources and build configuration; first-use offline dependencies remain unresolved. |

## Implementation limits and next gates

1. Existing independent Claude Code, Cursor, Antigravity, Codex desktop, and other agent windows still lack verified attach hooks. They remain **unconfirmed**. Implement provider-specific hooks or protocols with stable task IDs, event sequence checks, opt-in setup, and real-version tests before claiming their responses are ready.
2. The NEXUS-owned CLI sidecar still needs Node and may run `npm ci` on first use. Package the runtime/dependencies for an offline installation, or replace the sidecar. The base return decision itself uses neither Node nor a model call.
3. Validate actual Windows lock, sleep, sign-out, shutdown, and cold login paths on hardware. Measure return card/audio latency, idle memory, and battery impact on a low-memory laptop. Fallback timestamps carry a conservative uncertainty margin at the 30-minute boundary; clock rollback and large manual clock jumps still need live hardening.
4. Exercise meeting deferral, TTS failure/retry, duplicate wake events, provider disconnects, sidecar crashes, and active typing. Do not promote this feature to production until the feature 139 acceptance matrix passes.
5. The quiet update is currently stored in Command Hub; a system-visible, non-focus-stealing notification while another app is foreground is still needed. A user typing continuously may otherwise see the status only when opening Command Hub.

## Source and rationale

The root-cause trace, design, acceptance matrix, and official Microsoft/provider source links are in `docs/features/139-production-return-review-and-agent-task-reconciliation-plan.md`. The supplied boot log showed an unconditional afternoon greeting and no persisted task/absence evidence, motivating the consolidated return path.

The explicit lock-state query uses Microsoft's [WTSQuerySessionInformationW](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/nf-wtsapi32-wtsquerysessioninformationw) and [WTSINFOEX SessionFlags](https://learn.microsoft.com/en-us/windows/win32/api/wtsapi32/ns-wtsapi32-wtsinfoex_level1_w), rather than treating any input at the lock screen as proof of an unlocked owner.

## Verification in this pass

- Final Rust `cargo check --features custom-protocol,admin-brain` passed with no errors. It emitted 29 dead-code warnings, mainly from the old boot-briefing and health path left dormant by the new return coordinator, plus one concurrent `power_cmd` warning. These require cleanup before a zero-warning release.
- Rust `return_review::tests` passed (1/1). `memcore::agent_tasks::tests` passed (3/3), including the possible-response wording and once-per-revision behavior.
- Frontend `npx tsc --noEmit` and `npm run build` passed. The build still emits the upstream `lottie-web` eval warning.
- Sidecar `node --test server.test.cjs` passed: rejects missing/wrong local token, refuses a blank agent launch, and exits when its parent pipe closes.
- The IPC contract checker has no new return/watcher gaps. It still fails on `get_voice_sync_state`, `ghost:action_receipts`, and baseline updates owned by other work.
- Live lock/sleep/logout/reboot, audio completion, provider version, and low-memory measurements have not been run; the production gates above remain open.
