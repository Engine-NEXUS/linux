# 140 — How agents like Claude Code and Codex control a computer (and delete things) safely, and the plan to give NEXUS the same control (RESEARCH + PLAN, nothing built)

Date: 2026-10-17. Owner ask: understand how AI agents control the laptop, run commands and delete files "perfectly", so NEXUS can get that control and understanding. Method: official docs of Claude Code and Codex, Microsoft's UFO2 paper, Anthropic computer-use material, prompt-injection design papers, Windows file-API docs, plus a read of NEXUS's own code on `D:\ULTRON`. Sources at the end; items marked (secondary) came from blogs/summaries, not vendor docs.

## 0. The honest answer first
No agent is "perfect". What makes Claude Code and Codex *reliable* is not a smarter model; it is **architecture around the model**, in four layers. Each layer assumes the one before it can be fooled:

| Layer | Question it answers | Claude Code | Codex |
|---|---|---|---|
| 1. Typed tools | What *can* the model even ask for? | Narrow tools with schemas: Read, Edit (exact-string replace), Write, Glob, Grep, Bash, WebFetch... The model never touches the machine directly | A shell tool + `apply_patch` |
| 2. Policy / permissions | Is *this call* allowed, ask, or denied? | Rules: **deny, then ask, then allow; first match wins**; per-tool specifiers (`Bash(git commit *)`, `Read(./.env)`); permission modes (manual, acceptEdits, plan, dontAsk, auto classifier, bypass) | Two independent axes: **sandbox mode** (read-only / workspace-write / danger-full-access) x **approval policy** (on-request / never); optional reviewer agent (`auto_review`) |
| 3. OS sandbox | If the model/policy is wrong, what is the blast radius? | Seatbelt (macOS) / bubblewrap (Linux, WSL2): writes only in the working directory + temp, protected paths (`.git`, `.claude`) write-denied, network only through a proxy with an allowlist. **Native Windows: no sandbox** | Seatbelt / bubblewrap; native Windows sandbox with low-privilege sandbox users, ACLs, firewall rules (elevated) or a restricted token + ACLs (unelevated) |
| 4. Hooks / audit / undo | Can policy be extended, logged, reversed? | PreToolUse hooks can allow / deny / ask or rewrite the input; exit code 2 blocks; `rm -rf` guard is the documented example | Reviewer agent on escalations; version control as undo |

Key facts that matter for NEXUS:
1. **The policy is not the boundary.** Claude's docs say a Bash deny rule "isn't a security boundary around the program": it matches the command text the model wrote, not every way of spelling the same program. That is why the OS sandbox exists underneath.
2. **Compound commands are split.** `safe-cmd && other-cmd` needs every part allowed; a deny/ask rule fires if *any* part (even inside `$(...)` or a loop) matches. Wrappers (`timeout`, `nice`...) are stripped before matching. Without this, "allow git" would also allow `git status && rm -rf /`.
3. **Reads are cheap, writes are scoped, destructive things ask.** Built-in read-only commands run without a prompt; edits ask (or are auto-accepted inside the working directory in acceptEdits); `find -delete` and exec wrappers are specially refused for "don't ask again".
4. **Two axes beat one dial.** Codex separates *what the sandbox physically allows* from *when to stop and ask*. Auto-approval inside the sandbox is safe *because* the sandbox exists.
5. **Escape hatches are explicit and audited.** Claude's `dangerouslyDisableSandbox` retry can be switched off (`allowUnsandboxedCommands: false`) or put behind an ask rule; Codex's bypass flag (`--yolo`) is documented as "only inside a container or VM".
6. **GUI agents (computer use, UFO2)** loop *screenshot -> decide -> act -> screenshot*. Microsoft's UFO2 improves on pure vision by fusing **Windows UI Automation** with vision parsing, mixing native APIs and GUI actions, validating each step with UIA checks (is_enabled / is_visible), and running in an isolated virtual desktop so user and agent do not collide. Anthropic reported prompt-injection success of 23.6% before and 11.2% after mitigations (secondary): on-screen text is a real attack channel.
7. **Prompt injection is solved by structure, not filters.** Willison's "lethal trifecta" (private data + untrusted content + a way to communicate out): remove one leg. The design-pattern rule: once an agent has ingested untrusted input it must be constrained so that input cannot trigger consequential actions. Dual-LLM (a tool-less model reads untrusted text, a privileged one acts) and Google's CaMeL (the plan is code; values carry provenance; policies are enforced at tool-call time; ~7 percentage points of task success traded for provable safety, secondary) are the two reference designs.
8. **Deleting safely** is a stack of small habits, not one feature: sandbox limits the blast radius to the project folder; destructive commands ask; hooks can hard-deny `rm -rf`; version control is the real undo. On Windows the OS offers a recoverable delete: shell file operations with the undo flag (`FOF_ALLOWUNDO`, newer `FOFX_ADDUNDORECORD`) send items to the Recycle Bin, while `DeleteFile` is permanent. Caveat from Microsoft's docs: the undo record lives in the Explorer process for the user session, so an app that wants its *own* reliable undo needs its own manifest.

