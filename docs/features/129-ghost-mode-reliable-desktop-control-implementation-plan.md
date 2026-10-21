# Ghost Mode: reliable desktop control without a vision model

Implementation detail and remaining gates: [change 133](../changes/133-ghost-command-center-implementation-deep-dive-2026-10-16.md). Primary platform sources and local evidence: [research 13](../research/ghost-mode/13-no-vision-control-evidence-sources-2026-10-16.md).

Status: implementation in progress, 2026-10-16. The Windows Ghost command-center route is built: main-center delegation, strict shortcut/click grammar, keyboard and accessibility sub-centers, ordered receipts, no vision fallback in the older direct Ghost click path, and explicit teach/use/forget commands for controls with unique UIA identities. Broad command coverage, per-app live verification, and rollout gates below remain open.

## Goal and honest boundary

NEXUS should accept a spoken Ghost Mode command, understand the intended task, choose the safest available way to do it, and report success only after observing the result. For routine actions, the preferred order is an existing keyboard shortcut, a Windows UI Automation (UIA) control pattern, then a cursor action against a freshly resolved accessible element. The Ghost Mode action path should use no screenshot, OCR, or vision-model call. Screen-analysis requests remain a separate feature.

“Perfect control of every app” cannot be guaranteed. Some apps expose no useful accessibility element, shortcuts can be remapped, and a reported clickable point can be blocked or change before the click. The product guarantee should instead be: **never silently guess a target, never claim an unverified action succeeded, and stop safely when evidence is insufficient.** Such a stop is a correct outcome.

Initial scope is Windows foreground desktop apps. macOS and Linux need separate accessibility and input adapters; they should not be treated as covered by the Windows implementation.

## Current starting point

- The existing shortcut map, router, and executor in src-tauri/src/shortcuts already cover 23 actions with app-specific seeds and a foreground-window gate. Their current Unconfirmed and NotChecked outcomes need to become explicit user-facing outcomes, not “done.”
- src-tauri/src/screen.rs lists foreground UIA actionables with names, control types, bounds, enabled/offscreen state, and a short timeout. It needs stable semantic identity, pattern availability, and live re-resolution for reliable interaction.
- src-tauri/src/live/commands/mouse.rs can resolve a UIA target before clicking, but its Ghost click path still falls back to vision. That fallback must be removed from the no-vision Ghost executor.
- src-tauri/src/orchestrator.rs routes many Ghost intents, including some screen and mouse paths that use vision. The new action planner needs one entry point and an explicit boundary so a simple Ghost action cannot accidentally enter those paths.
- src-tauri/src/ghost.rs and frontend/src/net/ghostHotMic.ts already own session, queue, hot-mic, cancellation, and relisten behavior. Preserve these while adding execution receipts.
- docs/features/103-ghost-command-understanding-and-keyboard-first-implementation-plan.md describes earlier parser and shortcut work; docs/changes/119-shortcut-map-and-action-router-e1.md records the E1 implementation. The research and limitations behind this plan are in docs/research/ghost-mode/12-no-vision-keyboard-and-cursor-learning-plan-2026-10-16.md.

## One action contract

Introduce a normalized command and an execution receipt shared by the parser, router, executor, and spoken response. Keep these separate from free-form chat:

    GhostCommand {
      action_id, object, target_app, target_window, slots,
      source_utterance_id, confidence, risk_class
    }

    ActionCandidate {
      route: Shortcut | UIAPattern | Cursor | Unsupported,
      foreground_hwnd, app_identity, focus_context,
      locator_or_keybinding, expected_postcondition
    }

    ActionReceipt {
      outcome: Verified | NoEffectProven | Unconfirmed |
               WrongTargetPrevented | Blocked | Stopped,
      route, app_identity, foreground_hwnd,
      evidence, elapsed_ms
    }

The planner ranks candidates by evidence, not by a fixed assumption that every shortcut always works. It may choose a shortcut only when the foreground app and focus context match its binding. UIA patterns are preferred over cursor movement for named controls where a live element supports the right pattern. Cursor movement is last and requires a fresh target and hit-test. Unsupported means NEXUS asks for clarification, offers a teach flow, or explains that it cannot safely perform the action.

