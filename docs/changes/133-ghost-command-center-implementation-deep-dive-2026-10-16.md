# Ghost command center: implementation record

Date: 2026-10-16  
Status: first Windows implementation built; live application acceptance remains open.

## Purpose and scope

The user asked for a Ghost Mode that understands a spoken desktop task, chooses keyboard or cursor control without a vision model, learns reusable targets, and has its own command center and sub-centers connected to the main command center. This record describes the code that exists now. The design and remaining work are in [plan 129](../features/129-ghost-mode-reliable-desktop-control-implementation-plan.md); the evidence and external sources are in [research 13](../research/ghost-mode/13-no-vision-control-evidence-sources-2026-10-16.md).

No implementation can guarantee flawless control of every desktop application. The current contract is to perform a supported action with current target checks, distinguish observed completion from input dispatch, and stop when the target or outcome is uncertain.

## Command path and ownership

1. The active Ghost session offers its transcript to the strict local Ghost parser before general deterministic parsing. The parser returns a GhostTask of at most five ordered steps only when every step is recognized.
2. ParsedIntent::GhostAction enters the existing main command center. The main center owns turn identity, validity and side-effect gating, cancellation, and the spoken response. It delegates the desktop action to GhostCenter.
3. The orchestrator keeps the Ghost drill lifecycle and active request tracking, runs the desktop work on a blocking worker, emits ghost:action_receipts, then speaks the receipt and handles queued follow-up turns.
4. GhostCenter selects one sub-center per step: KeyboardCenter, AccessibilityCenter, CursorCenter when a live UIA point is needed, or LearningCenter. It records the selected route, outcome, detail, and elapsed time.
5. Any result other than Verified ends the sequence. A possibly completed action is never automatically repeated by a second route.

The command path is implemented in [intent_parser.rs](../../src-tauri/src/intent_parser.rs), [center.rs](../../src-tauri/src/center.rs), [orchestrator.rs](../../src-tauri/src/orchestrator.rs), and [ghost_center.rs](../../src-tauri/src/ghost_center.rs). The new modules are registered in [lib.rs](../../src-tauri/src/lib.rs).

## Parsing and dispatch

The local grammar accepts a bounded set of seeded shortcut actions, exact named accessible-control clicks, and explicit learning commands. Examples include a common shortcut request, “click the Search button,” “remember this button as settings,” “click learned settings,” and “forget learned settings.” Up to five recognized actions can be joined with “then.” The parser refuses a partial sequence, ambiguous generic target, unsafe target name, or invalid alias. This limits unintended actions when speech recognition produces plausible but incomplete text.

KeyboardCenter delegates to the existing shortcut map/router rather than implementing another key injector. The router's checks for foreground ownership, protected/elevated windows, and stopping remain in force. Only the router's Verified::Yes becomes a Ghost Verified receipt. A sent chord with no observed postcondition becomes Unconfirmed.

AccessibilityCenter searches the current foreground window for an exact, unique, enabled and visible control. It uses a current UI Automation element and available semantic pattern. Toggle and SelectionItem can be checked against their resulting state. Invoke dispatch is Unconfirmed unless an action-specific postcondition is available. The path refuses secure fields, changed foreground ownership, ambiguity, and unavailable targets.

CursorCenter is a fallback for a live accessible control when a semantic action is unavailable. It obtains a current UIA clickable point, checks the point still hit-tests to the intended element, then moves and clicks through the guarded mouse path. The mouse helper rechecks conditions after the glide and dwell. It accepts negative virtual-screen coordinates for monitors left of or above the primary display. Dispatch without observable app state is Unconfirmed.

The older direct Ghost click entry point now delegates to this center. It no longer calls a vision model on a UIA miss. Ghost ordinal clicks and named NLU clicks also join this route. This statement applies to the new Ghost desktop-action path; it does not claim that every other NEXUS screen-analysis feature has been removed.

## Learning and invalidation

LearningCenter supports an explicit teach/use/forget cycle in the foreground app. Teaching inspects the element under the user's cursor. It requires a usable, unique AutomationId, supported actionable control type, enabled and visible state, and a non-password control. It stores the alias with the executable name, hashed executable path, app file size and modification time, AutomationId, control type, and hashed accessible name. The local store is under the app data directory at ghost/learned_controls.json and is written through a temporary file followed by rename.