## 1. What NEXUS has today (read from code on D:\ULTRON)
| Piece | State |
|---|---|
| Intent -> center validation -> `center::action_disposition(intent, transcript, source, ownership, ghost_active)` | A real gate: voice ownership (Verified / Unenrolled / Uncertain / Rejected), command evidence (Strong / Medium / None), side-effect check. Good seed of a policy layer |
| `live/safety.rs` | Tool whitelist + `DENIED_TARGETS` (banks, password managers...) + confirm-before list |
| `shortcuts/router.rs` | Gates before sending keys: same-window re-check, protected windows (exe/title denylist), elevated targets refused, verify afterwards. Exactly the "pre-send re-check + verify" habit agents use |
| `ghost_center.rs` receipts | `Outcome::{Verified, Unconfirmed, Blocked, Stopped}` per step: the right receipt shape |
| `power_cmd.rs` / `power_close.rs` | Graceful shutdown: snapshot, goodbye, polite `WM_CLOSE`, 3 s cancel window (no `/f`) |
| `command_executor.rs` | open/close apps (polite `taskkill`), PowerShell/cmd one-offs |
| Delegated agents (`coder/`) | Spawns claude/aider directly from a voice command |
| **File operations (delete, move, copy, rename, create)** | **None.** NEXUS cannot touch files by voice today, which is also why it cannot yet "delete anything" |
| Typed tool registry with risk classes | **None**: gates are scattered per feature |
| Central policy file, protected-path list, path canonicaliser | **None** |
| Undo / quarantine | **None** |
| Persistent audit log of actions | Only MCP (`mcp_audit.jsonl`); Ghost receipts are transient |
| Provenance (did this action come from the user's voice or from text NEXUS read?) | **None**. Screen tour, OCR, email/WhatsApp readers feed text into NEXUS; today none of it can trigger a destructive action only because no destructive action exists |
| OS sandbox for spawned agents | **None** (same as Claude Code on native Windows) |

Voice-specific risks agents do not have: mishearing ("check down my laptop" was a real one), TV/other voices, and no keyboard to type an exact path. NEXUS must resolve words to *concrete objects* and read them back, never act on a spoken path string.

## 2. Design: the NEXUS Action Kernel
One choke point every side-effecting action passes, in this order (the Claude/Codex layers, adapted to voice):

1. **Typed tools** (`action_kernel::Tool`): name, JSON schema, `risk`, `preview()`, `execute()`, `verify()`, `undo()`. No generic "run this command" tool. File tools call Rust APIs directly (no `cmd /c del`, no shell parsing).
2. **Risk classes** drive defaults:
   * `Read` (list, find, size, open-info): allow, no prompt, instant (speed preserved).
   * `Reversible` (create folder, copy, move/rename without overwrite, open app): allow + receipt + undo.
   * `Destructive` (delete, overwrite, close an app with unsaved work): never permanent by default. Delete = **move to NEXUS quarantine** (a per-user trash folder with a manifest) so "undo that" always works; thresholds (more than 20 items, a folder with more than 200 files, anything outside the allowed roots) require a spoken confirmation of a *resolved preview* ("Delete these 3 PDFs in Downloads, 14 MB?").
   * `Irreversible` (permanent delete, send message/payment/post, format, registry): spoken confirmation **and** an on-screen preview, owner voice only, cooldown; not shipped in v1 for files.
   * `Forbidden`: Windows and Program Files, other users' profiles, `.git`, `.ssh`, NEXUS's own data directory, credential stores, security settings, drive roots; no override by voice.
3. **Policy engine**: data, not code. A JSON policy (defaults shipped, user edits in the Hub): evaluation order **deny -> ask -> allow, first match wins** (Claude's rule), `allowed_roots` (default: Downloads, Desktop, Documents, user project folders), `protected_paths`, and mode (Ask always / Smart / Never destructive). Evaluated on the **canonical path** (resolve `..`, junctions and symlinks, 8.3 short names, case, UNC, drive roots) so a path cannot be spelled around the rule.
4. **Voice gate** (reuse `action_disposition`): evidence strength + speaker ownership; Destructive and above require a verified owner voice. Misheard commands go through the ask-once flow already built for shutdown.
5. **TOCTOU re-check** right before execute (the object is still the one previewed), then **verify** (it is gone from the folder / present in quarantine / size matches).
6. **Receipt + audit**: `Receipt{outcome, before, after}` using Ghost's `Outcome`, one spoken line, plus a local JSONL audit (what, when, which phrase, which risk, result), viewable in the Hub, 90-day rotation.
7. **Undo registry**: last N reversible/destructive actions; "undo that" restores from quarantine/moves back. Quarantine is purged by age, never silently.
8. **Provenance (injection defence)**: every planned step carries `origin = UserVoice | Derived`. Anything derived from screen text, OCR, web, email, WhatsApp or file contents may only run `Read`/`Reversible` tools; Destructive and above need a *fresh* user voice confirmation after the tainted context. Cloud models get text only (already true): the dual-LLM property.
9. **Delegated shell work stays delegated**: for "run the tests / fix this bug", NEXUS asks the user's own Claude Code / Codex with safe flags (workspace-write + on-request style, never `--dangerously*`/bypass) and uses the existing completion watcher. NEXUS does not become a shell.
10. **GUI control unchanged**: UIA first, vision fallback, protected-window refusal; adopt UFO2's UIA-validation habit where cheap.

## 3. Plan, phases and gates (each phase ends with tests, not a claim)
| Phase | Work | Days | Gate |
|---|---|---|---|
| P0 | Inventory every side-effect runner and classify its risk; one audit-log choke point | 0.5 | table of all runners + risk; audit lines appear for every existing action |
| P1 | Pure kernel: `Risk`, `Policy` (deny/ask/allow, roots, protected), **path canonicaliser**, `Receipt`, audit | 1.5 | path fuzz: `..`, junctions, symlinks, UNC, 8.3 names, case, drive roots, trailing dots; red-team corpus ("delete everything", "C:\Windows", "my documents") 100% refused/asked; mutation-checked |
| P2 | File tools: list/find (Read), create/copy/move/rename (Reversible, no overwrite: "keep both"), delete -> quarantine with caps, preview card in the sidebar, undo | 2 | every tool tested only inside a temp root; undo round-trip; count/size caps; no shell strings anywhere (grep gate) |
| P3 | Voice layer: intents + object resolution + read-back + confirm via the existing offer/yes-no flow; "undo that"; owner-voice requirement | 1.5 | misheard/garbled phrases never delete; confirmation needs strict yes; stop works mid-way |
| P4 | Provenance + injection tests (screen/email text saying "delete X" cannot trigger it) | 1 | injection corpus blocked 100% |
| P5 | Hub "Actions & Permissions" page: policy editor, audit viewer, undo list | 1 | policy edits take effect without restart; audit shows every action |
| P6 | Delegated coding agents: safe flags and approval mapping, documented Windows guidance (WSL2 / Windows Sandbox) | 0.5 | no bypass flag reachable from voice |

## 4. Open questions / risks (not verified)
* **Recycle Bin restore:** programmatic restore is not a clean public API (Microsoft ties undo to Explorer's session state) -> own quarantine + manifest is the plan; a final "send to Recycle Bin" after the undo window is optional and untested.
* **COM threading:** shell file operations need an STA thread from the Tauri runtime; to be tested.
* **Windows sandboxing** has no simple first-party equivalent of bubblewrap; Codex's approach (dedicated low-privilege users + ACLs + firewall) needs admin setup and is out of scope for v1. The kernel's path scoping is the v1 boundary, honestly weaker than an OS sandbox.
* **Speaker verification** quality gates the "owner voice only" rule (wake-word recall baseline was 63% on a held-out set); until measured, destructive actions rely on spoken confirmation of the resolved preview.
* More capability = more attack surface (TV audio saying "delete my files"): hence ownership, preview and provenance together, not any single one.
* Numbers quoted from secondary sources (23.6% / 11.2%, CaMeL percentages) must be re-checked against the papers before being relied on.

## 5. Decisions for the owner
1. **D1 default mode:** Smart (reads and reversible actions instant, destructive asks) — recommended — or Ask-always.
2. **D2 default roots:** Downloads, Desktop, Documents and project folders under `D:\`? Anything else, anything to exclude?
3. **D3 destructive confirmation:** spoken confirmation of a read-back plus sidebar preview (recommended) or voice only.
4. **D4 permanent delete:** not in v1 (recommended); everything goes to quarantine first.
5. **D5 shell tasks:** delegate to Claude Code/Codex with safe flags (recommended) vs a NEXUS-owned allowlisted command runner later.

## Sources
* Claude Code permissions: https://code.claude.com/docs/en/permissions
* Claude Code sandbox: https://code.claude.com/docs/en/sandboxing
* Claude Code hooks: https://code.claude.com/docs/en/hooks
* Codex sandboxing and approvals: https://learn.chatgpt.com/docs/sandboxing (redirect of developers.openai.com/codex/concepts/sandboxing)
* Codex Windows sandbox: https://learn.chatgpt.com/codex/windows/windows-sandbox ; third-party write-up (secondary): https://codex.danielvaughan.com/2026/05/14/codex-cli-windows-sandbox-engineering-restricted-tokens-acls-elevated-architecture/
* UFO2 (Microsoft Research / arXiv 2504.14603): https://www.microsoft.com/en-us/research/publication/ufo2-the-desktop-agentos/ , https://arxiv.org/abs/2504.14603
* Computer-use loop and injection figures (secondary): https://ppc.land/computer-use-agent/ , https://qubittool.com/en/blog/computer-use-ai-agent-browser-desktop
* Prompt-injection design: https://simonw.substack.com/p/the-lethal-trifecta-for-ai-agents , https://simonwillison.net/2025/Jun/13/prompt-injection-design-patterns/ , CaMeL https://css.csail.mit.edu/6.858/2026/readings/camel.pdf , https://agentpatterns.ai/security/camel-control-data-flow-injection/
* Windows delete/undo: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags , https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shfileoperationw