Only Verified may produce an unqualified “Done.” NoEffectProven may permit a safe alternate route for an idempotent action. Unconfirmed must not trigger a blind retry of a non-idempotent action such as send, delete, purchase, or submit. WrongTargetPrevented and Blocked must stop. Cancellation must stop the action queue before any subsequent step.

## Implementation sequence

### Phase 0 — Baseline and fixtures

**Build:** A repeatable, read-only probe for foreground window identity, focused control, available UIA properties and patterns, app version, keyboard binding source, and postcondition observability. Record only structural data and timings; omit typed text, passwords, contact names, and message bodies. Create test fixtures for Notepad, Explorer, a browser, VS Code, and a disposable WhatsApp test chat. Include an inaccessible custom-drawn control as a negative fixture.

**Deliverable:** A capability table by app/action with four states: shortcut proven, UIA pattern proven, cursor target proven, unsupported. Baseline the existing latency and failure rate before modifying execution.

**Gate:** We can reproduce at least one successful and one blocked path for each route. No live user data is sent or modified during probing.

### Phase 1 — Guardrails and a single Ghost action entry point

**Build:** Add the command/receipt types and a Ghost action planner, then route eligible Ghost commands through it from src-tauri/src/orchestrator.rs. Keep the current session and queue behavior in src-tauri/src/ghost.rs. Put the no-vision policy at this entry point and remove the vision fallback from its mouse/UIA path in src-tauri/src/live/commands/mouse.rs. Do not remove vision from explicit “analyze my screen” requests.

Resolve the foreground HWND at plan time and check it again just before each action. Abort if the app/window changed, the screen is locked, a modal blocks the target, the target is elevated beyond the process's input rights, or the user canceled. Apply the existing confirmation policy to risky actions before execution. Ensure the planner cannot dispatch two executors for one command.

**Tests:** Unit tests for wrong-window prevention, cancellation between planning and execution, locked/elevated/modal states, and non-idempotent Unconfirmed handling. An integration test proves a simple Ghost command never calls the vision path.

**Gate:** No wrong-window action in adversarial tests; every attempted action yields a receipt; no “Done” for Unconfirmed.

### Phase 2 — Command understanding and ownership

**Build:** Expand src-tauri/src/intent_parser.rs and the existing command-understanding layer to normalize the 23 shortcut actions plus common phrasing, polite requests, self-corrections, app names, and object slots. Distinguish an instruction from ambient speech, especially in hot-mic Ghost sessions. Keep the owner-voice and meeting-suppression behavior. Ask one short clarification when the action or target has multiple plausible meanings; do not use a model to guess an irreversible action.

Example: “save this,” “please save the file,” and “save it in VS Code” normalize to Save, while “I wish this was saved” remains ambient unless context makes it a command. “Click the blue button” is unsupported without an accessible identity; color alone is not a safe target without vision.

**Tests:** Held-out spoken/transcribed phrases, noisy STT variants, negative ambient utterances, and ambiguous commands. Track intent accuracy and, separately, wrong-action rate; the latter is the safety metric. Initial release targets: at least 90% correct on the held-out supported-command set and at most 0.5% wrong actions, with zero execution on the designated ambient-negative set. Revisit these targets using real opt-in data before broad rollout.

**Gate:** The parser routes supported commands to canonical actions and abstains on ambiguity. Existing Ghost stop behavior remains intact: stop cancels the current task; explicit exit/Escape leaves Ghost Mode.

### Phase 3 — Capability registry and shortcut learning

**Build:** Extend src-tauri/src/shortcuts/map.rs, router.rs, and exec.rs instead of creating a second shortcut engine. Record app executable identity, version range, action, binding source, focus requirements, risk, and verifier. Inspect app-exposed bindings where available, especially VS Code user keybindings and context conditions. Treat seeded shortcuts as hypotheses until verified in the current app. Invalidate or demote a learned binding after app updates, binding changes, repeated NoEffectProven outcomes, or focus-context mismatch.