Reuse checks the current app fingerprint and resolves the same unique accessible identity again. An update, changed name or type, duplicate AutomationId, missing control, or damaged store blocks the action. Learned pixel coordinates, screenshots, field contents, and passwords are not stored. Forget removes the alias scoped to the current app. Non-Windows builds return a blocked result for this Windows UIA learning path.

The present locator is intentionally narrow. It does not include ancestry, tab state, or a user-visible rule manager. Controls without a stable unique AutomationId, including many custom canvases, cannot currently be learned. The teaching source is the control under the pointer, so the user must place the pointer on the intended control before the teach command.

## Receipts and user claims

| Outcome | Meaning | Next step |
|---|---|---|
| Verified | The available action-specific observation supports completion. | Continue an ordered task. |
| Unconfirmed | Input was sent or a UIA action dispatched, but completion was not observed. | Report uncertainty and stop the sequence. |
| Blocked | Parsing, target identity, app state, security, or capability failed a guard. | Explain the reason and stop. |
| Stopped | The Ghost session was cancelled. | Stop the active action. |

The receipt is evidence for a claim, not a claim that Ghost Mode works perfectly. In particular, a successful SendInput call, a clickable point, and an Invoke return are not universal proof that the requested application state changed.

## Changed code and reason

| File | Change | Reason |
|---|---|---|
| src-tauri/src/ghost_center.rs | Task grammar, route selection, guarded execution, receipts. | Give Ghost its own coordinator and bounded sub-center contract. |
| src-tauri/src/ghost_learning.rs | Local semantic alias store and live resolution. | Learn a control identity rather than a fixed coordinate. |
| src-tauri/src/intent_parser.rs | GhostAction intent. | Carry a structured task into the main center. |
| src-tauri/src/center.rs | Main-center routing and validity. | Keep central ownership and risk checks. |
| src-tauri/src/orchestrator.rs | Ghost parsing, lifecycle, delegation, receipts, speech. | Connect the centers without bypassing turn control. |
| src-tauri/src/live/commands/mouse.rs | Guarded click and negative monitor coordinates. | Revalidate the intended target at the point of action. |
| src-tauri/src/stt_groq.rs | Teach/use/forget phrases in decoder vocabulary. | Make explicit learning commands easier to recognize. |
| src-tauri/src/lib.rs | Module registration. | Compile and expose the new implementation. |

## Verification performed

- cargo check with custom-protocol and admin-brain features passed.
- Focused Ghost parser and receipt tests: 4 passed.
- Learning store tests: 2 passed, including replacement and malformed JSON refusal.
- Main-center and orchestrator routing tests: 2 passed.
- Mouse module unit tests: 9 passed.
- Speech-recognition vocabulary budget test: 1 passed.
- Rust formatting check for the new modules and whitespace check for tracked changes passed.

These are focused code checks, not an end-to-end live voice test. No real application click or mixed-DPI cursor run was performed in this implementation pass. The counts are evidence for those specific tests only.

## Plan change and remaining gates

The first implementation has completed the central Ghost entry point, strict command subset, guarded shortcut route, UIA pattern/point routes, and explicit semantic alias lifecycle from [plan 129](../features/129-ghost-mode-reliable-desktop-control-implementation-plan.md). The following work remains:

1. Run a read-only capability audit in disposable Notepad, Explorer, browser, VS Code, and a deliberately selected test chat or custom canvas. Measure unique control identity and supported patterns; avoid personal content.
2. Expand intent understanding with held-out phrasing and explicit target-app/context slots. Measure false actions and clarification rates, not only recognized phrases.
3. Add per-action postconditions for more UIA Invoke and cursor actions, and app shortcut override/context checks. Do not promote Unconfirmed to Verified from input dispatch alone.
4. Add visible learned-rule inspection and deletion, stronger locator context where AutomationId is not globally unique, and invalidation/retention policy.
5. Test focus switches at the final moment, duplicate labels, moving layouts, scroll, occlusion, mixed DPI, app updates, secure/elevated windows, cancellation, and repeated commands.
6. Publish per-app and per-action completion, wrong-target prevention, unsupported rate, and latency before broader rollout. Zero wrong-window actions and zero automatic replay after Unconfirmed are primary safety gates.

Until these gates pass, this is a guarded first implementation for a supported action subset, not a guarantee of perfect cross-app desktop control.
