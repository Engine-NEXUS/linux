# Feature 122: Keep-Awake + Agent-Completion Watcher — Architecture Reference

**Status:** Built, voice-wired, Settings panel shipped. Verified against the real
sidecar process (not assumed) — see §8.
**Date:** 2026-10-14
**Supersedes, for the agent-watch half only:** `docs/research/voice-tts-01-same-voice-offline-and-agent-task-watch-2026-10-14.md`
and `docs/research/voice-tts-02-agent-completion-hooks-and-keep-awake-2026-10-14.md`.
Those two documents sketched a **native-hooks** design (Claude Code `Stop` hook,
opencode `session.status`, Antigravity `hooks.json` → NEXUS's own `webhook.rs`).
That design was never built. What shipped instead is the **sidecar** design
described below, per an explicit user decision made mid-build (§2). The research
docs are kept for their license/prior-art research on Pillar 1 (voice unification,
still unbuilt) — do not use them for how agent-watch actually works today.

---

## 1. Why this exists

Three asks, tackled in order of dependency:

1. **Keep-awake** — the screen must stay on exactly while NEXUS is speaking or
   doing work, and go back to normal (allowed to sleep) the instant it's idle.
   Zero cost, zero new dependency — nothing in the codebase called any Windows
   sleep-prevention API before this.
2. **Agent-completion watcher** — NEXUS should notice, using only local signals
   (no burned API budget), when an external coding-agent CLI the user is running
   (Claude Code, Codex, Aider, etc.) finishes a task, keep the screen awake while
   waiting, and tell the user.
3. **Voice unification** (Pillar 1, separate — see `docs/research/voice-tts-01-*`)
   — cloud (Edge-TTS) and offline (Kokoro) should sound like the same voice.
   **Not built.** Still gated on its own 2-3 day spike (does OpenVoice V2's
   converter have an ONNX port, or does it need a Python sidecar like this
   project's STT already has). Not discussed further in this document.

## 2. The decision that shaped the build

The first design draft for item 2 scoped the watcher to "Antigravity + opencode
only." The user rejected this explicitly:

> "i just gave u an example opencode claude antigravity / there are many like
> trae kilo cursor etc etc / this is a production level project dont only
> protitse users that use antigravity it should help everyone / analyse an
> dresearc for possibiltes any projects that exist have this in them or not"

This is binding precedent for any future work on this feature: **it must stay
tool-agnostic.** Re-scoping it to a fixed named list of apps is the one thing
that's explicitly off the table.

The resulting research found real prior art —
[`HaruHunab1320/coding-agent-adapters`](https://github.com/HaruHunab1320/coding-agent-adapters)
(MIT, npm `coding-agent-adapters`) built on
[`HaruHunab1320/pty-manager`](https://www.npmjs.com/package/pty-manager) (MIT) —
that already implements exactly the generic pattern needed: spawn a CLI tool
inside a pseudo-terminal, per-tool adapter regexes detect `task_complete` /
`login_required` / `blocking_prompt` on the ANSI-stripped output. It ships
adapters for Claude, Gemini CLI, Codex, Aider, Hermes Agent, and opencode out of
the box, and a new tool later is a new adapter *inside that dependency tree* —
never a NEXUS code change.

The second decision was the integration shape. Two real options existed:
(a) reimplement the adapter *pattern* natively in Rust, or (b) orchestrate the
existing Node packages as a local sidecar. Asked directly, the user chose:

> "sidecar, it's less work and maintained upstream"

Everything below follows from that.

## 3. Architecture

```
Voice: "ask codex to fix the bug"
  │
  ▼
intent_parser.rs::parse_coding_command          (UNCHANGED — this phrase already existed)
  │  ParsedIntent::CodingAgentTask { agent: "codex", prompt: "fix the bug" }
  ▼
orchestrator.rs::run_coding_agent_task           (NEW check added at the top)
  │
  ├─ agent_watch_adapter_for("codex") → Some("codex")   (one of the 6 covered tools)
  │  │
  │  ▼
  │  agent_watch::start_watch(app, "codex", workdir, prompt)
  │     │
  │     ├─ ensure_deps_installed()   — npm install on first-ever use only (~68MB)
  │     ├─ ensure_sidecar_running()  — lazy-spawn node server.cjs, mirrors lazy_stt.rs
  │     ├─ POST http://127.0.0.1:39222/watch {type, workdir, prompt}
  │     ├─ keep_awake::acquire_lease()   — held for the watch's whole lifetime
  │     └─ background poll loop (tauri::async_runtime::spawn, every 3s)
  │           │
  │           ▼  GET /watch/:id   →  status: working | done | error | login_required | ...
  │           on terminal status → proactive_policy::submit(AlertUrgency::High)
  │                                 + emit "agent_watch:done"/"...:error" to frontend
  │                                 + drop the keep_awake lease
  │
  └─ (not one of the 6, e.g. "antigravity"/"cursor"/"trae")
     → falls through UNCHANGED to coder::dispatch_to_agent (spawn-if-possible, else clipboard)

Node sidecar (resources/agent_watch_sidecar/server.cjs):
  PTYManager (pty-manager) + 6 registered adapters (coding-agent-adapters)
  manager.spawn({type, workdir, adapterConfig}) → real PTY session, real child process
  manager.on('task_complete'|'session_error'|'login_required'|'blocking_prompt'|'session_stopped', ...)
  plain HTTP: GET /health, GET /adapters, POST /watch, GET /watch, GET /watch/:id, POST /watch/:id/stop
```

### Why this isn't "attach to a session the user already started by hand"

`pty-manager` is fundamentally a *spawn-and-own-the-child* library — it creates a
new PTY and a new child process; it does not attach to an already-running
terminal's output. Building that (a visible embedded terminal UI showing the
PTY's live output, so the user could type into a session NEXUS spawned) was out
of scope for this pass. **v1's actual shape is "delegate + watch": the user asks
NEXUS to run a task, NEXUS spawns it headlessly via the adapter's non-interactive
automation mode, and reports back when it's done** — not "watch a session I'm
already typing into." This is a real, deliberate scope decision, not an
oversight; revisit it if the actual desired UX turns out to be the latter.

## 4. Files

| File | Role |
|---|---|
| `src-tauri/src/keep_awake.rs` | RAII lease + heartbeat-watchdog `Controller`, structural copy of `volume.rs`'s pattern (see §5). |
| `src-tauri/src/agent_watch.rs` | Rust-side sidecar process manager (mirrors `lazy_stt.rs`'s lazy-spawn/idle-kill shape) + HTTP client + keep-awake/`proactive_policy` glue + 5 Tauri commands. |
| `src-tauri/resources/agent_watch_sidecar/server.cjs` | The Node sidecar. Plain HTTP (`node:http`, no Express), wraps `pty-manager` + `coding-agent-adapters`. **Must stay CommonJS** — see §6. |
| `src-tauri/resources/agent_watch_sidecar/package.json` | `"type": "commonjs"`, deps `pty-manager@^1.12.1` + `coding-agent-adapters@^0.18.0`. `node_modules/` and `package-lock.json` are gitignored — installed lazily, not bundled (§7). |
| `frontend/src/settings-sidebar/AgentWatchSection.tsx` | Command Hub card (mirrors `HandModeSection.tsx`'s shape) — status, covered-tools list, live 4s-polled active/recent watches with Stop buttons. Rendered inside `ConnectionsTab`. |
| `orchestrator.rs::agent_watch_adapter_for` + the block at the top of `run_coding_agent_task` | The one touch point in existing voice-dispatch code. |

### `keep_awake.rs` — deliberate deltas from the `volume.rs` template it copies

- **No baseline/crash-recovery file.** `SetThreadExecutionState`'s asserted state
  belongs to the process; Windows releases it when the process dies. Unlike
  system volume there's nothing to restore across a crash. *(Flagged as
  unverified in the original plan — "run `powercfg /requests` after a hard
  `taskkill` of NEXUS" is still an open live-hardware test, see §9.)*
- **No deferred-restore grace period.** Volume defers ~1.2s so utterances don't
  audibly dip; keep-awake has no such defect and the user wants it cleared the
  instant it's idle — `release()` clears immediately on the `count: 1→0` edge.
- **Watchdog re-asserts every tick, not just checks staleness.** Sources
  disagree on whether `SetThreadExecutionState(ES_CONTINUOUS|...)` holds
  indefinitely from one call or needs periodic re-assertion. Rather than trust
  either reading, the 500ms watchdog tick calls `backend.assert()` again on
  every pass while `count > 0` — a free Win32 call either way, correct under
  both interpretations.
- `set_working(bool)` + `start_working_poller()` — a 3s poll on
  `ghost::drill_running() || orchestrator::has_active_request()` auto-leases
  during any busy period, independent of the per-utterance TTS leases
  (`tts.rs` holds one at every existing `volume::acquire_volume_lease()` site).
- `RunEvent::Exit` calls `keep_awake::force_clear()` alongside the existing
  `volume::force_restore_volume()`.

## 5. `agent_watch.rs` — the sidecar lifecycle, mirroring `lazy_stt.rs`

| Concern | `lazy_stt.rs` (Python/STT, existing) | `agent_watch.rs` (Node, new) |
|---|---|---|
| Spawn trigger | Wake word fires | First `start_watch()` call |
| Responsiveness check | Raw TCP + `GET /health`, no tokio needed | Same shape, port 39222 |
| Concurrent-spawn guard | `STT_STARTING` CAS | Same (`SIDECAR_STARTING` CAS) |
| Console hiding | N/A (Python has no console on `pythonw`) | `CREATE_NO_WINDOW` (0x08000000), same idiom as `lazy_brain.rs` |
| Idle kill | Disabled (`STT_KEEP_ALIVE=true`) | Enabled, 10 min, since this is a much rarer feature |
| Dependency install | N/A (Python deps assumed pre-installed) | **New**: `ensure_deps_installed()` runs `npm install` on first-ever use (§7) |

`ACTIVE_WATCHES: Mutex<Vec<(String, Option<KeepAwakeLease>)>>` tracks in-flight
watches so `check_sidecar_idle()` never kills the sidecar out from under a live
watch, and so the idle-kill and the keep-awake lease lifetimes stay consistent.

## 6. The real bug found while verifying live

Not from documentation — from actually spawning all six adapters against the
real installed `node_modules` tree:

**`pty-manager` v1.12.1's published ESM build (`dist/index.mjs`) is broken.**
It's a tsup-generated bundle using a fake `__require` shim:

```js
var __require = ((x) => typeof require !== "undefined" ? require : ... )(...)
```

This assumes a global `require` exists — true in a bundler's output, **false in
a real Node ES module** (`"type": "module"`, no `require` global at all). When
`server.mjs` (the first version of the sidecar, written as ESM) called
`manager.spawn()`, every single adapter failed with the misleading message
`"node-pty is required but not installed. Run: npm install node-pty"` — the
*real*, swallowed error, visible only by reading `pty-manager`'s own source, was
`"Dynamic require of \"node-pty\" is not supported"`.

**Fix:** the sidecar is written as plain CommonJS — `server.cjs`,
`"type": "commonjs"` in its `package.json` — and imports via `require()`
throughout. `pty-manager`'s CJS build (`dist/index.js`) calls a real,
working `require("node-pty")`. Confirmed live after the fix: spawning `claude`
(installed on the dev machine) started a genuine PTY session (real PID, clean
`session_stopped` event on exit); the other five adapters correctly reported
`"File not found"` / Windows `CreateProcess` error code 2 — those CLIs simply
aren't installed on that machine, which is the *correct* behavior, not a bug.

**Lesson for anyone extending this sidecar:** do not switch it back to ESM
without re-verifying this exact failure mode is actually fixed upstream first.

## 7. Packaging: not bundled in the installer

`node-pty` (a `pty-manager` dependency) is a native addon. Its own install
script is `"install": "node scripts/prebuild.js || node-gyp rebuild"` — it tries
a **prebuilt binary** first, falling back to on-machine compilation only if that
fails. Confirmed on the dev machine: `npm install` pulled a real `win32-x64`
prebuilt `pty.node` with zero compilation (Microsoft ships the same package
inside VS Code's own integrated terminal, so this is a well-trodden path on
Windows specifically).

Despite that, the sidecar's `node_modules/` (~68MB, mostly `node-pty`'s
multi-platform prebuilt binaries) is **not bundled in the installer** —
`.gitignore`'d, and only `server.cjs` + `package.json` ship in
`tauri.conf.json`'s `resources`. `ensure_deps_installed()` runs `npm install`
the first time the feature is actually used, exactly mirroring how Kokoro's TTS
model/voice files are downloaded on first use rather than shipped (see the
Kokoro entry in `AGENTS.md`). This keeps the installer small for the large
majority of users who never touch this feature.

**Known gap, stated plainly:** `find_node()` only checks `node`/`node.exe` on
PATH — unlike `find_python()`, there's no Windows-registry fallback. A user
running coding-agent CLIs almost certainly already has Node installed for those
tools themselves, so this is a minor gap for this project's actual target
users, not a blocker — but it is a real gap, not an oversight to forget about.
This is a **dev/power-user feature by nature**, not a "family build" feature.

## 8. Verify

- `cargo check --features custom-protocol,admin-brain`: clean, 69 pre-existing
  warnings, **0 new**, throughout every edit in this feature.
- `cargo test --lib agent_watch::` 2/2, `keep_awake::` 6/6, `coder::` 6/6,
  `tts::` 24/24 (no regression from the added leases), plus
  `agent_watch_adapter_for_covers_only_the_tools_coding_agent_adapters_ships` 1/1
  and the pre-existing `test_coding_command_parsing` 1/1 (parser layer,
  confirmed unaffected).
- `npx tsc --noEmit` clean, `npx vitest run` 293/293 (zero regression),
  `npm run build` clean.
- **Sidecar smoke-tested standalone**, outside Rust entirely: `GET /health`,
  `GET /adapters`, `POST /watch` with an invalid type → 400, and all six real
  adapter spawn attempts (one genuine success — `claude` — five genuine
  "binary not found," both outcomes correct per §6).
- **Not run live**: the actual voice phrase through a packaged build (needs
  `nexus build` + a real mic) — only the parser layer, the orchestrator-routing
  layer, and the sidecar's own HTTP surface were exercised directly in this
  pass.

## 9. Still open

- The live-hardware keep-awake tests from the original plan: Windows
  sleep-timer behavior across a genuinely long spoken reply; `powercfg
  /requests` (elevated) immediately after a hard `taskkill` of NEXUS mid-lease,
  to settle whether the "no crash-recovery needed" assumption in §4 actually
  holds. **These need the user's own hands-on testing on real hardware** — not
  verifiable from an agent session.
- A Settings-panel "start a watch" button does not exist by design (§3) — the
  only entry point is the voice phrase. If that turns out to be the wrong
  call, building one is straightforward: `agent_watch_start` is already a
  registered Tauri command, it just needs a task-text input + workdir picker.
- A bare "watch X for me" with no task attached — not supported; v1 always
  needs a prompt to send (see the "delegate + watch" framing in §3).
- Pillar 1 (voice unification) is completely separate and untouched — see
  `docs/research/voice-tts-01-same-voice-offline-and-agent-task-watch-2026-10-14.md`
  for its own gated spike.

## 10. Where the historical record lives

- Full plan-mode document (approved): `C:\Users\Chitkul Lakshya\.claude\plans\delightful-scribbling-nautilus.md`
  — covers all three pillars, including the full rejected-first-draft →
  revised-generic-design arc for item 2 referenced in §2.
- Dated AGENTS.md entry: "Keep-Awake + Agent-Completion Watcher (2026-10-14)"
  — the terse changelog-style version of everything in this document.
- The bigger, not-yet-approved vision this feature is a stepping-stone toward
  (a central server that dispatches tasks to *multiple* friends' own NEXUS
  instances, each using their own credentials) is written up separately in
  `docs/features/121-nexus-cloud-multi-agent-mediator-and-telephony-plan.md` —
  that document is research/vision only, nothing in it is built.