Store structural capability data only. Keep per-user overrides and a “forget learned action” control. If a binding invokes an unknown extension or command, require an explicit user teach/approval step before reuse.

**Tests:** Default and remapped bindings, conflicting shortcuts, editor versus dialog focus, app updates, and a shortcut that succeeds but leaves the foreground window unchanged.

**Gate:** A shortcut is chosen only with matching app/focus evidence and an available postcondition. Where the postcondition is not observable, return Unconfirmed rather than asserting success.

### Phase 4 — UIA semantic action engine

**Build:** Expand src-tauri/src/screen.rs to expose AutomationId when present, control type, name, ancestry, enabled/offscreen/password state, supported patterns, and current element identity. Cache bulk properties for discovery, then re-resolve a live element before invoking a pattern. Add a small UIA executor for Invoke, Toggle, SelectionItem, ExpandCollapse, Value, and Scroll as appropriate. Choose the pattern from the element's advertised support and the requested action; do not call Invoke on every control.

Define semantic locators as a weighted combination of app identity, top-level window, AutomationId, control type, accessible name, and ancestry. Use names as contextual clues, not unique IDs. Reject multiple equally plausible matches. Do not store Value-pattern contents or password-field data in learned locators or logs.

**Tests:** Duplicate names, changing list order, element replacement, disabled/offscreen controls, controls that disappear after invocation, delayed UIA providers, and an app that exposes a control but no applicable pattern.

**Gate:** The engine can locate and act on supported controls after window resize and reopening; it refuses ambiguous or stale targets. Pattern invocation is followed by an observed postcondition.

### Phase 5 — Fresh cursor targeting

**Build:** Add a cursor executor only for live accessible targets without a suitable pattern. Scroll a target into view using UIA when possible, fetch a current clickable point or compute an inset point within the current bounds, and hit-test the point against the intended element/window. Recheck foreground HWND, target identity, enabled/visible state, display scaling, and occlusion immediately before input. Move once, click once, then verify the outcome. Do not save absolute x/y as the learned action.

If the hit-test fails or a custom canvas exposes no target identity, stop and report that the control cannot be reached safely without a visual or app-specific integration. No fallback from a failed target to an approximate coordinate.

**Tests:** Resizing, scrolling, multiple monitors, mixed DPI, overlay/modal appearance, animated elements, window switch between move and click, and an intentionally intercepted clickable point.

**Gate:** Zero wrong-target clicks across the adversarial fixture suite. A pointer movement alone never counts as success.

### Phase 6 — Teach, reuse, and forget

**Build:** Offer “teach this action” when a safe route is unavailable. With a user demonstration, capture the foreground app, semantic UIA element under the pointer, applicable pattern/shortcut, focus context, and expected visible state change. Generate a proposed reusable rule, show the target and effect in plain language, and ask the user to accept it once. Re-resolve and verify the rule on later runs. Allow the user to inspect and forget rules.

A demonstration on an inaccessible canvas cannot produce a generally reliable coordinate rule. Such a case remains a one-time, visibly supervised action or requires an app-specific API/plugin.

**Tests:** Reuse after relaunch, resize, update, duplicate labels, and removal of a learned rule. Confirm that learned data contains no message text, credentials, or raw screen image.

**Gate:** A learned action works after a window change or is safely invalidated; it never silently falls back to its demonstration coordinates.

### Phase 7 — End-to-end rollout

**Build:** Add one compact receipt per action with action ID, route, anonymized app identity, verifier, outcome, and timing. Show a clear user-facing explanation for Blocked/Unconfirmed. Run first in report-only shadow mode, then opt-in execution, then enable per app/action after its gates pass. Keep one rollback switch for the new Ghost planner; rollback must not re-enable a vision fallback for the no-vision route.

**Live acceptance matrix:** Notepad, Explorer, browser, VS Code, WhatsApp test chat, Settings, and a custom canvas; fresh and warmed sessions; hot-mic and queued commands; user stop; wrong speaker/ambient speech; permission mismatch; app updates; multiple monitors and scaling; offscreen and obstructed controls; app closing mid-action; delayed responses. Track supported-task Verified rate, WrongTargetPrevented count, Unconfirmed rate, wrong-action rate, and p50/p95 time to verified result per route.

**Release targets:** Zero accidental send/delete/purchase and zero wrong-window input in the adversarial suite. Aim for at least 98% Verified completion on the explicitly supported task matrix, with 100% explicit Blocked/Unconfirmed outcomes for unsupported or unobservable cases. Treat these as gates to demonstrate, not guarantees already achieved. Publish the supported app/action matrix rather than promising universal control.

## Suggested code ownership

| Area | Existing location | Proposed change |
| --- | --- | --- |
| Spoken command normalization | src-tauri/src/intent_parser.rs and command-understanding modules | Produce canonical GhostCommand and abstain on ambiguity |
| Routing and response | src-tauri/src/orchestrator.rs | One Ghost action entry point, receipt-aware spoken result |
| Session, queue, stop | src-tauri/src/ghost.rs; frontend/src/net/ghostHotMic.ts | Preserve lifecycle; propagate cancellation and receipts |
| Keyboard route | src-tauri/src/shortcuts/ | Extend map/router/verifier and user binding support |
| Accessible element discovery | src-tauri/src/screen.rs | Structural snapshot, pattern inventory, live semantic lookup |
| UIA/cursor execution | src-tauri/src/live/commands/mouse.rs plus focused new module | Pattern first, fresh hit-tested cursor, no vision fallback |
| Learned capabilities | New narrowly scoped ghost capability store | Versioned semantic rules, evidence, invalidation, forget |
| Settings/feedback | Existing Ghost/Settings UI | Show supported actions, teach/forget, truthful outcomes |

GhostCommandCenter is a dedicated center beneath the Main Command Center. The main center retains voice ownership, turn state, cancellation, validity, and confirmation policy; GhostCommandCenter owns desktop action planning and delegates to its keyboard, accessibility, and cursor sub-centers. It reuses the existing shortcut and mouse engines rather than duplicating them.

## Verification rules for every action

1. Capture the foreground app/window and current focus.
2. Normalize the command; clarify if ambiguous.
3. Enumerate only safe candidates for that app and context.
4. Select one route and record its expected postcondition before acting.
5. Recheck app/window and target just before input.
6. Execute once; wait within a bounded, action-specific interval.
7. Observe the postcondition through app state, UIA state, window state, or an explicit return signal.
8. Emit one receipt and speak according to its outcome.

Verification must match the action. A window title change is useful for opening a document, but does not prove Save, Copy, or Send. Clipboard state may verify Copy only if the action may safely inspect it and the result is not logged. A successful key injection or UIA API return is evidence of dispatch, not proof that the task completed.

## Technical references

- Microsoft: [UI Automation control patterns](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controlpatternsoverview) and [UIA caching for clients](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-cachingforclients). Cached discovery data is useful, but a cached pattern cannot be invoked as the live action.
- Microsoft: [UI Automation providers](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-providersoverview), [clickable point limitations](https://learn.microsoft.com/en-us/dotnet/api/system.windows.automation.automationelement.getclickablepoint), and [UIPI restrictions](https://learn.microsoft.com/en-us/troubleshoot/power-platform/power-automate/desktop-flows/ui-automation/uipi-issues). These explain why some apps/controls must return Blocked or Unsupported.
- [VS Code keyboard shortcuts](https://code.visualstudio.com/docs/configure/keybindings). User overrides and context conditions prevent treating a shortcut as globally fixed.
- [Electron accessibility](https://www.electronjs.org/docs/latest/tutorial/accessibility) and [Chromium Windows UIA support](https://developer.chrome.com/blog/windows-uia-support-update). Browser/Electron coverage should be validated in the live app matrix rather than assumed.
